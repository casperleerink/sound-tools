import { Circle, For, Fragment, Group, h, Input, Label, Line, memo, Notes, now, Path, Rect, selector, Show, Space, Text, untrack, Wave, type Accessor, type Child as Shapes, type Drag, type Dropped, type FilledStateOfTool, type Live, type Num, type Press, type Shortcut } from "./sdk";
import { lengthOf } from "./clip-editor";
import { LaneRow } from "./song-automation";
import { endMoved, itemCommand, moveItems, startMoved, type ClipCommand } from "./song-clips";
import { ACROSS, BEAT, clamp, clipsOf, colourOf, COLOR, DIM, itemTool, LEAD, LOW_ROW, muted, regionsOf, ROW, titleOf, type Child, type Page } from "./song-page";
import { NAMING, Shades, SoftFoot } from "./song-ruler";
import { addInstrumentTrack, cannotRender, notesTrackOf, RENDER_ITEM, renderToTrack } from "./song-tracks";
import { freeName } from "./track";
import { secondsBetween, ticksAcross } from "./song-time";

// The timeline: the clips of each track and the regions of each audio track in time, a take while
// it records, and the lanes of automation under a track that shows them.

const [MOVES, RESIZES, START_MOVES] = [["move"], ["resize"], ["resize-start"]];

