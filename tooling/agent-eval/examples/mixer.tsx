import { Arc, blend, Circle, h, Label, Line, list, Rect, Space, text, tool, Wave, type Connection, type Declaration, type Live, type UtilityState } from "./sdk";

// Each `tone` child is a strip: it plays through the utility `<name>-mix`, whose gain, pan and
// mute the strip turns, into the utility `master`.
const HEIGHT = 220;
const STRIP = 64;
const COLOR = {
  back: "#121317", track: "#1b1d23", green: "#7ee0a0", line: "#e9ebef", cap: "#a9b1ff", text: "#7c808c",
  on: "#a9b1ff", off: "#2a2d35", mute: "#f0b44c", solo: "#7ee0a0", dark: "#3a1418", red: "#ff4d5e",
} as const;

// Where a level sits on a meter, 0 to 1: the fader scale of the window, 0 dB at 0.8.
const place = (amplitude: Live) => amplitude.pow(20 / 61.94).times(0.8).min(1);
const up = (fraction: Live) => fraction.times(-HEIGHT).plus(HEIGHT);

/** Two bars of a level, falling after the sound, with a peak line held 1.5 s. */
function meter(level: (channel: 0 | 1) => Live, x: number) {
  return ([0, 1] as const).flatMap((channel) => {
    const bar = place(level(channel)).fall(0.5);
    const peak = place(level(channel)).hold(1.5).fall(0.5);
    const left = x + channel * 7;
    return [
      <Rect x={left} y={0} width={5} height={HEIGHT} color={COLOR.track} />,
      <Rect x={left} y={up(bar)} width={5} height={bar.times(HEIGHT)} color={COLOR.green} />,
      <Rect x={left} y={up(peak)} width={5} height={1} color={COLOR.line} opacity={peak.gt(0.01)} />,
    ];
  });
}

/** A fader over a meter: a drag up and down turns `field` of the child `child`, natively. */
function fader(key: string, child: string, level: (channel: 0 | 1) => Live, now: { childField: (child: string, name: string, options?: { fraction?: boolean }) => Live }) {
  const at = now.childField(child, "gain_db", { fraction: true });
  return (
    <Space key={key} width={40} height={HEIGHT} cursor="ns-resize"
      drag={{ y: { field: "gain_db", child, travel: HEIGHT, default: 0, label: "Change the volume" } }}>
      {meter(level, 4)}
      <Rect x={22} y={0} width={2} height={HEIGHT} color={COLOR.track} />
      <Rect x={16} y={up(at).minus(4)} width={16} height={8} radius={2} color={COLOR.cap} />
    </Space>
  );
}

/** A pan knob that shows the pan as it plays, so a cable into it moves it. */
function pan(key: string, child: string, now: { played: (name: `${string}.${string}`, options?: { fraction?: boolean }) => Live }) {
  const angle = now.played(`${child}.pan`, { fraction: true }).minus(0.5).times(0.75);
  return (
    <Space key={key} width={40} height={36} cursor="ns-resize" drag={{ y: { field: "pan", child, default: 0, label: "Change the pan" } }}>
      <Arc x={20} y={18} radius={12} from={-0.375} to={0.375} color={COLOR.track} width={3} />
      <Arc x={20} y={18} radius={12} from={0} to={angle} color={COLOR.cap} width={3} />
    </Space>
  );
}

function button(label: string, on: boolean, color: string, onClick: () => void) {
  return (
    <div style={{ width: 22, height: 18, radius: 4, align: "center", justify: "center", fontSize: 10, background: on ? (color as `#${string}`) : COLOR.off }} onClick={onClick}>
      {label}
    </div>
  );
}

