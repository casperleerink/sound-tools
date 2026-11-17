import { h, type Child } from "../sdk";
import { corner, height, type Size } from "./control";
import { Tooltip } from "./tooltip";

/** `primary` is the one thing to do; `subtle` most buttons; `ghost` one that shows only its words or glyph. */
export type Variant = "primary" | "subtle" | "ghost";

const looks = {
  primary: { fill: "text", hover: "#0c0d1026", ink: "text-inverse" },
  subtle: { fill: "fill", hover: "fill", ink: "text" },
  ghost: { fill: undefined, hover: "fill", ink: "text-secondary" },
} as const;

/**
 * A button with words, a glyph, or both; with a glyph alone it is square. `label` says what it
 * does for an agent, `tip` for the composer. `children` go inside, such as what it opens.
 */
export const Button = ({ id, label, text, glyph, variant = "subtle", size = "md", tip, onPress, children }: { id: string; label: string; text?: string; glyph?: string; variant?: Variant; size?: Size; tip?: string; onPress?: () => void; children?: Child }) => {
  const look = looks[variant];
  const square = text === undefined;
  const words = size === "sm" ? "control" : "heading";
  return (
    <row key={id} label={label} role="button" h={height(size)} {...(square ? { w: height(size) } : { px: size === "sm" ? 2 : 3 })} gap={1} align="center" justify="center" radius={corner(size)} fill={look.fill} hover={look.hover} onPress={onPress}>
      {glyph !== undefined && <text style="control" color={look.ink}>{glyph}</text>}
      {text !== undefined && <text style={words} color={look.ink}>{text}</text>}
      {tip !== undefined && <Tooltip text={tip} />}
      {children}
    </row>
  );
};
