import { Fader, For, Fragment, h, Knob, memo, NumberBox, Show, Toggle, untrack, type Color, type Peaks } from "./sdk";
import { colourOf, muted, titleOf, type Child, type Page } from "./song-page";

// The mixer under the timeline: a strip per track and per return, and the master's.
const STRIP = 72;
const MIXER_FADER = 140;
/** The most the mixer is tall, with three rows of sends; more scroll, with the faders under them. */
const MIXER_HEIGHT = 480;
/** A knob with its value under it; its name is in the column of names. */
const KNOB = 56;
/** The column of names at the left of the strips, which stays as they scroll: what each row of knobs turns. */
const NAMES = 56;
/** The strips of the tracks, of the returns or the master's, between the full-height lines. */
const GROUP = { direction: "row", gap: 4, paddingX: 8, paddingTop: 12, paddingBottom: 8, keep: true } as const;

/** The sends of a track or an audio track, `send-<return>`. */
export const sendsOf = (each: Child) => (each.tool === "track" || each.tool === "tape" ? each.children.filter((child) => child.name.startsWith("send-")).map((child) => child.name) : []);
/** What a send's knob is called: the title of its return, read once, as the knob opens. */
export const returnTitle = ({ returns }: Page, send: string) =>
  untrack(() => {
    const target = returns().find((one) => `send-${one.name}` === send);
    return target ? titleOf(target) : send.replace(/^send-/, "");
  });

/** The volume of the fader `path` over `meter`, as the window drew a strip's, and under it its gain, to drag or type. */
export const Volume = ({ path, length, meter }: { path: string; length: number; meter: Peaks }) => (
  <>
    <Fader field={`${path}.gain_db`} length={length} default={0} label="Change the volume" played meter={meter} />
    <NumberBox key={`gain-${path}`} field={`${path}.gain_db`} unit="db" played max={6} label="Change the volume" width={56} height={16} size={12} align="center" />
  </>
);

/** A strip per track, the returns' strips after them at the right while they fit, then the master's, which stays in view while they scroll; its limiter is in the master's panel. */
export const Mixer = ({ page }: { page: Page }) => {
  const { state, memory, rows, returns, openPanel, toggleMute, toggleSolo } = page;
  /** The returns some strip sends to, in their order: a row of knobs each. */
  const sent = memo(() => returns().filter((back) => rows().some((each) => sendsOf(each).includes(`send-${back.name}`))));
  /** What each row of knobs turns, said once at the left of the strips. */
  const Names = () => {
    const name = (text: () => string) => <div style={{ height: KNOB, paddingTop: 10, fontSize: 12, color: "gray-700", truncate: true }}>{text}</div>;
    return (
      <div style={{ width: NAMES, keep: true }}>
        <For each={sent}>{(back) => name(() => titleOf(back))}</For>
        {name(() => "Pan")}
      </div>
    );
  };
  /**
   * A strip of the mixer, as the track panel's: a knob in each row of sends, its pan, its volume
   * over its meter, then mute and solo and its name. A strip that does not send to a return keeps
   * its row empty, so the rows and the faders line up.
   */
  const Strip = ({ each, master }: { each?: Child; master?: true }) => {
    const path = each ? `${each.name}/mix` : "master";
    return (
      <div style={{ width: STRIP, align: "center" }}>
        <For each={sent}>
          {(back) => (
            <div style={{ height: KNOB, keep: true }}>
              {() => each && sendsOf(each).includes(`send-${back.name}`)
                ? <Knob field={`${each.name}/send-${back.name}.gain_db`} label={untrack(() => titleOf(back))} bare min={-60} max={6} default={0} unit="db" />
                : null}
            </div>
          )}
        </For>
        <Knob field={`${path}.pan`} label="Pan" bare min={-1} max={1} default={0} unit="pan" bipolar />
        <div style={{ align: "center", gap: 7, paddingTop: 4 }}>
          <Volume path={path} length={MIXER_FADER} meter={master ? "device" : { of: path }} />
        </div>
        <div style={{ height: 32, direction: "row", align: "center", gap: 4 }}>
          {each ? <Toggle key={`strip-mute-${each.name}`} label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} /> : null}
          {each && each.tool !== "bus" ? <Toggle key={`strip-solo-${each.name}`} label="S" tooltip="Solo" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} /> : null}
        </div>
        <div key={each ? `strip-name-${each.name}` : "strip-name-master"} tabStop
          style={{ width: STRIP - 4, height: 22, paddingX: 4, radius: 6, direction: "row", align: "center", justify: "center", gap: 5, hover: { background: "alpha/5" } }}
          onClick={() => (each ? openPanel(each.name) : (memory.below = { kind: "master" }))}>
          <div style={() => ({
            width: 6, height: 6, radius: 3, keep: true,
            ...(each ? { background: muted(each) ? (`${colourOf(each)}/40` as Color) : colourOf(each) } : { borderWidth: 1, borderColor: "gray-800" }),
          })} />
          <div style={{ fontSize: 12, color: "gray-800", truncate: true }}>{() => (each ? titleOf(each) : "Master")}</div>
        </div>
      </div>
    );
  };
  return (
    <div key="mixer" style={{ maxHeight: MIXER_HEIGHT, scroll: "y", keep: true }}>
      <div style={{ direction: "row", keep: true }}>
      <div style={{ ...GROUP, paddingRight: 0 }}>
        <Names />
      </div>
      <div key="strips" style={{ grow: true, direction: "row", scroll: "x" }}>
        <div style={GROUP}>
          <For each={rows}>{(each) => <Strip each={each} />}</For>
        </div>
        <Show when={() => returns().length > 0}>
          {() => (
            <>
              <div style={{ grow: true }} />
              <div style={{ width: 1, background: "alpha/5", keep: true }} />
              <div style={GROUP}>
                <For each={returns}>{(each) => <Strip each={each} />}</For>
              </div>
            </>
          )}
        </Show>
      </div>
      <div style={{ width: 1, background: "alpha/5", keep: true }} />
      <div style={GROUP}>
        <Strip master />
      </div>
      </div>
    </div>
  );
};
