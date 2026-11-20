import { h, type Color, type IconName } from "../sdk";
import { corner, height, tint, type Size } from "./control";
import { Tooltip } from "./tooltip";

/**
 * A switch that says its name, such as M for mute or Freeze: hollow while off, tinted with
 * `color` while on. `color` is a meaning, such as `warning` for mute; `on` (white) when it has
 * none. Its face is `icon`, else `text`, else `label`; `dot` makes it a circle, as the arm of a track.
 */
export const Toggle = ({ id, label, text, icon, on, color = "on", dot = false, size = "sm", w, tip, keys, onToggle }: { id: string; label: string; text?: string; icon?: IconName; on: boolean; color?: Color; dot?: boolean; size?: Size; w?: number; tip?: string; keys?: string; onToggle: () => void }) => {
  const face = text ?? label;
  const tall = height(size);
  const width = dot || icon !== undefined || face.length === 1 ? tall : w;
  const ink: Color = on ? color : "text-secondary";
  return (
    <row key={id} label={label} role="switch" value={on} h={tall} {...(width === undefined ? { pad: [2, 0] as const } : { w: width })} align="center" justify="center" radius={corner(size)} fill={on ? tint(color) : undefined} border={on ? "#00000000" : "border-strong"} hover="fill" onPress={onToggle} actions={{ toggle: onToggle }}>
      {dot ? <box w={10} h={10} radius="full" fill={on ? color : undefined} border={on ? undefined : ink} /> : icon !== undefined ? <icon name={icon} size={14} color={ink} /> : <text style="label" color={ink} truncate>{face}</text>}
      {tip !== undefined && <Tooltip text={tip} keys={keys} />}
    </row>
  );
};
