// The SDK of the project's own tools. Sound Tools writes this file when the project opens, so
// an edit here is lost. Read `agent-docs/extensions.md` first, and `agent-docs/hum.md` for Hum.
//
// A tool is a `.ts` or `.tsx` file next to this one that calls `tool`: the fields of its
// record, the controls its card plays, its doc for agents, its sound in Hum and its card.

// ---------------------------------------------------------------------------------------------
// Fields of a record: saved, undoable, and what an agent writes.

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

/** A list of numbers, such as the steps of a sequence. It plays live, as a list in Hum. */
export interface PatternField {
  kind: "pattern";
  length: number;
  min: number;
  max: number;
  default: number;
  label?: string;
}

export type Field = KnobField | ToggleField | ChoiceField | PatternField;
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
export const pattern = (field: Omit<PatternField, "kind">): PatternField => ({
  kind: "pattern",
  ...field,
});

// ---------------------------------------------------------------------------------------------
// Controls: what the card plays and nothing saves, as a performer plays an instrument.

/** A number the card sets as it is played, such as an XY pad. Not saved; starts at `default`. */
export interface LiveControl {
  kind: "live";
  min: number;
  max: number;
  default: number;
  label?: string;
}

/** A bang: 1 in Hum for the one sample after the card fires it. */
export interface TriggerControl {
  kind: "trigger";
  label?: string;
}

export type Control = LiveControl | TriggerControl;
export type Controls = Record<string, Control>;

export const live = (control: Omit<LiveControl, "kind">): LiveControl => ({
  kind: "live",
  ...control,
});
export const trigger = (control: Omit<TriggerControl, "kind"> = {}): TriggerControl => ({
  kind: "trigger",
  ...control,
});

type ValueOf<F> = F extends KnobField
  ? number
  : F extends ToggleField
    ? boolean
    : F extends ChoiceField<infer Option>
      ? Option
      : F extends PatternField
        ? number[]
        : never;

/** The `state` of a record of a tool with these fields. A field left out is at its default. */
export type StateOf<S extends Fields> = { [Name in keyof S]?: ValueOf<S[Name]> };

/** What `sound` gets: a choice as its value, anything else as a `Param` to put in the Hum. */
export type SoundOf<S extends Fields, C extends Controls> = {
  [Name in keyof S]: S[Name] extends ChoiceField<infer Option> ? Option : Param;
} & { [Name in keyof C]: Param };

// ---------------------------------------------------------------------------------------------
// Hum

/**
 * A field or a control in Hum code. Put it in the code as `${rate}`, or `${steps}[i]` for a
 * pattern. It is no number, so code cannot branch on it: a turn must not run `sound` again.
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

export interface ToolSpec<S extends Fields, C extends Controls> {
  /** The `tool` of its records and the name of its doc: lowercase letters, digits, `-`, `_`. */
  name: string;
  /** What the card and the picker call it. */
  title: string;
  /** One line for the map of docs: when an agent should read the doc. */
  when: string;
  /** What it does and how its fields change the sound, in Markdown. Goes into its doc. */
  doc: string;
  /**
   * `effect` (the default) goes in a track's effects and reads `in`. `instrument` is what a
   * track plays: the code runs once per note. `source` is what a track plays too, one voice
   * that runs all the time and follows the newest note.
   */
  kind?: "effect" | "instrument" | "source";
  /** How many notes an instrument plays at once, 1 to 8. 8 when left out. */
  voices?: number;
  /** The fields of its record. Field names are Hum names: `rate`, `tone_hz`. */
  state: S;
  /** What its card plays and nothing saves. Names are Hum names too. */
  controls?: C;
  /** Its sound in Hum. */
  sound: (fields: SoundOf<S, C>) => Hum;
  /** Its card. Without one it gets a knob per knob and a button per option and toggle. */
  card?: (card: Card<StateOf<S>, C>) => Node;
}