export const mixer = tool({
  name: "mixer",
  title: "Mixer",
  when: "You change the mixer page: its strips, meters, faders and what it shows of the master",
  doc: "A mixer of the `tone` children: each plays through the utility `<name>-mix` into the utility `master`. A strip turns the volume and the pan of its utility and mutes it; `solo` names the strips that play alone. A child `sway` moves the pan of the first strip.",
  state: { solo: list(text(), { max: 64 }) },
  behaviour: ({ state, children }): Declaration => {
    const strips = children.filter((child) => child.tool === "tone").map((child) => child.name);
    const soloed = state.solo.filter((name) => strips.includes(name));
    const connections: Connection[] = [];
    for (const name of strips) {
      connections.push({ from: { child: name }, to: { child: `${name}-mix` } });
      connections.push({ from: { child: `${name}-mix` }, to: { meter: name } });
      if (soloed.length === 0 || soloed.includes(name)) {
        connections.push({ from: { child: `${name}-mix` }, to: { child: "master" } });
      }
    }
    connections.push({ from: { child: "master" }, to: { port: "audio" } });
    connections.push({ from: { child: "master" }, to: { meter: "master" } });
    if (strips.length > 0 && children.some((child) => child.name === "sway")) {
      connections.push({ from: { child: "sway", number: "pan" }, to: { child: `${strips[0]}-mix`, number: "pan" } });
    }
    return { connections };
  },
  page: ({ state, children, update, now }) => {
    const strips = children.filter((child) => child.tool === "tone").map((child) => child.name);
    const mixOf = (name: string) => children.find((child) => child.name === `${name}-mix`)?.state as UtilityState | undefined;
    const muted = (name: string) => mixOf(name)?.mute === true;
    const toggleMute = (name: string) =>
      update("Mute a strip", ({ children }) => {
        const mix = children.find((child) => child.name === `${name}-mix`)?.state as UtilityState | undefined;
        if (mix) mix.mute = !mix.mute;
      });
    const toggleSolo = (name: string) =>
      update("Solo a strip", ({ state }) => {
        state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
      });
    const master = (channel: 0 | 1) => now.peak("master", channel);
    return (
      <div style={{ direction: "row", gap: 16, padding: 8, background: COLOR.back }}>
        <div style={{ direction: "row", gap: 4 }}>
          {strips.map((name) => (
            <div key={name} style={{ width: STRIP, gap: 6, align: "center" }}>
              {pan(`pan-${name}`, `${name}-mix`, now)}
              {fader(`fader-${name}`, `${name}-mix`, (channel) => now.peak(name, channel), now)}
              <Space width={STRIP} height={14}>
                <Label x={STRIP / 2} y={0} text={now.childField(`${name}-mix`, "gain_db")} unit="db" size={10} color={COLOR.text} align="center" />
              </Space>
              <div style={{ direction: "row", gap: 4 }}>
                {button("M", muted(name), COLOR.mute, () => toggleMute(name))}
                {button("S", state.solo.includes(name), COLOR.solo, () => toggleSolo(name))}
              </div>
              <div style={{ fontSize: 11, color: COLOR.text }}>{name}</div>
            </div>
          ))}
        </div>
        <div style={{ width: STRIP, gap: 6, align: "center" }}>
          <div style={{ height: 36 }} />
          {fader("fader-master", "master", master, now)}
          <div style={{ fontSize: 11, color: COLOR.text }}>master</div>
        </div>
        <div style={{ gap: 10 }}>
          <div style={{ direction: "row", gap: 6, align: "center" }}>
            <Space width={16} height={16}>
              <Circle x={8} y={8} radius={5} color={blend(COLOR.dark, COLOR.red, now.time.mod(1).lt(0.5))} />
            </Space>
            <div style={{ fontSize: 11, color: COLOR.text }}>Live</div>
          </div>
          <div style={{ fontSize: 11, color: COLOR.text }}>Master, the last 20 ms</div>
          <Space width={260} height={80} background={COLOR.track} radius={4}>
            <Line from={[0, 40]} to={[260, 40]} color={COLOR.off} width={1} />
            <Wave x={0} y={0} width={260} height={80} list={now.scope("master", { frames: 960 })} color={COLOR.green} size={1.5} />
          </Space>
          <div style={{ fontSize: 11, color: COLOR.text }}>Master level, the last 4 s</div>
          <Space width={260} height={60} background={COLOR.track} radius={4}>
            <Wave x={0} y={0} width={260} height={60} list={now.history(place(now.peak("master")).fall(0.5), 240)} low={0} high={1} color={COLOR.cap} />
          </Space>
          <div style={{ fontSize: 11, color: COLOR.text }}>Device out</div>
          <Space width={30} height={HEIGHT}>{meter((channel) => now.output(channel), 6)}</Space>
        </div>
      </div>
    );
  },
});