export const Timeline = ({ page }: { page: Page }) => {
  const { state, children, memory, update, clock, outputs, openMenu, exportNotes, rows, layout, tops, topOf, rowAt, starts, barOf, end, choose, clipAt, itemAt, lengthAt, editClips, timeline, playhead, handle } = page;
  const isPicked = selector(() => memory.selected);
  /** The track that says how a clip is added: the first that takes clips, while the song has none. */
  const hinted = memo(() => (rows().some((each) => clipsOf(each).length > 0) ? undefined : rows().find((each) => each.tool === "track")?.name));
  const openClip = (key: string) => {
    memory.below = { kind: "editor", clip: key };
    memory.notes = new Set();
  };

  /** The row of the timeline `dy` rows of the layout down from row `row`: a lane's is its track's. */
  const rowMoved = (row: number, dy: number) => {
    const name = rows()[row]?.name;
    const from = layout().findIndex(({ each, lane, add }) => each.name === name && !lane && !add);
    const to = layout()[clamp(from + dy, 0, layout().length - 1)]?.each.name;
    return rows().findIndex((each) => each.name === to);
  };
  // The drags of clips and regions land on the grid of the snap in the corner, a tick apart while
  // it is off.
  const moveDrag = (snap: number): Drag => ({
    id: "move",
    snap: { x: snap || 1, y: 1, grid: snap > 0 },
    onEnd: ({ dx, dy, alt }) =>
      choose(moveItems(page, state.order, memory.selected, Math.round(dx), (row) => rowMoved(row, Math.round(dy)), alt)),
  });
  const copying = now.drag("move").alt;
  /** A chosen clip or region where it was, while a drag with alt copies it: the original stays. */
  const Original = ({ x, row, width, picked, dim = () => 1, contents }: { x: Accessor<number>; row: Accessor<number>; width: Accessor<number>; picked: Accessor<boolean>; dim?: Accessor<number>; contents: (opacity: Live) => Shapes }) => (
    <Show when={picked}>
      {() => (
        <Group x={x} y={() => topOf(row())} clip={() => ({ width: width(), height: 1 })}>
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="fill" border="border-strong" borderWidth={1} opacity={() => copying.times(dim())} />
          {contents(copying)}
        </Group>
      )}
    </Show>
  );
  /** A click on one of several chosen chooses it alone. */
  const clickItem = (key: string) => ({ shift, cmd }: Press) => !shift && !cmd && memory.selected.size > 1 && choose(new Set([key]));
  const marquee = now.drag("clip-marquee");
  const marqueeDrag: Drag = {
    id: "clip-marquee",
    onEnd: ({ startX, startY, x, y, shift, cmd }) => {
      const [left, right] = [Math.min(startX, x), Math.max(startX, x)];
      const [top, bottom] = [Math.min(startY, y), Math.max(startY, y)];
      const inside = layout().flatMap(({ each, lane, add }, row) =>
        lane || add || topOf(row) + 1 <= top || topOf(row) >= bottom
          ? []
          : [...clipsOf(each), ...regionsOf(each)]
              .map((item) => `${each.name}/${item.name}`)
              .filter((key) => {
                const start = itemAt(key)?.state.start ?? 0;
                return start < right && start + lengthAt(key) > left;
              }),
      );
      choose(new Set([...(shift || cmd ? memory.selected : []), ...inside]));
    },
  };
  const resizeDrag = (snap: number): Drag => ({
    id: "resize",
    axis: "x",
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ dx }) =>
      editClips("Resize clips", memory.selected, (clip) => endMoved(clip, Math.max(lengthOf(clip) + Math.round(dx), snap || BEAT / 4))),
  });
  const resizeStartDrag = (snap: number): Drag => ({
    id: "resize-start",
    axis: "x",
    snap: { x: snap || 1, grid: snap > 0 },
    onEnd: ({ dx }) => editClips("Resize clips", memory.selected, (clip) => startMoved(clip, Math.round(dx), snap || BEAT / 4)),
  });

  // As the arrangement's: what the keys do, and a clip's name.
  const CLIP_MENU: { label: string; command?: ClipCommand; renders?: true; exports?: true; shortcut?: Shortcut; separator?: true }[] = [
    { label: "Cut", command: "cut", shortcut: "song.cut_clips" }, { label: "Copy", command: "copy", shortcut: "song.copy_clips" }, { label: "Duplicate", command: "duplicate", shortcut: "song.duplicate_clips" },
    { label: "Rename", separator: true }, { label: "Export notes…", exports: true }, RENDER_ITEM, { label: "Delete", command: "delete", shortcut: "song.delete_clips", separator: true },
  ];
  const itemMenu = async (key: string) => {
    if (!memory.selected.has(key)) choose(new Set([key]));
    const menu = CLIP_MENU.filter(({ command, renders }) => command || renders || itemTool(children, key) === "timeline");
    const row = key.split("/")[0]!;
    const why = cannotRender(rows().find((each) => each.name === row));
    const items = menu.map(({ label, shortcut, separator, renders }) => ({ label, shortcut, separator, ...(renders && why ? { disabled: true, detail: why } : {}) }));
    const picked = await openMenu(items, { width: 240 });
    const item = picked === null ? undefined : menu[picked];
    if (item?.command) itemCommand(item.command, state.order, page);
    else if (item?.renders) renderToTrack(page, row, [...memory.selected].filter((each) => each.startsWith(`${row}/`)));
    else if (item?.exports) {
      const track = rows().find((each) => each.name === row);
      void exportNotes(key, { name: clipAt(key)?.state.title || (track ? titleOf(track) : row) });
    }
    else if (item) memory.renamingClip = key;
  };
  // The key that opens the menu of the chosen clips asks here, where the menu is.
  memo(() => {
    const key = memory.clipMenu;
    if (key === null) return;
    untrack(() => {
      memory.clipMenu = null;
      void itemMenu(key);
    });
  });
  /** A press on a clip or a region: a double click opens a clip's notes, and a region chosen alone shows its card in its track's panel. */
  const pressItem = (key: string) => ({ clicks, shift, cmd, button }: Press) => {
    const region = itemTool(children, key) === "region";
    if (button === "right") {
      itemMenu(key);
    } else if (clicks === 2 && !region) {
      openClip(key);
    } else if (shift || cmd) {
      const next = new Set(memory.selected);
      if (!next.delete(key)) next.add(key);
      choose(next);
    } else if (!memory.selected.has(key)) {
      choose(new Set([key]));
    }
    if (region && memory.selected.size === 1 && memory.selected.has(key)) memory.below = { kind: "track", name: key.split("/")[0]! };
  };
  /** The start of the cell of the snap's grid at `x`, or the tick there while it is off. */
  const cellAt = (x: number) => Math.max(memory.snap > 0 ? Math.floor(Math.round(x) / memory.snap) * memory.snap : Math.round(x), 0);
  const pressTimeline = ({ clicks, x, y }: Press) => {
    const row = layout()[rowAt(y)];
    if (clicks !== 2 || row?.add || row?.each.tool !== "track") {
      choose(new Set());
      return;
    }
    // A bar long, in the cell of the grid under the pointer.
    const start = cellAt(x);
    const bar = barOf(start);
    const length = (starts()[bar] ?? 0) - (starts()[bar - 1] ?? 0) || 4 * BEAT;
    let added: string | undefined;
    update("Add a clip", ({ children }) => {
      const owner = children.find((child) => child.name === row.each.name);
      if (owner?.tool !== "track") return;
      const name = freeName(owner.children.map((child) => child.name), "clip");
      owner.children = [...owner.children, { name, tool: "timeline", state: { start, length, notes: [] } }];
      added = `${owner.name}/${name}`;
    });
    if (added) choose(new Set([added]));
  };
  const dropOnTimeline = ({ files, timelines, x, y }: Dropped) => {
    const row = layout()[rowAt(y)]?.each;
    if (timelines.length > 0) dropNotes(timelines, row, cellAt(x));
    const owner = row?.tool === "tape" ? row : rows().find((each) => each.tool === "tape");
    if (!owner || files.length === 0) return;
    const start = Math.max(Math.round(x / BEAT) * BEAT, 0);
    update("Add audio", ({ children }) => {
      const tape = children.find((child) => child.name === owner.name);
      if (tape?.tool !== "tape") return;
      for (const file of files) {
        const name = freeName(tape.children.map((child) => child.name), "region");
        tape.children = [...tape.children, { name, tool: "region", state: { asset: file, start } }];
      }
    });
  };

  /** The parts of a MIDI file at `start`: one onto the instrument track `row`, else each onto a new instrument track named after it. */
  const dropNotes = (timelines: Dropped["timelines"], row: Child | undefined, start: number) => {
    let added: string[] = [];
    update("Import notes", (part) => {
      const owner = part.children.find((child) => child.name === row?.name);
      const [only] = timelines;
      if (owner?.tool === "track" && only && timelines.length === 1) {
        const name = freeName(owner.children.map((child) => child.name), "clip");
        owner.children.push({ name, tool: "timeline", state: { ...only, start } });
        added = [`${owner.name}/${name}`];
      } else {
        added = timelines.map((timeline) => `${addInstrumentTrack(part, timeline.title ?? "Notes", [{ name: "clip", state: { ...timeline, start } }])}/clip`);
      }
    });
    choose(new Set(added));
  };

  /** A clip of a track on row `row`: its notes, its title, a handle to resize it. */
  const ClipShapes = ({ each, clip, row, snap }: { each: Child; clip: ReturnType<typeof clipsOf>[number]; row: Accessor<number>; snap: number }) => {
    const key = `${each.name}/${clip.name}`;
    const picked = () => isPicked(key);
    const length = () => lengthOf(clip.state);
    const resizes = () => (picked() ? RESIZES : undefined);
    const startMoves = () => (picked() ? START_MOVES : undefined);
    const title = () => clip.state.title || "";
    // Its notes as the window drew them: a line each, between its lowest and highest pitch, under its title while it has one.
    const miniature = memo(() => {
      const pitches = clip.state.notes.map((note) => note.pitch);
      const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
      const range = highest - lowest + 1;
      const shown = Math.max(range, 12);
      const step = (ROW - (title() ? 38 : 24)) / shown;
      const height = clamp(step, 2, 4);
      const bottom = ROW - 12 - ((shown - range) / 2) * step;
      return { lowest, step, height, bottom };
    });
    const dim = () => (muted(each) ? DIM : 1);
    const notes = (opacity: () => Num) => (
      <For each={() => clip.state.notes}>
        {(note) => (
          <Rect x={() => note.start} y={() => { const m = miniature(); return (m.bottom - (note.pitch - m.lowest + 0.5) * m.step - m.height / 2) / ROW; }}
            width={() => timeline.pointX.negate().plus(note.length).max(timeline.pointX)} height={() => miniature().height / ROW} color={() => colourOf(each)} opacity={opacity} />
        )}
      </For>
    );
    // The clip and its notes move together, and its notes end at its end.
    return (
      <>
        <Original x={() => clip.state.start} row={row} width={length} picked={picked} dim={dim} contents={(shown) => notes(() => shown.times(dim()))} />
        <Group x={() => clip.state.start} y={() => topOf(row())} clip={() => ({ width: length(), height: 1 })}
          moves={() => (picked() ? MOVES : undefined)} grows={resizes}>
          <Rect x={0} y={4 / ROW} width={length} height={(ROW - 8) / ROW} radius={6} anchor={{ x: 0, y: 0 }}
            color="fill" border={() => (picked() ? "text" : "border-strong")} borderWidth={1} opacity={dim}
            grows={resizes} moves={startMoves} shrinks={startMoves} drag={moveDrag(snap)} onPress={pressItem(key)} onClick={clickItem(key)} cursor="grab" />
          {notes(dim)}
          <Label x={timeline.pointX.times(8)} y={9 / ROW} text={title} color="text-secondary" opacity={dim} />
          <Rect x={() => handle.negate().plus(length())} y={4 / ROW} width={handle} height={(ROW - 8) / ROW} color={COLOR.clear}
            anchor={() => ({ x: length() })} moves={resizes}
            drag={resizeDrag(snap)} onPress={pressItem(key)} cursor="ew-resize" />
          <Rect x={0} y={4 / ROW} width={handle} height={(ROW - 8) / ROW} color={COLOR.clear}
            moves={startMoves} drag={resizeStartDrag(snap)} onPress={pressItem(key)} cursor="ew-resize" />
        </Group>
      </>
    );
  };

  /**
   * The handles of a chosen region at its top, 7 points down: a fade at each corner, the gain
   * hollow in the middle, and each fade a line from its bottom corner to its handle. A fade's
   * handle drags across, the gain's up and down, 200 points for its whole range, each with a
   * readout of what it makes.
   */
  const RegionHandles = ({ region, width, plays, edit }: { region: ReturnType<typeof regionsOf>[number]; width: Accessor<number>; plays: Accessor<number>; edit: (label: string, change: (state: FilledStateOfTool<"region">) => void) => void }) => {
    const point = timeline.pointX;
    const [top, bottom] = [(4 + 7) / ROW, (ROW - 4) / ROW];
    const changes = () => clock.tempo_map.tempo_changes;
    const fade = (ms: number) => ticksAcross(changes(), region.state.start, ms / 1000);
    const inside = (x: Live) => x.max(point.times(5)).min(point.times(-5).plus(width()));
    const [fadingIn, fadingOut, gaining] = [now.drag("fade-in"), now.drag("fade-out"), now.drag("gain")];
    const fadeIn = () => inside(fadingIn.dx.plus(fade(region.state.fade_in_ms)));
    const fadeOut = () => inside(fadingOut.dx.plus(width() - fade(region.state.fade_out_ms)));
    // A fade is a time, not a place on the grid: as long as the region plays, less the other fade.
    const fadeMs = (ticks: number, other: number) => clamp(Math.round(secondsBetween(changes(), region.state.start, region.state.start + Math.max(ticks, 0)) * 1000), 0, Math.max(plays() * 1000 - other, 0));
    const fadeDrag = (id: "fade-in" | "fade-out"): Drag => ({
      id,
      axis: "x",
      onEnd: ({ dx }) =>
        id === "fade-in"
          ? edit("Change fade in", (state) => (state.fade_in_ms = fadeMs(fade(state.fade_in_ms) + dx, state.fade_out_ms)))
          : edit("Change fade out", (state) => (state.fade_out_ms = fadeMs(fade(state.fade_out_ms) - dx, state.fade_in_ms))),
    });
    const GAIN_RANGE = [-48, 24] as const;
    const dbPerRow = (ROW * (GAIN_RANGE[1] - GAIN_RANGE[0])) / 200;
    const gainDrag: Drag = {
      id: "gain",
      axis: "y",
      onEnd: ({ dy }) => edit("Change gain", (state) => (state.gain_db = clamp(Math.round((Math.max(state.gain_db, GAIN_RANGE[0]) - dy * dbPerRow) * 10) / 10, GAIN_RANGE[0], GAIN_RANGE[1]))),
    };
    const dot = (x: Accessor<Live> | Live, hollow: boolean, drag: Drag, cursor: "ew-resize" | "ns-resize") => [
      <Circle x={x} y={top} radius={5} color={hollow ? "text" : "well"} drag={drag} cursor={cursor} />,
      <Circle x={x} y={top} radius={3.5} color={hollow ? "well" : "text"} drag={drag} cursor={cursor} />,
    ];
    /** What a handle makes while it is dragged, in a pill right of it `wide` points wide, as `title` and the value. */
    const readout = (x: Accessor<Live>, title: string, wide: number, value: Accessor<Live>, unit: "ms" | "db", held: Live) => [
      <Rect x={() => x().plus(point.times(10))} y={top - 2 / ROW} width={point.times(wide)} height={22 / ROW} radius={6} color="background" border="border-strong" borderWidth={1} opacity={held} />,
      <Label x={() => x().plus(point.times(17))} y={top + 2 / ROW} text={title} color="text" opacity={held} />,
      <Label x={() => x().plus(point.times(wide + 3))} y={top + 2 / ROW} text={value} unit={unit} color="text" align="right" opacity={held} />,
    ];
    // A fade's line shows while it has one, and while its handle drags one out.
    const shown = (ms: () => number, held: Live) => () => held.max(ms() > 0 ? 1 : 0);
    /** The shade over what a fade turns down: half as dark as the well. */
    const shade = (ms: () => number, held: Live) => () => shown(ms, held)().times(0.5);
    const msOf = (ticks: Live) => ticks.times(60_000 / BEAT).over(now.bpm).max(0);
    const edge = 4 / ROW;
    return (
      <>
        <Path points={() => [[0, bottom], [0, edge], [fadeIn(), edge], [fadeIn(), top]]} color="well" opacity={shade(() => region.state.fade_in_ms, fadingIn.active)} />
        <Path points={() => [[width(), bottom], [width(), edge], [fadeOut(), edge], [fadeOut(), top]]} color="well" opacity={shade(() => region.state.fade_out_ms, fadingOut.active)} />
        <Line from={[0, bottom]} to={() => [fadeIn(), top]} color="text" width={1.5} opacity={shown(() => region.state.fade_in_ms, fadingIn.active)} />
        <Line from={() => [width(), bottom]} to={() => [fadeOut(), top]} color="text" width={1.5} opacity={shown(() => region.state.fade_out_ms, fadingOut.active)} />
        {dot(fadeIn, false, fadeDrag("fade-in"), "ew-resize")}
        {dot(fadeOut, false, fadeDrag("fade-out"), "ew-resize")}
        {dot(() => point.times(0).plus(width() / 2), true, gainDrag, "ns-resize")}
        {readout(fadeIn, "Fade in", 96, () => msOf(fadingIn.dx.plus(fade(region.state.fade_in_ms))), "ms", fadingIn.active)}
        {readout(fadeOut, "Fade out", 104, () => msOf(fadingOut.dx.negate().plus(fade(region.state.fade_out_ms))), "ms", fadingOut.active)}
        {readout(() => point.times(0).plus(width() / 2), "", 48, () => gaining.dy.times(-dbPerRow).plus(region.state.gain_db).times(10).round().over(10), "db", gaining.active)}
      </>
    );
  };
  /** A take while it records, as a region or a clip looks: over what it lies on, with a red border, its sound or its notes so far, from `from` to the playhead. */
  const TakeShapes = ({ each, row, from }: { each: Child; row: Accessor<number>; from: number }) => {
    const width = now.tick.minus(from).max(1);
    return (
      <Group x={from} y={() => topOf(row())}>
        <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="background" />
        <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="fill" border="red" borderWidth={1} />
        <Wave x={0} y={(ROW / 2 - 22) / ROW} width={width} height={44 / ROW} list={now.take(`${each.name}/rec`)} band low={-1} high={1} color={() => colourOf(each)} />
        <Notes x={0} y={12 / ROW} width={width} height={(ROW - 24) / ROW} list={now.take(`${each.name}/rec`)} color={() => colourOf(each)} />
      </Group>
    );
  };
  /** A region of an audio track on row `row`: its file's waveform; drag it to move it, an edge to trim it. */
  const RegionShapes = ({ each, region, row, snap }: { each: Child; region: ReturnType<typeof regionsOf>[number]; row: Accessor<number>; snap: number }) => {
    const key = `${each.name}/${region.name}`;
    const chosen = () => isPicked(key);
    // Its handles show, and its gain and trims drag, while it is chosen alone.
    const alone = () => chosen() && memory.selected.size === 1;
    // To its last warp point, else seconds of its file as ticks through the tempo map from where it starts.
    const width = () => region.state.warp?.at(-1)?.ticks ?? ticksAcross(clock.tempo_map.tempo_changes, region.state.start, outputs[`${key}.seconds`] ?? 0);
    const sound = () => now.file(region.state.asset, { from: region.state.file_start_seconds, to: region.state.file_end_seconds });
    // While its gain handle drags, how much louder it is drawn: the handle is the chosen region's.
    const dragged = now.tick.times(0).plus(10).pow(now.drag("gain").dy.times((-ROW * 72) / 200 / 20)).min((ROW - 8) / 44);
    const louder = () => (alone() ? dragged : now.tick.times(0).plus(1));
    // Drawn as it sounds: its gain, and its fades straight from silence at its ends.
    const level = (): [number, number][] => {
      const gain = Math.pow(10, region.state.gain_db / 20);
      const across = width();
      const fade = (ms: number) => Math.min(ticksAcross(clock.tempo_map.tempo_changes, region.state.start, ms / 1000), across);
      const [into, out] = [fade(region.state.fade_in_ms), fade(region.state.fade_out_ms)];
      if (into + out > across && into > 0 && out > 0) {
        const meet = (across * into) / (into + out);
        return [[0, 0], [meet, (gain * meet) / into], [across, 0]];
      }
      return [[0, into > 0 ? 0 : gain], [into, gain], [across - out, gain], [across, out > 0 ? 0 : gain]];
    };
    const wave = (opacity: Num = 1) => (
      <Wave x={0} y={() => louder().times(-22 / ROW).plus(0.5)} width={width} height={() => louder().times(44 / ROW)} list={sound} band low={-1} high={1} level={level}
        color={() => colourOf(each)} opacity={opacity} />
    );
    const editRegion = (label: string, change: (state: FilledStateOfTool<"region">) => void) =>
      update(label, ({ children }) => {
        const owner = children.find((child) => child.name === each.name);
        const draft = owner?.tool === "tape" ? owner.children.find((child) => child.name === region.name) : undefined;
        if (draft?.tool === "region") change(draft.state);
      });
    const trimRegion: Drag = {
      id: "trim",
      axis: "x",
      snap: { x: snap || 1, grid: snap > 0 },
      onEnd: ({ dx }) =>
        editRegion("Trim a region", (state) => {
          const ticks = Math.max(Math.round(dx), -state.start);
          const moved = secondsBetween(clock.tempo_map.tempo_changes, state.start, state.start + ticks);
          state.start += ticks;
          state.file_start_seconds = Math.max(state.file_start_seconds + moved, 0);
        }),
    };
    // How long it plays, which the region tells: the end of its file unless it ends before.
    const trimEnd: Drag = {
      id: "end",
      axis: "x",
      snap: { x: BEAT / 4 },
      onEnd: ({ dx }) =>
        editRegion("Trim a region", (state) => {
          const plays = outputs[`${key}.seconds`] ?? 0;
          // Its whole file plays already: there is nothing after its end.
          if (plays <= 0 || (state.file_end_seconds === null && dx >= 0)) return;
          const changes = clock.tempo_map.tempo_changes;
          const end = state.start + ticksAcross(changes, state.start, plays);
          state.file_end_seconds = state.file_start_seconds + Math.max(plays + secondsBetween(changes, end, end + dx), 0.05);
        }),
    };
    return (
      <>
        <Original x={() => region.state.start} row={row} width={width} picked={chosen} contents={wave} />
        <Group x={() => region.state.start} y={() => topOf(row())} moves={() => (chosen() ? MOVES : undefined)}>
          <Rect x={0} y={4 / ROW} width={width} height={(ROW - 8) / ROW} radius={6} color="fill" border={() => (chosen() ? "text" : "border-strong")} borderWidth={1}
            drag={moveDrag(snap)} onPress={pressItem(key)} onClick={clickItem(key)} cursor="grab" />
          {wave()}
          <Rect x={0} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimRegion} onPress={pressItem(key)} cursor="ew-resize"
            moves={() => (alone() ? ["trim"] : undefined)} />
          <Rect x={() => handle.negate().plus(width())} y={0.06} width={handle} height={0.88} color={COLOR.clear} drag={trimEnd} onPress={pressItem(key)} cursor="ew-resize"
            moves={() => (alone() ? ["end"] : undefined)} />
          <Show when={alone}>{() => <RegionHandles region={region} width={width} plays={() => outputs[`${key}.seconds`] ?? 0} edit={editRegion} />}</Show>
        </Group>
      </>
    );
  };

  return (
    // A row past the last, as the heads end with the row that adds a track.
    <Space key="timeline" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [-LEAD, end()], y: [0, (tops().at(-1) ?? 0) + 1] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }}
      drag={marqueeDrag} onPress={pressTimeline} onDrop={dropOnTimeline}>
      <Shades page={page} height={1000} />
      <Rect x={-1e6} y={() => tops().at(-1) ?? 0} width={2e6} height={timeline.pointY} color="border" />
      <For each={layout}>
        {({ each, lane, add }, row) =>
          add ? null : lane ? (
            () => {
              const snap = memory.snap;
              return <LaneRow page={page} track={each.name} lane={lane} row={row} colour={() => colourOf(each)} snap={snap} />;
            }
          ) : (
            <>
              <Rect x={-1e6} y={() => topOf(row())} width={2e6} height={timeline.pointY} color="border" />
              <Label x={() => timeline.left.max(0).plus(timeline.pointX.times(12))} y={() => timeline.pointY.times(-7).plus(topOf(row()) + 0.5)}
                text={() => (memory.recording !== null || clipsOf(each).length + regionsOf(each).length > 0 ? "" : each.tool === "tape" ? "Drop an audio file here" : hinted() === each.name ? "Double-click to add a clip" : "")}
                color="text-muted" />
              {() => {
                const snap = memory.snap;
                return <For each={() => regionsOf(each)}>{(region) => <RegionShapes each={each} region={region} row={row} snap={snap} />}</For>;
              }}
              {() => {
                const from = memory.recording;
                const takes = memory.armed.has(each.name) || each.name === notesTrackOf(children, state.order, memory.track);
                return from !== null && takes ? <TakeShapes each={each} row={row} from={from} /> : null;
              }}
              {() => {
                const snap = memory.snap;
                return <For each={() => clipsOf(each)}>{(clip) => <ClipShapes each={each} clip={clip} row={row} snap={snap} />}</For>;
              }}
            </>
          )
        }
      </For>
      <SoftFoot page={page} />
      <Rect x={marquee.startX.min(marquee.x)} y={marquee.startY.min(marquee.y)} width={marquee.dx.abs()} height={marquee.dy.abs()}
        color="fill" border="border-strong" borderWidth={1} radius={2} opacity={marquee.active} />
      <Line from={[playhead, 0]} to={[playhead, 1000]} color="text" width={1} />
    </Space>
  );
};

/** The timeline beside the returns and the master, which have no clips: its shades and the playhead go on to its foot. */
export const TimelineFoot = ({ page }: { page: Page }) => (
  <Space key="timeline-foot" height={() => page.returnsHeight() + LOW_ROW} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, page.end()] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }}
    follow={{ x: "timeline" }} onPress={() => page.choose(new Set())}>
    <Shades page={page} height={1000} />
    <Line from={[page.playhead, 0]} to={[page.playhead, 1000]} color="text" width={1} />
  </Space>
);

/** The field to name the clip being named, over the timeline. */
export const ClipName = ({ page }: { page: Page }) => {
  const { memory, clipAt, editClips } = page;
  return (
    <>
      {() => {
        const key = memory.renamingClip;
        if (!key) return null;
        return (
          <div key="clip-name" style={NAMING}>
            <Text variant="label" tone="muted">Clip name</Text>
            <Input key="clip-name-field" value={() => clipAt(key)?.state.title || clipAt(key)?.name || ""} width={140} focus
              onCommit={(title) => {
                memory.renamingClip = null;
                editClips("Rename a clip", [key], (clip) => (clip.title = title));
              }} />
          </div>
        );
      }}
    </>
  );
};
