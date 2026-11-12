import {
  adsr, beat, boolean, Button, choice, Circle, Controls, DeviceCard, Dialog, Display, Fader, floor, For, Fragment, h, Input, input, interval,
  Knob, Label, lowpass, Match, Meter, mtof, note, Notice, number, NumberBox, onCleanup, output, Pad, Page, Panel, Path, pattern, playing,
  Popover, positionOf, Rect, Row, saw, SegmentedControl, Select, Show, sine, Slider, smooth, Space, StepGrid, Steps, Switch, Text, timeout, Toggle,
  tool, Wave,
  type Child, type Declaration, type Press, type Style,
} from "./sdk";

const box: Style = { width: 84, height: 36, align: "center", justify: "center", radius: "md", background: "fill-strong" };
const NAMES = ["Bass", "Bells", "Brass", "Choir", "Drums", "Flute", "Keys", "Pad", "Strings"];
const WAVES = [{ value: "sine", label: "Sine" }, { value: "saw", label: "Saw" }, { value: "square", label: "Square" }] as const;

const Section = (props: { title: string; children?: Child }) => <Panel title={props.title} width={372}>{props.children}</Panel>;

/** Where a frequency stands across the display of `sweep`, as its cutoff does. */
const across = (hz: number) => positionOf({ min: 100, max: 8000, scale: "log" }, hz);

/** A card as a view composes one: its display, then its controls, and the window lays it out. */
const sweep = tool({
  name: "sweep",
  title: "Sweep",
  when: "You check how a card of the view SDK looks",
  doc: "A low-pass filter. `cutoff` is where it closes, `resonance` how much it rings there, `drive` and `mix` how hard and how much of it plays, and `hold` keeps it open.",
  state: {
    cutoff: number({ min: 100, max: 8000, default: 1200, unit: "hz" }),
    resonance: number({ min: 0, max: 1, default: 0.2 }),
    mode: choice({ options: ["soft", "hard", "fold"], default: "hard" }),
    wave: choice({ options: ["sine", "saw", "square"], default: "saw" }),
    hold: boolean({ default: false }),
    drive: number({ min: 0, max: 1, default: 0.3 }),
    mix: number({ min: 0, max: 1, default: 1 }),
  },
  sound: ({ cutoff }) => lowpass(input, cutoff),
  view: ({ now }) => {
    const cutoff = now.value("cutoff", { fraction: true });
    return (
      <>
        <Display scale={{ x: [[across(200), "200"], [across(1000), "1k"], [across(5000), "5k"]] }} zero={0.5} cursor="ew-resize"
          drag={{ x: { value: "cutoff", absolute: true } }} control={<SegmentedControl field="mode" />}>
          <Path points={() => [[0, 0.5], [cutoff.times(0.8), 0.5], [cutoff, 0.25], [cutoff.plus(0.3), 1]]} color="text" width={1.5} />
          <Circle x={cutoff} y={0.25} radius={5} color="text" />
        </Display>
        <Controls>
          <Knob field="cutoff" />
          <Knob field="resonance" />
          <Select size="sm" field="wave" />
          <Toggle field="hold" />
          <Knob field="drive" />
          <Knob field="mix" />
        </Controls>
      </>
    );
  },
});

const children = { sweep } as const;

const write = (memory: { log: string[]; count: number }, line: string) => {
  memory.log = [`${++memory.count} ${line}`, ...memory.log].slice(0, 30);
};

const DRUMS = ["kick", "hat"] as const;

