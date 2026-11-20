import { h, live, memo, tool, type Color, type Connection, type Declaration, type Drawn, type Lines, type Node, type ValuesOf } from "./sdk";
import { Fader, MAX_DB, MIN_DB } from "./ui/fader";
import { control, type Bound } from "./ui/field";
import { Dot } from "./ui/dot";
import { Meter } from "./ui/meter";
import { Section } from "./ui/blocks/panel";
import { Toggle } from "./ui/toggle";
import { lengthAt, lengthOf } from "./song/clips";
import { editsOf } from "./song/edits";
import { actions } from "./song/keys";
import { colourOf, fields, isStrip, memory, mixOf, muted, ordered, parts, titleOf } from "./song/model";
import { notesTrackOf, trackEdits } from "./song/tracks";
import type { bus } from "./bus";
import type { tape } from "./tape";
import type { track } from "./track";
import { BAR, BEAT } from "./song/time";

// A song: tracks of clips in time, the devices of the chosen track, and a mixer. What it does is
// in `song/`; this view draws it.
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
    // `update` stays named here: the window's tests put a button into this view that calls it.
    const { memory, update } = ctx;
    const edits = editsOf(ctx);
    const strips = ordered(self.children.filter(isStrip), self.state.order);
    const rows = strips.filter((each) => each.tool !== "bus");
    const returns = strips.filter((each) => each.tool === "bus");
    const shown = memory.below?.kind === "track" ? memory.below.name : (memory.track ?? rows[0]?.name);
    const chosen = strips.find((each) => each.name === shown) ?? rows[0];
    /** The volume of `name`'s fader: a track, a return or the master. */
    const volume = (name: string, title: string, gain: number | "-inf" | undefined) =>
      control(ctx.held, `${name === "master" ? "master" : `${name}/mix`}.gain_db`, GAIN, dbOf(gain), (db) => edits.tracks.volume(name, db));
    return (
      <col w="full" h="full" fill="background">
        <Ruler />
        <box grow={1} w="full" clip>
          <col w="full" h="full" scroll="y">
            {strips.map((each) => (
              <TrackRow
                each={each}
                chosen={each === chosen}
                armed={memory.armed.includes(each.name)}
                soloed={self.state.solo.includes(each.name)}
                tempo={ctx.clock.tempo_map.tempo_changes}
                onChoose={() => edits.tracks.openPanel(each.name)}
                onMute={() => edits.tracks.mute(each.name)}
                onSolo={() => edits.tracks.solo(each.name)}
                onArm={() => edits.tracks.arm(each.name)}
              />
            ))}
          </col>
          <box inset={[HEAD, 0, 0, 0]} clip inert>
            <box key="playhead" inset={[0, 0, null, 0]} w={1} fill="text" at={[live.playhead.times(ZOOM).plus(LEAD), 0]} />
          </box>
        </box>
        {chosen && <Panel each={chosen} title={titleOf(chosen)} />}
        <row w="full" h={MIXER} px={3} gap={1} fill="surface" border="border-strong" borderSides={[1, 0, 0, 0]} scroll="x">
          {[...rows, ...returns].map((each) => {
            const title = titleOf(each);
            return <Strip id={each.name} name={each.name} title={title} fader={volume(each.name, title, mixOf(each)?.state.gain_db)} />;
          })}
          <box grow={1} />
          <Strip id="master" name="master" title="Master" fader={volume("master", "Master", self.children.find((child) => child.name === "master" && child.tool === "fader")?.state.gain_db)} />
        </row>
      </col>
    );
  },
});

type Seen = Drawn<ValuesOf<typeof fields>, typeof parts>["children"][number];
/** A track, an audio track or a return, as the view gets it. */
type Strip = Extract<Seen, { tool: typeof track | typeof tape | typeof bus }>;

