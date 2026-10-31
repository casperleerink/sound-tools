import { Button, For, Fragment, h, Icon, Input, Label, Meter, now, onCleanup, Select, Show, Space, Toggle, untrack, type Color } from "./sdk";
import { bus } from "./bus";
import { chain } from "./chain";
import { snapsOf } from "./clip-editor";
import type { Song } from "./song";
import { addLane, LaneHead, learnName, showLanes } from "./song-automation";
import { copyOf } from "./song-clips";
import { ADD_LANE, BEAT, colourOf, COLOR, HEADER, inputOf, itemTool, lanesOf, MASTER_ROW, muted, PALETTE, ROW, RULER, TEMPO, titleOf, type Child, type Page } from "./song-page";
import { tape } from "./tape";
import { freeName, track } from "./track";

// The heads of the rows: a track's name and toggles left of its clips, its lanes' names, the
// returns, and the master; adding, moving, naming, arming and deleting tracks.

/** Over the timeline while the song has no track: what to start with, as the window's empty project says it. */
export const EmptySong = ({ page }: { page: Page }) => (
  <div style={{ absolute: { top: 0, left: 0, right: 0, bottom: 0 }, align: "center", justify: "center", gap: 4 }}>
    <div style={{ fontSize: 16, weight: "medium", color: "gray-950" }}>An empty song</div>
    <div style={{ fontSize: 14, color: "gray-700" }}>Add a track to play or record, or ask the agent for one.</div>
    <div style={{ direction: "row", gap: 8, paddingTop: 12 }}>
      <Button key="empty-instrument" label="Instrument track" icon="plus" variant="subtle" size="sm" onClick={() => addTrack(page, 0)} />
      <Button key="empty-audio" label="Audio track" icon="plus" variant="subtle" size="sm" onClick={() => addTrack(page, 1)} />
    </div>
  </div>
);

/** The column of heads left of the timeline, under the snap, which scrolls with it. */
export const Heads = ({ page }: { page: Page }) => {
  const { memory, layout } = page;
  return (
    <div style={{ width: HEADER - 1, keep: true }}>
      <div style={{ height: RULER, direction: "row", align: "center", gap: 8, paddingLeft: 8, paddingRight: 8 }}>
        <Toggle key="tempo-lane" icon="tempo-change" tooltip="Tempo" on={() => memory.tempoLane} onClick={() => (memory.tempoLane = !memory.tempoLane)} />
        <div style={{ grow: true, fontSize: 12, color: "gray-700" }}>Snap</div>
        <Select key="song-snap" title="Snap" value={() => memory.snap} options={snapsOf(4 * BEAT).map(({ label, ticks }) => ({ value: ticks, label }))}
          onChange={(snap) => (memory.snap = snap)} />
      </div>
      <Show when={() => memory.tempoLane}>
        {() => (
          <div style={{ height: TEMPO }}>
            <div style={{ grow: true, justify: "center", gap: 2, paddingLeft: 36, paddingRight: 8 }}>
              <div style={{ fontSize: 12, color: "gray-800" }}>Tempo</div>
              <div style={{ direction: "row", align: "center", gap: 3 }}>
                <Space key="tempo-value" width={28} height={16}><Label x={0} y={1} text={now.bpm.round()} size={12} color="gray-700" /></Space>
                <div style={{ fontSize: 12, color: "gray-700" }}>bpm</div>
              </div>
            </div>
            <div style={{ height: 1, background: "alpha/5", keep: true }} />
          </div>
        )}
      </Show>
      <div style={{ grow: true }} follow={{ y: "timeline" }}>
        <For each={layout}>
          {({ each, lane, add }) =>
            add ? (
              <div style={{ height: ADD_LANE, paddingX: 8, paddingY: 4 }}>
                <div key={`add-lane-${each.name}`} tabStop style={{ grow: true, direction: "row", align: "center", paddingLeft: 12, paddingRight: 10, radius: 6, background: "alpha/5", hover: { background: "alpha/8" } }}
                  onClick={() => addLane(page, each)}>
                  <div style={{ grow: true, fontSize: 13, weight: "medium", color: "gray-950" }}>Add lane</div>
                  <Icon name="chevron-down" size={14} color="gray-700" />
                </div>
              </div>
            ) : lane ? (
              <div style={{ height: ROW }}>
                <div style={{ height: 1, background: "alpha/5", keep: true }} />
                <div style={{ grow: true, direction: "row", align: "center", paddingLeft: 36, paddingRight: 8 }}>
                  <LaneHead page={page} track={each.name} lane={lane} />
                </div>
              </div>
            ) : (
              <div key={`head-${each.name}`} carry={`track:${each.name}`} onDrop={({ carried }) => carried?.startsWith("track:") && carried.slice(6) !== each.name && moveTrack(page, carried.slice(6), each.name)}
                style={{ height: ROW }}>
                <div style={{ height: 1, background: "alpha/5", keep: true }} />
                <TrackHead page={page} each={each} />
              </div>
            )
          }
        </For>
        <div style={{ height: 1, background: "alpha/5", keep: true }} />
        <div style={{ height: ROW, direction: "row", align: "center", paddingX: 8 }}>
          <div key="add-track" tabStop style={{ grow: true, direction: "row", align: "center", gap: 5, height: ROW - 8, paddingLeft: 9, paddingRight: 10, radius: 6, hover: { background: "alpha/5" } }}
            onClick={() => addTrack(page, 0)}>
            <Icon name="plus" size={14} color="gray-800" />
            <div style={{ fontSize: 12, weight: "medium", color: "gray-800" }}>Add track</div>
          </div>
          <div style={{ width: 1, height: 12, background: "alpha/10" }} />
          <div key="add-track-kind" tabStop tooltip="Add an audio or return track" style={{ width: 24, height: 24, align: "center", justify: "center", radius: 6, hover: { background: "alpha/8" } }} onClick={() => addTrack(page)}>
            <Icon name="chevron-down" size={14} color="gray-700" />
          </div>
        </div>
      </div>
    </div>
  );
};

