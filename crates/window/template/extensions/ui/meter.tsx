import { h, type LiveName } from "../sdk/view";

/** A level that moves every frame, filling a well from the bottom. Green: a meter is the one place colour fills an area. */
export const Meter = ({ id, label, live, w, h: height }: { id: string; label: string; live: LiveName; w: number; h: number }) => (
  <box key={`${id}-well`} label={label} role="meter" w={w} h={height} radius="sm" fill="well" clip>
    <box key={id} w={w} h={height} fill="green" level={live} />
  </box>
);
