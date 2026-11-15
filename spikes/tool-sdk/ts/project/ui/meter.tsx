import { type Color, type LiveName } from "../sdk";

/** A level that moves every frame, filling a well from the bottom. */
export const Meter = ({ id, live, w, h: height, color }: { id: string; live: LiveName; w: number; h: number; color: Color }) => (
  <box w={w} h={height} radius="sm" fill="well" clip>
    <box key={id} w={w} h={height} fill={color} level={live} />
  </box>
);