/** Points per tick, and the bars the timeline shows. `PANEL` is the most the devices take before they scroll. */
const [ZOOM, BARS] = [0.03, 32];
const [HEAD, ROW, RULER, LEAD, PANEL, MIXER, FADER] = [208, 56, 24, 8, 320, 208, 120];
const [LOW, HIGH] = [36, 96];
/** The gain of a fader, as its control reads it. */
const GAIN = { min: MIN_DB, max: MAX_DB, unit: "db" } as const;
/** A fader's gain, which may be `"-inf"`, as a number of dB. */
const dbOf = (gain: number | "-inf" | undefined) => (gain === "-inf" ? MIN_DB : (gain ?? 0));

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

type TrackRowProps = { each: Strip; chosen: boolean; armed: boolean; soloed: boolean; tempo: readonly { tick: number; bpm: number }[]; onChoose: () => void; onMute: () => void; onSolo: () => void; onArm: () => void };

/** A row of a track, an audio track or a return: its head, and its clips or regions in time. A return has no solo, and only an audio track arms. */
const TrackRow = ({ each, chosen, armed, soloed, tempo, onChoose, onMute, onSolo, onArm }: TrackRowProps) => {
  const title = titleOf(each);
  const isMuted = muted(each);
  const colour = colourOf(each);
  const header = memo(`head-${each.name}`, [title, chosen, isMuted, soloed, armed, colour], () => (
    <row key={`track-${each.name}`} label={`Choose ${title}`} role="tab" checked={chosen} hoverGroup w={HEAD} h={ROW} px={3} gap={2} align="center" fill={chosen ? "selected" : "surface"} hover={chosen ? undefined : "fill/50"} border="border-strong" borderSides={[0, 1, 0, 0]} onPress={onChoose}>
      <Dot color={colour} />
      <col grow={1} gap={1}>
        <text style="heading" color={chosen ? "text" : "text-secondary"} truncate>{title}</text>
        <row gap={1}>
          <Quiet shown={isMuted || chosen}><Toggle id={`mute-${each.name}`} label={`Mute ${title}`} text="M" on={isMuted} color="warning" onToggle={onMute} /></Quiet>
          {each.tool !== "bus" && <Quiet shown={soloed || chosen}><Toggle id={`solo-${each.name}`} label={`Solo ${title}`} text="S" on={soloed} color="hot" onToggle={onSolo} /></Quiet>}
          {each.tool === "tape" && <Quiet shown={armed || chosen}><Toggle id={`arm-${each.name}`} label={`Arm ${title}`} on={armed} color="danger" dot onToggle={onArm} /></Quiet>}
        </row>
      </col>
      <Meter id={`meter-head-${each.name}`} label={`Level ${title}`} path={`${each.name}/mix`} w={4} h={ROW - 16} />
    </row>
  ));
  const lane = memo(`lane-${each.name}`, [each.children, colour, tempo], () => (
    <box w={LEAD + BARS * BAR * ZOOM} h={ROW}>
      {each.children.flatMap((item) => {
        if (item.tool === "timeline") return [<Clip name={item.state.title || ""} start={item.state.start} length={lengthOf(item.state)} notes={item.state.notes} colour={colour} />];
        if (item.tool === "region") {
          const length = lengthAt({ children: [each], clock: { tempo_map: { tempo_changes: tempo } } }, `${each.name}/${item.name}`);
          return [<Clip name={item.name} start={item.state.start} length={Math.max(length, BEAT)} notes={[]} colour={colour} />];
        }
        return [];
      })}
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

/** The devices of the chosen track or return: what plays it, then its effects in their order. */
const Panel = ({ each, title }: { each: Strip; title: string }) => {
  const chain = each.children.find((child) => child.name === "chain");
  // Each it names, also one whose code did not load: it says so where it sits.
  const effects = chain?.tool === "chain" ? chain.state.order : [];
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

const Strip = ({ id, name, title, fader }: { id: string; name: string; title: string; fader: Bound }) => (
  <col key={`strip-${id}`} w={72} h="full" py={3} gap={2} align="center">
    <text style="label" color="text-secondary" truncate>{title}</text>
    <row gap={2} align="start">
      <Fader {...fader} id={`fader-${id}`} label={`Volume ${title}`} height={FADER} />
      <Meter id={`meter-${id}`} label={`Level ${title}`} path={name === "master" ? "master" : `${name}/mix`} w={4} h={FADER} />
    </row>
  </col>
);
