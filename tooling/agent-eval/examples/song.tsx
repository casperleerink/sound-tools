import { For, Fragment, Group, h, Label, Line, list, now, Rect, selector, Show, Space, text, tool, type Accessor, type Declaration, type Drag, type FilledStateOfTool, type Live, type Press } from "./sdk";
import { track } from "./track";

// A song: tracks of clips in time, with a mixer strip per track and a note editor for one clip.
const BEAT = 960;
const BAR = 4 * BEAT;
const BARS = 64;
const END = BARS * BAR;
const ROW = 40;
const HEADER = 168;
const RULER = 22;
const EDITOR = 240;
const NOTE_ROW = 8;
// Points per tick at zoom 1: a bar is 96 points.
const ACROSS = 0.025;
// The pitches a clip's picture spans.
const [LOW, HIGH] = [36, 96];

const COLOR = {
  back: "#121317", lane: "#17191e", bar: "#2a2d35", text: "#7c808c", chosen: "#e9ebef", playhead: "#7ee0a0",
  note: "#00000080", off: "#2a2d35", mute: "#f0b44c", solo: "#7ee0a0", meter: "#7ee0a0", clear: "#00000000",
  edit: "#a9b1ff",
} as const;

const [MOVES, RESIZES, NOTE] = [["move"], ["resize"], ["note"]];
const parts = { master: "utility", "*": track } as const;

type Memory = { selected: Set<string>; open: string | null; note: number | null };

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
// Where a level sits on a meter, 0 to 1: 0 dB at 0.8.
const place = (amplitude: Live) => amplitude.pow(20 / 61.94).times(0.8).min(1);

/** A name for a clip in `names` that is not taken. */
function freeName(names: string[], wanted: string): string {
  let name = wanted;
  for (let number = 2; names.includes(name); number++) name = `${wanted}-${number}`;
  return name;
}

function Button({ label, on, color, onPress }: { label: string; on: Accessor<boolean>; color: `#${string}`; onPress: () => void }) {
  return (
    <div style={() => ({ width: 20, height: 18, radius: 4, align: "center", justify: "center", fontSize: 10, background: on() ? color : COLOR.off })} onPress={onPress}>
      {label}
    </div>
  );
}

