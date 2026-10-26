import { Fader, For, Fragment, h, Knob, memo, NumberBox, Toggle, untrack, type Color, type Peaks } from "./sdk";
import { colourOf, muted, titleOf, type Child, type Page } from "./song-page";

// The mixer under the timeline: a strip per track and per return, and the master's.
const STRIP = 72;
const MIXER_FADER = 140;
/** A knob with its name and value under it. */
const KNOB = 72;

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

/** A strip per track, the returns' strips after them, and on the right the master's; its limiter is in the master's panel. */
export const Mixer = ({ page }: { page: Page }) => {
  const { state, memory, rows, returns, openPanel, toggleMute, toggleSolo } = page;
  const mostSends = memo(() => Math.max(0, ...rows().map((each) => sendsOf(each).length)));
  /**
   * A strip of the mixer, as the track panel's: a knob per send, its pan, its volume over its
   * meter, then mute and solo and its name. Every strip keeps the room of the most sends a
   * strip has and of mute and solo, so the faders line up.
   */
  const Strip = ({ each, master }: { each?: Child; master?: true }) => {
    const path = each ? `${each.name}/mix` : "master";
    const sends = () => (each ? sendsOf(each) : []);
    return (
      <div style={{ width: STRIP, align: "center" }}>
        <For each={sends}>{(send) => <Knob field={`${each!.name}/${send}.gain_db`} label={returnTitle(page, send)} min={-60} max={6} default={0} unit="db" />}</For>
        <div style={() => ({ height: (mostSends() - sends().length) * KNOB, keep: true })} />
        <Knob field={`${path}.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
        <div style={{ align: "center", gap: 7, paddingTop: 4 }}>
          <Volume path={path} length={MIXER_FADER} meter={master ? "device" : { of: path }} />
        </div>
        <div style={{ height: 32, direction: "row", align: "center", gap: 4 }}>
          {each ? <Toggle key={`strip-mute-${each.name}`} label="M" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} /> : null}
          {each && each.tool !== "bus" ? <Toggle key={`strip-solo-${each.name}`} label="S" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} /> : null}
        </div>
        <div key={each ? `strip-name-${each.name}` : "strip-name-master"} style={{ width: STRIP - 8, direction: "row", align: "center", justify: "center", gap: 5 }}
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
    <div key="mixer" style={{ direction: "row", keep: true }}>
      <div style={{ grow: true, direction: "row", gap: 4, paddingLeft: 8, paddingTop: 12, paddingBottom: 8, scroll: "x" }}>
        <For each={rows}>{(each) => <Strip each={each} />}</For>
        {() => (returns().length > 0 ? <div style={{ width: 1, background: "alpha/5", keep: true }} /> : null)}
        <For each={returns}>{(each) => <Strip each={each} />}</For>
      </div>
      <div style={{ width: 1, background: "alpha/5", keep: true }} />
      <div style={{ paddingLeft: 8, paddingRight: 8, paddingTop: 12, paddingBottom: 8, keep: true }}>
        <Strip master />
      </div>
    </div>
  );
};