/**
 * A track's header: its colour, its name, what plays it, and of its toggles those that are on,
 * all of them while the pointer is over it or its panel shows. A click anywhere on it but its
 * toggles shows its panel, a double click names it, a right press opens its menu.
 */
const TrackHead = ({ page, each }: { page: Page; each: Child }) => {
  const { state, children, memory, panelOf, openPanel, toggleMute, toggleSolo } = page;
  const all = () => memory.hovered === each.name || panelOf() === each.name;
  // A head drawn anew, as when lanes show, hears no leave of the one it replaces.
  onCleanup(() => memory.hovered === each.name && (memory.hovered = null));
  return (
    <div key={`track-head-${each.name}`} style={() => ({ height: ROW - 1, direction: "row", align: "center", gap: 8, paddingRight: 8, background: panelOf() === each.name && ![...memory.selected].some((key) => key.startsWith(`${each.name}/`) && itemTool(children, key) === "region") ? "alpha/5" : COLOR.clear })}
      onHover={(over) => (memory.hovered = over ? each.name : memory.hovered === each.name ? null : memory.hovered)}
      onPress={({ button }) => button === "right" && trackMenu(page, each)}
      onClick={({ clicks }) => (clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))} tabStop>
      <div style={() => ({ width: 3, height: ROW - 13, radius: 1.5, keep: true, background: muted(each) ? `${colourOf(each)}/40` as Color : colourOf(each) })} />
      <div style={{ grow: true, minWidth: 0, gap: 2, paddingLeft: 5 }}>
        <div style={{ direction: "row", align: "center", gap: 8 }}>
          {() =>
            memory.renaming === each.name ? (
              <Input value={() => titleOf(each)} width={120} focus onCommit={(title) => rename(page, each.name, title)} />
            ) : (
              <div key={`name-${each.name}`} style={() => ({ grow: true, fontSize: 14, weight: "medium", truncate: true, color: muted(each) ? "gray-900/40" : "gray-900" })}>
                {() => titleOf(each)}
              </div>
            )
          }
          {() => (memory.armed.has(each.name) ? <Meter key={`input-${each.name}`} peaks={{ input: inputOf(each) }} horizontal /> : null)}
        </div>
        <div style={{ height: 24, direction: "row", align: "center", gap: 4 }}>
          <div style={() => ({ grow: true, fontSize: 12, truncate: true, color: muted(each) ? "gray-700/40" : "gray-700" })}>
            {() => (each.tool === "tape" ? "Audio" : (memory.names[`${each.name}/synth`] ?? ""))}
          </div>
          {() => {
            // Runs again, and learns its name again, when another instrument or plugin takes its place.
            const synth = each.children.find((child) => child.name === "synth");
            const playing = synth && `${synth.tool} ${(synth.state as { plugin_id?: string }).plugin_id}`;
            if (playing) untrack(() => learnName(page, each.name, "synth"));
            return null;
          }}
          <Show when={() => all() || memory.lanes.has(each.name)}>
            {() => <Toggle key={`lanes-${each.name}`} icon="automation" tooltip="Automation" on={() => memory.lanes.has(each.name)}
              count={() => lanesOf(each).length} onClick={() => showLanes(page, each, !memory.lanes.has(each.name))} />}
          </Show>
          <Show when={() => each.tool === "tape" && (all() || memory.armed.has(each.name))}>
            {() => <Toggle key={`arm-${each.name}`} dot tooltip="Arm for recording" on={() => memory.armed.has(each.name)} color="red" onClick={() => toggleArm(page, each.name)} />}
          </Show>
          <Show when={() => all() || muted(each)}>
            {() => <Toggle key={`mute-${each.name}`} label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} />}
          </Show>
          <Show when={() => all() || state.solo.includes(each.name)}>
            {() => <Toggle key={`solo-${each.name}`} label="S" tooltip="Solo" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} />}
          </Show>
        </div>
      </div>
    </div>
  );
};

