import { h, live, type Child } from "../../../sdk";
import { publish } from "../../../song/clips";
import { EDITOR, itemAt } from "../../../song/model";
import { DIVIDER, MIXER, pageOf, RACK, ROW, SHORTEST, type Page } from "../../../song/view";
import { Editor } from "./editor";
import { Heads } from "./heads";
import { Mixer } from "./mixer";
import { Rack } from "./rack";
import { PITCH } from "./roll-view";

/** The page of a song as its view starts it: opened on its first track, chosen, with its panel below, once. */
export function openPage(...[self, ctx]: Parameters<typeof pageOf>): Page {
  const page = pageOf(self, ctx);
  const { memory } = ctx;
  if (!memory.opened) {
    memory.opened = true;
    const first = page.tracks[0];
    if (first && memory.below === null) [memory.below, memory.track] = [{ kind: "track", name: first.name }, first.name];
  }
  publish(ctx);
  return page;
}

/**
 * A song's page: the band over the tracks, the heads beside the tracks in time, and under them
 * the devices of the chosen track, the notes of a clip or the mixer. The tracks page and the
 * score page differ in their band and their time.
 */
export const SongPage = ({ page, band, time }: { page: Page; band: Child; time: Child }) => {
  const { memory } = page.song;
  const below = memory.below;
  const clip = below?.kind === "editor" && itemAt(page.self.children, below.clip)?.tool === "timeline" ? below.clip : null;
  const large = clip !== null && memory.editorLarge;
  return (
    <col w="full" h="full" fill="background">
      {band}
      <row w="full" {...(large ? { h: ROW + page.low } : { grow: 1 })}>
        <Heads page={page} />
        {time}
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
};

/** What shows under the tracks, `tall` points, under a line: drawn over it, as a border would lie under what fills it. */
const Below = ({ h: tall, children }: { h: number; children?: Child }) => (
  <box w="full" h={tall}>
    {children}
    <box inset={[0, 0, 0, null]} h={1} fill="border-strong" inert />
  </box>
);

/** How much a step of the editor's line makes it taller or shorter: a row of notes. */
const STEP = PITCH;

/** The line over the note editor: a drag makes the editor taller a row of notes at a time, a double click puts it back, the arrows step it. */
const Divider = ({ page }: { page: Page }) => {
  const { memory } = page.song;
  const resize = (height: number) => {
    memory.editor = Math.max(Math.round(height), SHORTEST);
    memory.editorLarge = false;
  };
  return (
    <box key="editor-divider" label="Note editor height" role="slider" value={memory.editor} w="full" h={DIVIDER} drag={{ y: true, snap: [1, STEP], from: [0, 0] }}
      onDragEnd={(p) => resize(memory.editor - Math.round(p.dy / STEP) * STEP)} onPress={(p) => p.clicks === 2 && resize(EDITOR)}
      actions={{ increment: (by) => resize(memory.editor + by * STEP), decrement: (by) => resize(memory.editor - by * STEP), reset: () => resize(EDITOR) }}>
      <box at={[0, live.drag("editor-divider").dy.plus((DIVIDER - 1) / 2)]} w={100000} h={1} fill="border-strong" />
    </box>
  );
};
