import { h, type Color } from "../sdk/view";

/** A small switch that says its name, such as M for mute: tinted with `color` while on, hollow while off. */
export const Toggle = ({ id, label, letter, on, color, onToggle }: { id: string; label: string; letter: string; on: boolean; color: Color; onToggle: () => void }) => (
  <row key={id} label={label} role="switch" checked={on} w={20} h={20} radius="sm" align="center" justify="center" fill={on ? `${color}/25` as Color : undefined} border={on ? color : "border-strong"} onPress={onToggle} actions={{ toggle: onToggle }}>
    <text style="label" color={on ? color : "text-muted"}>{letter}</text>
  </row>
);
