import { defaultStates, h, live, type Card, type Drag, type FilledStateOfTool, type Node, type Pointer } from "../../../sdk";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Cell } from "../../cell";
import { Choice } from "../../choice";
import { Curve } from "../../curve";
import { Display, drawingOf, FREQUENCY_TICKS, OCTAVES } from "../../display";
import { binder, type Binder, type Bound, type PathsOf } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { Select } from "../../select";
import { Slider } from "../../slider";
import { Toggle } from "../../toggle";
import { Tooltip } from "../../tooltip";
import { fitted, liveValueAt, shown, travelled, valueAt, type Range } from "../../value";
import { DeviceCard, type Section } from "../device-card";

const CELL = theme.size.cell;
/** Where the `frames` feed puts the position up its drawing: 0 at the front a fifth up, 1 at the back two fifths higher, so a drag up the drawing moves it as far as its frame moves. */
const POSITION_TRAVEL: Range = { min: -0.2 / 0.4, max: 0.8 / 0.4 };
/** The `envelope_N` feed lays each time out over its knob's travel, the note held 0.36 between decay and release; the drawing shows a little past both ends, and levels to a little over full. */
const [HOLD, LEFT, RIGHT] = [0.36, -0.14, 3.43];
const LEVEL: [number, number] = [1.15, -0.1];
/** The response of a filter, as dB down the drawing, and where its handle puts the resonance: none on the 0 dB line, all of it near the top. */
const DB: [number, number] = [18, -36];
const RESONANCE_TRAVEL: Range = { min: -2.36, max: 1.17 };
const ENVELOPES = [["amp_env", "Amp"], ["env_2", "Env 2"], ["env_3", "Env 3"]] as const;
const LFOS = [["lfo_1", "LFO 1"], ["lfo_2", "LFO 2"]] as const;
const PAGES = ["Osc", "Voice", "Filter", "Env", "LFO", "Matrix"] as const;
type State = FilledStateOfTool<"wavetable">;
type Group = "osc_1" | "osc_2" | "sub" | "unison" | "filter_1" | "filter_2" | "routing" | "amp_env" | "env_2" | "env_3" | "lfo_1" | "lfo_2" | "voicing" | "matrix";

/** The travel of a time whose stage starts at `start` across the envelope drawing, as the feed lays it out: `time`'s own range, moved along. */
const stage = (time: Range, start: number): Range => {
  const ratio = time.max / time.min;
  const min = time.min * ratio ** (LEFT - start);
  return { min, max: min * ratio ** (RIGHT - LEFT), scale: "log" };
};

/** Whether `value` is `initial` where it says anything: a record leaves out what is its default, and keeps 0.4 where the default says 0.4000000059604645. */
const same = (value: unknown, initial: unknown): boolean => {
  if (value === undefined) return true;
  if (typeof value === "number" && typeof initial === "number") return Math.abs(value - initial) <= 1e-6 * Math.max(1, Math.abs(initial));
  if (Array.isArray(value) || Array.isArray(initial)) return Array.isArray(value) && Array.isArray(initial) && value.length === initial.length && value.every((each, at) => same(each, initial[at]));
  if (typeof value === "object" && value !== null && typeof initial === "object" && initial !== null) return Object.entries(value).every(([key, each]) => same(each, (initial as Record<string, unknown>)[key]));
  return value === initial;
};

/** Whether what a page shows differs from a new Wavetable's: what the card shows folded is on no page. */
const differs = (page: (typeof PAGES)[number], state: State, initial: State) => {
  const all = (...groups: Group[]) => groups.every((group) => same(state[group], initial[group]));
  const but = (group: Group, ...shown: string[]) => same({ ...(state[group] as object), ...Object.fromEntries(shown.map((key) => [key, undefined])) }, initial[group]);
  switch (page) {
    case "Osc": return !all("osc_2") || !but("osc_1", "table", "position");
    case "Voice": return !all("sub", "unison", "voicing");
    case "Filter": return !all("filter_2", "routing") || !but("filter_1", "cutoff_hz", "resonance");
    case "Env": return !all("amp_env", "env_2", "env_3");
    case "LFO": return !all("lfo_1", "lfo_2");
    case "Matrix": return !all("matrix");
  }
};