// As tall as the master row; four and a half show before they scroll, which says there are more.
const RETURN_ROW = MASTER_ROW;
/**
 * The returns, over the master row while the song has any: one row that folds them, with
 * their names when folded and a plus that adds one; open, a row each with its dot, its name
 * and M. A press shows its panel, a right-click its menu.
 */
export const Returns = ({ page }: { page: Page }) => {
  const { memory, returns } = page;
  return (
    <>
      <div style={{ height: 1, background: "alpha/5", keep: true }} />
      <div style={{ height: 31, direction: "row", align: "center", keep: true }}>
        <div style={{ width: HEADER - 1, height: 31, direction: "row", align: "center", paddingRight: 12, keep: true }}>
          <div key="returns-header" tabStop style={{ grow: true, height: 31, direction: "row", align: "center", gap: 5, paddingLeft: 17 }}
            onClick={() => (memory.returnsOpen = !memory.returnsOpen)}>
            <Icon name={() => (memory.returnsOpen ? "chevron-down" : "chevron-right")} size={14} color="gray-700" />
            <div style={{ fontSize: 12, color: "gray-900" }}>Returns</div>
            <div style={{ fontSize: 12, color: "gray-700" }}>{() => String(returns().length)}</div>
          </div>
          <Button key="add-return" icon="plus" variant="ghost" size="xs" tooltip="Add a return" onClick={() => addTrack(page, 2)} />
        </div>
        <div style={{ width: 1, height: 31, background: "alpha/5" }} />
        <div key="returns-folded" style={{ grow: true, height: 31, direction: "row", align: "center", gap: 16, paddingLeft: 16 }}
          onClick={() => (memory.returnsOpen = !memory.returnsOpen)}>
          <Show when={() => !memory.returnsOpen}>
            {() => (
              <For each={returns}>
                {(each) => (
                  <div style={{ direction: "row", align: "center", gap: 6 }}>
                    <div style={() => ({ width: 6, height: 6, radius: 3, keep: true, background: colourOf(each) })} />
                    <div style={{ fontSize: 12, color: "gray-700", truncate: true }}>{() => titleOf(each)}</div>
                  </div>
                )}
              </For>
            )}
          </Show>
        </div>
      </div>
      <Show when={() => memory.returnsOpen}>
        {() => (
          <div key="returns-list" style={() => ({ height: Math.min(returns().length, 4.5) * RETURN_ROW, scroll: "y", keep: true })}>
            <For each={returns}>{(each) => <ReturnRow page={page} each={each} />}</For>
          </div>
        )}
      </Show>
    </>
  );
};
const ReturnRow = ({ page, each }: { page: Page; each: Child }) => {
  const { memory, panelOf, openPanel, toggleMute } = page;
  return (
    <div style={{ height: RETURN_ROW, direction: "row", keep: true }}>
      <div style={{ width: HEADER - 1, height: RETURN_ROW, paddingX: 8, paddingY: 4, keep: true }}>
        <div key={`return-${each.name}`} tabStop style={() => ({ grow: true, direction: "row", align: "center", gap: 8, paddingLeft: 12, paddingRight: 4, radius: 6, background: panelOf() === each.name ? "alpha/5" : COLOR.clear })}
          onPress={({ button }) => button === "right" && trackMenu(page, each)}
          onClick={({ clicks }) => (clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))}>
          <div style={() => ({ width: 8, height: 8, radius: 4, keep: true, background: muted(each) ? `${colourOf(each)}/40` as Color : colourOf(each) })} />
          {() =>
            memory.renaming === each.name ? (
              <Input value={() => titleOf(each)} width={110} focus onCommit={(title) => rename(page, each.name, title)} />
            ) : (
              <div style={() => ({ grow: true, fontSize: 14, weight: "medium", truncate: true, color: muted(each) ? "gray-950/40" : "gray-950" })}>{() => titleOf(each)}</div>
            )
          }
          <Toggle key={`mute-${each.name}`} label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} />
        </div>
      </div>
      <div style={{ width: 1, height: RETURN_ROW, background: "alpha/5" }} />
    </div>
  );
};

