import { Button, For, Fragment, h, Icon, Input, onCleanup, Show, Text, Toggle, untrack } from "./sdk";
import { bus } from "./bus";
import { chain } from "./chain";
import type { Piece } from "./piece";
import { toggleLoop } from "./piece-choice";
import { colourOf, DIM, HEADER, LOW_ROW, Mark, MARKS, muted, PALETTE, ROW, rowLook, titleOf, type Child, type Page } from "./piece-page";
import { freeName, track } from "./track";

// The heads of the rows: an instrument's name and toggles left of its music, and under them the
// returns and the master; adding, moving, naming, colouring and deleting instruments.

const Hairline = () => <div style={{ height: 1, background: "border", keep: true }} />;

/** The column left of the rows: the corner beside the marks, with the loop while it can be used, the names, which scroll with the rows, and under them the returns and the master, which stay. */
export const Heads = ({ page }: { page: Page }) => {
  const { rows, returns, clock, memory } = page;
  // Loop shows while it loops, while bars are chosen, and under the pointer.
  const loopShows = () => clock.repeat !== null || memory.chosen !== null || memory.hovered === CORNER;
  return (
    <div style={{ width: HEADER - 1, keep: true }}>
      <div style={{ height: MARKS, keep: true, justify: "center", paddingX: "md" }}
        onHover={(over) => (memory.hovered = over ? CORNER : memory.hovered === CORNER ? null : memory.hovered)}>
        <div style={{ direction: "row", gap: "sm" }}>
          <Show when={loopShows}>
            {() => (
              <Toggle key="loop" label="Loop" size="sm" on={() => clock.repeat !== null} shortcut="piece.loop" onClick={() => toggleLoop(page)}
                tooltip={() => (clock.repeat !== null ? "Stop looping" : memory.chosen ? "Loop the chosen bars as it plays" : "Loop the section that plays")} />
            )}
          </Show>
          <Show when={() => clock.playing && !memory.follows}>
            {() => <Button key="follow" label="Follow" size="sm" variant="ghost" shortcut="piece.follow" tooltip="Turn the page with the playhead again" onClick={() => (memory.follows = true)} />}
          </Show>
        </div>
      </div>
      <div style={{ grow: true }} follow={{ y: "rows" }}>
        <For each={rows}>
          {(each) => (
            <div key={`head-${each.name}`} carry={`track:${each.name}`} onDrop={({ carried }) => carried?.startsWith("track:") && carried.slice(6) !== each.name && moveTrack(page, carried.slice(6), each.name)}
              style={{ height: ROW }}>
              <TrackHead page={page} each={each} />
            </div>
          )}
        </For>
        <div style={{ height: LOW_ROW, direction: "row", align: "center", paddingX: "xs" }}>
          <Button key="add-instrument" label="Instrument" icon="plus" variant="ghost" color="text-muted" size="sm" onClick={() => addTrack(page, 0)} />
          <Button key="add-kind" icon="chevron-down" variant="ghost" color="text-muted" size="sm" tooltip="Add an instrument or a return" onClick={() => addTrack(page)} />
        </div>
      </div>
      <div style={{ keep: true }}>
        <Show when={() => returns().length > 0}>{() => <Returns page={page} />}</Show>
        <MasterRow page={page} />
      </div>
    </div>
  );
};
/** What `hovered` holds while the pointer is over the corner: no instrument's name has capitals. */
const CORNER = "Corner";

/**
 * An instrument's head: a dot of its colour and its name. While the pointer is over it or it is
 * chosen, also what plays it and its toggles; of these only those that are on show otherwise. A
 * click anywhere on it but its toggles shows its panel, a double click names it, a right press
 * opens its menu.
 */