/** Columns of two cells, as given: tops first, then bottoms, as the card lays one width out. */
const stacked = (columns: [Node, Node?][]): Node[] => [...columns.map(([top]) => top), ...columns.flatMap(([, bottom]) => (bottom ? [bottom] : []))];

/**
 * The Wavetable: folded, the table of the first oscillator, whose position a drag up and down on
 * it moves, with the position, the gain and the first filter's cutoff and resonance. Behind More,
 * one page at a time: Osc, Voice, Filter, Env, LFO and Matrix, a dot on each that differs from a
 * new one. Every value has one control: those the folded card shows are on no page.
 */
export const card: Card<"wavetable", { expanded: boolean; lfo: number; page: number; envelope: number }> = (self, ctx) => {
  const field = binder(self, ctx);
  const { memory, update } = ctx;
  const { state } = self;
  const expanded = memory.expanded ?? false;
  const number = (path: PathsOf<State, number>, text: string, object = "") => ({ ...field.number(path, object ? `${object} ${text.toLowerCase()}` : text), text });
  const choice = (path: PathsOf<State, string | number>, text: string, object = "") => field.choice(path, object ? `${object} ${text.toLowerCase()}` : text);
  const toggle = (path: PathsOf<State, boolean>, text: string, what: string, span = 1) => {
    const bound = field.toggle(path, what);
    return <Cell span={span}><Toggle {...bound} text={text} w={span * CELL - 8} /></Cell>;
  };

  /** The table of oscillator `n` with its frames, the bright one where it plays; a drag up and down anywhere on it moves its position. */
  const oscillator = (n: 1 | 2) => {
    const { w, h: tall } = drawingOf(4, true);
    const osc = `osc_${n}` as const;
    const position = number(`${osc}.position`, "Position", `Osc ${n}`);
    const tables = choice(`${osc}.table`, "Table", `Osc ${n}`);
    const names = new Map((self.facts.tables ?? []).map((table) => [table.value as string | number, table.name]));
    const options = tables.options.map((option) => ({ ...option, text: names.get(option.value) ?? option.text }));
    const suffix = n === 1 ? "" : "_2";
    return (
      <Display strip={<Select {...tables} options={options} w={140} menuW={200} />}>
        <shapes at={[0, 0]} w={w} h={tall} units={[[0, 1], [1, 0]]} rects={[{ color: "text/24", rects: live.feed(`frames${suffix}`) }, { color: "text", rects: live.feed(`frame${suffix}`) }]} />
        <Area axis={position} w={w} h={tall} />
      </Display>
    );
  };

  /** The rest of an oscillator: on, level, the tuning, the pan, and the effect and its amount. */
  const voices = (n: 1 | 2): Node[] => {
    const [osc, name] = [`osc_${n}` as const, `Osc ${n}`];
    return [
      ...stacked([
        [toggle(`${osc}.on`, name, name), <Knob {...number(`${osc}.gain`, "Level", name)} unit="percent" />],
        [<Knob {...number(`${osc}.octave`, "Octave", name)} unit="octaves" step={1} bipolar />, <Knob {...number(`${osc}.semitone`, "Semitone", name)} unit="semitones" step={1} bipolar />],
        [<Knob {...number(`${osc}.detune_cents`, "Detune", name)} unit="cents" bipolar />, <Knob {...number(`${osc}.pan`, "Pan", name)} unit="pan" bipolar />],
      ]),
      <col w={2 * CELL} gap={1}>
        <Cell span={2} text="Effect"><Choice {...choice(`${osc}.effect`, "Effect", name)} w={2 * CELL - 8} /></Cell>
        <Knob {...number(`${osc}.effect_amount`, "Amount", name)} unit="percent" />
      </col>,
    ];
  };

  /** A filter's switch, drive and slope over its type and character. */
  const filter = (n: 1 | 2): Node => {
    const [path, name] = [`filter_${n}` as const, `Filter ${n}`];
    return (
      <col w={4 * CELL} gap={1}>
        <row>{toggle(`${path}.on`, name, name)}<Knob {...number(`${path}.drive_db`, "Drive", name)} unit="db" /><Cell span={2} text="Slope"><Choice {...choice(`${path}.slope`, "Slope", name)} /></Cell></row>
        <row><Cell span={2} text="Type"><Choice {...choice(`${path}.type`, "Type", name)} w={2 * CELL - 8} /></Cell><Cell span={2}><Choice {...choice(`${path}.character`, "Character", name)} /></Cell></row>
      </col>
    );
  };
  const filterHandle = (n: 1 | 2, w: number, tall: number) => {
    const [path, name] = [`filter_${n}` as const, `Filter ${n}`];
    const [cutoff, resonance] = [number(`${path}.cutoff_hz`, "Cutoff", name), number(`${path}.resonance`, "Resonance", name)];
    return (
      <Handle id={`${path}-handle`} label={`${field.title} ${name} cutoff and resonance`} w={w} h={tall} number={String(n)} chosen={n === 1} dimmed={!(state[path as Group] as { on: boolean }).on}
        x={cutoff} y={{ ...resonance, travel: RESONANCE_TRAVEL }}
        onChange={(hz, amount) => field.change(`Change ${name} cutoff and resonance`, { [`${path}.cutoff_hz`]: hz, [`${path}.resonance`]: amount })} />
    );
  };

  const pages = (): Section[] => {
    const { w, h: tall } = drawingOf(4, true);
    const [envelope, envelopeName] = ENVELOPES[(memory.envelope ?? 0)] ?? ENVELOPES[0];
    const [lfo, lfoName] = LFOS[memory.lfo ?? 0] ?? LFOS[0];
    const [attack, decay, sustain, release] = [number(`${envelope}.attack_seconds`, "Attack", envelopeName), number(`${envelope}.decay_seconds`, "Decay", envelopeName), number(`${envelope}.sustain`, "Sustain", envelopeName), number(`${envelope}.release_seconds`, "Release", envelopeName)];
    const peak = travelled(attack.value, attack);
    const held = peak + travelled(decay.value, decay) + HOLD;
    const top = (1 - LEVEL[1]) / (LEVEL[0] - LEVEL[1]);
    const bottom = -LEVEL[1] / (LEVEL[0] - LEVEL[1]);
    const lfoState = state[lfo] as { sync: boolean };
    const mono = state.voicing.mode === "mono";
    const glide = number("voicing.glide_seconds", "Glide", "");
    const initial = defaultStates.wavetable as State;
    const page = (name: (typeof PAGES)[number], cells: Node[]): Section => ({ name, cells, changed: differs(name, state, initial) });
    return [
      page("Osc", [...voices(1), oscillator(2), <Knob {...number("osc_2.position", "Position", "Osc 2")} unit="percent" />, ...voices(2)]),
      page("Voice", [
        ...stacked([
          [<Knob {...number("sub.gain", "Sub", "")} unit="percent" />, <Cell text="Octave"><Choice {...choice("sub.octave", "Octave", "Sub")} /></Cell>],
          [<Knob {...number("unison.voices", "Unison", "")} step={1} />, <Knob {...number("unison.amount", "Spread", "Unison")} unit="percent" />],
          [<Cell><Toggle id="voicing.mode" label={`${field.title} Mono`} text="Mono" on={mono} onToggle={() => field.change(`Turn mono ${mono ? "off" : "on"}`, { "voicing.mode": mono ? "poly" : "mono" })} /></Cell>, <Knob {...glide} unit="seconds" said={glide.value === 0 ? "Off" : undefined} />],
          // Mono plays one voice, so its number does nothing then.
          ...(mono ? [] : [[<Knob {...number("voicing.polyphony", "Voices", "")} step={1} />] as [Node]]),
        ]),
      ]),
      page("Filter", [
        <Display strip={<Choice {...choice("routing", "Routing")} />} across={FREQUENCY_TICKS} zero={DB[0] / (DB[0] - DB[1])}>
          <Curve heights={live.feed("response_2")} w={w} h={tall} across={OCTAVES} range={DB} filled={false} color="text-muted" />
          <Curve heights={live.feed("response_1")} w={w} h={tall} across={OCTAVES} range={DB} />
          {filterHandle(2, w, tall)}
          {filterHandle(1, w, tall)}
        </Display>,
        filter(1),
        ...stacked([[<Knob {...number("filter_2.cutoff_hz", "Cutoff", "Filter 2")} unit="hz" />, <Knob {...number("filter_2.resonance", "Resonance", "Filter 2")} unit="percent" />]]),
        filter(2),
      ]),
      page("Env", [
        <Display strip={<Choice id="envelope-shown" label={`${field.title} envelope shown`} options={ENVELOPES.map(([, text], value) => ({ value, text }))} value={(memory.envelope ?? 0)} onChange={(to) => (memory.envelope = Number(to))} />}>
          <Curve heights={live.feed(`envelope_${(memory.envelope ?? 0) + 1}` as "envelope_1")} w={w} h={tall} across={[LEFT, RIGHT]} range={LEVEL} />
          <Handle id="attack" label={`${field.title} ${envelopeName} attack point`} w={w} h={tall} x={{ ...attack, travel: stage(attack, 0) }} y={top} />
          <Handle id="decay" label={`${field.title} ${envelopeName} decay and sustain`} w={w} h={tall} x={{ ...decay, travel: stage(decay, peak) }} y={{ ...sustain, travel: { min: LEVEL[1], max: LEVEL[0] } }}
            onChange={(seconds, level) => field.change(`Change ${envelopeName} decay and sustain`, { [`${envelope}.decay_seconds`]: seconds, [`${envelope}.sustain`]: level })} />
          <Handle id="release" label={`${field.title} ${envelopeName} release point`} w={w} h={tall} x={{ ...release, travel: stage(release, held) }} y={bottom} />
        </Display>,
        ...stacked([
          [<Knob {...attack} unit="seconds" />, <Knob {...number(`${envelope}.attack_curve`, "Curve", `${envelopeName} attack`)} unit="percent" />],
          [<Knob {...decay} unit="seconds" />, <Knob {...number(`${envelope}.decay_curve`, "Curve", `${envelopeName} decay`)} unit="percent" />],
          [<Knob {...sustain} unit="percent" />],
          [<Knob {...release} unit="seconds" />, <Knob {...number(`${envelope}.release_curve`, "Curve", `${envelopeName} release`)} unit="percent" />],
        ]),
      ]),
      page("LFO", [
        <Display strip={[<Choice id="lfo-shown" label={`${field.title} LFO shown`} options={LFOS.map(([, text], value) => ({ value, text }))} value={memory.lfo ?? 0} onChange={(to) => (memory.lfo = to)} />, <Select {...choice(`${lfo}.shape`, "Shape", lfoName)} w={110} />]} zero={0.5}>
          <Curve heights={live.feed(lfo)} w={w} h={tall} across={[0, 2]} range={[1.3, -1.3]} filled={false} />
        </Display>,
        <col w={2 * CELL} gap={1}>{toggle(`${lfo}.sync`, "Sync", `${lfoName} sync`, 2)}{toggle(`${lfo}.retrigger`, "Retrigger", `${lfoName} retrigger`, 2)}</col>,
        // In time, the rate is a note of the tempo and its feel; free, a number of Hz.
        lfoState.sync
          ? <col w={2 * CELL} gap={1}><Cell span={2} text="Rate"><Choice {...choice(`${lfo}.division`, "Division", lfoName)} w={2 * CELL - 8} /></Cell><Cell span={2} text="Feel"><Choice {...choice(`${lfo}.feel`, "Feel", lfoName)} w={2 * CELL - 8} /></Cell></col>
          : <Knob {...number(`${lfo}.rate_hz`, "Rate", lfoName)} unit="hz" />,
      ]),
      page("Matrix", [<Matrix field={field} routes={state.matrix as Route[]} most={matrixMost(self.facts.fields)} update={update} />]),
    ];
  };

  return (
    <DeviceCard id="card" title={field.title} display={oscillator(1)}
      cells={stacked([
        [<Knob {...number("osc_1.position", "Position", "Osc 1")} unit="percent" />, <Knob {...number("gain", "Gain")} unit="percent" />],
        [<Knob {...number("filter_1.cutoff_hz", "Cutoff", "Filter 1")} unit="hz" />, <Knob {...number("filter_1.resonance", "Resonance", "Filter 1")} unit="percent" />],
      ])}
      sections={pages()} page={memory.page ?? 0} onPage={(page) => (memory.page = page)}
      expanded={expanded} onExpand={() => (memory.expanded = !expanded)} />
  );
};

