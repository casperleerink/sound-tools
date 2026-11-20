import { h, input, live, Live, tool, type Child } from "./sdk";
import { Button } from "./ui/button";
import { Cell } from "./ui/cell";
import { Display, displayWidth, DISPLAY_HEIGHT } from "./ui/display";
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
import { DeviceCard } from "./ui/blocks/device-card";
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
};

const pattern = (on: number[]): Step[] => Array.from({ length: 16 }, (_, i) => ({ on: on.includes(i), velocity: on.includes(i) ? 0.5 + (i % 4) * 0.15 : 0.8 }));

/** Every component and block of `ui/` in its states, to look at the kit at once: `sound-tools . --main gallery --snap gallery.png`. */
export const gallery = tool({
  name: "gallery",
  title: "Gallery",
  when: "You want to see every part of the look of this project: its components and blocks",
  doc: "Shows every component of `ui/` and every block of `ui/blocks/` in its states. Plays nothing.",
  state: {},
  sound: () => input,
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
  }),
  view: (_, { memory, play }) => {
    const flip = (name: string) => () => (memory.toggles[name] = !memory.toggles[name]);
    const value = (name: string) => ({ value: memory.values[name] ?? 0, onChange: (to: number) => (memory.values[name] = to) });
    // Memory, not a record: the controls are wired by hand to it, as `control` takes.
    const bound = (id: string, label: string, field: NumberLike) => ({ ...control(() => false, id, field, memory.values[id] ?? field.default ?? field.min, (to) => (memory.values[id] = to)), label });
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
                  {knob("cutoff", "Cutoff", { min: 20, max: 20000, log: true, unit: "hz", initial: 2000 })}
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
            <DeviceCard
              id="card"
              title="Filter"
              display={<Display caption={shown(memory.values.cutoff ?? 0, "hz")}><Curve /></Display>}
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
const Curve = () => {
  const [w, tall] = [displayWidth(4), DISPLAY_HEIGHT];
  const points: number[] = [];
  const y = (x: number) => tall * 0.35 + Math.max(0, x - w * 0.55) ** 2 / 90;
  for (let x = 0; x < w; x += 4) points.push(x, y(x), x + 4, y(x + 4));
  return <shapes at={[0, 0]} w={w} h={tall} lines={[{ color: "text", width: 1.5, points, fill: "text/5" }]} />;
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