const TrackHead = ({ page, each }: { page: Page; each: Child }) => {
  const { state, memory, chosenTrack, openPanel, toggleMute, toggleSolo } = page;
  const chosen = () => chosenTrack() === each.name;
  const all = () => memory.hovered === each.name || chosen();
  const dim = () => (muted(each) ? DIM : 1);
  // A head drawn anew hears no leave of the one it replaces.
  onCleanup(() => memory.hovered === each.name && (memory.hovered = null));
  return (
    <div key={`track-head-${each.name}`} style={() => ({ height: ROW, justify: "center", paddingLeft: "md", paddingRight: "sm", ...rowLook(chosen()) })}
      onHover={(over) => (memory.hovered = over ? each.name : memory.hovered === each.name ? null : memory.hovered)}
      onPress={({ button }) => button === "right" && trackMenu(page, each)}
      onClick={({ clicks }) => (clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))} tabStop>
      <div style={{ direction: "row", align: "center", gap: "sm", paddingTop: "lg" }}>
        <Mark colour={() => colourOf(each)} dim={() => muted(each)} />
        {() =>
          memory.renaming === each.name ? (
            <Input value={() => titleOf(each)} width={104} focus onCommit={(title) => rename(page, each.name, title)} />
          ) : (
            <Text key={`name-${each.name}`} variant="heading" tone={() => (chosen() ? "text" : "secondary")} style={() => ({ truncate: true, opacity: dim() })}>{() => titleOf(each)}</Text>
          )
        }
      </div>
      <div style={{ height: 24, direction: "row", align: "center", gap: "xs", paddingLeft: "md" }}>
        <Text variant="label" tone="muted" style={() => ({ grow: true, truncate: true, opacity: all() ? 1 : 0 })}>{() => memory.names[`${each.name}/synth`] ?? ""}</Text>
        {() => {
          // Runs again, and learns its name again, when another instrument or plugin takes its place.
          const synth = each.children.find((child) => child.name === "synth");
          const playing = synth && `${synth.tool} ${(synth.state as { plugin_id?: string }).plugin_id}`;
          if (playing) untrack(() => learnName(page, each.name));
          return null;
        }}
        <Show when={() => all() || muted(each)}>
          {() => <Toggle key={`mute-${each.name}`} size="sm" label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} />}
        </Show>
        <Show when={() => all() || state.solo.includes(each.name)}>
          {() => <Toggle key={`solo-${each.name}`} size="sm" label="S" tooltip="Solo" on={() => state.solo.includes(each.name)} color="yellow" onClick={() => toggleSolo(each.name)} />}
        </Show>
      </div>
    </div>
  );
};

/** Learns what the window calls the device that plays the instrument `name`, for its head. */
async function learnName({ memory, offers }: Page, name: string) {
  const { offers: offered, here } = await offers("notes", { at: `${name}/synth` });
  const found = offered.find((each) => each.key === here)?.name;
  if (found) memory.names = { ...memory.names, [`${name}/synth`]: found };
}

/**
 * The returns, over the master's row while the piece has any: one row that folds them, with a
 * plus that adds one; open, a row each with its dot, its name and M. A press shows its panel,
 * a right-click its menu.
 */