/** A drag up and down anywhere on a drawing that moves `axis` as far as the drawing lays it out: the sound follows, its end is one step. */
const Area = ({ axis, w, h: tall }: { axis: Bound; w: number; h: number }) => {
  const at = travelled(axis.value, POSITION_TRAVEL);
  const id = `${axis.id}-area`;
  const moved = live.drag(id).dy.negate().over(tall).plus(at);
  const drag: Drag = { y: [-(1 - at) * tall, at * tall], ...(axis.path && !axis.held ? { plays: { [axis.path]: liveValueAt(moved, POSITION_TRAVEL).clamp(axis.min, axis.max) } } : {}) };
  const set = (value: number) => axis.onChange(fitted(value, axis));
  return (
    <box key={id} label={`${axis.label} on the table`} role="slider" value={axis.held ?? axis.value} at={[0, 0]} w={w} h={tall}
      {...(axis.held ? {} : { drag, onDragEnd: ({ dy }: Pointer) => set(valueAt(at - dy / tall, POSITION_TRAVEL)) })}
      actions={{ set, increment: (by: number) => set(axis.value + by * (axis.max - axis.min) / 50), decrement: (by: number) => set(axis.value - by * (axis.max - axis.min) / 50), ...(axis.initial === undefined ? {} : { reset: () => set(axis.initial!) }) }}>
      <Tooltip text="Drag up or down to move through the table" />
    </box>
  );
};

