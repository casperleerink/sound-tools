import { Arc, blend, Circle, For, Group, h, Label, Line, list, now, Rect, Space, text, Text, Toggle, tool, Wave, type Connection, type Declaration, type Live, type UtilityState } from "./sdk";

// Each `tone` child is a strip: it plays through the utility `<name>-mix`, whose gain, pan and
// mute the strip turns, into the utility `master`. The meter `<name>-meter` hears the mix.
const HEIGHT = 220;
const STRIP = 64;

// Where a level sits on a meter, 0 to 1: the fader scale of the window, 0 dB at 0.8.
const place = (amplitude: Live) => amplitude.pow(20 / 61.94).times(0.8).min(1);
const up = (fraction: Live) => fraction.times(-HEIGHT).plus(HEIGHT);

/** Two bars of a level, falling after the sound, with a peak line held 1.5 s. */
function Meter({ level, x }: { level: (channel: 0 | 1) => Live; x: number }) {
  return <Group>{([0, 1] as const).flatMap((channel) => {
    const bar = place(level(channel)).fall(0.5);
    const peak = place(level(channel)).hold(1.5).fall(0.5);
    const left = x + channel * 7;
    return [
      <Rect x={left} y={0} width={5} height={HEIGHT} color="fill" />,
      <Rect x={left} y={up(bar)} width={5} height={bar.times(HEIGHT)} color="green" />,
      <Rect x={left} y={up(peak)} width={5} height={1} color="text" opacity={peak.gt(0.01)} />,
    ];
  })}</Group>;
}

/** A fader over a meter: a drag up and down turns `gain_db` of the child `child`, natively. */
function Fader({ name, child, level }: { name: string; child: string; level: (channel: 0 | 1) => Live }) {
  const at = now.value(`${child}.gain_db`, { fraction: true });
  return (
    <Space key={`fader-${name}`} width={40} height={HEIGHT} cursor="ns-resize"
      drag={{ y: { value: `${child}.gain_db`, travel: HEIGHT, default: 0, label: "Change the volume" } }}>
      <Meter level={level} x={4} />
      <Rect x={22} y={0} width={2} height={HEIGHT} color="fill-strong" />
      <Rect x={16} y={up(at).minus(4)} width={16} height={8} radius={2} color="text" />
    </Space>
  );
}

/** A pan knob that shows the pan as it plays, so a cable into it moves it. */
function Pan({ name, child }: { name: string; child: string }) {
  const angle = now.value(`${child}.pan`, { played: true, fraction: true }).minus(0.5).times(0.75);
  return (
    <Space key={`pan-${name}`} width={40} height={36} cursor="ns-resize" drag={{ y: { value: `${child}.pan`, default: 0, label: "Change the pan" } }}>
      <Arc x={20} y={18} radius={12} from={-0.375} to={0.375} color="fill-strong" width={3} />
      <Arc x={20} y={18} radius={12} from={0} to={angle} color="text" width={3} />
    </Space>
  );
}

export const mixer = tool({
  name: "mixer",
  title: "Mixer",
  when: "You change the mixer page: its strips, meters, faders and what it shows of the master",
  doc: "A mixer of the `tone` children: each plays through the utility `<name>-mix` into the utility `master`, and the meter `<name>-meter` hears the mix, as `master-meter` hears the master. A strip turns the volume and the pan of its utility and mutes it; `solo` names the strips that play alone. A child `sway` moves the pan of the first strip.",
  state: { solo: list(text(), { max: 64 }) },
  behaviour: ({ state, children }): Declaration => {
    const strips = children.filter((child) => child.tool === "tone").map((child) => child.name);
    const soloed = state.solo.filter((name) => strips.includes(name));
    const connections: Connection[] = [];
    for (const name of strips) {
      connections.push({ from: { child: name }, to: { child: `${name}-mix` } });
      connections.push({ from: { child: `${name}-mix` }, to: { child: `${name}-meter` } });
      if (soloed.length === 0 || soloed.includes(name)) {
        connections.push({ from: { child: `${name}-mix` }, to: { child: "master" } });
      }
    }
    connections.push({ from: { child: "master" }, to: { port: "audio" } });
    connections.push({ from: { child: "master" }, to: { child: "master-meter" } });
    if (strips.length > 0 && children.some((child) => child.name === "sway")) {
      connections.push({ from: { child: "sway", number: "pan" }, to: { child: `${strips[0]}-mix`, number: "pan" } });
    }
    return { connections };
  },
  view: ({ state, children, update }) => {
    const strips = () => children.filter((child) => child.tool === "tone").map((child) => child.name);
    const muted = (name: string) => (children.find((child) => child.name === `${name}-mix`)?.state as UtilityState | undefined)?.mute === true;
    const toggleMute = (name: string) =>
      update("Mute a strip", ({ children }) => {
        const mix = children.find((child) => child.name === `${name}-mix`)?.state as UtilityState | undefined;
        if (mix) mix.mute = !mix.mute;
      });
    const toggleSolo = (name: string) =>
      update("Solo a strip", ({ state }) => {
        state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
      });
    return (
      <div style={{ direction: "row", gap: "lg", padding: "sm", background: "background" }}>
        <div style={{ direction: "row", gap: "xs" }}>
          <For each={strips}>
            {(name) => (
              <div style={{ width: STRIP, gap: "sm", align: "center" }}>
                <Pan name={name} child={`${name}-mix`} />
                <Fader name={name} child={`${name}-mix`} level={(channel) => now.peak(`${name}-meter`, channel)} />
                <Space width={STRIP} height={16}>
                  <Label x={STRIP / 2} y={0} text={now.value(`${name}-mix.gain_db`)} unit="db" color="text-muted" align="center" />
                </Space>
                <div style={{ direction: "row", gap: "xs" }}>
                  <Toggle label="M" size="sm" on={() => muted(name)} color="peach" onClick={() => toggleMute(name)} />
                  <Toggle label="S" size="sm" on={() => state.solo.includes(name)} color="yellow" onClick={() => toggleSolo(name)} />
                </div>
                <Text variant="label">{name}</Text>
              </div>
            )}
          </For>
        </div>
        <div style={{ width: STRIP, gap: "sm", align: "center" }}>
          <div style={{ height: 36 }} />
          <Fader name="master" child="master" level={(channel) => now.peak("master-meter", channel)} />
          <Text variant="label">master</Text>
        </div>
        <div style={{ gap: "sm" }}>
          <div style={{ direction: "row", gap: "sm", align: "center" }}>
            <Space width={16} height={16}>
              <Circle x={8} y={8} radius={5} color={blend("red/20", "red", now.elapsed.mod(1).lt(0.5))} />
            </Space>
            <Text variant="label">Live</Text>
          </div>
          <Text variant="label">Master, the last 20 ms</Text>
          <Space width={260} height={80} background="well" radius="sm">
            <Line from={[0, 40]} to={[260, 40]} color="border-strong" width={1} />
            <Wave x={0} y={0} width={260} height={80} list={now.scope("master-meter", { frames: 960 })} color="green" size={1.5} />
          </Space>
          <Text variant="label">Master level, the last 4 s</Text>
          <Space width={260} height={60} background="well" radius="sm">
            <Wave x={0} y={0} width={260} height={60} list={now.history(place(now.peak("master-meter")).fall(0.5), 240)} low={0} high={1} color="text" />
          </Space>
          <Text variant="label">Device out</Text>
          <Space width={30} height={HEIGHT}>
            <Meter level={(channel) => now.devicePeak(channel)} x={6} />
          </Space>
        </div>
      </div>
    );
  },
});