/** The master's row: a press shows its panel; the button at its end shows the mixer. */
export const MasterRow = ({ page: { memory } }: { page: Page }) => (
  <div style={{ height: MASTER_ROW - 1, direction: "row", align: "center", keep: true }}>
    <div style={{ width: HEADER - 1, height: MASTER_ROW - 1, paddingX: 8, paddingY: 4, keep: true }}>
      <div key="master-row" tabStop style={() => ({ grow: true, direction: "row", align: "center", gap: 12, paddingLeft: 16, radius: 6, background: memory.below?.kind === "master" ? "alpha/5" : COLOR.clear })}
        onClick={() => (memory.below = { kind: "master" })}>
        <div style={{ width: 8, height: 8, radius: 4, borderWidth: 1.5, borderColor: "gray-800", keep: true }} />
        <div style={{ fontSize: 14, weight: "medium", color: "gray-900" }}>Master</div>
      </div>
    </div>
    <div style={{ width: 1, height: MASTER_ROW - 1, background: "alpha/5" }} />
    <div style={{ grow: true }} />
    <Button key="mixer-toggle" icon="sliders-horizontal" variant="ghost" size="xs" tooltip={() => (memory.below?.kind === "mixer" ? "Hide the mixer" : "Show the mixer")}
      onClick={() => (memory.below = memory.below?.kind === "mixer" ? null : { kind: "mixer" })} />
    <div style={{ width: 8 }} />
  </div>
);