type Route = { source: string; destination: string; amount: number };
/** The most routes a matrix holds, as its record says. */
const matrixMost = (fields: readonly { name: string; kind: string; maxItems?: number | null }[]) => fields.find((each) => each.name === "matrix")?.maxItems ?? 8;

/** One row per route: its source, its destination and its amount, and a way to take it out; then a way to add one while there is room. A longer list scrolls. */
const Matrix = ({ field, routes, most, update }: { field: Binder<State>; routes: readonly Route[]; most: number; update: (label: string, change: (draft: { state: object }) => void) => void }) => {
  const matrix = (draft: { state: object }) => (draft.state as { matrix: Route[] }).matrix;
  return (
    <col key="routes" label={`${field.title} routes`} w={360} h={2 * theme.size.row} gap={1} scroll="y">
      {routes.map((route, at) => {
        const amount = field.number(`matrix[${at}].amount`, `Route ${at + 1} amount`);
        return (
          <row h={24} gap={1} align="center">
            <Select {...field.choice(`matrix[${at}].source`, `Route ${at + 1} source`)} w={96} menuW={200} />
            <icon name="arrow-right" size={12} color="text-muted" />
            <Select {...field.choice(`matrix[${at}].destination`, `Route ${at + 1} destination`)} w={128} menuW={200} />
            <Slider {...amount} w={64} />
            <text style="number" color="text" w={36} align="end">{`${route.amount > 0 ? "+" : ""}${shown(route.amount, "percent")}`}</text>
            <Button id={`route-remove-${at}`} label={`${field.title} remove route ${at + 1}`} icon="x" variant="ghost" size="sm" tip="Remove route" onPress={() => update("Remove route", (draft) => void matrix(draft).splice(at, 1))} />
          </row>
        );
      })}
      {routes.length < most && (
        <row><Button id="route-add" label={`${field.title} add route`} text="Add route" icon="plus" size="sm" onPress={() => update("Add route", (draft) => void matrix(draft).push({ source: "lfo_1", destination: "osc_1_position", amount: 0 }))} /></row>
      )}
    </col>
  );
};
