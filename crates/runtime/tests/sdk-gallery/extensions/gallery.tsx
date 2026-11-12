import {
  adsr, Button, choice, Circle, Dialog, Fader, For, h, Input, interval, Knob, Label, Match, Meter, note, Notice, number,
  NumberBox, onCleanup, output, Pad, Path, Popover, Rect, SegmentedControl, Select, Show, sine, Slider, Space, Steps,
  Switch, Text, timeout, Toggle, tool, type Child, type Press, type Style,
} from "./sdk";

const card: Style = { width: 372, padding: "md", gap: "sm", radius: "lg", background: "fill" };
const row: Style = { direction: "row", gap: "sm", align: "center" };
const box: Style = { width: 84, height: 36, align: "center", justify: "center", radius: "md", background: "fill-strong" };
const NAMES = ["Bass", "Bells", "Brass", "Choir", "Drums", "Flute", "Keys", "Pad", "Strings"];
const WAVES = [{ value: "sine", label: "Sine" }, { value: "saw", label: "Saw" }, { value: "square", label: "Square" }] as const;
const MODES = [{ value: "soft", label: "Soft" }, { value: "hard", label: "Hard" }, { value: "fold", label: "Fold" }] as const;

const Section = (props: { title: string; children?: Child }) => (
  <div style={card}>
    <Text variant="heading">{props.title}</Text>
    {props.children}
  </div>
);

const write = (memory: { log: string[]; count: number }, line: string) => {
  memory.log = [`${++memory.count} ${line}`, ...memory.log].slice(0, 30);
};