const Returns = ({ page }: { page: Page }) => {
  const { memory, returns, returnsHeight } = page;
  return (
    <>
      <Hairline />
      <div style={{ height: LOW_ROW - 1, direction: "row", align: "center", paddingRight: "sm", keep: true }}>
        <div key="returns-header" tabStop style={{ grow: true, height: LOW_ROW - 1, direction: "row", align: "center", gap: "xs", paddingLeft: "lg" }}
          onClick={() => (memory.returnsOpen = !memory.returnsOpen)}>
          <Icon name={() => (memory.returnsOpen ? "chevron-down" : "chevron-right")} size={12} color="text-muted" />
          <Text variant="label" tone="text">Returns</Text>
          <Text variant="label" tone="muted">{() => String(returns().length)}</Text>
        </div>
        <Button key="add-return" icon="plus" variant="ghost" size="sm" tooltip="Add a return" onClick={() => addTrack(page, 1)} />
      </div>
      <Show when={() => memory.returnsOpen}>
        {() => (
          <div key="returns-list" style={() => ({ height: returnsHeight() - LOW_ROW, scroll: "y", keep: true })}>
            <For each={returns}>{(each) => <ReturnRow page={page} each={each} />}</For>
          </div>
        )}
      </Show>
    </>
  );
};
const ReturnRow = ({ page, each }: { page: Page; each: Child }) => {
  const { memory, chosenTrack, openPanel, toggleMute } = page;
  return (
    <div key={`return-${each.name}`} tabStop style={() => ({ height: LOW_ROW, direction: "row", align: "center", gap: "sm", paddingLeft: "lg", paddingRight: "sm", keep: true, ...rowLook(chosenTrack() === each.name) })}
      onPress={({ button }) => button === "right" && trackMenu(page, each)}
      onClick={({ clicks }) => (clicks === 2 ? (memory.renaming = each.name) : openPanel(each.name))}>
      <Mark colour={() => colourOf(each)} dim={() => muted(each)} />
      {() =>
        memory.renaming === each.name ? (
          <Input value={() => titleOf(each)} width={104} focus onCommit={(title) => rename(page, each.name, title)} />
        ) : (
          <Text variant="heading" style={() => ({ grow: true, truncate: true, opacity: muted(each) ? DIM : 1 })}>{() => titleOf(each)}</Text>
        )
      }
      <Toggle key={`mute-${each.name}`} size="sm" label="M" tooltip="Mute" on={() => muted(each)} color="peach" onClick={() => toggleMute(each.name)} />
    </div>
  );
};

/** The master's row: a press shows its panel; the toggle at its end shows the mixer. */
const MasterRow = ({ page: { memory, openMaster } }: { page: Page }) => (
  <>
    <Hairline />
    <div key="master-row" tabStop style={() => ({ height: LOW_ROW - 1, direction: "row", align: "center", gap: "sm", paddingLeft: "lg", paddingRight: "sm", keep: true, ...rowLook(memory.below?.kind === "master") })}
      onClick={openMaster}>
      <Mark />
      <Text variant="heading" style={{ grow: true }}>Master</Text>
      <Toggle key="mixer-toggle" size="sm" icon="sliders-horizontal" tooltip={() => (memory.below?.kind === "mixer" ? "Hide the mixer" : "Show the mixer")}
        on={() => memory.below?.kind === "mixer"} onClick={() => (memory.below = memory.below?.kind === "mixer" ? null : { kind: "mixer" })} />
    </div>
  </>
);

/** What an undo step about the instrument or return `name` calls it. */
const kindOf = ({ children }: Pick<Piece, "children">, name: string) => (children.find((child) => child.name === name)?.tool === "bus" ? "a return" : "an instrument");

function rename({ memory, update, children }: Page, name: string, title: string) {
  memory.renaming = null;
  update(`Rename ${kindOf({ children }, name)}`, ({ children }) => {
    const owner = children.find((child) => child.name === name);
    if (owner?.tool === "track" || owner?.tool === "bus") owner.state.title = title;
  });
}
/** Puts the instrument `moved` before `before` in `order`. */
const moveTrack = ({ update, rows }: Page, moved: string, before: string) =>
  update("Move an instrument", ({ state }) => {
    const names = rows().map((each) => each.name).filter((name) => name !== moved);
    names.splice(names.indexOf(before), 0, moved);
    state.order = names;
  });

