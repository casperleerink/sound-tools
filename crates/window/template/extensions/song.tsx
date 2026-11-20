import { h, live, tool, type Child, type Connection, type Declaration } from "./sdk";
import { Editor } from "./ui/blocks/song/editor";
import { Heads } from "./ui/blocks/song/heads";
import { Mixer } from "./ui/blocks/song/mixer";
import { Rack } from "./ui/blocks/song/rack";
import { Ruler } from "./ui/blocks/song/ruler";
import { Timeline } from "./ui/blocks/song/timeline";
import { actions } from "./song/keys";
import { EDITOR, fields, itemAt, memory, parts } from "./song/model";
import { notesTrackOf, trackEdits } from "./song/tracks";
import { DIVIDER, MIXER, pageOf, RACK, ROW, SHORTEST, type Page } from "./song/view";

// A song: tracks of clips in time, and under them the devices of the chosen track, the notes of a
// clip or the mixer. What it does is in `song/`; its blocks, in `ui/blocks/song/`, draw it.
export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, panel or mixer",
  doc: "Tracks of clips in time, the devices of the chosen track under them, and a mixer. Each `track` child and each audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the tracks, `solo` names the tracks that play alone. Each return `ret-*` plays what the sends `send-<return>` of the tracks bring into the master too. A track's arm opens its recorder `rec`; R records every armed track and the keyboard's notes on the chosen track. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays at `tap`. Its keys are its `actions`, in `song/keys.ts`; what they and the view change, in `song/edits.ts`.",
  state: fields,
  children: parts,
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const tracks = children.filter((child) => child.tool === "track" || child.tool === "tape");
    const returns = children.filter((child) => child.tool === "bus");
    const soloed = state.solo.filter((name) => tracks.some((each) => each.name === name));
    const connections: Connection<typeof parts>[] = [
      ...[...tracks, ...returns].map((each) => ({ from: { child: each.name }, to: { child: "master" } })),
      ...(state.limit_off
        ? [{ from: { child: "master" }, to: { port: "audio" } } as const]
        : [{ from: { child: "master" }, to: { child: "limit" } } as const, { from: { child: "limit" }, to: { port: "audio" } } as const]),
    ];
    // What the song hears, the keyboard while nothing is chosen, plays its first track.
    const first = notesTrackOf(children, state.order, null);
    if (first) connections.push({ from: { notes: "notes" }, to: { child: first, notes: "notes" } });
    // A solo mutes every other track's fader, gliding, so a lane that moves its volume plays on under it; a return always plays.
    const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
    for (const each of tracks) {
      if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
    }
    // A send of a track plays into its return: after the track's fader, or before it for a return that says so. A muted or soloed-out track sends nothing either way.
    for (const each of tracks) {
      const mix = each.children.find((child) => child.name === "mix");
      const silent = soloedOut(each.name) || (mix?.tool === "fader" && mix.state.mute === true);
      for (const back of returns) {
        if (back.tool !== "bus" || !each.children.some((child) => child.name === `send-${back.name}`)) continue;
        const from = back.state.pre_fader ? "chain" : "mix";
        const send = `${each.name}/send-${back.name}` as const;
        connections.push({ from: { child: `${each.name}/${from}` as const }, to: { child: send } });
        connections.push({ from: { child: send }, to: { child: back.name } });
        if (back.state.pre_fader && silent) connections.push({ from: { child: "quiet", number: "value" }, to: { child: send, number: "mute" } });
      }
    }
    // A sidechain keys an effect with a track: what plays into its effects, what comes out of them, or what comes out of its fader.
    for (const { effect, from, tap } of state.sidechains) {
      const source = tracks.find((each) => each.name === from);
      if (!source) continue;
      const before = source.children.filter((child) => child.tool === "region" || child.name === "synth").map((child) => child.name);
      for (const key of tap === "pre_fx" ? before : [tap === "post_fx" ? "chain" : "mix"]) {
        connections.push({ from: { child: `${from}/${key}` as `${string}/${string}` }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
      }
    }
    return { connections };
  },
  memory,
  actions,
  tick: (song) => trackEdits(song).stopped(),
  view: (self, ctx) => {
    const { memory } = ctx;
    const page = pageOf(self, ctx);
    // A song opens on its first track, chosen, with its panel below.
    if (!memory.opened) {
      memory.opened = true;
      const first = page.tracks[0];
      if (first && memory.below === null) [memory.below, memory.track] = [{ kind: "track", name: first.name }, first.name];
    }
    const below = memory.below;
    const clip = below?.kind === "editor" && itemAt(self.children, below.clip)?.tool === "timeline" ? below.clip : null;
    const large = clip !== null && memory.editorLarge;
    return (
      <col w="full" h="full" fill="background">
        <Ruler page={page} />
        <row w="full" {...(large ? { h: ROW + page.low } : { grow: 1 })}>
          <Heads page={page} />
          <Timeline page={page} />
        </row>
        {clip !== null ? (
          <col w="full" {...(large ? { grow: 1 } : { h: DIVIDER + Math.max(memory.editor, SHORTEST) })}>
            <Divider page={page} />
            <Editor page={page} clip={clip} />
          </col>
        ) : below?.kind === "mixer" ? (
          <Below h={MIXER}><Mixer page={page} /></Below>
        ) : below?.kind === "master" || (below?.kind === "track" && page.strips.some((each) => each.name === below.name)) ? (
          <Below h={RACK}><Rack page={page} name={below.kind === "master" ? "master" : below.name} /></Below>
        ) : null}
      </col>
    );
  },
});

/** What shows under the tracks, `tall` points, under a line. */
const Below = ({ h: tall, children }: { h: number; children?: Child }) => (
  <box w="full" h={tall} border="border-strong" borderSides={[1, 0, 0, 0]}>
    {children}
  </box>
);

/** How much an arrow on the editor's line makes it taller or shorter. */
const STEP = 16;

/** The line over the note editor: a drag makes the editor taller, a double click puts it back, the arrows step it. */
const Divider = ({ page }: { page: Page }) => {
  const { memory } = page.song;
  const resize = (height: number) => {
    memory.editor = Math.max(Math.round(height), SHORTEST);
    memory.editorLarge = false;
  };
  return (
    <box key="editor-divider" label="Note editor height" role="slider" value={memory.editor} w="full" h={DIVIDER} drag={{ y: true }}
      onDragEnd={(p) => resize(memory.editor - p.dy)} onPress={(p) => p.clicks === 2 && resize(EDITOR)}
      actions={{ increment: (by) => resize(memory.editor + by * STEP), decrement: (by) => resize(memory.editor - by * STEP), reset: () => resize(EDITOR) }}>
      <box at={[0, live.drag("editor-divider").dy.plus((DIVIDER - 1) / 2)]} w={100000} h={1} fill="border-strong" />
    </box>
  );
};
