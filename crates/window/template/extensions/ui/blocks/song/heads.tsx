import { h, live, memo, type Offer, type Pointer } from "../../../sdk";
import { colourOf, lanesOf, muted, PALETTE, titleOf } from "../../../song/model";
import { cannotRender } from "../../../song/tracks";
import { chosenTrack, deviceName, HEAD, laneGroups, LOW_ROW, PLANE, RETURNS, ROW, type Page, type Row, type Strip, type Track } from "../../../song/view";
import { freeName } from "../../../track";
import { Button } from "../../button";
import { Dot } from "../../dot";
import { keysOf } from "../../kbd";
import { MenuList, type Item } from "../../menu";
import { Quiet } from "../../quiet";
import { Meter } from "../../meter";
import { TextField } from "../../text-field";
import { Toggle } from "../../toggle";
import { LaneHeads } from "./lanes";
import { InputLevel } from "./takes";

export type HeadsProps = { page: Page };

/** How much of a muted track shows: its colour, its name and its clips. */
export const DIM = 0.4;

/**
 * The column left of the timeline: a head for each track and its lanes, which scroll with it, the
 * row that adds a track, and under them the returns and the master, which stay.
 */
export const Heads = ({ page }: HeadsProps) => {
  const { song, rows, returns, tracks, height } = page;
  const { memory } = song;
  return (
    <col w={HEAD} h="full" border="border" borderSides={[0, 1, 0, 0]}>
      <box key="heads" label="Tracks" grow={1} w="full" plane={{ y: PLANE }}>
        {rows.map((row) => (row.kind === "track" ? <Head page={page} row={row} /> : null))}
        {laneGroups(rows).map((lanes) => <LaneHeads page={page} rows={lanes} />)}
        {tracks.length > 0 && (
          <row place={[0, height, HEAD - 1, 1]} pad={[1, 0]} align="center" border="border" borderSides={[1, 0, 0, 0]}>
            <Button id="add-track" label="Add track" text="Add track" icon="plus" variant="ghost" size="sm" onPress={() => addTrack(page)} />
            <Button id="add-track-kind" label="Add a track of a kind" icon="chevron-down" variant="ghost" size="sm" tip="Add an instrument, audio or return track">
              <MenuList id="add-track-kind" label="Add a track of a kind" items={kinds(page)} w={240} />
            </Button>
          </row>
        )}
      </box>
      {returns.length > 0 && (
        <col w="full" border="border" borderSides={[1, 0, 0, 0]}>
          <row w="full" h={LOW_ROW} pad={2} gap={1} align="center">
            <row key="returns-header" label={memory.returnsOpen ? "Fold the returns" : "Show the returns"} role="button" grow={1} h={LOW_ROW - 8} pad={[1, 0]} gap={1} align="center" radius="md" hover="fill" onPress={() => (memory.returnsOpen = !memory.returnsOpen)}>
              <icon name={memory.returnsOpen ? "chevron-down" : "chevron-right"} size={12} color="text-muted" />
              <text style="label" color="text">Returns</text>
              <text style="label" color="text-muted">{returns.length}</text>
            </row>
            <Button id="add-return" label="Add a return" icon="plus" variant="ghost" size="sm" tip="Add a return" onPress={() => page.edits.tracks.add("return")} />
          </row>
          {memory.returnsOpen && (
            <col key="returns-list" label="Returns" w="full" maxH={RETURNS * LOW_ROW} scroll="y">
              {returns.map((each) => <Low page={page} each={each} />)}
            </col>
          )}
        </col>
      )}
      <MasterRow page={page} />
    </col>
  );
};

/** What the plus of "Add track" adds: a track of the first instrument the catalog has, as the empty song's. */
const addTrack = (page: Page, offer?: Offer) => {
  const { song, edits } = page;
  const name = freeName(song.children.map((child) => child.name), "track");
  edits.tracks.add("instrument");
  // A track comes with the synth: another instrument takes its place.
  if (offer && offer.key !== "instrument.synth") edits.effects(name).replace(offer.key);
};

/** The tracks the chevron of "Add track" offers: one of each instrument there is, an audio track and a return. */
const kinds = (page: Page): Item[] => [
  ...page.song.catalog.filter((offer) => offer.takes === "notes" && offer.unavailable === null).map((offer) => ({ text: `${offer.name} track`, icon: offer.icon, onPress: () => addTrack(page, offer) })),
  { text: "Audio track", icon: "audio-lines", onPress: () => page.edits.tracks.add("audio") },
  { text: "Return track", icon: "send", onPress: () => page.edits.tracks.add("return") },
];

