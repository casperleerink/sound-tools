// The SDK of the project's own tools and cards. Sound Tools writes this file when the project
// opens, so an edit here is lost. Read `agent-docs/extensions.md` first.
//
// A tool is a `.ts` file next to this one that calls `tool`: the fields of its record, its doc
// for agents, and its sound in Hum. A card for a built-in tool calls `card`.

// ---------------------------------------------------------------------------------------------
// Fields of a record

/** What a readout shows after the number. */
export type Unit = "hz" | "ms" | "db" | "percent";

/** A number a knob turns. It plays live: a turn glides the sound and runs no code. */
export interface KnobField {
  kind: "knob";
  min: number;
  max: number;
  default: number;
  unit?: Unit;
  /** What the card calls it. The field name, in words, when left out. */
  label?: string;
}

/** On or off. It plays live, as 1 or 0 in Hum. */
export interface ToggleField {
  kind: "toggle";
  default: boolean;
  label?: string;
}

/**
 * One of a few options. It decides the structure of the sound, such as how many delays there
 * are, so `sound` runs again when it changes and the new sound fades in.
 */
export interface ChoiceField<Option extends string | number = string | number> {
  kind: "choice";
  options: readonly Option[];
  default: Option;
  label?: string;
}

export type Field = KnobField | ToggleField | ChoiceField;
export type Fields = Record<string, Field>;

export const knob = (field: Omit<KnobField, "kind">): KnobField => ({ kind: "knob", ...field });
export const toggle = (field: Omit<ToggleField, "kind">): ToggleField => ({
  kind: "toggle",
  ...field,
});
export const choice = <const Option extends string | number>(field: {
  options: readonly Option[];
  default: Option;
  label?: string;
}): ChoiceField<Option> => ({ kind: "choice", ...field });

type ValueOf<F> = F extends KnobField
  ? number
  : F extends ToggleField
    ? boolean
    : F extends ChoiceField<infer Option>
      ? Option
      : never;

/** The `state` of a record of a tool with these fields. A field left out is at its default. */
export type StateOf<S extends Fields> = { [Name in keyof S]?: ValueOf<S[Name]> };

/** What `sound` gets: a choice as its value, a knob or a toggle as a `Param`. */
export type SoundOf<S extends Fields> = {
  [Name in keyof S]: S[Name] extends ChoiceField<infer Option> ? Option : Param;
};

// ---------------------------------------------------------------------------------------------
// Hum

/**
 * A knob or a toggle in Hum code. Put it in the code as `${rate}`; it is no number, so code
 * cannot branch on it: a turn must not run `sound` again.
 */
export class Param {
  constructor(readonly name: string) {}
}

/** Lines of Hum. Make them with the `hum` tag. */
export class Hum {
  constructor(readonly text: string) {}
}

export type Piece = Param | Hum | number | string | readonly Piece[];

/**
 * Hum code. `${...}` takes a `Param`, a number, a name, other Hum, or a list of those, which
 * goes in one after the other. Lines are trimmed, so indent freely.
 */
export function hum(strings: TemplateStringsArray, ...pieces: Piece[]): Hum {
  let text = strings[0] ?? "";
  pieces.forEach((piece, index) => {
    text += humText(piece) + (strings[index + 1] ?? "");
  });
  return new Hum(text);
}

function humText(piece: Piece): string {
  if (piece instanceof Param) return piece.name;
  if (piece instanceof Hum) return piece.text;
  if (typeof piece === "number") return humNumber(piece);
  if (typeof piece === "string") return piece;
  return piece.map(humText).join("\n");
}

/** Hum reads digits and a point, so a number is written without an exponent. */
function humNumber(value: number): string {
  if (!Number.isFinite(value)) {
    throw new Error(`${value} is not a number Hum can play`);
  }
  const text = Math.abs(value) < 1e-9 ? "0" : value.toFixed(9).replace(/\.?0+$/, "");
  return value < 0 ? `(${text})` : text;
}

// ---------------------------------------------------------------------------------------------
// Tools

export interface ToolSpec<S extends Fields> {
  /** The `tool` of its records and the name of its doc: lowercase letters, digits, `-`, `_`. */
  name: string;
  /** What the rack and the effect picker call it. */
  title: string;
  /** One line for the map of docs: when an agent should read the doc. */
  when: string;
  /** What it does and how its fields change the sound, in Markdown. Goes into its doc. */
  doc: string;
  /** The fields of its record. Field names are Hum names: `rate`, `tone_hz`. */
  state: S;
  /** Its sound in Hum, an effect from `in` to `out`. */
  sound: (fields: SoundOf<S>) => Hum;
  /** Its card. Without one it gets a knob per knob and a button per option. */
  card?: (card: Card<StateOf<S>>) => Node;
}

/** Makes a tool of the project: an effect that goes in a track's `effects`. */
export function tool<const S extends Fields>(spec: ToolSpec<S>): void {
  if (host.tools.has(spec.name)) {
    throw new Error(`a tool named ${spec.name} is already defined`);
  }
  host.tools.set(spec.name, spec as unknown as ToolSpec<Fields>);
}

