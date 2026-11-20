import { h, live, Live, tool, type Child, type Declaration } from "./sdk";
import { Button } from "./ui/button";
import { Cell } from "./ui/cell";
import { Bar } from "./ui/bar";
import { Choice } from "./ui/choice";
import { Curve } from "./ui/curve";
import { decibelTicks, Display, displayWidth, drawingOf, DISPLAY_HEIGHT, FREQUENCY, FREQUENCY_TICKS, OCTAVES } from "./ui/display";
import { Handle } from "./ui/handle";
import { Readout } from "./ui/readout";
import { Waveform } from "./ui/waveform";
import { Dot } from "./ui/dot";
import { Fader } from "./ui/fader";
import { control, type NumberLike } from "./ui/field";
import { Kbd } from "./ui/kbd";
import { Knob } from "./ui/knob";
import { Menu } from "./ui/menu";
import { Meter } from "./ui/meter";
import { Notice } from "./ui/notice";
import { Pad } from "./ui/pad";
import { Popover } from "./ui/popover";
import { Segmented } from "./ui/segmented";
import { Select } from "./ui/select";
import { Slider } from "./ui/slider";
import { TextField } from "./ui/text-field";
import { Toggle } from "./ui/toggle";
import { shown } from "./ui/value";
import { DeviceCard, Frame } from "./ui/blocks/device-card";
import { EmptyState } from "./ui/blocks/empty-state";
import { Keyboard } from "./ui/blocks/keyboard";
import { PaintGrid } from "./ui/blocks/paint-grid";
import { Panel, Section } from "./ui/blocks/panel";
import { StepGrid, type Step } from "./ui/blocks/steps";

const WAVES = [{ value: "saw", text: "Saw" }, { value: "square", text: "Square" }, { value: "sine", text: "Sine" }] as const;
const SHAPES = [{ value: "low", text: "Low pass" }, { value: "high", text: "High pass" }, { value: "band", text: "Band pass" }] as const;
type Wave = (typeof WAVES)[number]["value"];
type Shape = (typeof SHAPES)[number]["value"];

type Memory = {
  toggles: Record<string, boolean>;
  wave: Wave;
  shape: Shape;
  values: Record<string, number>;
  name: string;
  clip: string;
  renaming: boolean;
  expanded: boolean;
  chosen: boolean;
  steps: Step[][];
  cells: boolean[][];
  clips: Clip[];
  chosenClip: string;
};

const pattern = (on: number[]): Step[] => Array.from({ length: 16 }, (_, i) => ({ on: on.includes(i), velocity: on.includes(i) ? 0.5 + (i % 4) * 0.15 : 0.8 }));

const PARTS = { hum: "timeline", synth: "instrument.synth", scope: "analyzer", rec: "recorder" } as const;