/** What plays a track, as its head says under its name: its instrument, or that it plays audio. */
function kindOf(each: Strip, catalog: readonly Offer[]): string {
  if (each.tool === "tape") return "Audio";
  if (each.tool === "bus") return "Return";
  const synth = each.children.find((child) => child.name === "synth");
  return synth ? deviceName(catalog, synth) : "";
}

/** The keyed element under the pointer at the end of a drag, as the track it is in. */
const trackUnder = (page: Page, over: string | undefined) => page.tracks.find((each) => over === `track-${each.name}` || over?.endsWith(`-${each.name}`))?.name;

/** A press on the head of a track or a return shows its panel, a double press names it; a right press is its menu's. */
const pressHead = (page: Page, name: string) => (p: Pointer) => {
  if (p.right) return;
  if (p.clicks === 2) page.song.memory.renaming = name;
  else page.edits.tracks.openPanel(name);
};

/** The menu of a track's or a return's name: rename, its lanes, render it, delete it. */
function trackMenu(page: Page, each: Strip): Item[] {
  const { song } = page;
  const items: Item[] = [{ text: "Rename", icon: "pencil", keys: keysOf(song.shortcut("rename_track")), onPress: () => (page.song.memory.renaming = each.name) }];
  if (each.tool !== "bus") {
    const shown = song.memory.lanes.includes(each.name);
    items.push({ text: shown ? "Hide automation" : "Show automation", icon: "automation", onPress: () => page.edits.lanes.show(each.name, !shown) });
    const keys = each.children.flatMap((child) => (child.tool === "timeline" || child.tool === "region" ? [`${each.name}/${child.name}`] : []));
    items.push(...renders(page, each.name, () => keys, keys.length === 0 ? "The track has no clips" : cannotRender(each)));
  }
  items.push({ text: "Delete", icon: "trash-2", danger: true, keys: keysOf(song.shortcut("delete_track")), onPress: () => void page.edits.tracks.askToRemove(each.name) });
  return items;
}

/** Render to audio track, with what rings out after the end or without: of the clips `keys` gives of `track`, or why it cannot. */
export const renders = (page: Page, track: string, keys: () => string[], why: string | undefined): Item[] =>
  [true, false].map((tail) => ({
    text: tail ? "Render to audio track" : "Render without the tail",
    detail: tail ? "Reverbs and releases ring out after the end" : "It stops where the clips end",
    icon: "audio-lines",
    ...(why === undefined ? {} : { disabled: why }),
    onPress: () => void page.edits.tracks.render(track, keys(), tail),
  }));

/** The colours a track can have, its own ticked. */
const colours = (page: Page, each: Strip): Item[] =>
  PALETTE.map(({ name, colour }) => ({ text: name, checked: colourOf(each) === colour, onPress: () => page.edits.tracks.colour(each.name, colour) }));

/** A track's name, or the field that renames it in place. */
const Name = ({ page, each, title }: { page: Page; each: Strip; title: string }) =>
  page.song.memory.renaming === each.name ? (
    <TextField id={`rename-${each.name}`} label={`Name of ${title}`} value={title} focus size="sm" w={120} onCommit={(text) => page.edits.tracks.rename(each.name, text)} />
  ) : (
    <text style="heading" color="text" opacity={muted(each) ? DIM : 1} truncate>{title}</text>
  );

/**
 * A track's head: its colour, which picks another, its name, what plays it, and of its toggles
 * those that are on, all of them while it is pointed at or chosen. A press shows its panel, a
 * double click names it, a right press opens its menu, a drag onto another head puts it there.
 */