// ---------------------------------------------------------------------------------------------
// Cards

/** A colour as `#rrggbb` or `#rrggbbaa`. */
export type Color = `#${string}`;

/** How a `div` lays out and looks. Numbers are points. */
export interface Style {
  /** How children line up. A div is a column unless this says `row`. */
  direction?: "row" | "column";
  gap?: number;
  padding?: number;
  paddingX?: number;
  paddingY?: number;
  width?: number;
  height?: number;
  /** Takes the room left in its parent. */
  grow?: boolean;
  /** Where children sit across the direction. */
  align?: "start" | "center" | "end";
  /** Where children sit along the direction. */
  justify?: "start" | "center" | "end" | "between";
  background?: Color;
  color?: Color;
  fontSize?: number;
  radius?: number;
  borderColor?: Color;
  borderWidth?: number;
}

/** The saved state of each built-in tool a card can be written for. */
export interface Tools {
  script: {
    name?: string;
    code: string[];
    values?: Record<string, number>;
  };
}

/** What a card gets each time it draws. */
export interface Card<State> {
  /** The record as it is now. */
  state: State;
  /**
   * Changes the record as one undo step called `label`. `change` gets a copy to edit. A record
   * the tool does not accept is refused, and the window says why.
   */
  update(label: string, change: (state: State) => void): void;
}

/** Gives the cards of a built-in tool this look. */
export function card<Tool extends keyof Tools>(
  tool: Tool,
  draw: (card: Card<Tools[Tool]>) => Node,
): void {
  host.cards.set(tool, draw as (card: Card<unknown>) => Node);
}

/**
 * A knob on a number of the record, at `path` such as `values.rate`. It turns the record
 * directly, so a drag is smooth and one undo step. A number the record leaves out shows at
 * `default`.
 */
export function Knob(props: {
  path: string;
  label: string;
  min: number;
  max: number;
  default: number;
  unit?: Unit;
}): Node {
  return { type: "knob", ...props };
}

export type Child = Node | string | number | boolean | null | undefined | Child[];

export type Node =
  | { type: "div"; style?: Style; onClick?: () => void; children: Child[] }
  | {
      type: "knob";
      path: string;
      label: string;
      min: number;
      max: number;
      default: number;
      unit?: Unit;
    };

interface DivProps {
  style?: Style;
  onClick?: () => void;
  children?: Child;
}

type Component<Props> = (props: Props) => Node;

/** Makes the nodes of JSX. `tsconfig.json` next to this file points JSX here. */
export function h<Props>(
  type: "div" | Component<Props>,
  props: (Props & DivProps) | null,
  ...children: Child[]
): Node {
  if (typeof type === "function") {
    return type({ ...(props as Props), children });
  }
  return { type, style: props?.style, onClick: props?.onClick, children };
}

export namespace h {
  export namespace JSX {
    export type Element = Node;
    export interface IntrinsicElements {
      div: DivProps;
    }
  }
}

/** `tone_hz` is `Tone hz`. */
export function words(name: string): string {
  const spaced = name.replaceAll("_", " ");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

const BUTTON: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };
const CHOSEN: Style = { ...BUTTON, background: "#7c3aed", color: "#ffffff" };

/** The card of a tool that has none: a knob per knob, a button per option and per toggle. */
export function defaultCard<S extends Fields>(fields: S) {
  return ({ state, update }: Card<StateOf<S>>): Node => {
    const knobs: Node[] = [];
    const rows: Node[] = [];
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "knob") {
        knobs.push(
          Knob({ path: name, label, min: field.min, max: field.max, default: field.default, unit: field.unit }),
        );
      } else if (field.kind === "toggle") {
        const on = (state[name] as boolean | undefined) ?? field.default;
        rows.push(
          h("div", {
            style: on ? CHOSEN : BUTTON,
            onClick: () => update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, (next) => {
              (next as Record<string, unknown>)[name] = !on;
            }),
          }, label),
        );
      } else {
        const chosen = (state[name] as string | number | undefined) ?? field.default;
        rows.push(
          h("div", { style: { direction: "row", gap: 4, align: "center" } },
            h("div", { style: { color: "#9ca3af" } }, label),
            field.options.map((option) =>
              h("div", {
                style: option === chosen ? CHOSEN : BUTTON,
                onClick: () => update(`Set ${label.toLowerCase()} to ${option}`, (next) => {
                  (next as Record<string, unknown>)[name] = option;
                }),
              }, String(option)),
            ),
          ),
        );
      }
    }
    return h("div", { style: { gap: 8 } }, h("div", { style: { direction: "row", gap: 8 } }, knobs), rows);
  };
}

/** What the host reads. Not for tools or cards. */
export const host = {
  tools: new Map<string, ToolSpec<Fields>>(),
  cards: new Map<string, (card: Card<unknown>) => Node>(),
};
