import { h, live, Live, type Drag, type Num, type Pointer } from "../sdk";
import { played, type Bound } from "./field";
import { fitted, liveTravelled, liveValueAt, shown, stepped, travelled, valueAt, type Range } from "./value";

/** What a pointer takes: more than the dot, for a trackpad. The dot, and the dot with a number in it. */
const [TARGET, DOT, NUMBERED] = [20, 10, 16];

/** A number a handle moves, bound as a knob is (`field.number(...)`), spread over the drawing by `travel`: its own range when left out. */
export type HandleAxis = Bound & { travel?: Range };

export type HandleProps = {
  id: string;
  /** What an agent knows it by, such as "Band 2". */
  label: string;
  /** The drawing it moves in, in points. */
  w: number;
  h: number;
  /** Sideways and up: a number it moves, or a place, 0 to 1 from the left or the bottom, where it stays. */
  x: HandleAxis | Num;
  y: HandleAxis | Num;
  /** A number or a letter in the dot. */
  number?: string;
  /** Of several, the one the knobs show: the others are hollow. */
  chosen?: boolean;
  /** At 40 %, for what does nothing now, such as a band that is off. It still drags. */
  dimmed?: boolean;
  /** What the foot of the display says while it is pointed at or dragged: only what no knob in view says. */
  caption?: string;
  onPress?: () => void;
  /** Both numbers at the end of a drag, as one step; left out, each axis changes its own. */
  onChange?: (x: number | undefined, y: number | undefined) => void;
};

const moves = (axis: HandleAxis | Num): axis is HandleAxis => typeof axis === "object" && "onChange" in axis;

/**
 * A dot at a place in a display that drags one number sideways and one up, or one alone, with the
 * gesture of a knob: the sound and the curve follow the hand, Shift is finer, a double click sets
 * the defaults, arrows nudge its first number, and the end of a drag is one step. A number a lane
 * holds does not drag; the handle still hears a press. It shows where its numbers play, so it
 * follows its knob, a lane and an undo.
 */
export const Handle = ({ id, label, w, h: tall, x, y, number, chosen = true, dimmed = false, caption, onPress, onChange }: HandleProps) => {
  const drag = live.drag(id);
  const [across, up] = [along(x, w, drag.dx), along(y, tall, drag.dy.negate())];
  const set = (nx: number | undefined, ny: number | undefined) => {
    if (onChange) return onChange(nx, ny);
    if (nx !== undefined && nx !== across.axis?.value) across.axis?.onChange(nx);
    if (ny !== undefined && ny !== up.axis?.value) up.axis?.onChange(ny);
  };
  const plays = Object.fromEntries([across, up].flatMap((each) => (each.plays ? [each.plays] : [])));
  const dragged = across.along || up.along
    ? ({ ...(across.along ? { x: across.along } : {}), ...(up.along ? { y: up.along.map((d) => -d).reverse() } : {}), ...(Object.keys(plays).length > 0 ? { plays } : {}) } as Drag)
    : undefined;
  const dragEnd = ({ dx, dy }: Pointer) => set(across.end?.(dx), up.end?.(-dy));
  // The keys and an agent move its first number that drags.
  const first = across.along ? across.axis : up.along ? up.axis : undefined;
  const to = (value: number) => (first === across.axis ? set(value, undefined) : set(undefined, value));
  const actions = first && {
    increment: (by: number) => to(stepped(first.value, by, first)),
    decrement: (by: number) => to(stepped(first.value, -by, first)),
    set: (value: number) => to(fitted(value, first)),
    reset: () => set(across.along ? across.axis?.initial : undefined, up.along ? up.axis?.initial : undefined),
  };
  const said = [across.axis, up.axis].flatMap((axis) => (axis ? [shown(axis.value, axis.unit)] : [])).join(" · ");
  const [left, top] = [Live.of(0).plus(across.place).times(w).minus(TARGET / 2), Live.of(1).minus(up.place).times(tall).minus(TARGET / 2)];
  const dot = number === undefined ? DOT : NUMBERED;
  return (
    <box key={id} label={label} role="slider" value={said} checked={chosen} at={[left, top]} w={TARGET} h={TARGET} radius="full" opacity={dimmed ? 0.4 : 1} hoverGroup
      {...(dragged ? { drag: dragged, onDragEnd: dragEnd } : {})} {...(actions ? { actions } : {})} {...(onPress ? { onPress } : {})}>
      <row at={[(TARGET - dot) / 2, (TARGET - dot) / 2]} w={dot} h={dot} align="center" justify="center" radius="full" fill={chosen ? "text" : "well"} border={chosen ? undefined : "text"}>
        {number !== undefined && <text style="label" color={chosen ? "text-inverse" : "text"}>{number}</text>}
      </row>
      {caption !== undefined && (
        <row reveal at={[Live.of(6).minus(left), Live.of(tall - 22).minus(top)]} h={18} px={1} align="center" radius="sm" fill="well">
          <text style="number" color="text-secondary">{caption}</text>
        </row>
      )}
    </box>
  );
};

/**
 * One way a handle goes, `size` points of drawing, `moved` points by the drag along it (up is
 * more): where it is, 0 to 1, and for a number it moves, its bounds, what it plays and where it ends.
 */
const along = (axis: HandleAxis | Num, size: number, moved: Live) => {
  if (!moves(axis)) return { place: axis };
  const travel = axis.travel ?? axis;
  const at = travelled(axis.value, travel);
  const now = moved.over(size).plus(at);
  // What plays: a drag's `plays`, a knob, a lane, an undo; the drag itself where nothing names the number.
  const place: Num = axis.held ? liveTravelled(axis.held, travel) : axis.path === undefined ? now.clamp(0, 1) : liveTravelled(live.field(played(axis.path)), travel);
  if (axis.held) return { axis, place };
  return {
    axis,
    place,
    along: [-at * size, (1 - at) * size] as [number, number],
    plays: axis.path === undefined ? undefined : ([axis.path, liveValueAt(now.clamp(0, 1), travel).clamp(axis.min, axis.max)] as const),
    end: (d: number) => (d === 0 ? axis.value : fitted(valueAt(at + d / size, travel), axis)),
  };
};