/** Makes a tool of the project. */
export function tool<const S extends Fields, const C extends Controls = {}>(
  spec: ToolSpec<S, C>,
): void {
  if (host.tools.has(spec.name)) {
    throw new Error(`a tool named ${spec.name} is already defined`);
  }
  host.tools.set(spec.name, spec as unknown as ToolSpec<Fields, Controls>);
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

/** What a card gets each time it draws. */
export interface Card<State, C extends Controls = Controls> {
  /** The record as it is now. */
  state: State;
  /** The last value of each `watch` of the Hum, by name. The card draws again as they move. */
  watches: Record<string, number>;
  /**
   * Changes the record as one undo step called `label`. `change` gets a copy to edit. A record
   * the tool does not accept is refused, and the window says why.
   */
  update(label: string, change: (state: State) => void): void;
  /** Moves a live control. Not saved, no undo step. */
  set(control: keyof C & string, value: number): void;
  /** Fires a trigger. */
  fire(control: keyof C & string): void;
}

/**
 * A knob. With `path`, it turns the number at `path` in the record, such as `rate` or
 * `values.rate`: a drag is smooth and one undo step, and a number the record leaves out shows
 * at `default`. With `live`, it plays a live control instead, which nothing saves.
 */
export function Knob(props: {
  path?: string;
  live?: string;
  label: string;
  min: number;
  max: number;
  default: number;
  unit?: Unit;
}): Node {
  return { type: "knob", ...props };
}

/**
 * A row of steps on a pattern of the record: a click turns a step on (to the pattern's max)
 * or off (to its min). `playing` names a watch whose value is the step that plays, which
 * lights up.
 */
export function Steps(props: { path: string; max?: number; playing?: string }): Node {
  return { type: "steps", ...props };
}

/** A bar that shows a watch from 0 to 1, such as a level. */
export function Meter(props: { watch: string; label?: string }): Node {
  return { type: "meter", ...props };
}

/** A square to play with the pointer: across sets the live control `x`, up sets `y`. */
export function Pad(props: { x: string; y: string; size?: number }): Node {
  return { type: "pad", ...props };
}

export type Child = Node | string | number | boolean | null | undefined | Child[];

export type Node =
  | { type: "div"; style?: Style; onClick?: () => void; children: Child[] }
  | {
      type: "knob";
      path?: string;
      live?: string;
      label: string;
      min: number;
      max: number;
      default: number;
      unit?: Unit;
    }
  | { type: "steps"; path: string; max?: number; playing?: string }
  | { type: "meter"; watch: string; label?: string }
  | { type: "pad"; x: string; y: string; size?: number };

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

/**
 * The card of a tool that has none: a knob per knob and per live control, a row of steps per
 * pattern, a button per option, toggle and trigger.
 */
export function defaultCard(fields: Fields, controls: Controls = {}) {
  return ({ state, update, fire }: Card<Record<string, unknown>, Controls>): Node => {
    const knobs: Node[] = [];
    const rows: Node[] = [];
    const record = state as Record<string, unknown>;
    for (const [name, field] of Object.entries(fields)) {
      const label = field.label ?? words(name);
      if (field.kind === "knob") {
        knobs.push(
          Knob({ path: name, label, min: field.min, max: field.max, default: field.default, unit: field.unit }),
        );
      } else if (field.kind === "pattern") {
        rows.push(Steps({ path: name, max: field.max }));
      } else if (field.kind === "toggle") {
        const on = (record[name] as boolean | undefined) ?? field.default;
        rows.push(
          h("div", {
            style: on ? CHOSEN : BUTTON,
            onClick: () => update(`Turn ${label.toLowerCase()} ${on ? "off" : "on"}`, (next) => {
              next[name] = !on;
            }),
          }, label),
        );
      } else {
        const chosen = (record[name] as string | number | undefined) ?? field.default;
        rows.push(
          h("div", { style: { direction: "row", gap: 4, align: "center" } },
            h("div", { style: { color: "#9ca3af" } }, label),
            field.options.map((option) =>
              h("div", {
                style: option === chosen ? CHOSEN : BUTTON,
                onClick: () => update(`Set ${label.toLowerCase()} to ${option}`, (next) => {
                  next[name] = option;
                }),
              }, String(option)),
            ),
          ),
        );
      }
    }
    for (const [name, control] of Object.entries(controls)) {
      const label = control.label ?? words(name);
      if (control.kind === "live") {
        knobs.push(Knob({ live: name, label, min: control.min, max: control.max, default: control.default }));
      } else {
        rows.push(h("div", { style: BUTTON, onClick: () => fire(name) }, label));
      }
    }
    return h("div", { style: { gap: 8 } }, h("div", { style: { direction: "row", gap: 8 } }, knobs), rows);
  };
}

/** What the host reads. Not for tools. */
export const host = {
  tools: new Map<string, ToolSpec<Fields, Controls>>(),
};
