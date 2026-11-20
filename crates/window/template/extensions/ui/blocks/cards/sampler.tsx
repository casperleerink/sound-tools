import { h, live, Live, type Card, type Child, type Dropped, type Node, type Num } from "../../../sdk";
import theme from "../../../theme.json";
import { Button } from "../../button";
import { Cell } from "../../cell";
import { Display, drawingOf } from "../../display";
import { binder, type Bound } from "../../field";
import { Handle } from "../../handle";
import { Knob } from "../../knob";
import { MenuList, type Group, type Item } from "../../menu";
import { Toggle } from "../../toggle";
import { shown } from "../../value";
import { Waveform } from "../../waveform";
import { DeviceCard } from "../device-card";

/** The shortest part of a file a note plays; where full level is up the display, so its handles can be taken; where the trim handles sit. */
const [SHORTEST, TOP, LOW] = [0.01, 0.88, 0.06];
const CELL = theme.size.cell;

/** A download's size as the library lists them. */
const megabytes = (bytes: number) => `${Math.round(bytes / 1e6)} MB`;

/**
 * The sampler: its whole file across the display, the start and end lines, the loop start and
 * what repeats while Loop is on, the envelope over it in the time of the file and a green line
 * where the last note is; the instrument, root, velocity, release and gain; behind More the
 * numbers the handles move, the loop, reverse, the envelope and tune. A file dropped on the
 * display or chosen becomes the sample. A library or SFZ instrument has its own pitch and
 * envelope: the card has only its gain.
 */
