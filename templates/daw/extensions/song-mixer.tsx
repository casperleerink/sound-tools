import { Fader, For, Fragment, h, Knob, Label, memo, now, NumberBox, Show, signal, Space, Text, Toggle, untrack, type Accessor, type Live, type Peaks } from "./sdk";
import { colourOf, lanesOf, Mark, mixOf, muted, rowLook, titleOf, type Child, type Page } from "./song-page";

// The mixer under the timeline: a strip per track and per return, and the master's.

/** As tall as leaves seven whole rows of tracks over it in a window of 1470 by 920 points. */
const MIXER = 298;
/** A strip: its fader, and beside it a column of what else it holds. */
const [STRIP, SIDE] = [88, 44];
/** A dial, and the line of a strip's name; a small toggle. */
const [DIAL, TOGGLE] = [28, 24];
/** The shortest fader, as tall as the column beside it; with more sends than fit under it the mixer scrolls. */
const SHORTEST = 128;
/** What a strip holds over and under its fader and its sends: the room around it and its name. */
const AROUND = 52;
/** The strips of the tracks, of the returns or the master's, between the full-height lines. */
const GROUP = { direction: "row", gap: "xs", paddingX: "sm", paddingTop: "sm", paddingBottom: "md", keep: true } as const;
/** What a strip holds, one under the other; the names of the sends left of the strips line up with it. */
const COLUMN = { gap: "xs", keep: true } as const;

/** The sends of a track or an audio track, `send-<return>`. */
export const sendsOf = (each: Child) => (each.tool === "track" || each.tool === "tape" ? each.children.filter((child) => child.name.startsWith("send-")).map((child) => child.name) : []);
/** What a send's knob is called: the title of its return, read once, as the knob opens. */
export const returnTitle = ({ returns }: Page, send: string) =>
  untrack(() => {
    const target = returns().find((one) => `send-${one.name}` === send);
    return target ? titleOf(target) : send.replace(/^send-/, "");
  });

const VOLUME = "Change the volume";
/** The volume of the fader `path` over `meter`, as the window drew a strip's. */
const Level = ({ path, length, meter }: { path: string; length: number; meter: Peaks }) => (
  <Fader field={`${path}.gain_db`} length={length} default={0} label={VOLUME} meter={meter} />
);
/** The gain of the fader `path`, to drag or type. */
const Gain = ({ path, width, align }: { path: string; width: number; align?: "center" }) => (
  <NumberBox key={`gain-${path}`} field={`${path}.gain_db`} unit="db" max={6} label={VOLUME} width={width} size="sm" height={16} align={align} />
);
/** A fader and under it its gain, in a column. */
export const Volume = ({ path, length, meter }: { path: string; length: number; meter: Peaks }) => (
  <>
    <Level path={path} length={length} meter={meter} />
    <Gain path={path} width={56} align="center" />
  </>
);

/** 1 while `value` moves and a second after: a drag, a key or a lane turns it. */
const moving = (value: Live) => value.smooth(60).minus(value).abs().gt(0.001).hold(1);
/**
 * What a dial plays, in words, so a mixer at rest says no number twice: it shows while its strip
 * is `told`, while its number moves, and `away` from where its dial rests, when that says something.
 */
const Reading = ({ field, unit, told, away }: { field: string; unit: "pan" | "db"; told: Accessor<boolean>; away?: (value: Live) => Live }) => {
  const value = now.value(field);
  const turned = away ? away(value).max(moving(value)) : moving(value);
  return (
    <Space width={SIDE} height={16}>
      <Label x={0} y={0} text={value} unit={unit} color="text" opacity={() => (told() ? 1 : turned)} />
    </Space>
  );
};

/**
 * A strip per track, which scroll, then the returns' and the master's, which stay at the right;
 * the master's limiter is in its panel. A strip is its name, its fader over its meter, and beside
 * that its pan, its gain, mute and solo, as a track's panel lays them out; under them a dial per
 * send, as a send takes the sound after the fader. A return's name is said once, left of the row
 * of its sends. At rest a strip says only the numbers that are not where they rest; under the
 * pointer it says them all. A click on a strip's name chooses it, a double click shows its panel.
 */