function rename({ memory, update }: Page, name: string, title: string) {
  memory.renaming = null;
  update("Rename a track", ({ children }) => {
    const owner = children.find((child) => child.name === name);
    if (owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus") owner.state.title = title;
  });
}
/** Puts the track `moved` before `before` in `order`. */
const moveTrack = ({ update, rows }: Page, moved: string, before: string) =>
  update("Move a track", ({ state }) => {
    const names = rows().map((each) => each.name).filter((name) => name !== moved);
    names.splice(names.indexOf(before), 0, moved);
    state.order = names;
  });
/** Adds a track of `kind`: 0 an instrument track, 1 an audio track, 2 a return; the menu asks without one. */
async function addTrack({ children, memory, update, openMenu, rows, tracks, returns }: Page, kind?: number) {
  const picked = kind ?? (await openMenu([{ label: "Instrument track" }, { label: "Audio track" }, { label: "Return track" }]));
  if (picked === null) return;
  const names = children.map((child) => child.name);
  const colour = PALETTE[rows().length % PALETTE.length]!.colour;
  update(["Add a track", "Add an audio track", "Add a return"][picked]!, (part) => {
    const sends = part.children.filter((child) => child.tool === "bus").map((back) => ({ name: `send-${back.name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } }));
    if (picked === 0) {
      const name = freeName(names, "track");
      part.children.push({
        name, tool: track, state: { colour, title: `Track ${tracks().length + 1}` },
        children: [
          { name: "synth", tool: "instrument.synth", state: {} },
          { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, { name: "rec", tool: "recorder", state: {} },
          ...sends,
        ],
      });
      part.state.order = [...rows().map((each) => each.name), name];
    } else if (picked === 1) {
      const name = freeName(names, "audio-1") as `audio-${string}`;
      const audio = rows().filter((each) => each.tool === "tape").length + 1;
      part.children.push({ name, tool: tape, state: { colour, title: `Audio ${audio}` }, children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, ...sends] });
      part.state.order = [...rows().map((each) => each.name), name];
    } else {
      const name = [..."abcdefghijklmnopqrstuvwxyz"].map((letter) => `ret-${letter}` as const).find((each) => !names.includes(each)) ?? (freeName(names, "ret-z") as `ret-${string}`);
      part.children.push({ name, tool: bus, state: { title: `Return ${returns().length + 1}`, colour: PALETTE[(returns().length + 3) % PALETTE.length]!.colour }, children: [{ name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }] });
      for (const each of part.children) {
        if (each.tool === "track" || each.tool === "tape") each.children.push({ name: `send-${name}` as const, tool: "fader", state: { gain_db: "-inf" } });
      }
    }
  });
  if (picked === 2) memory.returnsOpen = true;
}
/** A clip's and a track's menu: with the tail, what reverbs and releases sound after the end, or without. */
export const RENDER_ITEMS = [{ label: "Render to audio track", tail: true, separator: true }, { label: "Render to audio track, no tail", tail: false }] as const;
/** Renders the clips and regions `keys` of the track `name`, from the first to the last, after its effects and before its fader, onto a new audio track under it that takes its fader, sends, their automation, its solo and what it keys after its fader, and mutes `name`: the song sounds as before. */
export async function renderToTrack({ update, render, rows, itemAt, lengthAt }: Page, name: string, keys: readonly string[], tail: boolean) {
  const source = rows().find((each) => each.name === name);
  const starts = keys.flatMap((key) => itemAt(key)?.state.start ?? []);
  if (!source || starts.length === 0) return;
  const [from, to] = [Math.min(...starts), Math.max(...keys.map((key) => (itemAt(key)?.state.start ?? 0) + lengthAt(key)))];
  const title = titleOf(source);
  const asset = await render(`${name}/chain`, { from, to, tail, name: title });
  if (asset === null) return;
  update("Render to audio track", (part) => {
    const owner = part.children.find((child) => child.name === name);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return;
    const audio = freeName(part.children.map((child) => child.name), "audio-1") as `audio-${string}`;
    const automation = owner.state.automation.filter(({ target }) => target.startsWith("mix.") || target.startsWith("send-"));
    const faders = owner.children.flatMap((child) => (child.name === "mix" || child.name.startsWith("send-")) && child.tool === "fader" ? [{ name: child.name as "mix" | `send-${string}`, tool: "fader" as const, state: { ...child.state } }] : []);
    const lanes = owner.children.flatMap((child) => automation.some(({ lane }) => lane === child.name) && child.tool === "lane" ? [{ name: child.name as `lane-${string}`, tool: "lane" as const, state: copyOf(child.state) }] : []);
    part.children.push({
      name: audio, tool: tape, state: { colour: owner.state.colour, title: `${title} (audio)`, automation: copyOf(automation) },
      children: [{ name: "rec", tool: "recorder", state: { channels: [0] } }, { name: "chain", tool: chain, state: {} }, ...faders, ...lanes, { name: "render", tool: "region", state: { asset, start: from } }],
    });
    const order = rows().map((each) => each.name);
    order.splice(order.indexOf(name) + 1, 0, audio);
    part.state.order = order;
    if (part.state.solo.includes(name)) part.state.solo = [...part.state.solo, audio];
    part.state.sidechains = part.state.sidechains.map((each) => (each.from === name && each.tap === "post_fader" ? { ...each, from: audio } : each));
    // A lane on its mute would play it again.
    owner.state.automation = owner.state.automation.filter(({ target }) => target !== "mix.mute");
    const mix = owner.children.find((child) => child.name === "mix");
    if (mix?.tool === "fader") mix.state.mute = true;
  });
}
async function trackMenu(page: Page, each: Child) {
  const { memory, update, openMenu } = page;
  const lanesShown = memory.lanes.has(each.name);
  const keys = each.tool === "track" || each.tool === "tape" ? each.children.filter((child) => child.tool === "timeline" || child.tool === "region").map((child) => `${each.name}/${child.name}`) : [];
  const renders = each.tool === "bus" ? [] : RENDER_ITEMS.map((item) => ({ ...item, disabled: keys.length === 0 }));
  const items = [{ label: "Rename" }, { label: "Colour…" }, { label: lanesShown ? "Hide automation" : "Show automation" },
    { label: "Add automation…" }, ...renders, { label: "Delete", shortcut: "backspace" as const, separator: renders.length > 0 }];
  const picked = await openMenu(items);
  const item = picked === null ? undefined : items[picked];
  if (item && "tail" in item) return renderToTrack(page, each.name, keys, item.tail);
  if (picked === 0) memory.renaming = each.name;
  else if (picked === 1) {
    const colour = await openMenu(PALETTE.map(({ name, colour }) => ({ label: name, checked: colourOf(each) === colour })));
    if (colour !== null) {
      update("Colour a track", ({ children }) => {
        const owner = children.find((child) => child.name === each.name);
        if (owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus") owner.state.colour = PALETTE[colour]!.colour;
      });
    }
  } else if (picked === 2) showLanes(page, each, !lanesShown);
  else if (picked === 3) addLane(page, each);
  else if (picked === items.length - 1) deleteRow(page, each.name);
}

// An armed track's input is open: its level shows, and a take has its sound from the first frame.
function toggleArm({ memory, send }: Page, name: string) {
  const armed = new Set(memory.armed);
  const on = !armed.delete(name);
  if (on) armed.add(name);
  memory.armed = armed;
  send({ child: `${name}/rec` as const, number: "arm" }, on ? 1 : 0);
  if (memory.recording !== null) send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
}

/**
 * The track the keyboard plays and records on: the chosen one, or the first in `order` while
 * none is; none while an audio track is chosen.
 */
export function notesTrackOf(children: readonly { name: string; tool: string }[], order: readonly string[], chosen: string | null): string | null {
  const rank = (name: string) => (order.includes(name) ? order.indexOf(name) : order.length);
  const tracks = children.filter((child) => child.tool === "track").sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
  if (chosen !== null && children.some((child) => child.name === chosen)) return tracks.some((each) => each.name === chosen) ? chosen : null;
  return tracks[0]?.name ?? null;
}

/**
 * Records from the playhead, or ends the recording: every armed track, and the notes of the
 * keyboard on its track. With nothing armed and an audio track chosen, it arms that one first.
 */
export function toggleRecordingOf({ memory, send, clock, children }: Pick<Song, "memory" | "send" | "clock" | "children">, order: readonly string[]) {
  const recordable = children.flatMap((each) => ((each.tool === "track" || each.tool === "tape") && each.children.some((child) => child.name === "rec") ? [each.name] : []));
  const record = (name: string, on: boolean) => send({ child: `${name}/rec` as const, number: "record" }, on ? 1 : 0);
  if (memory.recording !== null) {
    for (const name of recordable) record(name, false);
    memory.recording = null;
    return;
  }
  const chosen = children.find((each) => each.name === memory.track);
  if (memory.armed.size === 0 && chosen?.tool === "tape" && recordable.includes(chosen.name)) {
    memory.armed = new Set([chosen.name]);
    send({ child: `${chosen.name}/rec` as const, number: "arm" }, 1);
  }
  const notes = notesTrackOf(children, order, memory.track);
  memory.recording = clock.tick;
  for (const name of new Set([...memory.armed, ...(notes ? [notes] : [])])) if (recordable.includes(name)) record(name, true);
  clock.play();
}

/**
 * Deletes the track or return `name` once the composer says yes, as the arrangement asked: with
 * no clip chosen the keys go to the track, so one delete too many after a clip's would take it.
 */
export async function askToDelete(tool: Song, name: string) {
  const row = tool.children.find((child) => child.name === name);
  if (row?.tool !== "track" && row?.tool !== "tape" && row?.tool !== "bus") return;
  const title = row.state.title || name;
  const said = await tool.confirm(
    row.tool === "bus"
      ? { message: `Delete the return "${title}"?`, detail: "Its effects, automation and the sends into it go with it. Undo brings it back.", confirm: "Delete" }
      : { message: `Delete the track "${title}"?`, detail: "Its clips, devices and automation go with it. Undo brings it back.", confirm: "Delete" },
  );
  if (said) deleteRow(tool, name);
}

/** Deletes the track, audio track or return `name`, with the sends into it and what chose it. */
function deleteRow({ update, memory }: Pick<Song, "update" | "memory">, name: string) {
  update("Delete a track", ({ state, children }) => {
    state.order = state.order.filter((each) => each !== name);
    state.solo = state.solo.filter((each) => each !== name);
    state.sidechains = state.sidechains.filter(({ effect, from }) => from !== name && !effect.startsWith(`${name}/`));
    for (const each of children) {
      if (each.tool === "track") each.children = each.children.filter((child) => child.name !== `send-${name}`);
      if (each.tool === "tape") each.children = each.children.filter((child) => child.name !== `send-${name}`);
    }
    children.splice(0, children.length, ...children.filter((child) => child.name !== name));
  });
  if (memory.track === name) memory.track = null;
  const below = memory.below;
  if ((below?.kind === "track" && below.name === name) || (below?.kind === "editor" && below.clip.startsWith(`${name}/`))) memory.below = null;
  memory.selected = new Set([...memory.selected].filter((key) => !key.startsWith(`${name}/`)));
}