export const card: Card<"sampler", { expanded: boolean }> = (self, ctx) => {
  const field = binder(self, ctx);
  const { memory, update } = ctx;
  const { state, facts } = self;
  const expanded = memory.expanded ?? false;
  const { w, h: tall } = drawingOf(6);
  const file = state.sample ? (facts.file ?? { name: state.sample, state: "unknown" as const }) : null;
  const length = file?.state === "plays" ? (file.seconds ?? 0) : 0;
  const plays = length > 0;
  const start = state.start_seconds;
  const end = state.end_seconds ?? (plays ? length : 1);
  const loopStart = state.loop_start_seconds ?? start;

  /** A file brought into the project plays from its start to its end: the old trims mean nothing in it. */
  const load = (name: string) =>
    update("Load sample", (draft) => {
      const into = draft.state as Record<string, unknown>;
      if (into.sample === name) return;
      for (const gone of ["sfz", "library", "end_seconds", "loop_start_seconds"]) delete into[gone];
      Object.assign(into, { sample: name, start_seconds: 0 });
    });
  const chooseFile = async () => {
    const chosen = await ctx.openFile({ title: "Choose a sound for the sampler" });
    if (chosen !== null) load(chosen);
  };
  const drop = (dropped: Dropped) => dropped.audio[0] !== undefined && load(dropped.audio[0]);

  // The line that moved stays inside the others: a start before the end, which pushes the loop
  // start along; an end after the start, none at the end of the file; a loop start between them.
  // So every record the card writes plays.
  const trims = (moved: "start" | "end" | "loop", to: number) =>
    update(`Change ${moved === "loop" ? "loop start" : moved}`, (draft) => {
      const into = draft.state as unknown as Record<string, number | undefined>;
      let [from, until, loop] = [into.start_seconds ?? 0, into.end_seconds ?? length, into.loop_start_seconds];
      if (moved === "start") from = Math.min(Math.max(to, 0), Math.max(until - SHORTEST, 0));
      if (moved === "end") until = Math.min(Math.max(to, from + SHORTEST), length);
      if (moved === "loop") loop = to;
      if (loop !== undefined) loop = Math.min(Math.max(loop, from), Math.max(until - SHORTEST, from));
      into.start_seconds = from;
      if (until >= length - 1e-4) delete into.end_seconds;
      else into.end_seconds = until;
      if (loop !== undefined) into.loop_start_seconds = loop;
    });
  /** A number of seconds of the file, from 0 to its length, which the trims write. */
  const seconds = (id: string, text: string, value: number, initial: number, set: (to: number) => void, path?: string): Bound & { text: string } => ({
    id, label: `${field.title} ${text}`, text, value, min: 0, max: plays ? length : 1, unit: "seconds", initial, onChange: set, ...(path ? { path } : {}),
  });
  const startAt = seconds("start_seconds", "Start", start, 0, (to) => trims("start", to), "start_seconds");
  // A record without an end or a loop start plays to the end, or loops from the start: the line follows the drag alone.
  const endAt = seconds("end_seconds", "End", end, length, (to) => trims("end", to), state.end_seconds === undefined || state.end_seconds === null ? undefined : "end_seconds");
  const loopAt = seconds("loop_start_seconds", "Loop start", loopStart, start, (to) => trims("loop", to), state.loop_start_seconds === undefined || state.loop_start_seconds === null ? undefined : "loop_start_seconds");
  const [attack, decay, sustain] = [field.number("attack_seconds", "Attack"), field.number("decay_seconds", "Decay"), field.number("sustain", "Sustain")];
  const say = (text: string) => (expanded ? undefined : text);

  /** Where `seconds` (a number of the record, or what plays of it) is across the drawing, in points. */
  const across = (at: Bound): Num => (at.path ? live.field(at.path) : Live.of(at.value)).over(length).times(w).clamp(0, w);
  const line = (x: Num, color: "text-secondary" | "sound") => <box at={[x, 0]} w={1} h={tall} fill={color} inert />;
  const [startX, endX, loopX] = [across(startAt), across(endAt), across(loopAt)];
  const position = live.feed("position");
  const drawing = plays && (
    <box w={w} h={tall}>
      <Waveform wave={live.feed("wave")} seconds={[0, length]} file={file} w={w} h={tall} />
      <box at={[0, 0]} w={startX} h={tall} fill="background/60" inert />
      <box at={[endX, 0]} w={Live.of(w).minus(endX).max(0)} h={tall} fill="background/60" inert />
      {state.loop && <box at={[loopX, 0]} w={Live.of(endX).minus(loopX).max(0)} h={tall} fill="text/8" inert />}
      {line(startX, "text-secondary")}
      {line(endX, "text-secondary")}
      {state.loop && line(loopX, "text-secondary")}
      <shapes at={[0, 0]} w={w} h={tall} units={[[0, length], [1 / TOP, 0]]} lines={[{ color: "text", width: 1.5, points: live.feed("envelope") }]} />
      <box at={[position.over(length).times(w), 0]} w={1} h={tall} fill="sound" opacity={position.gt(0)} inert />
      <Handle id="start" label={`${field.title} start line`} w={w} h={tall} x={startAt} y={LOW} chosen={false} caption={say(`Start ${shown(start, "seconds")}`)} />
      <Handle id="end" label={`${field.title} end line`} w={w} h={tall} x={endAt} y={LOW} chosen={false} caption={say(`End ${shown(end, "seconds")}`)} />
      {state.loop && <Handle id="loop" label={`${field.title} loop start line`} w={w} h={tall} x={loopAt} y={1 - LOW} chosen={false} caption={say(`Loop start ${shown(loopStart, "seconds")}`)} />}
      <Handle id="attack" label={`${field.title} attack point`} w={w} h={tall} x={{ ...attack, travel: { min: -start, max: length - start } }} y={TOP} caption={say(`Attack ${shown(attack.value, "seconds")}`)} />
      <Handle id="decay" label={`${field.title} decay and sustain`} w={w} h={tall}
        x={{ ...decay, travel: { min: -(start + attack.value), max: length - start - attack.value } }} y={{ ...sustain, travel: { min: 0, max: 1 / TOP } }}
        caption={say(`Decay ${shown(decay.value, "seconds")} · Sustain ${shown(sustain.value, "percent")}`)}
        onChange={(s, level) => field.change("Change decay and sustain", { decay_seconds: s, sustain: level })} />
    </box>
  );

  const instrument = facts.instrument;
  const words: [string, string?] | undefined = instrument
    ? instrument.state === "downloading" ? [`Downloading ${instrument.name} · ${megabytes(instrument.downloaded)} of ${megabytes(instrument.download)}`]
      : instrument.state === "failed" ? [`The download of ${instrument.name} failed`, instrument.error ?? undefined]
        : instrument.state === "here" ? [`${instrument.name} · ${[instrument.license, instrument.attribution].filter(Boolean).join(" · ")}`]
          : [`${instrument.name} is not downloaded on this computer`, instrument.license]
    : state.sfz ? [`${facts.sfz ?? state.sfz} · SFZ instrument`]
      : !plays && file?.state === "unknown" ? [""] : undefined;
  const chooser = <Button id="choose-file" label={`${field.title} choose file`} text="Choose file" size="sm" onPress={() => void chooseFile()} />;
  const says = (lines: [string, string?], button?: Child) => (
    <col at={[0, 0]} w={w} h={tall} gap={2} align="center" justify="center">
      <text style="label" color="text-secondary" w={w - 16} align="center" truncate>{lines[0]}</text>
      {lines[1] !== undefined && <text style="label" color="text-muted" w={w - 16} align="center" truncate>{lines[1]}</text>}
      {button}
    </col>
  );
  // Only the composer starts a download; a failed one says why and offers it again.
  const asks = instrument?.state === "missing" ? "Download" : instrument?.state === "failed" ? "Try again" : undefined;
  const size = facts.library.find((offer) => offer.id === state.library)?.size;
  const download = asks && <Button id="download" label={`${field.title} download`} text={`${asks} · ${size} on disk`} size="sm" onPress={() => ctx.send("download", 1)} />;
  const shownInside = words ? says(words, download) : plays ? drawing : (
    <box w={w} h={tall}>
      <Waveform wave={live.feed("wave")} seconds={[0, 1]} file={file} w={w} h={tall} empty="Drop an audio file here" />
      <row at={[0, tall / 2 + 12]} w={w} justify="center">{chooser}</row>
    </box>
  );
  const display = (
    <box key="sampler-display" label={`${field.title} file`} role="group" hover="fill" onDrop={drop}>
      <Display columns={6}>{shownInside}</Display>
    </box>
  );

  // The instrument: the file or SFZ of the record, a file from the computer, and the library by category.
  // Picking it from a list that shows its size is asking for it.
  const pick = (id: string) => {
    update("Load instrument", (draft) => {
      const into = draft.state as Record<string, unknown>;
      for (const gone of ["sample", "sfz"]) delete into[gone];
      into.library = id;
    });
    ctx.send("download", 1);
  };

  const own: Item[] = [
    ...(state.sample ? [{ text: state.sample, checked: !state.library && !state.sfz, onPress: () => {} }] : []),
    ...(state.sfz ? [{ text: facts.sfz ?? state.sfz, checked: !state.library, onPress: () => {} }] : []),
    { text: "Audio file…", key: "audio-file", onPress: () => void chooseFile() },
  ];
  const categories = [...new Set(facts.library.map((offer) => offer.category))];
  const groups: Group[] = categories.map((category) => ({
    heading: category,
    items: facts.library.filter((offer) => offer.category === category).map((offer) => ({
      text: offer.name, key: offer.id, checked: state.library === offer.id, detail: offer.here ? `${offer.size} · downloaded` : `${offer.size} on disk`, onPress: () => pick(offer.id),
    })),
  }));
  const named = instrument?.name ?? (state.sfz ? (facts.sfz ?? state.sfz) : (state.sample ?? "Audio file…"));
  const picker = (
    <Cell span={2} text="Instrument">
      <row key="instrument" label={`${field.title} instrument`} role="button" value={named} w={2 * CELL - 8} h={24} pad={[2, 0]} gap={1} align="center" justify="between" radius="md" fill="fill" hover="fill">
        <text style="label" color="text" truncate>{named}</text>
        <icon name="chevron-down" size={14} color="text-muted" />
        <MenuList id="instrument" label={`${field.title} instrument`} items={[...own, ...groups]} w={280} search />
      </row>
    </Cell>
  );
  const gain = <Knob {...field.number("gain_db", "Gain")} unit="db" bipolar />;
  if (state.library || state.sfz) return <DeviceCard id="card" title={field.title} display={display} cells={[<col w={2 * CELL} gap={1}>{picker}{gain}</col>]} />;

  const toggle = (path: "loop" | "reverse", text: string, span: number) => {
    const on = state[path] === true;
    return <Cell span={span}><Toggle id={path} label={`${field.title} ${text}`} text={text} on={on} w={span * CELL - 8} onToggle={() => field.change(`Change ${text.toLowerCase()}`, { [path]: !on })} /></Cell>;
  };
  // The root is a key, which the record's fields do not list as a number; tune takes no lane,
  // which `number`'s type leaves out.
  const root: Bound & { text: string } = { id: "root", path: "root", label: `${field.title} Root`, text: "Root", value: state.root, min: 0, max: 127, step: 1, unit: "pitch", initial: 60, onChange: (to) => field.change("Change root", { root: Math.round(to) }) };
  // The instrument heads the knobs, as the sound of a drum pad does.
  const head = (
    <col w={2 * CELL} gap={1}>
      {picker}
      <row><Knob {...root} /><Knob {...field.number("velocity_to_volume", "Velocity")} unit="percent" /></row>
    </col>
  );
  return (
    <DeviceCard id="card" title={field.title} display={display}
      cells={[head, <Knob {...field.number("release_seconds", "Release")} unit="seconds" />, gain]}
      more={[
        <Knob {...startAt} disabled={!plays} />, <Knob {...endAt} disabled={!plays} />,
        <col w={2 * CELL} gap={1}><row>{toggle("loop", "Loop", 1)}<Knob {...loopAt} disabled={!plays || !state.loop} /></row>{toggle("reverse", "Reverse", 2)}</col>,
        <Knob {...attack} unit="seconds" />, <Knob {...sustain} unit="percent" />, <Knob {...decay} unit="seconds" />, <Knob {...field.number("tune" as never, "Tune")} unit="semitones" bipolar />,
      ]}
      expanded={expanded} onExpand={() => (memory.expanded = !expanded)} />
  );
};