/** Every component and block of `ui/` in its states, to look at the kit at once: `sound-tools . --main gallery --snap gallery.png`. */
export const gallery = tool({
  name: "gallery",
  title: "Gallery",
  when: "You want to see every part of the look of this project: its components and blocks",
  doc: "Shows every component of `ui/` and every block of `ui/blocks/` in its states, and feeds: its clip `hum` plays a chord on its `synth` into the analyzer `scope` while the song plays.",
  state: {},
  children: PARTS,
  behaviour: (): Declaration<typeof PARTS> => ({
    connections: [
      { from: { child: "hum", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { notes: "notes" }, to: { child: "rec", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "scope" } },
      { from: { child: "scope" }, to: { port: "audio" } },
    ],
  }),
  memory: (): Memory => ({
    toggles: { mute: true, solo: false, arm: true, freeze: true, sync: false },
    wave: "saw",
    shape: "low",
    values: { note: 60, deep: 0, cutoff: 2000, resonance: 0.3, pan: -0.25, drive: 0.6, mix: 0.75, fader: -6, slider: 0.4 },
    name: "Bass",
    clip: "Verse",
    renaming: false,
    expanded: false,
    chosen: true,
    steps: [pattern([0, 4, 8, 12]), pattern([4, 12]), pattern([0, 2, 4, 6, 8, 10, 12, 14])],
    cells: Array.from({ length: 4 }, (_, r) => Array.from({ length: 12 }, (_, c) => (r + c) % 5 === 0)),
    clips: [{ name: "Intro", start: 0, row: 0, length: 2 * BAR }, { name: "Verse", start: 2 * BAR, row: 1, length: 4 * BAR }, { name: "Bass", start: BAR, row: 2, length: 3 * BAR }, { name: "Hook", start: 7 * BAR, row: 0, length: 2 * BAR }],
    chosenClip: "Verse",
  }),
  view: (_, { memory, play }) => {
    const flip = (name: string) => () => (memory.toggles[name] = !memory.toggles[name]);
    const value = (name: string) => ({ value: memory.values[name] ?? 0, onChange: (to: number) => (memory.values[name] = to) });
    // Memory, not a record: the controls are wired by hand to it, as `control` takes, and play no number.
    const bound = (id: string, label: string, field: NumberLike) => {
      const { path: _, ...wired } = control(() => false, id, field, memory.values[id] ?? field.default ?? field.min, (to) => (memory.values[id] = to));
      return { ...wired, label };
    };
    const knob = (id: string, label: string, props: NumberLike & { bipolar?: boolean; initial?: number }) => <Knob {...bound(id, label, { ...props, default: props.initial })} bipolar={props.bipolar} />;
    const gain = (id: string, label: string) => bound(id, label, { min: -60, max: 6, unit: "db" });
    const toggle = (id: string, label: string, color?: "warning" | "hot" | "danger", dot?: boolean, text?: string) => <Toggle id={id} label={label} text={text} on={memory.toggles[id] ?? false} color={color} dot={dot} onToggle={flip(id)} />;
    return (
      <col w="full" h="full" scroll="y">
        <col pad={6} gap={6}>
          <row gap={8}>
            <Section title="Buttons">
              <Grid>
                <Button id="primary" label="Add a track" text="Add track" variant="primary" />
                <Button id="subtle" label="Duplicate" text="Duplicate" />
                <Button id="ghost" label="Rename" text="Rename" variant="ghost" />
                <Button id="glyph" label="Add" icon="plus" tip="Add a device" />
                <Button id="with-icon" label="Record" text="Record" icon="mic" />
              </Grid>
              <Grid>
                <Button id="small" label="Small" text="Small" size="sm" />
                <Button id="medium" label="Medium" text="Medium" size="md" />
                <Button id="large" label="Large" text="Large" size="lg" />
              </Grid>
            </Section>
            <Section title="Toggles">
              <Grid>
                {toggle("mute", "Mute", "warning", false, "M")}
                {toggle("solo", "Solo", "hot", false, "S")}
                {toggle("arm", "Arm", "danger", true)}
                {toggle("freeze", "Freeze")}
                {toggle("sync", "Sync")}
                <Toggle id="lanes" label="Show lanes" icon="automation" on={memory.toggles.lanes ?? false} onToggle={flip("lanes")} />
              </Grid>
            </Section>
            <Section title="Choices">
              <Grid>
                <Segmented id="wave" label="Waveform" options={WAVES} value={memory.wave} onChange={(wave) => (memory.wave = wave)} />
                <Select id="shape" label="Filter" options={SHAPES} value={memory.shape} search onChange={(shape) => (memory.shape = shape)} />
              </Grid>
              <Grid>
                <Menu id="menu" label="Track menu" items={[{ text: "Rename", icon: "pencil", onPress: () => {}, keys: "⏎" }, { text: "Duplicate", icon: "copy", onPress: () => {}, keys: "⌘D" }, { text: "Delete", icon: "trash-2", onPress: () => {}, danger: true }]} />
                <Popover id="popover" label="Swing" text="Swing" title="Swing">
                  {knob("swing", "Swing", { min: 0, max: 1, unit: "percent" })}
                </Popover>
              </Grid>
              <row key="row" label="Choose Bass" role="tab" checked={memory.chosen} w={240} h={32} px={2} gap={2} align="center" justify="between" radius="md" fill={memory.chosen ? "selected" : undefined} hover="fill" onPress={() => (memory.chosen = !memory.chosen)}>
                <text style="body" color="text">Bass</text>
                <Menu id="row-menu" label="Bass menu" size="sm" items={[{ text: "Rename", onPress: () => {} }, { text: "Delete", onPress: () => {}, danger: true }]} />
              </row>
            </Section>
          </row>
          <row gap={8}>
            <Section title="Values">
              <row gap={4} align="end">
                <row>
                  {knob("cutoff", "Cutoff", { min: 20, max: 20000, scale: "log", unit: "hz", initial: 2000 })}
                  {knob("resonance", "Resonance", { min: 0, max: 1, unit: "percent" })}
                  {knob("pan", "Pan", { min: -1, max: 1, unit: "pan", bipolar: true, initial: 0 })}
                  <Knob {...bound("note", "Note", { min: 36, max: 96, integer: true, unit: "pitch" })} />
                  <Knob id="held" label="Held" min={20} max={20000} scale="log" unit="hz" value={500} held={Live.of(8000)} onChange={() => {}} />
                </row>
                <Fader {...gain("fader", "Volume")} height={120} />
                <Meter id="meter" label="Level" path="nothing" w={4} h={120} />
                <Fader {...gain("deep", "Deep volume")} height={120} min={-96} />
                <Slider id="slider" label="Amount" w={160} min={0} max={1} {...value("slider")} />
              </row>
            </Section>
            <Section title="Text and marks">
              <col gap={3}>
                <TextField id="name" label="Track name" value={memory.name} onCommit={(name) => name !== null && (memory.name = name)} />
                {memory.renaming
                  ? <TextField id="rename" label="Rename the clip" value={memory.clip} focus onCommit={(name) => { memory.renaming = false; if (name !== null) memory.clip = name; }} />
                  : <row key="clip" label="Clip" role="button" w={160} h={28} px={2} align="center" radius="lg" hover="fill" onPress={() => (memory.renaming = true)}><text style="body" color="text">{memory.clip}</text></row>}
                <row gap={2} align="center">
                  <Kbd keys="⌘K" />
                  <Dot color="focus" />
                  <Dot color="blue" />
                  <Dot color="sound" />
                </row>
                <Notice text="Exported to the folder." />
                <Notice text="This project does not load plugins." tone="warning" />
                <Notice text="The file could not be read." tone="danger" />
                <Take />
              </col>
            </Section>
            <Section title="Pads">
              <row gap={1}>
                <Pad id="kick" label="Kick" pitch={36} chosen onPress={() => play(36)} />
                <Pad id="snare" label="Snare" pitch={38} onPress={() => play(38)} />
                <Pad id="hat" label="Hat" pitch={42} onPress={() => play(42)} />
              </row>
            </Section>
          </row>
          <Section title="Device card">
            <Frame id="frame" title="Filter" onPower={() => {}} onRemove={() => {}}>
            <DeviceCard
              id="card"
              title="Filter"
              display={<Display caption={shown(memory.values.cutoff ?? 0, "hz")}><LowPass /></Display>}
              cells={[
                knob("drive", "Drive", { min: 0, max: 1, unit: "percent" }),
                knob("mix", "Mix", { min: 0, max: 1, unit: "percent", initial: 1 }),
                <Cell text="Shape" span={2}><Select id="card-shape" label="Filter shape" options={SHAPES} value={memory.shape} w={120} onChange={(shape) => (memory.shape = shape)} /></Cell>,
                <Cell span={2}><Toggle id="card-freeze" label="Freeze" on={memory.toggles.freeze ?? false} w={120} onToggle={flip("freeze")} /></Cell>,
              ]}
              more={[knob("card-res", "Resonance", { min: 0, max: 1, unit: "percent" }), knob("card-pan", "Pan", { min: -1, max: 1, unit: "pan", bipolar: true })]}
              expanded={memory.expanded}
              onExpand={() => (memory.expanded = !memory.expanded)}
            />
            </Frame>
          </Section>
          <Section title="Card kit">
            <CardKit values={memory.values} bound={bound} wave={memory.wave} onWave={(wave) => (memory.wave = wave)} />
          </Section>
          <Section title="Feeds">
            <Feeds />
          </Section>
          <row gap={8}>
            <Section title="Keys">
              <Keyboard id="keys" low={48} high={72} onPlay={(pitch) => play(pitch)} />
            </Section>
          </row>
          <row gap={8}>
            <Section title="Steps">
              <StepGrid
                id="steps"
                label="Drums"
                rows={["Kick", "Snare", "Hat"].map((name, r) => ({ name, steps: memory.steps[r] ?? [] }))}
                onToggle={(r, i) => memory.steps[r]?.[i] && (memory.steps[r][i].on = !memory.steps[r][i].on)}
                onVelocity={(r, i, velocity) => memory.steps[r]?.[i] && (memory.steps[r][i].velocity = velocity)}
              />
            </Section>
            <Section title="Paint">
              <PaintGrid id="cells" label="Cell" cells={memory.cells} onPaint={(r, c, on) => memory.cells[r] && (memory.cells[r][c] = on)} onToggle={(r, c) => memory.cells[r] && (memory.cells[r][c] = !memory.cells[r][c])} />
            </Section>
          </row>
          <Section title="Clips in time">
            <Clips memory={memory} />
          </Section>
          <row gap={8}>
            <Panel title="Panel" end={<Button id="panel-add" label="Add a device" icon="plus" size="sm" variant="ghost" />}>
              <text style="body" color="text-secondary">A surface with a title, for a part of a page.</text>
            </Panel>
            <box w={360} radius="lg" border="border">
              <EmptyState title="No devices yet" text="Add one to shape the sound.">
                <Button id="empty-add" label="Add a device" text="Add device" variant="primary" size="sm" />
              </EmptyState>
            </box>
          </row>
        </col>
      </col>
    );
  },
});

/** Components of one kind side by side. */
const Grid = ({ children }: { children?: Child }) => (
  <row gap={2} align="center">
    {children}
  </row>
);

/** A low pass curve, the one bright line of the display, filled softly below. */
const LowPass = () => {
  const heights = Array.from({ length: 49 }, (_, i) => -(Math.max(0, i - 26) ** 2) / 20);
  return <Curve heights={heights} {...drawingOf(4)} across={[0, 48]} range={[6, -30]} />;
};

/** A response with a handle on two numbers (a bell at its frequency and gain), the scale of a display, a bar, a wave that is missing, a choice above the drawing and a readout. */
const CardKit = ({ values, bound, wave, onWave }: { values: Record<string, number>; bound: (id: string, label: string, field: NumberLike) => ReturnType<typeof control>; wave: Wave; onWave: (wave: Wave) => void }) => {
  const { w, h: tall } = drawingOf(4, true);
  const [hz, db] = [bound("kit-hz", "Frequency", { ...FREQUENCY, unit: "hz", default: 1000 }), bound("kit-db", "Gain", { min: -15, max: 15, unit: "db", default: 0 })];
  // A bell, drawn here from its numbers: a card draws the device's `response` feed instead.
  const bell = Array.from({ length: 97 }, (_, i) => (values["kit-db"] ?? 0) * Math.exp(-(((i / 96) * OCTAVES[1] - Math.log2((values["kit-hz"] ?? 1000) / 20)) ** 2)));
  return (
    <row gap={4} align="end">
    <Display strip={<Choice id="kit-wave" label="Wave" options={WAVES} value={wave} onChange={onWave} />} across={FREQUENCY_TICKS} up={decibelTicks([-18, 18], 6)} zero={0.5}>
      <Curve heights={bell} w={w} h={tall} across={OCTAVES} range={[18, -18]} />
      <Handle id="kit-band" label="Band 1" w={w} h={tall} x={hz} y={{ ...db, travel: { min: -18, max: 18 } }} number="1" caption={`${shown(hz.value, "hz")} · ${shown(db.value, "db")}`} />
    </Display>
    <Display columns={6} caption="A file the card cannot find">
      <Waveform wave={live.feed("wave")} seconds={[0, 1]} file={{ name: "kick.wav", state: "missing" }} {...drawingOf(6)} />
    </Display>
    <box w={12} h={DISPLAY_HEIGHT} radius="sm" fill="well" clip><Bar value={live.feed("loudness", "scope")} range={[-60, 0]} w={12} h={DISPLAY_HEIGHT} color="sound" hold /></box>
    <Readout id="kit-loudness" label="Loudness" text="Loudness" value={live.feed("loudness", "scope")} digits={1} />
    </row>
  );
};

/** What the children publish while the song plays, drawn without the view: a number, a list as a filled line, and the notes of a take as rects. */
const Feeds = () => {
  const [w, tall] = [displayWidth(4), DISPLAY_HEIGHT];
  return (
    <row gap={4} align="end">
      <col gap={1}>
        <text style="label" color="text-secondary">Loudness</text>
        <text key="loudness" live={live.feed("loudness", "scope")} digits={1} w={48} />
      </col>
      <col gap={1}>
        <text style="label" color="text-secondary">Spectrum</text>
        <box w={w} h={tall} radius="sm" fill="well" clip>
          <shapes key="spectrum" w={w} h={tall} units={[w, [0, -90]]} lines={[{ color: "sound", width: 1.5, points: live.feed("spectrum", "scope"), fill: "sound/24" }]} />
        </box>
      </col>
      <col gap={1}>
        <text style="label" color="text-secondary">Notes of a take</text>
        <box w={w} h={tall} radius="sm" fill="well" clip>
          <shapes key="taken" w={w} h={tall} units={[15360, [84, 36]]} rects={[{ color: "text", rects: live.feed("notes", "rec") }]} />
        </box>
      </col>
    </row>
  );
};

/** A take as it records: its wave grows with the time, filled below, and runs no view. */
const Take = () => {
  const points: number[] = [];
  for (let x = 0; x < 32; x += 1) points.push(x, 0.5 - 0.4 * Math.sin(x * 0.9) * Math.sin(x * 0.17), x + 1, 0.5 - 0.4 * Math.sin((x + 1) * 0.9) * Math.sin((x + 1) * 0.17));
  return (
    <box w={200} h={24} radius="sm" fill="well" clip>
      <shapes w={live.elapsed.times(80).min(200)} h={24} units={[32, 1]} lines={[{ color: "sound", width: 1.5, points, fill: "sound/24" }]} />
    </box>
  );
};

const [BAR, ROW] = [3840, 28];
type Clip = { name: string; start: number; row: number; length: number };

/**
 * Clips in time on a plane: they drag along bars and rows onto sixteenths (Cmd: free), the wheel
 * scrolls, a pinch or Cmd-wheel zooms, the ruler moves with them, a right press opens a menu of
 * the clip pressed. The primitives only: a timeline block builds on them.
 */
const Clips = ({ memory }: { memory: Memory }) => {
  const item = (text: string, run: () => void) => (
    <row key={`clips-${text}`} label={text} role="menuitem" w="full" h={28} px={2} align="center" radius="md" hover="fill" onPress={run}>
      <text style="label" color="text">{text}</text>
    </row>
  );
  const chosen = () => memory.clips.find((clip) => clip.name === memory.chosenClip);
  return (
    <col w={480} radius="md" border="border" clip>
      <box key="clips-ruler" label="Ruler" w={480} h={20} fill="surface" plane={{ x: "clips" }}>
        {Array.from({ length: 16 }, (_, bar) => (
          <box place={[bar * BAR, 0, BAR, 20]} px={1} border="border" borderSides={[0, 0, 0, 1]}>
            <text style="label" color="text-muted">{bar + 1}</text>
          </box>
        ))}
      </box>
      <box key="clips" label="Clips" w={480} h={ROW * 4} fill="well" plane={{ x: 0.025, y: ROW, zoom: [0.25, 4] }}>
        {memory.clips.map((clip) => {
          const moved = live.drag(`clip-${clip.name}`);
          return (
            <box key={`clip-${clip.name}`} label={`Clip ${clip.name}`} place={[moved.dx.plus(clip.start), moved.dy.plus(clip.row), clip.length, 1]}
              radius="sm" fill={clip.name === memory.chosenClip ? "selected" : "blue/40"} border={clip.name === memory.chosenClip ? "focus" : "blue"}
              drag={{ x: [-clip.start, 15 * BAR], y: [-clip.row, 3 - clip.row], snap: [BAR / 4, 1] }}
              onPress={() => (memory.chosenClip = clip.name)} onDragEnd={(p) => Object.assign(clip, { start: clip.start + p.dx, row: clip.row + p.dy })}>
              <text at={[6, 6]} style="label" color="text" truncate>{clip.name}</text>
            </box>
          );
        })}
        <col overlay={{ open: "menu" }} role="menu" group label="Clip menu" w={160} pad={1} radius="lg" fill="raised" border="border-strong" shadow="md">
          {item("Duplicate", () => {
            const clip = chosen();
            if (clip) memory.clips.push({ ...clip, name: `${clip.name} 2`, start: clip.start + clip.length });
          })}
          {item("Delete", () => (memory.clips = memory.clips.filter((clip) => clip.name !== memory.chosenClip)))}
        </col>
      </box>
    </col>
  );
};
