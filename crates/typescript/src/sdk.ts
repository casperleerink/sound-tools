// The SDK of cards written in TypeScript. Sound Tools writes this file when the project opens,
// so an edit here is lost. A card is a `.tsx` file next to it:
//
//   import { card, h, Knob } from "./sdk";
//
//   card("script", ({ state, update }) => (
//     <div style={{ direction: "row", gap: 8 }}>
//       <Knob path="values.rate" label="Rate" min={0.1} max={20} default={4} />
//       <div onClick={() => update("Reset rate", (state) => { delete state.values?.rate; })}>
//         Reset
//       </div>
//     </div>
//   ));
//
// The card draws again whenever its record changes, from the composer, an agent or undo. It
// keeps no state of its own: what it shows is the record. Saving a file reloads every card.

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

/** The saved state of each tool a card can be written for, as in its `state/*.json` files. */
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

/** Gives the cards of `tool` this look, in place of its built-in card. */
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
}): Node {
  return { type: "knob", ...props };
}

export type Child = Node | string | number | boolean | null | undefined | Child[];

export type Node =
  | { type: "div"; style?: Style; onClick?: () => void; children: Child[] }
  | { type: "knob"; path: string; label: string; min: number; max: number; default: number };

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

/** What the host reads. Not for cards. */
export const host = {
  cards: new Map<string, (card: Card<unknown>) => Node>(),
};
