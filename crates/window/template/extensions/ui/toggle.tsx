import { h, type Color, type IconName } from "../sdk";
import { corner, height, tint, type Size } from "./control";
import { Tooltip } from "./tooltip";

/**
 * A switch that says its name, such as M for mute or Freeze: hollow while off, tinted with
 * `color` while on. `color` is a meaning, such as `warning` for mute; `on` (white) when it has
 * none. Its face is `icon`, else `text`, else `label`; `dot` makes it a circle, as the arm of a track.
 */
export const Toggle = ({ id, label, text, icon, on, color = "on", dot = false, size = "sm", w, tip, onToggle }: { id: string; label: string; text?: string; icon?: IconName; on: boolean; color?: Color; dot?: boolean; size?: Size; w?: number; tip?: string; onToggle: () => void }) => {
  const face = text ?? label;
  const tall = height(size);
  const width = dot || icon !== undefined ? tall : face.length === 1 ? tall + 4 : w;
  const ink: Color = on ? color : "text-secondary";
  return (
    <row key={id} label={label} role="switch" checked={on} h={tall} {...(width === undefined ? { px: 2 } : { w: width })} align="center" justify="center" radius={corner(size)} fill={on ? tint(color) : undefined} border={on ? "#00000000" : "border-strong"} hover="fill" onPress={onToggle} actions={{ toggle: onToggle }}>
      {dot ? <box w={10} h={10} radius="full" fill={on ? color : undefined} border={on ? undefined : ink} /> : icon !== undefined ? <icon name={icon} size={14} color={ink} /> : <text style="label" color={ink} truncate>{face}</text>}
      {tip !== undefined && <Tooltip text={tip} />}
    </row>
  );
};