/** Adds an instrument, 0, or a return, 1, after the others; the menu asks without one. */
export async function addTrack({ children, memory, update, openMenu, rows, returns, openPanel }: Page, kind?: number) {
  const picked = kind ?? (await openMenu([{ label: "Instrument" }, { label: "Return" }]));
  if (picked === null) return;
  const names = children.map((child) => child.name);
  if (picked === 0) {
    const name = freeName(names, "instrument");
    update("Add an instrument", (part) => {
      const sends = part.children.filter((child) => child.tool === "bus").map((back) => ({ name: `send-${back.name}` as const, tool: "fader" as const, state: { gain_db: "-inf" as const } }));
      part.children.push({
        name, tool: track, state: { colour: PALETTE[rows().length % PALETTE.length]!.colour, title: `Instrument ${rows().length + 1}` },
        children: [{ name: "synth", tool: "instrument.synth", state: {} }, { name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }, ...sends],
      });
      part.state.order = [...rows().map((each) => each.name), name];
    });
    openPanel(name);
    return;
  }
  const name = [..."abcdefghijklmnopqrstuvwxyz"].map((letter) => `ret-${letter}` as const).find((each) => !names.includes(each)) ?? (freeName(names, "ret-z") as `ret-${string}`);
  update("Add a return", (part) => {
    part.children.push({ name, tool: bus, state: { title: `Return ${returns().length + 1}`, colour: PALETTE[(returns().length + 3) % PALETTE.length]!.colour }, children: [{ name: "chain", tool: chain, state: {} }, { name: "mix", tool: "fader", state: {} }] });
    for (const each of part.children) {
      if (each.tool === "track") each.children.push({ name: `send-${name}` as const, tool: "fader", state: { gain_db: "-inf" } });
    }
  });
  memory.returnsOpen = true;
}

async function trackMenu(page: Page, each: Child) {
  const { update, memory, openMenu } = page;
  const items = [{ label: "Rename" }, { label: "Colour…" }, { label: "Delete", shortcut: "piece.delete_instrument" as const, separator: true }];
  const picked = await openMenu(items);
  if (picked === 0) memory.renaming = each.name;
  else if (picked === 1) {
    const colour = await openMenu(PALETTE.map(({ name, colour }) => ({ label: name, checked: colourOf(each) === colour })));
    if (colour === null) return;
    update(`Colour ${kindOf(page, each.name)}`, ({ children }) => {
      const owner = children.find((child) => child.name === each.name);
      if (owner?.tool === "track" || owner?.tool === "bus") owner.state.colour = PALETTE[colour]!.colour;
    });
  } else if (picked === 2) deleteRow(page, each.name);
}

/** Deletes the instrument or return `name` once the composer says yes: a delete too many after another would take it. */
export async function askToDelete(tool: Piece, name: string) {
  const row = tool.children.find((child) => child.name === name);
  if (row?.tool !== "track" && row?.tool !== "bus") return;
  const title = row.state.title || name;
  const said = await tool.confirm(
    row.tool === "bus"
      ? { message: `Delete the return "${title}"?`, detail: "Its effects and the sends into it go with it. Undo brings it back.", confirm: "Delete" }
      : { message: `Delete the instrument "${title}"?`, detail: "Its music and its devices go with it. Undo brings it back.", confirm: "Delete" },
  );
  if (said) deleteRow(tool, name);
}

/** Deletes the instrument or return `name`, with the sends into it and what chose it. */
function deleteRow({ update, memory, children }: Pick<Piece, "update" | "memory" | "children">, name: string) {
  update(`Delete ${kindOf({ children }, name)}`, ({ state, children }) => {
    state.order = state.order.filter((each) => each !== name);
    state.solo = state.solo.filter((each) => each !== name);
    state.sidechains = state.sidechains.filter(({ effect, from }) => from !== name && !effect.startsWith(`${name}/`));
    for (const each of children) if (each.tool === "track") each.children = each.children.filter((child) => child.name !== `send-${name}`);
    children.splice(0, children.length, ...children.filter((child) => child.name !== name));
  });
  if (memory.track === name) memory.track = null;
  const below = memory.below;
  if ((below?.kind === "track" && below.name === name) || (below?.kind === "editor" && below.track === name)) memory.below = null;
}