export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, mixer strips or note editor",
  doc:
    "Tracks of clips in time. Each `track` child plays into the utility `master`; `solo` names the tracks that play alone. Drag a clip to move it (alt copies it, up and down moves it to another track), drag its right end to resize it, double-click empty space to add one and a clip to edit its notes below. Delete removes the chosen clips, cmd-D copies them on. M mutes a track, S solos it.",
  state: { solo: list(text(), { max: 64 }) },
  children: parts,
  transport: true,
  memory: (): Memory => ({ selected: new Set(), open: null, note: null }),
  keys: ["delete", "backspace", "cmd-d"],
  onKey: ({ memory, update }, { key, down }) => {
    if (!down || memory.selected.size === 0) return;
    const chosen = new Set(memory.selected);
    update(key === "d" ? "Copy clips on" : "Remove clips", ({ children }) => {
      for (const row of children) {
        if (row.tool !== "track") continue;
        const mine = row.children.filter((clip) => clip.tool === "timeline" && chosen.has(`${row.name}/${clip.name}`));
        if (key !== "d") {
          row.children = row.children.filter((clip) => !mine.includes(clip));
          continue;
        }
        const copies = mine.flatMap((clip) => {
          if (clip.tool !== "timeline") return [];
          const length = clip.state.length ?? BAR;
          const name = freeName(row.children.map((each) => each.name), clip.name);
          return [{ name, tool: "timeline" as const, state: { ...clip.state, start: clip.state.start + length } }];
        });
        row.children = [...row.children, ...copies];
      }
    });
    memory.selected = new Set();
  },
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const tracks = children.filter((child) => child.tool === "track").map((child) => child.name);
    const soloed = state.solo.filter((name) => tracks.includes(name));
    return {
      connections: [
        ...tracks
          .filter((name) => soloed.length === 0 || soloed.includes(name))
          .map((name) => ({ from: { child: name }, to: { child: "master" } })),
        { from: { child: "master" }, to: { port: "audio" } },
      ],
    };
  },
  view: ({ state, children, memory, update }) => {
    const tracks = () => children.filter((child) => child.tool === "track");
    const isPicked = selector(() => memory.selected);
    const clipsOf = (each: (typeof children)[number]) => (each.tool === "track" ? each.children.filter((child) => child.tool === "timeline") : []);
    const open = () => {
      const [trackName, clipName] = memory.open?.split("/") ?? [];
      const clip = tracks().find((each) => each.name === trackName)?.children.find((child) => child.name === clipName);
      return clip?.tool === "timeline" ? clip : undefined;
    };

    /** Changes the clips `keys` names, `track/clip`, in one undo step. */
    const editClips = (label: string, keys: Iterable<string>, change: (state: FilledStateOfTool<"timeline">) => void) =>
      update(label, ({ children }) => {
        for (const key of keys) {
          const [trackName, clipName] = key.split("/");
          const row = children.find((each) => each.name === trackName);
          const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
          if (clip?.tool === "timeline") change(clip.state);
        }
      });

    const moveDrag: Drag = {
      id: "move",
      snap: { x: BEAT, y: 1, grid: true },
      onEnd: ({ dx, dy, alt }) =>
        update(alt ? "Copy clips" : "Move clips", ({ children }) => {
          const rows = children.filter((child) => child.tool === "track");
          const chosen = new Set<string>();
          for (const key of memory.selected) {
            const [trackName, clipName] = key.split("/");
            const from = rows.findIndex((each) => each.name === trackName);
            const to = rows[clamp(from + Math.round(dy), 0, rows.length - 1)];
            const source = rows[from];
            if (to?.tool !== "track" || source?.tool !== "track") continue;
            const clip = source.children.find((child) => child.name === clipName);
            if (clip?.tool !== "timeline") continue;
            const start = Math.max(clip.state.start + dx, 0);
            if (!alt && to === source) {
              clip.state.start = start;
              chosen.add(key);
              continue;
            }
            const name = freeName(to.children.map((child) => child.name), clipName);
            to.children = [...to.children, { name, tool: "timeline", state: { ...clip.state, start } }];
            if (!alt) source.children = source.children.filter((child) => child !== clip);
            chosen.add(`${to.name}/${name}`);
          }
          memory.selected = chosen;
        }),
    };
    const resizeDrag: Drag = {
      id: "resize",
      axis: "x",
      snap: { x: BEAT, grid: true },
      onEnd: ({ dx }) => editClips("Resize clips", memory.selected, (clip) => (clip.length = Math.max((clip.length ?? BAR) + dx, BEAT))),
    };
    const noteDrag: Drag = {
      id: "note",
      snap: { x: BEAT / 4, y: 1, grid: true },
      onEnd: ({ dx, dy }) => {
        const index = memory.note;
        if (index === null || !memory.open) return;
        editClips("Move a note", [memory.open], (clip) => {
          clip.notes = clip.notes.map((note, each) =>
            each === index ? { ...note, start: Math.max(note.start + dx, 0), pitch: clamp(note.pitch - dy, 0, 127) } : note,
          );
        });
      },
    };

    const pressClip = (key: string) => ({ clicks, shift }: Press) => {
      if (clicks === 2) {
        memory.open = key;
        memory.note = null;
      } else if (shift) {
        const next = new Set(memory.selected);
        if (!next.delete(key)) next.add(key);
        memory.selected = next;
      } else if (!memory.selected.has(key)) {
        memory.selected = new Set([key]);
      }
    };
    const pressLane = ({ clicks, x, y }: Press) => {
      const row = tracks()[Math.floor(y)];
      if (clicks !== 2 || !row) {
        memory.selected = new Set();
        return;
      }
      const start = Math.floor(x / BAR) * BAR;
      update("Add a clip", ({ children }) => {
        const owner = children.find((child) => child.name === row.name);
        if (owner?.tool !== "track") return;
        const name = freeName(owner.children.map((child) => child.name), "clip");
        owner.children = [...owner.children, { name, tool: "timeline", state: { start, length: BAR, notes: [] } }];
        memory.selected = new Set([`${owner.name}/${name}`]);
      });
    };
    const toggleMute = (name: string) =>
      update("Mute a track", ({ children }) => {
        const owner = children.find((child) => child.name === name);
        const mix = owner?.tool === "track" ? owner.children.find((child) => child.name === "mix") : undefined;
        if (mix?.tool === "utility") mix.state.mute = !mix.state.mute;
      });
    const toggleSolo = (name: string) =>
      update("Solo a track", ({ state }) => {
        state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
      });
    const muted = (each: (typeof children)[number]) => {
      const mix = each.tool === "track" ? each.children.find((child) => child.name === "mix") : undefined;
      return mix?.tool === "utility" && mix.state.mute === true;
    };

    // A text field holds any text: the colour is checked here.
    const colourOf = (each: (typeof children)[number]) => {
      const colour = each.tool === "track" ? each.state.colour : undefined;
      return (colour?.startsWith("#") ? colour : "#a9b1ff") as `#${string}`;
    };
    const timeline = now.space("timeline");
    const handle = timeline.pointX.times(6);

    const TrackHead = ({ each }: { each: (typeof children)[number] }) => (
      <>
        <div style={{ width: 56, fontSize: 11, color: COLOR.text }}>{each.name}</div>
        <Button label="M" on={() => muted(each)} color={COLOR.mute} onPress={() => toggleMute(each.name)} />
        <Button label="S" on={() => state.solo.includes(each.name)} color={COLOR.solo} onPress={() => toggleSolo(each.name)} />
        <Space key={`meter-${each.name}`} width={40} height={6} background={COLOR.lane} radius={2}>
          <Rect x={0} y={0} width={place(now.peak(`${each.name}/level`)).fall(0.6).times(40)} height={6} radius={2} color={COLOR.meter} />
        </Space>
      </>
    );

    return (
      <div style={{ grow: true, background: COLOR.back }}>
        <div style={{ direction: "row", grow: true }}>
          <div style={{ width: HEADER }}>
            <div style={{ height: RULER }} />
            <div style={{ grow: true }} follow={{ y: "timeline" }}>
              <For each={tracks}>
                {(each) => (
                  <div style={{ height: ROW, direction: "row", gap: 6, align: "center", paddingX: 8 }}>
                    <TrackHead each={each} />
                  </div>
                )}
              </For>
            </div>
          </div>
          <div style={{ grow: true }}>
            <Space key="ruler" height={RULER} unit={{ x: ACROSS }} extent={{ x: [0, END] }} zoom={{ x: [0.1, 8] }} follow={{ x: "timeline" }}>
              {Array.from({ length: BARS }, (_, bar) => (
                <Label x={bar * BAR + 60} y={5} text={String(bar + 1)} size={10} color={COLOR.text} />
              ))}
            </Space>
            <Space key="timeline" grow unit={{ x: ACROSS, y: ROW }} extent={() => ({ x: [0, END], y: [0, Math.max(tracks().length, 1)] })} zoom={{ x: [0.1, 8] }}
              background={COLOR.lane} onPress={pressLane}>
              {Array.from({ length: BARS }, (_, bar) => (
                <Line from={[bar * BAR, 0]} to={[bar * BAR, 1000]} color={COLOR.bar} width={1} />
              ))}
              <For each={tracks}>
                {(each, row) => (
                  <For each={() => clipsOf(each)}>
                    {(clip) => {
                      const key = `${each.name}/${clip.name}`;
                      const picked = () => isPicked(key);
                      const length = () => clip.state.length ?? BAR;
                      const resizes = () => (picked() ? RESIZES : undefined);
                      // The clip and its notes move together, and its notes end at its end.
                      return (
                        <Group x={() => clip.state.start ?? 0} y={row} clip={() => ({ width: length(), height: 1 })}
                          moves={() => (picked() ? MOVES : undefined)} grows={resizes}>
                          <Rect x={0} y={0.06} width={length} height={0.88} radius={3} anchor={{ x: 0, y: 0 }}
                            color={() => (picked() ? COLOR.chosen : colourOf(each))}
                            grows={resizes} drag={moveDrag} onPress={pressClip(key)} cursor="grab" />
                          <For each={() => clip.state.notes ?? []}>
                            {(note) => (
                              <Rect x={() => note.start} y={() => 0.12 + (1 - (clamp(note.pitch, LOW, HIGH) - LOW) / (HIGH - LOW)) * 0.7}
                                width={() => note.length} height={0.05} color={COLOR.note} />
                            )}
                          </For>
                          <Rect x={() => handle.negate().plus(length())} y={0.06} width={handle} height={0.88} color={COLOR.clear}
                            anchor={() => ({ x: length() })} moves={resizes}
                            drag={resizeDrag} onPress={pressClip(key)} cursor="ew-resize" />
                        </Group>
                      );
                    }}
                  </For>
                )}
              </For>
              <Line from={[now.tick, 0]} to={[now.tick, 1000]} color={COLOR.playhead} width={2} />
            </Space>
          </div>
        </div>
        <Show when={open}>
          {() => (
            <div style={{ height: EDITOR, gap: 4, padding: 8 }}>
              <div style={{ direction: "row", gap: 8, align: "center" }}>
                <div style={{ fontSize: 11, color: COLOR.text }}>{() => memory.open ?? ""}</div>
                <div style={{ fontSize: 11, color: COLOR.edit }} onPress={() => (memory.open = null)}>Close</div>
              </div>
              <Space key="editor" grow unit={{ x: ACROSS * 4, y: NOTE_ROW }} extent={{ x: [0, 8 * BAR], y: [0, 128] }} zoom={{ x: [0.25, 8] }}
                scroll={{ y: 127 - 84 }} background={COLOR.lane}
                onPress={({ clicks, x, y }) => {
                  const key = memory.open;
                  if (clicks !== 2 || !key) return;
                  const note = { start: Math.floor(x / (BEAT / 4)) * (BEAT / 4), length: BEAT / 2, pitch: 127 - Math.floor(y), velocity: 100 };
                  editClips("Add a note", [key], (clip) => (clip.notes = [...clip.notes, note]));
                }}>
                <For each={() => open()?.state.notes ?? []}>
                  {(note, index) => (
                    <Rect x={() => note.start} y={() => 127 - note.pitch} width={() => note.length} height={0.9} radius={2} color={COLOR.edit}
                      moves={() => (memory.note === index() ? NOTE : undefined)} drag={noteDrag} cursor="grab"
                      onPress={({ clicks }) => {
                        const key = memory.open;
                        if (clicks === 2 && key) {
                          memory.note = null;
                          editClips("Remove a note", [key], (clip) => (clip.notes = clip.notes.filter((_, each) => each !== index())));
                        } else {
                          memory.note = index();
                        }
                      }} />
                  )}
                </For>
              </Space>
            </div>
          )}
        </Show>
      </div>
    );
  },
});
