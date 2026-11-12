import { Fader, For, Fragment, h, Knob, memo, NumberBox, Show, Text, Toggle, untrack, type Peaks } from "./sdk";
import { colourOf, Mark, muted, rowLook, titleOf, type Child, type Page } from "./song-page";

// The mixer under the timeline: a strip per track and per return, and the master's.
const STRIP = 96;
const MIXER_FADER = 120;
/** The most the mixer is tall, with two rows of sends; more scroll. */
const MIXER_HEIGHT = 480;
/** A knob with its name and its value. */
const KNOB = 72;
/** The strips of the tracks, of the returns or the master's, between the full-height lines. */
const GROUP = { direction: "row", gap: "xs", paddingX: "sm", paddingTop: "sm", paddingBottom: "md", keep: true } as const;

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
    <Fader field={`${path}.gain_db`} length={length} default={0} label="Change the volume" meter={meter} />
    <NumberBox key={`gain-${path}`} field={`${path}.gain_db`} unit="db" max={6} label="Change the volume" width={56} size="sm" height={16} align="center" />
  </>
);

/**
 * A strip per track, then the returns' and the master's, which stays in view while the others
 * scroll; its limiter is in the master's panel. A strip reads from its name down: its pan, its
 * volume over its meter, mute and solo, and last a track's sends, as they take its sound after
 * its fader. A press on a name shows its panel.
 */
export const Mixer = ({ page }: { page: Page }) => {
  const { state, rows, returns, openPanel, openMaster, toggleMute, toggleSolo } = page;
  /** The returns some strip sends to, in their order: a row of knobs each, so they line up across the tracks. */
  const sent = memo(() => returns().filter((back) => rows().some((each) => sendsOf(each).includes(`send-${back.name}`))));
  const Strip = ({ each }: { each?: Child }) => {
    const path = each ? `${each.name}/mix` : "master";
    return (
      <div style={{ width: STRIP, align: "center", keep: true }}>
        <div key={each ? `strip-name-${each.name}` : "strip-name-master"} tabStop
          style={{ width: STRIP, height: 28, paddingX: "xs", radius: "md", direction: "row", align: "center", justify: "center", gap: "xs", ...rowLook(false) }}
          onClick={() => (each ? openPanel(each.name) : openMaster())}>
          <Mark colour={each && (() => colourOf(each))} dim={() => each !== undefined && muted(each)} />
          <Text variant="label" tone="text" style={{ truncate: true }}>{() => (each ? titleOf(each) : "Master")}</Text>
        </div>
        <Knob field={`${path}.pan`} label="Pan" min={-1} max={1} default={0} unit="pan" bipolar />
        <div style={{ align: "center", gap: "sm", paddingTop: "xs" }}>
          <Volume path={path} length={MIXER_FADER} meter={each ? { of: path } : "device"} />
        </div>
        <div style={{ height: 40, direction: "row", align: "center", gap: "xs" }}>
          {each ? <Toggle key={`strip-mute-${each.name}`} size="sm" label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} /> : null}
          {each && each.tool !== "bus" ? <Toggle key={`strip-solo-${each.name}`} size="sm" label="S" tooltip="Solo" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} /> : null}
        </div>
        {each && each.tool !== "bus" ? (
          <For each={sent}>
            {(back) => (
              <div style={{ height: KNOB, keep: true }}>
                {() => sendsOf(each).includes(`send-${back.name}`)
                  ? <Knob field={`${each.name}/send-${back.name}.gain_db`} label={untrack(() => titleOf(back))} min={-60} max={6} default={0} unit="db" />
                  : null}
              </div>
            )}
          </For>
        ) : null}
      </div>
    );
  };
  const Rule = () => <div style={{ width: 1, background: "border", keep: true }} />;
  return (
    <div key="mixer" style={{ maxHeight: MIXER_HEIGHT, scroll: "y", keep: true }}>
      <div style={{ direction: "row", keep: true }}>
        <div key="strips" style={{ direction: "row", scroll: "x" }}>
          <div style={GROUP}>
            <For each={rows}>{(each) => <Strip each={each} />}</For>
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
        </div>
        <Rule />
        <div style={GROUP}>
          <Strip />
        </div>
      </div>
    </div>
  );
};
