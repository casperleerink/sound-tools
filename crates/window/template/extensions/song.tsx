import { boolean, choice, h, list, live, memo, text, tool, type Color, type Live, type Connection, type Declaration, type Drawn, type Lines, type Node, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import { tape } from "./tape";
import { track } from "./track";
import { Fader, MIN_DB } from "./ui/fader";
import { Dot } from "./ui/dot";
import { Meter } from "./ui/meter";
import { Section } from "./ui/blocks/panel";
import { Toggle } from "./ui/toggle";

// A song: tracks of clips in time, the devices of the chosen track, and a mixer.
export const parts = { master: "fader", limit: "limiter", quiet: "lane", "audio-*": tape, "ret-*": bus, "*": track } as const;
const fields = {
  order: list(text(), { max: 128 }),
  solo: list(text(), { max: 64 }),
  sidechains: list({ effect: text(), from: text(), tap: choice({ options: ["pre_fx", "post_fx", "post_fader"], default: "post_fader" }) }, { max: 64 }),
  limit_off: boolean({ default: false }),
};

export const song = tool({
  name: "song",
  title: "Song",
  when: "You change the song page: its tracks, clips, panel or mixer",
  doc: "Tracks of clips in time, the devices of the chosen track under them, and a mixer. Each `track` child and each audio track `audio-*` plays into the fader `master`, then its limiter `limit` unless `limit_off`; `order` is the order of the tracks, `solo` names the tracks that play alone. Each of `sidechains` keys an effect, by its path such as `t00/chain/comp`, with what a track `from` plays at `tap`.",
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
    // What the song hears, such as the keyboard, plays its first track.
    const first = ordered(children.filter((child) => child.tool === "track"), state.order)[0];
    if (first) connections.push({ from: { notes: "notes" }, to: { child: first.name, notes: "notes" } });
    // A solo mutes every other track's fader, gliding; a return always plays.
    const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
    for (const each of tracks) {
      if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
    }
    // A send of a track plays into its return, after the track's fader or before it.
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
  memory: (): Memory => ({ chosen: null, armed: [] }),
  view: (self, { memory, update, held }) => {
    /** The gain of `name`'s fader as a lane plays it, when one does. */
    const playing = (gain: string) => (held(gain) ? live.field(gain) : undefined);
    const tracks = ordered(self.children.filter((child): child is Track => child.tool === "track"), self.state.order);
    const chosen = tracks.find((each) => each.name === memory.chosen) ?? tracks[0];
    /** Changes the fader of `name`, a track or the master. */
    const mixing = (name: string, label: string, edit: (mix: { gain_db: number | "-inf"; mute: boolean }) => void) =>
      update(label, (draft) => {
        const owner = draft.children.find((child) => child.name === name);
        const mix = name === "master" ? owner : owner?.children.find((child) => child.name === "mix");
        if (mix?.tool === "fader") edit(mix.state);
      });
    return (
      <col w="full" h="full" fill="background">
        <Ruler />
        <box grow={1} w="full" clip>
          <col w="full" h="full" scroll="y">
            {tracks.map((each, index) => (
              <TrackRow
                each={each}
                index={index}
                chosen={each === chosen}
                armed={memory.armed.includes(each.name)}
                soloed={self.state.solo.includes(each.name)}
                onChoose={() => (memory.chosen = each.name)}
                onMute={() => mixing(each.name, `Mute ${titleOf(each, index)}`, (mix) => (mix.mute = !mix.mute))}
                onSolo={() =>
                  update(`Solo ${titleOf(each, index)}`, (draft) => {
                    const solo = draft.state.solo;
                    draft.state.solo = solo.includes(each.name) ? solo.filter((name) => name !== each.name) : [...solo, each.name];
                  })
                }
                onArm={() => (memory.armed = memory.armed.includes(each.name) ? memory.armed.filter((name) => name !== each.name) : [...memory.armed, each.name])}
              />
            ))}
          </col>
          <box inset={[HEAD, 0, 0, 0]} clip inert>
            <box key="playhead" inset={[0, 0, null, 0]} w={1} fill="text" at={[live.playhead.times(ZOOM).plus(LEAD), 0]} />
          </box>
        </box>
        {chosen && <Panel each={chosen} title={titleOf(chosen, tracks.indexOf(chosen))} />}
        <row w="full" h={MIXER} px={3} gap={1} fill="surface" border="border-strong" borderSides={[1, 0, 0, 0]} scroll="x">
          {tracks.map((each, index) => (
            <Strip id={each.name} name={each.name} title={titleOf(each, index)} db={dbOf(mixOf(each)?.gain_db)} held={playing(`${each.name}/mix.gain_db`)} onChange={(db) => mixing(each.name, `Volume ${titleOf(each, index)}`, (mix) => (mix.gain_db = db))} />
          ))}
          <box grow={1} />
          <Strip id="master" name="master" title="Master" db={dbOf(self.children.find((child) => child.name === "master" && child.tool === "fader")?.state.gain_db)} held={playing("master.gain_db")} onChange={(db) => mixing("master", "Volume Master", (mix) => (mix.gain_db = db))} />
        </row>
      </col>
    );
  },
});

/** `children` in the order `order` names them, then the others by name. */
function ordered<T extends { name: string }>(children: readonly T[], order: readonly string[]): T[] {
  const rank = (name: string) => (order.includes(name) ? order.indexOf(name) : order.length);
  return [...children].sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
}

type Song = Drawn<ValuesOf<typeof fields>, typeof parts>;
type Child = Song["children"][number];
type Track = Extract<Child, { tool: typeof track }>;
type Memory = { chosen: string | null; armed: string[] };

const [BAR, BEAT] = [3840, 960];
/** Points per tick, and the bars the timeline shows. `PANEL` is the most the devices take before they scroll. */
const [ZOOM, BARS] = [0.03, 32];
const [HEAD, ROW, RULER, LEAD, PANEL, MIXER, FADER] = [208, 56, 24, 8, 320, 208, 120];
const [LOW, HIGH] = [36, 96];
const COLOURS = ["blue", "green", "yellow", "peach", "red", "lavender", "sky", "teal", "mauve", "pink"];

const colourOf = (each: Track): Color => (COLOURS.includes(each.state.colour) ? (each.state.colour as Color) : "blue");
const titleOf = (each: Track, index: number) => each.state.title || `Track ${index + 1}`;
/** A fader's gain, which may be `"-inf"`, as a number of dB. */
const dbOf = (gain: number | "-inf" | undefined) => (gain === "-inf" ? MIN_DB : (gain ?? 0));
const mixOf = (each: Track) => {
  const mix = each.children.find((child) => child.name === "mix");
  return mix?.tool === "fader" ? mix.state : undefined;
};


const Ruler = () =>
  memo("ruler", [], () => {
    const points: number[] = [];
    const labels = [];
    for (let tick = 0; tick <= BARS * BAR; tick += BEAT) {
      const x = LEAD + tick * ZOOM;
      points.push(x, tick % BAR === 0 ? 8 : 16, x, RULER);
      if (tick % BAR === 0 && tick < BARS * BAR) labels.push(<text at={[x + 4, 4]} style="label" color="text-muted">{tick / BAR + 1}</text>);
    }
    return (
      <row w="full" h={RULER} fill="surface" border="border-strong" borderSides={[0, 0, 1, 0]}>
        <box w={HEAD} h={RULER} />
        <box grow={1} h={RULER} clip>
          <shapes at={[0, 0]} w={LEAD + BARS * BAR * ZOOM} h={RULER} lines={[{ color: "border-strong", width: 1, points }]} />
          {labels}
        </box>
      </row>
    );
  });

const TrackRow = ({ each, index, chosen, armed, soloed, onChoose, onMute, onSolo, onArm }: { each: Track; index: number; chosen: boolean; armed: boolean; soloed: boolean; onChoose: () => void; onMute: () => void; onSolo: () => void; onArm: () => void }) => {
  const title = titleOf(each, index);
  const muted = mixOf(each)?.mute ?? false;
  const header = memo(`head-${each.name}`, [title, chosen, muted, soloed, armed, each.state.colour], () => (
    <row key={`track-${each.name}`} label={`Choose ${title}`} role="tab" checked={chosen} hoverGroup w={HEAD} h={ROW} px={3} gap={2} align="center" fill={chosen ? "selected" : "surface"} hover={chosen ? undefined : "fill/50"} border="border-strong" borderSides={[0, 1, 0, 0]} onPress={onChoose}>
      <Dot color={colourOf(each)} />
      <col grow={1} gap={1}>
        <text style="heading" color={chosen ? "text" : "text-secondary"} truncate>{title}</text>
        <row gap={1}>
          <Quiet shown={muted || chosen}><Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={muted} color="warning" onToggle={onMute} /></Quiet>
          <Quiet shown={soloed || chosen}><Toggle id={`solo-${each.name}`} label={`Solo ${title}`} text="S" on={soloed} color="hot" onToggle={onSolo} /></Quiet>
          <Quiet shown={armed || chosen}><Toggle id={`arm-${each.name}`} label={`Arm ${title}`} on={armed} color="danger" dot onToggle={onArm} /></Quiet>
        </row>
      </col>
      <Meter id={`meter-head-${each.name}`} label={`Level ${title}`} path={`${each.name}/mix`} w={4} h={ROW - 16} />
    </row>
  ));
  const lane = memo(`lane-${each.name}`, [each.children, each.state.colour], () => (
    <box w={LEAD + BARS * BAR * ZOOM} h={ROW}>
      {each.children.flatMap((clip) => (clip.tool !== "timeline" ? [] : [
        <Clip name={clip.name} start={clip.state.start ?? 0} length={clip.state.length ?? Math.max(BAR, ...clip.state.notes.map((note) => note.start + note.length))} notes={clip.state.notes} colour={colourOf(each)} />,
      ]))}
    </box>
  ));
  return (
    <row w="full" h={ROW} border="border" borderSides={[0, 0, 1, 0]}>
      {header}
      <box grow={1} h={ROW} clip>
        {lane}
      </box>
    </row>
  );
};

/** A clip: its notes are drawn in ticks across and pitches down, so they stretch with the clip. */
const Clip = ({ name, start, length, notes, colour }: { name: string; start: number; length: number; notes: readonly { start: number; length: number; pitch: number }[]; colour: Color }) => {
  const [w, tall] = [length * ZOOM, ROW - 8];
  const points: number[] = [];
  for (const note of notes) points.push(note.start, HIGH - note.pitch, note.start + note.length, HIGH - note.pitch);
  const lines: Lines[] = [{ color: colour, width: 2, points }];
  return (
    <box at={[LEAD + start * ZOOM, 4]} w={w} h={tall} radius="sm" fill={`${colour}/15` as Color} border={`${colour}/60` as Color} clip>
      <text at={[6, 2]} style="label" color="text">{name}</text>
      <shapes at={[0, 20]} w={w} h={tall - 24} units={[length, HIGH - LOW]} lines={lines} />
    </box>
  );
};

/** A toggle of a track's head shows while on, or while the track is chosen or pointed at. */
const Quiet = ({ shown, children }: { shown: boolean; children?: Node }) => (shown ? <box>{children}</box> : <box reveal>{children}</box>);

/** The devices of the chosen track: what plays it, then its effects in their order. */
const Panel = ({ each, title }: { each: Track; title: string }) => {
  const chain = each.children.find((child) => child.name === "chain");
  const effects = chain?.tool === "chain" ? chain.state.order.filter((name) => chain.children.some((child) => child.name === name)) : [];
  const devices = [...(each.children.some((child) => child.name === "synth") ? ["synth"] : []), ...effects.map((name) => `chain/${name}`)];
  return (
    <col w="full" px={4} py={3} fill="background" border="border-strong" borderSides={[1, 0, 0, 0]}>
      <Section title={title}>
        <box w="full" maxH={PANEL} scroll="both">
          <row gap={2}>
            {devices.map((path) => (
              <child path={`${each.name}/${path}`} />
            ))}
          </row>
        </box>
      </Section>
    </col>
  );
};

const Strip = ({ id, name, title, db, held, onChange }: { id: string; name: string; title: string; db: number; held?: Live; onChange: (db: number) => void }) => (
  <col key={`strip-${id}`} w={72} h="full" py={3} gap={2} align="center">
    <text style="label" color="text-secondary" truncate>{title}</text>
    <row gap={2} align="start">
      <Fader id={`fader-${id}`} label={`Volume ${title}`} db={db} height={FADER} held={held} onChange={onChange} />
      <Meter id={`meter-${id}`} label={`Level ${title}`} path={name === "master" ? "master" : `${name}/mix`} w={4} h={FADER} />
    </row>
  </col>
);
