import { h, type Color } from "../sdk";

/** A small mark of a state, such as a track's colour or the agent at work. */
export const Dot = ({ color, size = 8 }: { color: Color; size?: number }) => <box w={size} h={size} radius="full" fill={color} />;
