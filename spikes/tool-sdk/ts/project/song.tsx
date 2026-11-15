// The song page: transport, ruler, a row per track with its clips, and the mixer.
import { change, memo, type Lines } from "./sdk";
import { Button } from "./ui/button";
import { Fader } from "./ui/fader";
import { Meter } from "./ui/meter";
import { Toggle } from "./ui/toggle";

type Note = { start: number; length: number; pitch: number; velocity: number };
type Clip = { name: string; start: number; length: number; notes: Note[] };
type Track = { name: string; colour: `#${string}`; clips: Clip[]; mute: boolean; solo: boolean; armed: boolean; gain_db: number };
/** `view` is where the timeline is: points per tick, and which page of `PAGE` points is in view. */
export type State = { song: { tracks: Track[] }; playing: boolean; view: { zoom: number; page: number } };

const [BAR, BEAT] = [3840, 960];
const [ROW, HEADER, LEAD, RULER, TRANSPORT, MIXER, STRIP, METER] = [64, 176, 8, 28, 44, 232, 72, 140];
const [WIDTH, HEIGHT, PAGE] = [1440, 900, 1200];
const [LOW, HIGH] = [48, 84];

const edit = change<State>;

/** The ticks the timeline builds: a page each side of the one in view, in whole bars. */
const built = ({ zoom, page }: State["view"]) => {
  const tick = (x: number) => Math.max(0, (x - LEAD) / zoom);
  const from = Math.floor(tick(Math.max(0, page * PAGE - PAGE)) / BAR) * BAR;
  return [from, Math.ceil(tick(page * PAGE + WIDTH + PAGE) / BAR) * BAR] as const;
};

export const view = (s: State) => (
  <box w="full" h="full" fill="bg">
    <col w="full" h="full">
      <row w="full" h={TRANSPORT} px={3} gap={4} align="center" fill="surface">
        <Button id="play" glyph="▶" on={s.playing} onPress={() => edit((d) => void (d.playing = !d.playing))} />
        <text key="time" style="mono" color="text" w={90} live="time">1.1.000</text>
        <text style="sm" color="text-muted">120 bpm</text>
      </row>
      <Ruler view={s.view} />
      <col w="full" h={HEIGHT - TRANSPORT - RULER - MIXER} scroll>
        <col w="full">{s.song.tracks.map((track, i) => <TrackRow track={track} index={i} view={s.view} />)}</col>
      </col>
      <row w="full" h={MIXER} px={2} gap={1} fill="surface" borderSides={[1, 0, 0, 0]} border="border">
        {s.song.tracks.map((track, i) => <Strip index={i} name={track.name} db={track.gain_db} color={track.colour} />)}
        <Strip index={s.song.tracks.length} name="Master" db={0} color="accent" />
      </row>
    </col>
    <box inset={[HEADER, TRANSPORT, 0, MIXER]} clip inert>
      <box key="playhead-scroll" inset={[0, 0, 0, 0]} x={{ live: "scroll", scale: -1 }}>
        <box key="playhead" inset={[0, 0, null, 0]} w={1} fill="accent" x={{ live: "playhead", scale: s.view.zoom, offset: LEAD }} />
      </box>
    </box>
  </box>
);

const Ruler = ({ view }: { view: State["view"] }) => {
  const [from, to] = built(view);
  const marks = memo("ruler", [view.zoom, from, to], () => {
    const points: number[] = [];
    const labels = [];
    for (let tick = from; tick <= to; tick += BEAT) {
      const x = LEAD + tick * view.zoom;
      points.push(x, tick % BAR === 0 ? 8 : 18, x, RULER);
      if (tick % BAR === 0) labels.push(<text at={[x + 4, 2]} style="xs" color="text-muted">{tick / BAR + 1}</text>);
    }
    const width = LEAD + to * view.zoom;
    return (
      <box w={width} h={RULER}>
        <shapes at={[0, 0]} w={width} h={RULER} lines={[{ color: "border", width: 1, points }]} />
        {labels}
      </box>
    );
  });
  return (
    <row w="full" h={RULER} fill="surface">
      <box w={HEADER} h={RULER} />
      <box grow={1} h={RULER} clip>
        <box key="ruler-scroll" at={[0, 0]} x={{ live: "scroll", scale: -1 }}>{marks}</box>
      </box>
    </row>
  );
};