export const Mixer = ({ page }: { page: Page }) => {
  const { state, children, rows, returns, chosenTrack, choose, openPanel, openMaster, toggleMute, toggleSolo } = page;
  /** The strip the pointer is over. */
  const [over, point] = signal<string | null>(null);
  /** The returns some strip sends to, in their order: a row of dials each, so they line up across the tracks. */
  const sent = memo(() => returns().filter((back) => rows().some((each) => sendsOf(each).includes(`send-${back.name}`))));
  /** The faders take the room the sends leave. */
  const fader = memo(() => Math.max(MIXER - AROUND - sent().length * (DIAL + 4), SHORTEST));
  const Strip = ({ each }: { each?: Child }) => {
    const [name, path] = each ? [each.name, `${each.name}/mix`] : ["master", "master"];
    const told = () => over() === name;
    /** Its fader at 0 dB with no lane on it: its gain then needs no saying. */
    const rests = () => {
      const fader = each ? mixOf(each) : children.find((child) => child.name === "master");
      return fader?.tool === "fader" && (fader.state.gain_db ?? 0) === 0 && !(each && lanesOf(each).some(({ target }) => target === "mix.gain_db"));
    };
    return (
      <div style={{ ...COLUMN, width: STRIP }} onHover={(is) => (is ? point(name) : told() && point(null))}>
        <div key={`strip-name-${name}`} tabStop
          style={() => ({ height: DIAL, paddingX: "xs", radius: "md", direction: "row", align: "center", gap: "sm", ...rowLook(each !== undefined && chosenTrack() === each.name) })}
          onClick={({ clicks }) => (clicks === 2 ? (each ? openPanel(each.name) : openMaster()) : each && choose(new Set(), each.name))}>
          <Mark colour={each && (() => colourOf(each))} dim={() => each !== undefined && muted(each)} />
          <Text variant="label" tone="text" style={{ truncate: true }}>{() => (each ? titleOf(each) : "Master")}</Text>
        </div>
        {() => {
          const length = fader();
          return (
            <div style={{ direction: "row", gap: "sm", paddingX: "xs", keep: true }}>
              <Level path={path} length={length} meter={each ? { of: path } : "device"} />
              <div style={{ width: SIDE, height: length, gap: "xs" }}>
                <Knob field={`${path}.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar text="none" />
                <Reading field={`${path}.pan`} unit="pan" told={told} away={(pan) => pan.abs().gt(0.005)} />
                <div style={{ grow: true }} />
                <div style={{ height: 16, keep: true }}>
                  <Show when={() => told() || !rests()}>{() => <Gain path={path} width={SIDE} />}</Show>
                </div>
                {/* A strip without one keeps its room, so the gains of all strips are on one line. */}
                <div style={{ height: TOGGLE, keep: true }}>
                  {each ? <Toggle key={`strip-mute-${each.name}`} size="sm" label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} /> : null}
                </div>
                <div style={{ height: TOGGLE, keep: true }}>
                  {each && each.tool !== "bus" ? <Toggle key={`strip-solo-${each.name}`} size="sm" label="S" tooltip="Solo" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} /> : null}
                </div>
              </div>
            </div>
          );
        }}
        {each && each.tool !== "bus" ? (
          <For each={sent}>
            {(back) => (
              <div style={{ height: DIAL, direction: "row", align: "center", gap: "sm", paddingX: "xs", keep: true }}>
                {() => sendsOf(each).includes(`send-${back.name}`) ? (
                  <>
                    <Knob field={`${each.name}/send-${back.name}.gain_db`} label={untrack(() => titleOf(back))} min={-60} max={6} default={0} unit="db" text="none" />
                    <Reading field={`${each.name}/send-${back.name}.gain_db`} unit="db" told={told} />
                  </>
                ) : null}
              </div>
            )}
          </For>
        ) : null}
      </div>
    );
  };
  const Rule = () => <div style={{ width: 1, background: "border", keep: true }} />;
  return (
    <div key="mixer" style={{ height: MIXER, scroll: "y", keep: true }}>
      <div style={{ direction: "row", minHeight: MIXER, keep: true }}>
        <Show when={() => sent().length > 0}>
          {() => (
            <div style={{ ...COLUMN, width: 72, paddingLeft: "lg", paddingTop: "sm" }}>
              <div style={() => ({ height: DIAL + 4 + fader(), keep: true })} />
              <For each={sent}>
                {(back) => (
                  <div style={{ height: DIAL, justify: "center", keep: true }}>
                    <Text variant="label" style={{ truncate: true }}>{() => titleOf(back)}</Text>
                  </div>
                )}
              </For>
            </div>
          )}
        </Show>
        <div key="strips" style={{ direction: "row", scroll: "x", grow: true }}>
          <div style={GROUP}>
            <For each={rows}>{(each) => <Strip each={each} />}</For>
          </div>
        </div>
        <Show when={() => returns().length > 0}>
          {() => (
            <>
              <Rule />
              <div style={GROUP}>
                <For each={returns}>{(each) => <Strip each={each} />}</For>
              </div>
            </>
          )}
        </Show>
        <Rule />
        <div style={GROUP}>
          <Strip />
        </div>
      </div>
    </div>
  );
};
