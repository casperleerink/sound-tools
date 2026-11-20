import { h, type Child, type IconName } from "../sdk";
import { corner, height, type Size } from "./control";
import { Tooltip } from "./tooltip";

/** `primary` is the one thing to do; `subtle` most buttons; `ghost` one that shows only its words or icon. */
export type Variant = "primary" | "subtle" | "ghost";

const looks = {
  primary: { fill: "text", hover: "#0c0d1026", ink: "text-inverse" },
  subtle: { fill: "fill", hover: "fill", ink: "text" },
  ghost: { fill: undefined, hover: "fill", ink: "text-secondary" },
} as const;

/**
 * A button with words, an icon of `ui/icons/`, or both; with an icon alone it is square. `label`
 * says what it does for an agent, `tip` for the composer. `children` go inside, such as what it opens.
 */
export const Button = ({ id, label, text, icon, variant = "subtle", size = "md", tip, onPress, children }: { id: string; label: string; text?: string; icon?: IconName; variant?: Variant; size?: Size; tip?: string; onPress?: () => void; children?: Child }) => {
  const look = looks[variant];
  const square = text === undefined;
  const words = size === "sm" ? "label" : "heading";
  return (
    <row key={id} label={label} role="button" h={height(size)} {...(square ? { w: height(size) } : { px: size === "sm" ? 2 : 3 })} gap={1} align="center" justify="center" radius={corner(size)} fill={look.fill} hover={look.hover} onPress={onPress}>
      {icon !== undefined && <icon name={icon} size={square && size !== "lg" ? 14 : 16} color={look.ink} />}
      {text !== undefined && <text style={words} color={look.ink}>{text}</text>}
      {tip !== undefined && <Tooltip text={tip} />}
      {children}
    </row>
  );
};