const TrackRow = ({ track, index, view }: { track: Track; index: number; view: State["view"] }) => {
  const [from, to] = built(view);
  const flip = (field: "mute" | "solo" | "armed") => () => edit((d) => void (d.song.tracks[index]![field] = !d.song.tracks[index]![field]));
  const header = memo(`head-${index}`, [track.name, track.colour, track.mute, track.solo, track.armed], () => (
    <row w={HEADER} h={ROW} fill="surface" border="border" borderSides={[0, 1, 0, 0]}>
      <box w={4} h={ROW} fill={track.colour} />
      <col grow={1} pad={2} gap={2}>
        <text style="sm" color="text">{track.name}</text>
        <row gap={1}>
          <Toggle id={`mute-${index}`} label="M" on={track.mute} color="orange" onPress={flip("mute")} />
          <Toggle id={`solo-${index}`} label="S" on={track.solo} color="green" onPress={flip("solo")} />
          <Toggle id={`arm-${index}`} label="R" on={track.armed} color="red" onPress={flip("armed")} />
        </row>
      </col>
      <box pad={1}>
        <Meter id={`meter-head-${index}`} live={`meter-${index}`} w={4} h={ROW - 8} color={track.colour} />
      </box>
    </row>
  ));
  const lane = memo(`lane-${index}`, [track.clips, track.colour, view.zoom, from, to], () => {
    const width = LEAD + to * view.zoom;
    const bars: number[] = [];
    for (let tick = from; tick <= to; tick += BAR) bars.push(LEAD + tick * view.zoom, 0, LEAD + tick * view.zoom, ROW);
    const clips = track.clips
      .map((clip, c) => [clip, c] as const)
      .filter(([clip]) => clip.start + clip.length >= from && clip.start <= to)
      .map(([clip, c]) => memo(`clip-${index}-${c}`, [clip, track.colour, view.zoom], () => <ClipBox clip={clip} id={`${index}-${c}`} colour={track.colour} zoom={view.zoom} />));
    return (
      <box w={width} h={ROW}>
        <shapes at={[0, 0]} w={width} h={ROW} lines={[{ color: "border/50", width: 1, points: bars }]} />
        {clips}
      </box>
    );
  });
  return (
    <row w="full" h={ROW} border="border" borderSides={[0, 0, 1, 0]}>
      {header}
      <box grow={1} h={ROW} clip>
        <box key={`lane-scroll-${index}`} at={[0, 0]} x={{ live: "scroll", scale: -1 }}>{lane}</box>
      </box>
    </row>
  );
};

/** A clip: its notes are drawn in ticks across and pitches down, so a zoom only stretches them. */
const ClipBox = ({ clip, id, colour, zoom }: { clip: Clip; id: string; colour: `#${string}`; zoom: number }) => {
  const [w, h] = [clip.length * zoom, ROW - 6];
  const notes = memo(`notes-${id}`, [clip.notes, clip.length, colour], () => {
    const points: number[] = [];
    for (const n of clip.notes) {
      const y = HIGH - n.pitch;
      points.push(n.start, y, n.start + n.length, y);
    }
    const lines: Lines[] = [{ color: `${colour}e6`, width: 2, points }];
    return <shapes at={[0, 16]} w={w} h={h - 20} units={[clip.length, HIGH - LOW]} lines={lines} />;
  });
  return (
    <box at={[LEAD + clip.start * zoom, 3]} w={w} h={h} radius="sm" fill={`${colour}2e`} border={`${colour}cc`} clip>
      <text at={[4, 2]} style="xs" color="text">{clip.name}</text>
      {notes}
    </box>
  );
};

const Strip = ({ index, name, db, color }: { index: number; name: string; db: number; color: `#${string}` | "accent" }) => (
  <col key={`strip-${index}`} w={STRIP} h="full" py={2} gap={2} align="center">
    <text style="sm" color="text" truncate>{name}</text>
    <row gap={1}>
      <Fader id={`fader-${index}`} db={db} height={METER} onChange={(gain) => edit((d) => void (d.song.tracks[index] && (d.song.tracks[index].gain_db = gain)))} />
      <Meter id={`meter-l-${index}`} live={`meter-${index}`} w={4} h={METER} color={color} />
      <Meter id={`meter-r-${index}`} live={`meter-${index}`} w={4} h={METER} color={color} />
    </row>
    <text style="xs" color="text-muted">{`${db.toFixed(1)} dB`}</text>
  </col>
);