tool({
  name: "gallery",
  title: "Gallery",
  when: "You want a tool to copy, or check how a part of the view SDK looks and behaves",
  doc: "Three tools to copy: a drum machine of rows of steps, each with its knob, and a swing; the card of its child `sweep`; and a drone with its scope. After them the parts of the view SDK that the other snapshots do not show, and a log of what code hears, newest first, once there is something. The pad under Pointer holds a note while it is pressed.",
  state: {
    kick: pattern({ length: 8, min: 0, max: 1, default: 0 }),
    hat: pattern({ length: 8, min: 0, max: 1, default: 0 }),
    kick_level: number({ min: 0, max: 1, default: 0.9 }),
    hat_level: number({ min: 0, max: 1, default: 0.6 }),
    swing: number({ min: 0, max: 1, default: 0.3 }),
    pitch: number({ min: 24, max: 72, default: 45, unit: "pitch" }),
    tone: number({ min: 100, max: 8000, default: 1200, unit: "hz" }),
    level: number({ min: -30, max: 6, default: -6, unit: "db" }),
    mode: choice({ options: ["soft", "hard", "fold"], default: "hard" }),
    voices: number({ min: 1, max: 8, default: 2, integer: true }),
  },
  children,
  behaviour: (): Declaration<typeof children> => ({ connections: [] }),
  memory: () => ({
    log: [] as string[], count: 0, held: false, gain: 0.5, fader: 0, amount: 4, wave: "saw", pad: { x: 0.3, y: 0.7 },
    steps: [1, 0, 0, 1], filter: "", popover: false, dialog: false, flash: false, part: false, repeats: 0,
  }),
  sound: ({ pitch, tone }) => {
    output("gate", note.gate);
    output("step", floor(beat.times(2)).mod(8));
    const drone = lowpass(saw(mtof(pitch)), tone).times(smooth(playing, 50));
    return drone.plus(sine(note.freq).times(adsr(note.gate, 5, 50, 0.8, 200))).times(0.1);
  },
  onKey: ({ memory }, { key, down }) => {
    if (down) write(memory, `key ${key}`);
  },
  view: ({ memory, now, play, send }) => {
    const log = (line: string) => write(memory, line);
    const pointer = (what: string) => ({
      onPress: (press: Press) => log(`${what} press ${press.button}`),
      onRelease: (press: Press) => log(`${what} release ${press.button}`),
      onClick: () => log(`${what} click`),
    });
    let stopRepeat = () => {};
    const Part = () => {
      onCleanup(() => log("part cleaned up"));
      return <Text variant="label" style={{ color: "green" }}>Logs when it goes</Text>;
    };
    return (
      <Page>
        <div style={{ grow: true, gap: "lg" }}>
          <Row align="start">
            <Panel title="Drum machine">
              <StepGrid columns={["Level"]}>
                {DRUMS.map((drum) => (
                  <Steps field={drum} playing="step">
                    <Knob field={`${drum}_level`} text="value" />
                  </Steps>
                ))}
                <Controls>
                  <Knob field="swing" />
                </Controls>
              </StepGrid>
            </Panel>
            <DeviceCard path="sweep" />
            <Panel title="Drone">
              <Row>
                <Display zero={0.5}>
                  <Wave list={now.scope()} color="green" />
                </Display>
                <Controls>
                  <Knob field="pitch" />
                  <Knob field="tone" />
                </Controls>
              </Row>
            </Panel>
          </Row>
          <div style={{ direction: "row", wrap: true, align: "start", gap: "lg" }}>
            <Section title="Field or kept">
              <Row gap="sm">
                <Controls>
                  <Knob key="field" field="tone" label="Field" />
                  <Knob value={() => memory.gain} onChange={(gain) => { memory.gain = gain; log(`knob end ${gain.toFixed(2)}`); }}
                    label="Kept" min={0} max={1} default={0.5} unit="percent" />
                </Controls>
                <Meter value={() => memory.gain} />
                <Pad size={64} field={{ x: "tone", y: "level" }} />
                <Pad size={64} value={() => memory.pad} onChange={(pad) => { memory.pad = pad; log(`pad ${pad.x.toFixed(2)} ${pad.y.toFixed(2)}`); }} />
                <Fader length={64} min={0} max={1} default={0} value={() => memory.fader}
                  onChange={(fader) => { memory.fader = fader; log(`fader end ${fader.toFixed(2)}`); }} />
              </Row>
              <Row gap="sm">
                <NumberBox key="amount" width={32} min={1} max={16} value={() => memory.amount}
                  onChange={(amount) => { memory.amount = amount; log(`number ${amount}`); }} />
                <NumberBox key="voices" field="voices" width={32} align="center" />
                <Select value={() => memory.wave} options={WAVES} onChange={(wave) => { memory.wave = wave; log(`select ${wave}`); }} />
                <Slider field="level" />
              </Row>
              <Row gap="sm">
                <SegmentedControl field="mode" />
                <Switch fallback={() => "A square"}>
                  <Match when={() => memory.wave === "sine"}>{() => <Text variant="label" style={{ color: "teal" }}>A sine</Text>}</Match>
                  <Match when={() => memory.wave === "saw"}>{() => <Text variant="label" style={{ color: "peach" }}>A saw</Text>}</Match>
                </Switch>
              </Row>
              <Row gap="sm">
                <Steps value={() => memory.steps} onChange={(steps) => { memory.steps = [...steps]; log(`steps ${steps.join("")}`); }} />
                <Button icon="minus" size="sm" variant="subtle" onClick={() => { memory.steps = memory.steps.slice(0, -1); }} />
                <Button icon="plus" size="sm" variant="subtle" onClick={() => { memory.steps = [...memory.steps, 0]; }} />
              </Row>
            </Section>

            <Section title="Text">
              <div style={{ direction: "row", gap: "sm", align: "end" }}>
                <Text variant="display">Display</Text>
                <Text variant="title">Title</Text>
                <Text>Body 14</Text>
                <Text variant="label">Label</Text>
                <Text variant="label" tone="muted">Muted 120.5</Text>
              </div>
              <Input value="" placeholder="Filter: type “b”" onChange={(text) => { memory.filter = text; }} onCommit={(text) => log(`filter commit ${text}`)} />
              <div style={{ direction: "row", gap: "xs", wrap: true }}>
                <For each={() => NAMES.filter((name) => name.toLowerCase().includes(memory.filter.toLowerCase()))}>
                  {(name) => <div style={{ paddingX: "sm", paddingY: "xs", radius: "sm", background: "fill-strong" }}>{name}</div>}
                </For>
              </div>
              <Input value="Disabled field" disabled onCommit={() => log("disabled commit")} />
              <Input value="" placeholder="Enter adds a line, cmd-enter commits" rows={3} width={338}
                onCommit={(text) => log(`area commit ${JSON.stringify(text)}`)} />
            </Section>

            <Section title="Overlays and timers">
              <Row gap="sm">
                <div>
                  <Button label="Popover" variant="subtle" onClick={() => { memory.popover = !memory.popover; }} />
                  <Popover open={() => memory.popover} onClose={() => { memory.popover = false; log("popover closed"); }}>
                    {() => (
                      <div style={{ padding: "sm", gap: "sm" }}>
                        <Text>A popover of its own</Text>
                        <Button label="Pick" onClick={() => { memory.popover = false; log("popover pick"); }} />
                      </div>
                    )}
                  </Popover>
                </div>
                <Button label="Dialog" variant="subtle" onClick={() => { memory.dialog = true; }} />
                <Dialog open={() => memory.dialog} onClose={() => { memory.dialog = false; log("dialog closed"); }}>
                  {() => (
                    <div style={{ width: 320, padding: "lg", gap: "md", align: "start" }}>
                      <Text variant="title">A dialog</Text>
                      <Text tone="secondary">The page hears no key while it is open.</Text>
                      <Input value="" placeholder="Its own field" focus onCommit={(text) => log(`dialog commit ${text}`)} />
                      <Select value={() => memory.wave} options={WAVES} onChange={(wave) => { memory.wave = wave; log(`dialog select ${wave}`); }} />
                      <Button label="Close" onClick={() => { memory.dialog = false; log("dialog close button"); }} />
                    </div>
                  )}
                </Dialog>
              </Row>
              <Row gap="sm">
                <Button label="Hold to repeat"
                  onPress={() => { memory.repeats++; stopRepeat = interval(() => { memory.repeats++; }, 100); }}
                  onRelease={() => { stopRepeat(); log(`repeated ${memory.repeats}`); }} />
                <Text variant="label" tone="text">{() => `${memory.repeats}`}</Text>
              </Row>
              <Row gap="sm">
                <Button label="Flash" variant="subtle" onClick={() => { memory.flash = true; timeout(() => { memory.flash = false; }, 1000); }} />
                <Show when={() => memory.flash}>{() => <Text variant="label" style={{ color: "yellow" }}>Gone in a second</Text>}</Show>
                <Button label="Part" variant="subtle" onClick={() => { memory.part = !memory.part; }} />
                <Show when={() => memory.part}>{() => <Part />}</Show>
              </Row>
              <Notice tone="warning" message="A notice with an action" action={{ label: "Act", onClick: () => log("notice action") }} onDismiss={() => log("notice dismissed")} />
            </Section>

            <Show when={() => memory.log.length > 0}>
              {() => (
                <Panel title="What code hears" width={220}>
                  <div style={{ gap: "xs" }}>
                    <For each={() => memory.log}>{(line) => <Text variant="label" tone="text">{line}</Text>}</For>
                  </div>
                </Panel>
              )}
            </Show>

            <Section title="Pointer">
              <Row gap="sm">
                <div key="div" style={box} {...pointer("div")}>Div</div>
                <Space key="shapes" width={84} height={36} background="fill" radius="md" {...pointer("space")}>
                  <Rect x={8} y={8} width={32} height={20} radius={4} color="lavender" hover="pink" cursor="pointer" {...pointer("rect")} />
                  <Circle x={62} y={18} radius={10} color="teal" />
                </Space>
                <Button label="Button" {...pointer("button")} />
                <Toggle label="T" on={() => memory.held} {...pointer("toggle")} />
              </Row>
              <Row gap="sm">
                <div key="hold" style={() => ({ ...box, width: 200, background: memory.held ? "green/40" : "fill-strong" })}
                  onPress={() => { memory.held = true; play(60, { hold: true }); log("note on"); }}
                  onRelease={() => { memory.held = false; send("notes", { type: "noteOff", pitch: 60 }); log("note off"); }}>
                  {() => (memory.held ? "Holds a note: drag off" : "Hold for a note")}
                </div>
                <Meter value="gate" />
              </Row>
            </Section>

            <Section title="Looks">
              <Row gap="sm">
                <div key="look" cursor="pointer" tooltip="A tooltip on a div" onClick={() => {}}
                  style={{ ...box, hover: { background: "selected" }, pressed: { background: "lavender", color: "text-inverse" } }}>Press</div>
                <div disabled onClick={() => log("disabled div click")} style={box}>Disabled</div>
                <div disabled style={{ ...box, disabled: { background: "red/20", color: "red" } }}>Failed</div>
              </Row>
              <Row gap="sm">
                <Button label="Primary" tooltip="A tooltip on a button" />
                <Button label="Subtle" variant="subtle" />
                <Button label="Solid" variant="solid" color="teal" />
                <Button icon="trash-2" variant="ghost" rounded />
                <Button label="Off" disabled onClick={() => log("disabled button click")} />
              </Row>
              <Row gap="sm">
                <Button label="Small" size="sm" variant="subtle" />
                <Button label="Medium" variant="subtle" />
                <Button label="Large" size="lg" variant="subtle" />
              </Row>
              <Row gap="sm">
                <Toggle label="M" on color="yellow" tooltip="A letter" />
                <Toggle icon="metronome" count={3} on={false} tooltip="An icon with a count" />
                <Toggle dot on color="red" tooltip="A dot" />
                <Toggle label="D" on disabled onClick={() => log("disabled toggle click")} />
              </Row>
              <Row gap="sm">
                <div style={{ width: 120, height: 60, padding: "sm", radius: "lg", background: "surface", shadow: true }}>
                  Shadow
                  <div style={{ absolute: { top: -6, right: -6 }, width: 18, height: 18, radius: "full", align: "center", justify: "center", background: "red" }}>
                    <Text variant="label" style={{ color: "text-inverse" }}>3</Text>
                  </div>
                  <div style={{ absolute: { left: 0, right: 0, bottom: 0 }, height: 22, align: "center", justify: "center", background: "fill-strong" }}>Overlay</div>
                </div>
                <div style={{ width: 64, height: 40, radius: "lg", background: "fill", clip: true }}>
                  <div style={{ width: 120, height: 24, background: "teal/40" }}>Clipped at its edge</div>
                </div>
                <div style={{ direction: "row", wrap: true, gap: "xs", maxWidth: 96 }}>
                  {Array.from({ length: 6 }, (_, index) => (
                    <div style={{ width: 28, height: 18, radius: "sm", align: "center", justify: "center", background: "fill-strong" }}>{index}</div>
                  ))}
                </div>
              </Row>
              <Row gap="sm">
                <div style={{ minWidth: 150, padding: "sm", background: "fill", textAlign: "end" }}>Aligned end</div>
                <div style={{ padding: "sm", background: "fill", opacity: 0.4 }}>Opacity 0.4</div>
              </Row>
            </Section>

            <Section title="Shapes">
              <Space key="paths" height={100} background="well" radius="md">
                <Path points={[[10, 80], [60, 0, 110, 80]]} color="peach" width={2} />
                <Path points={[[130, 80], [150, 10, 200, 10, 220, 80]]} color="sky" width={2} />
                <Path points={[[250, 80], [290, 15], [330, 80], [250, 80]]} color="green" width={2} />
                <Path points={[[250, 92], [290, 60, 330, 92]]} color="green/40" />
                <Wave x={10} y={4} width={220} height={20} list={now.history(now.value("tone", { fraction: true }), 120)} low={0} high={1} />
                <Label x={10} y={82} text="quad" tone="muted" />
                <Label x={130} y={82} text="cubic" tone="muted" />
                <Label x={338} y={2} text="closed" tone="text" align="right" />
              </Space>
            </Section>
          </div>
        </div>
      </Page>
    );
  },
});