const Head = ({ page, row }: { page: Page; row: Row }) => {
  const { song, tracks } = page;
  const { memory } = song;
  const each: Track = row.track;
  const title = titleOf(each);
  const kind = kindOf(each, song.catalog);
  const chosen = chosenTrack(memory) === each.name;
  const [isMuted, soloed, armed, colour] = [muted(each), song.state.solo.includes(each.name), memory.armed.includes(each.name), colourOf(each)];
  const [lanes, shown, renaming] = [lanesOf(each).length, memory.lanes.includes(each.name), memory.renaming === each.name];
  const keys = [song.shortcut("rename_track"), song.shortcut("delete_track")];
  const order = tracks.map((track) => track.name).join(" ");
  const key = `track-${each.name}`;
  return memo(`head-${each.name}`, [title, kind, chosen, isMuted, soloed, armed, colour, row.top, lanes, shown, renaming, order, ...keys, each.children], () => (
    <row key={key} label={`Choose ${title}`} role="tab" value={chosen} hoverGroup place={[0, live.drag(key).dy.plus(row.top), HEAD - 1, 1]} opacity={live.drag(key).active.times(-0.4).plus(1)}
      pad={[2, 0]} gap={2} align="center" fill={chosen ? "selected" : undefined} hover="fill" border="border" borderSides={[0, 0, 1, 0]}
      drag={{ y: true }} onDragEnd={(p) => { const before = trackUnder(page, p.over); if (before) page.edits.tracks.moveBefore(each.name, before); }}
      onPress={pressHead(page, each.name)}>
      <row key={`colour-${each.name}`} label={`Colour of ${title}`} role="button" w={12} h={ROW - 16} align="center" justify="center" radius="sm" hover="fill">
        <box w={4} h={ROW - 16} radius="full" fill={colour} opacity={isMuted ? DIM : 1} />
        <MenuList id={`colour-${each.name}`} label={`Colour of ${title}`} items={colours(page, each)} w={180} />
      </row>
      <col grow={1} gap={1}>
        <row h={24} gap={2} align="center">
          <row grow={1} gap={2} align="center"><Name page={page} each={each} title={title} />{armed && <InputLevel path={`${each.name}/rec`} title={title} />}</row>
          {each.tool === "tape" && <Quiet shown={armed || chosen}><Toggle id={`arm-${each.name}`} label={`Arm ${title}`} on={armed} color="danger" dot tip="Arm for recording" onToggle={() => page.edits.tracks.arm(each.name)} /></Quiet>}
        </row>
        <row h={24} gap={1} align="center">
          <row grow={1}><text style="label" color="text-muted" opacity={isMuted ? DIM : 1} truncate>{kind}</text></row>
          <Quiet shown={shown || chosen}><Toggle id={`lanes-${each.name}`} label={`Automation of ${title}`} icon="automation" on={shown} tip={lanes > 0 ? `Automation: ${lanes}` : "Automation"} onToggle={() => page.edits.lanes.show(each.name, !shown)} /></Quiet>
          <Quiet shown={isMuted || chosen}><Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" tip="Mute" onToggle={() => page.edits.tracks.mute(each.name)} /></Quiet>
          <Quiet shown={soloed || chosen}><Toggle id={`solo-${each.name}`} label={`Solo ${title}`} text="S" on={soloed} color="hot" tip="Solo" onToggle={() => page.edits.tracks.solo(each.name)} /></Quiet>
        </row>
      </col>
      <Meter id={`meter-head-${each.name}`} label={`Level ${title}`} path={`${each.name}/mix`} w={4} h={ROW - 16} quiet />
      <MenuList id={`menu-${each.name}`} label={`Menu of ${title}`} items={trackMenu(page, each)} open="menu" />
    </row>
  ));
};

/** The row of a return under the heads: a press shows its panel, a double click names it, a right press opens its menu. */
const Low = ({ page, each }: { page: Page; each: Strip }) => {
  const title = titleOf(each);
  const isMuted = muted(each);
  const chosen = chosenTrack(page.song.memory) === each.name;
  return (
    <row key={`track-${each.name}`} label={`Choose ${title}`} role="tab" value={chosen} w="full" h={LOW_ROW} pad={[3, 0]} gap={2} align="center" fill={chosen ? "selected" : undefined} hover="fill"
      onPress={pressHead(page, each.name)}>
      <box opacity={isMuted ? DIM : 1}><Dot color={colourOf(each)} /></box>
      <row grow={1}><Name page={page} each={each} title={title} /></row>
      <Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" tip="Mute" onToggle={() => page.edits.tracks.mute(each.name)} />
      <MenuList id={`menu-${each.name}`} label={`Menu of ${title}`} items={trackMenu(page, each)} open="menu" />
    </row>
  );
};

/** The master's row: a press shows its panel; the toggle at its end shows the mixer. */
const MasterRow = ({ page }: { page: Page }) => {
  const { memory } = page.song;
  const [chosen, mixer] = [memory.below?.kind === "master", memory.below?.kind === "mixer"];
  return (
    <row key="master" label="Master" role="tab" value={chosen} w="full" h={LOW_ROW} pad={[3, 0]} gap={2} align="center" fill={chosen ? "selected" : undefined} hover="fill" border="border" borderSides={[1, 0, 0, 0]}
      onPress={() => page.edits.tracks.openMaster()}>
      <Dot />
      <row grow={1}><text style="heading" color="text">Master</text></row>
      <Toggle id="mixer-toggle" label={mixer ? "Hide the mixer" : "Show the mixer"} icon="sliders-horizontal" on={mixer} tip={mixer ? "Hide the mixer" : "Show the mixer"} keys={keysOf(page.song.shortcut("mixer"))} onToggle={() => page.edits.tracks.toggleMixer()} />
    </row>
  );
};