tool({
  name: "gallery",
  title: "Gallery",
  when: "You check how every part of the view SDK looks and behaves",
  doc: "The components, styles, pointer events, overlays and timers of the view SDK that the other snapshots do not show. The log on the right shows what code hears, newest first; the pad under Pointer holds a note while it is pressed.",
  state: {
    cutoff: number({ min: 100, max: 8000, default: 1200, unit: "hz" }),
    level: number({ min: -30, max: 6, default: -6, unit: "db" }),
    mode: choice({ options: ["soft", "hard", "fold"], default: "hard" }),
  },
  memory: () => ({
    log: [] as string[], count: 0, held: false, gain: 0.5, fader: 0, amount: 4, wave: "saw", pad: { x: 0.3, y: 0.7 },
    steps: [1, 0, 0, 1], filter: "", popover: false, dialog: false, flash: false, part: false, repeats: 0,
  }),
  sound: () => {
    output("gate", note.gate);
    return sine(note.freq).times(adsr(note.gate, 5, 50, 0.8, 200)).times(0.1);
  },
  onKey: ({ memory }, { key, down }) => {
    if (down) write(memory, `key ${key}`);
  },
  view: ({ memory, play, send }) => {
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
      <div style={{ grow: true, direction: "row", gap: "md", padding: "md" }}>
        <div style={{ grow: true, direction: "row", wrap: true, align: "start", gap: "md" }}>
          <Section title="Pointer">
            <div style={row}>
              <div key="div" style={box} {...pointer("div")}>Div</div>
              <Space key="shapes" width={84} height={36} background="fill" radius="md" {...pointer("space")}>
                <Rect x={8} y={8} width={32} height={20} radius={4} color="lavender" hover="pink" cursor="pointer" {...pointer("rect")} />
                <Circle x={62} y={18} radius={10} color="teal" />
              </Space>
              <Button label="Button" {...pointer("button")} />
              <Toggle label="T" on={() => memory.held} {...pointer("toggle")} />
            </div>
            <div style={row}>
              <div key="hold" style={() => ({ ...box, width: 200, background: memory.held ? "green/40" : "fill-strong" })}
                onPress={() => { memory.held = true; play(60, { hold: true }); log("note on"); }}
                onRelease={() => { memory.held = false; send("notes", { type: "noteOff", pitch: 60 }); log("note off"); }}>
                {() => (memory.held ? "Holds a note: drag off" : "Hold for a note")}
              </div>
              <Meter value="gate" />
            </div>
          </Section>

          <Section title="Field or kept">
            <div style={row}>
              <Knob field="cutoff" label="Field" min={100} max={8000} default={1200} unit="hz" />
              <Knob value={() => memory.gain} onChange={(gain) => { memory.gain = gain; log(`knob end ${gain.toFixed(2)}`); }}
                label="Kept" min={0} max={1} default={0.5} />
              <Meter value={() => memory.gain} />
              <Pad size={64} field={{ x: "cutoff", y: "level" }} />
              <Pad size={64} value={() => memory.pad} onChange={(pad) => { memory.pad = pad; log(`pad ${pad.x.toFixed(2)} ${pad.y.toFixed(2)}`); }} />
              <Fader length={64} min={0} max={1} default={0} value={() => memory.fader}
                onChange={(fader) => { memory.fader = fader; log(`fader end ${fader.toFixed(2)}`); }} />
            </div>
            <div style={row}>
              <NumberBox key="amount" width={32} min={1} max={16} value={() => memory.amount}
                onChange={(amount) => { memory.amount = amount; log(`number ${amount}`); }} />
              <Select value={() => memory.wave} options={WAVES} onChange={(wave) => { memory.wave = wave; log(`select ${wave}`); }} />
              <Slider field="level" min={-30} max={6} default={-6} unit="db" />
            </div>
            <div style={row}>
              <SegmentedControl field="mode" options={MODES} />
              <Switch fallback={() => "A square"}>
                <Match when={() => memory.wave === "sine"}>{() => <Text variant="label" style={{ color: "teal" }}>A sine</Text>}</Match>
                <Match when={() => memory.wave === "saw"}>{() => <Text variant="label" style={{ color: "peach" }}>A saw</Text>}</Match>
              </Switch>
            </div>
            <div style={row}>
              <Steps value={() => memory.steps} onChange={(steps) => { memory.steps = [...steps]; log(`steps ${steps.join("")}`); }} />
              <Button icon="minus" size="sm" variant="subtle" onClick={() => { memory.steps = memory.steps.slice(0, -1); }} />
              <Button icon="plus" size="sm" variant="subtle" onClick={() => { memory.steps = [...memory.steps, 0]; }} />
            </div>
          </Section>

          <Section title="Looks">
            <div style={row}>
              <div key="look" cursor="pointer" tooltip="A tooltip on a div" onClick={() => {}}
                style={{ ...box, hover: { background: "selected" }, pressed: { background: "lavender", color: "text-inverse" } }}>Press</div>
              <div disabled onClick={() => log("disabled div click")} style={box}>Disabled</div>
              <div disabled style={{ ...box, disabled: { background: "red/20", color: "red" } }}>Own look</div>
            </div>
            <div style={row}>
              <Button label="Primary" tooltip="A tooltip on a button" />
              <Button label="Subtle" variant="subtle" />
              <Button label="Solid" variant="solid" color="teal" />
              <Button icon="trash-2" variant="ghost" rounded />
              <Button label="Off" disabled onClick={() => log("disabled button click")} />
            </div>
            <div style={row}>
              <Button label="Small" size="sm" variant="subtle" />
              <Button label="Medium" variant="subtle" />
              <Button label="Large" size="lg" variant="subtle" />
            </div>
            <div style={row}>
              <Toggle label="M" on color="yellow" tooltip="A letter" />
              <Toggle icon="metronome" count={3} on={false} tooltip="An icon with a count" />
              <Toggle dot on color="red" tooltip="A dot" />
              <Toggle label="D" on disabled onClick={() => log("disabled toggle click")} />
            </div>
            <div style={row}>
              <div style={{ width: 120, height: 60, padding: "sm", radius: "lg", background: "surface", shadow: true }}>
                Shadow
                <div style={{ absolute: { top: -6, right: -6 }, width: 18, height: 18, radius: "full", align: "center", justify: "center", background: "red" }}>
                  <Text variant="label" style={{ color: "text-inverse" }}>3</Text>
                </div>
                <div style={{ absolute: { left: 0, right: 0, bottom: 0 }, height: 22, align: "center", justify: "center", background: "fill-strong" }}>Overlay</div>
              </div>
              <div style={{ width: 64, height: 40, radius: "lg", background: "fill", clip: true }}>
                <div style={{ width: 120, height: 24, background: "teal/40" }}>Clipped text</div>
              </div>
              <div style={{ direction: "row", wrap: true, gap: "xs", maxWidth: 96 }}>
                {Array.from({ length: 6 }, (_, index) => (
                  <div style={{ width: 28, height: 18, radius: "sm", align: "center", justify: "center", background: "fill-strong" }}>{index}</div>
                ))}
              </div>
            </div>
            <div style={row}>
              <div style={{ minWidth: 150, padding: "sm", background: "fill", textAlign: "end" }}>Aligned end</div>
              <div style={{ padding: "sm", background: "fill", opacity: 0.4 }}>Opacity 0.4</div>
            </div>
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
            <Input value="" placeholder="Enter adds a line, cmd-enter commits" rows={3} width={340}
              onCommit={(text) => log(`area commit ${JSON.stringify(text)}`)} />
          </Section>

          <Section title="Overlays and timers">
            <div style={row}>
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
            </div>
            <div style={row}>
              <Button label="Hold to repeat"
                onPress={() => { memory.repeats++; stopRepeat = interval(() => { memory.repeats++; }, 100); }}
                onRelease={() => { stopRepeat(); log(`repeated ${memory.repeats}`); }} />
              <Text variant="label" tone="text">{() => `${memory.repeats}`}</Text>
            </div>
            <div style={row}>
              <Button label="Flash" variant="subtle" onClick={() => { memory.flash = true; timeout(() => { memory.flash = false; }, 1000); }} />
              <Show when={() => memory.flash}>{() => <Text variant="label" style={{ color: "yellow" }}>Gone in a second</Text>}</Show>
              <Button label="Part" variant="subtle" onClick={() => { memory.part = !memory.part; }} />
              <Show when={() => memory.part}>{() => <Part />}</Show>
            </div>
            <Notice tone="warning" message="A notice with an action" action={{ label: "Act", onClick: () => log("notice action") }} onDismiss={() => log("notice dismissed")} />
          </Section>

          <Section title="Paths">
            <Space key="paths" width={348} height={100} background="well" radius="md">
              <Path points={[[10, 80], [60, 0, 110, 80]]} color="peach" width={2} />
              <Path points={[[130, 80], [150, 10, 200, 10, 220, 80]]} color="sky" width={2} />
              <Path points={[[250, 80], [290, 15], [330, 80], [250, 80]]} color="green" width={2} />
              <Path points={[[250, 92], [290, 60, 330, 92]]} color="green/40" />
              <Label x={10} y={82} text="quad" color="text-muted" />
              <Label x={130} y={82} text="cubic" color="text-muted" />
              <Label x={300} y={2} text="closed" color="text-muted" />
            </Space>
          </Section>
        </div>

        <div style={{ ...card, width: 220, gap: "xs" }}>
          <Text variant="heading">What code hears</Text>
          <For each={() => memory.log}>{(line) => <Text variant="label" tone="text">{line}</Text>}</For>
        </div>
      </div>
    );
  },
});
