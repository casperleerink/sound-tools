// Things in time on a plane: its grid, its playhead and its clips. A page of its own draws a
// timeline with these, as the song, the note editor and the lanes do.
import { h, live, memo, type Child, type Color, type Drag, type Num, type Pointer } from "../../sdk";
import { MenuList, type Item } from "../menu";

/** How far a clip sits in from the top and the bottom of its row, in points. */
export const INSET = 4;

/** The ticks from 0 to `end`, `step` apart. */
const every = (step: number, end: number) => Array.from({ length: Math.ceil(end / step) }, (_, at) => at * step);

type GridProps = {
  plane: string;
  /** How far it reaches across and down, in the plane's units. */
  end: number;
  tall: number;
  /** A beat and a bar in units: 960 and 4 beats when left out. */
  beat?: number;
  bar?: number;
  /** Where the bars and the beats between them are, when they are not evenly apart. */
  bars?: readonly number[];
  beats?: readonly number[];
};

/** The lines of time down the plane `plane`: a line at each bar, fewer as it zooms out, and at each beat zoomed in. The window thins them; no view runs. */
export const Grid = ({ plane, end, tall, beat = 960, bar = 4 * beat, bars = every(bar, end), beats = every(beat, end).filter((tick) => tick % bar !== 0) }: GridProps) =>
  memo(`grid-${plane}`, [end, tall, bar, beat, bars, beats], () => {
    const { zoom } = live.plane(plane);
    // Bars an odd multiple of 2^k apart show while they are at least 24 points apart.
    const levels = [0, 1, 2, 3, 4].map((level) => ({
      shown: level === 4 ? 1 : zoom.ge(24 / (bar * 2 ** level)),
      ticks: bars.filter((_, at) => (level === 4 ? at % 16 === 0 : at % 2 ** (level + 1) === 2 ** level)),
    }));
    const lines = (ticks: readonly number[]) => ticks.flatMap((tick) => [tick, 0, tick, tall]);
    return (
      <box>
        <shapes place={[0, 0, end, tall]} opacity={zoom.ge(12 / beat).times(0.5)} lines={[{ color: "border", width: 1, points: lines(beats) }]} />
        {levels.map(({ shown, ticks }) => <shapes place={[0, 0, end, tall]} opacity={shown} lines={[{ color: "border", width: 1, points: lines(ticks) }]} />)}
      </box>
    );
  });

/** Taller than any window, in points: a line down what a plane shows, which adds nothing to what it spans. */
const TALLEST = 4000;

/** A line down all the plane `plane` shows, where `at` is across: the playhead when left out. Last inside the plane, over what is placed. */
export const Playhead = ({ plane, at = live.playhead, opacity, color = "text" }: { plane: string; at?: Num; opacity?: Num; color?: Color }) => (
  <box place={[at, live.plane(plane).top, 0, 0]} inset={[0, 0, -1, -TALLEST]} fill={color} opacity={opacity} inert />
);

type ClipProps = {
  id: string;
  /** What it is called, such as "Clip Verse". */
  label: string;
  /** Its start, row, length and rows in the plane's units: it sits a little in from its row's edges. */
  place: [x: Num, y: Num, w: Num, h: Num];
  chosen?: boolean;
  /** How it moves: along which axes, within what, on which `snap`. Its move is `live.drag(id)`, or of `drag.id`. */
  drag?: Drag;
  onPress?: (p: Pointer) => void;
  /** Where a drag left it, or the arrow keys: `dx` a step of the snap (a beat when it has none) a press. */
  onMove?: (p: Pointer) => void;
  /** Whether the arrow keys move it: they may be the page's. */
  keys?: boolean;
  /** What a right press on it offers. */
  menu?: readonly Item[];
  opacity?: Num;
  children?: Child;
};

/** A clip in time: placed, chosen with a press, moved with a drag on its snap or the arrow keys, its menu on a right press. */
export const Clip = ({ id, label, place, chosen = false, drag, onPress, onMove, keys = true, menu, opacity, children }: ClipProps) => {
  const step = (by: number) => {
    const [low, high] = drag?.x === undefined || drag.x === true ? [null, null] : drag.x;
    return { dx: Math.min(Math.max(by * (drag?.snap?.[0] || 960), low ?? -Infinity), high ?? Infinity) };
  };
  const arrows = keys && onMove ? { increment: (steps: number) => onMove(pointer(step(steps))), decrement: (steps: number) => onMove(pointer(step(-steps))) } : undefined;
  return (
    <box key={id} label={label} role="button" value={chosen} place={place} inset={[0, INSET, 0, INSET]} radius="md" fill="fill" hover="fill" border={chosen ? "text" : "border-strong"} opacity={opacity} clip
      drag={drag} onPress={onPress} onDragEnd={onMove} actions={arrows}>
      {children}
      {menu && <MenuList id={`menu-${id}`} label={`Menu of ${label}`} items={menu} open="menu" w={260} />}
    </box>
  );
};
const pointer = (p: Partial<Pointer>): Pointer => ({ x: 0, y: 0, dx: 0, dy: 0, clicks: 1, right: false, shift: false, alt: false, cmd: false, ...p });
