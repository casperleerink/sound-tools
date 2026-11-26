// What the score does to the piece where the composer points at it: bars chosen, added, taken
// out and cleared, tempo marks, rit. and accel., fermatas and time signatures typed on the music,
// and an instrument's notes opened. Each is one undo step, the tempo map and the records together.
import { tempoSteps, type Frozen, type TempoMap } from "../sdk";
import { freeName } from "../track";
import { addClip, choose, lengthOf } from "../song/clips";
import { FASTEST, SLOWEST } from "../song/marks";
import { isRow, type Memory, type Song } from "../song/model";
import { barAt, tickOfBar } from "../song/time";
import { beatOf, clearNotes, endLine, inSignature, moveMusic, pieceEnd, placesOf, retimed, signatureOf, ticksOfSignature, withBars } from "./bars";

type Changes = TempoMap["tempo_changes"];
type Read = Frozen<TempoMap>;
/** A mark being typed. */
export type Typing = NonNullable<Memory["typing"]>;

/** Where a new rit. is first offered to go: three quarters of the tempo, as scores mostly mean it. */
export const RIT = 0.75;
/** How many times as long a new fermata holds its beat, and the choices its menu offers. */
export const [HOLD, HOLDS] = [2, [1.5, 2, 3]] as const;

/** A tempo as the score says it, to a tenth. */
export const bpmOf = (bpm: number) => Math.round(Math.min(Math.max(bpm, SLOWEST), FASTEST) * 10) / 10;

/** What a tempo change says on the score, and what it moves to by the next change, if it moves. */
export type TempoMark = { tick: number; bpm: number; shows: boolean; glide: { to: number; end: number } | null };

/** The tempo marks: a change shows its tempo where it differs from what played before it. */
export function tempoMarks(changes: Read["tempo_changes"]): TempoMark[] {
  return changes.map((change, index) => {
    const [before, next] = [changes[index - 1], changes[index + 1]];
    const playing = before ? (before.to ?? before.bpm) : null;
    const glide = change.to !== undefined && next ? { to: change.to, end: next.tick } : null;
    return { tick: change.tick, bpm: change.bpm, shows: playing !== change.bpm, glide };
  });
}

/** The tempo that plays at `tick`, a rit. or an accel. as far as it got, a fermata left out. */
export const playingAt = (map: Read, tick: number) => bpmOf(tempoSteps({ ...map, fermatas: [] }).findLast((step) => step.tick <= tick)?.bpm ?? 120);

/** The changes without one that adds nothing: after tick 0, the tempo that already plays, with no move to start or end; the last moves to nothing. */
function needed(changes: Changes): Changes {
  const kept = changes.filter((change, index) => {
    const before = changes[index - 1];
    return !before || change.to !== undefined || before.to !== undefined || before.bpm !== change.bpm;
  });
  return kept.map((change, index) => (index === kept.length - 1 && change.to !== undefined ? { tick: change.tick, bpm: change.bpm } : change));
}

/** `changes` with `change` added: a rit. or an accel. it falls inside ends there, at the tempo `change` starts with. */
function inserted(changes: Changes, change: Changes[number]): Changes {
  const before = changes.findLast((each) => each.tick < change.tick);
  const ended = changes.map((each) => (each === before && each.to !== undefined ? { ...each, to: change.bpm } : each));
  return [...ended, change].sort((a, b) => a.tick - b.tick);
}

/** Whether two marks being typed are the same mark. */
const sameTyping = (one: Typing | null, other: Typing) =>
  one !== null && (one.kind === "signature" ? other.kind === "signature" && one.bar === other.bar : other.kind === one.kind && one.tick === other.tick);

/** The chosen bars of the piece, first and last, from 1. */
export function chosenBars({ memory: { bars }, children, clock: { tempo_map } }: Pick<Song, "memory" | "children" | "clock">) {
  if (!bars) return null;
  const final = barAt(tempo_map.time_signatures, pieceEnd(children, tempo_map) - 1).bar;
  const first = Math.min(bars.from, bars.to);
  return first > final ? null : { first, last: Math.min(Math.max(bars.from, bars.to), final) };
}

/** A time signature as typed, `"3/4"`, when the clock takes it. */
export function signatureFrom(text: string): string | null {
  const match = /^\s*(\d+)\s*\/\s*(\d+)\s*$/.exec(text);
  if (!match) return null;
  const [count, unit] = [Number(match[1]), Number(match[2])];
  return count >= 1 && count <= 32 && [1, 2, 4, 8, 16, 32].includes(unit) ? `${count}/${unit}` : null;
}

export const scoreEdits = (song: Song) => {
  const { clock, memory } = song;
  const runs = () => clock.tempo_map.time_signatures;
  const end = () => pieceEnd(song.children, clock.tempo_map);
  const changesOf = (label: string, change: (changes: Changes) => Changes) =>
    clock.update(label, (piece) => (piece.tempo_map.tempo_changes = change(piece.tempo_map.tempo_changes.map((each) => ({ ...each })))));
  /** The tick of the beat a fermata at the end of bar `bar` holds: its last. */
  const lastBeat = (bar: number) => tickOfBar(runs(), bar + 1) - beatOf(runs(), bar);
  /**
   * Adds `count` bars of the time signature of `bar` before it, or after it when `after`, or takes
   * `-count` out from it, with all after them: one undo step.
   */
  const bars = (label: string, bar: number, count: number, after = false) => {
    const from = tickOfBar(runs(), after ? bar + 1 : bar);
    const by = count * ticksOfSignature(signatureOf(runs(), bar));
    const last = end();
    const piece = withBars(clock, bar, count, after);
    // How long the audio plays, which what an update gets of a region does not say.
    const audio = placesOf(song.children, clock.tempo_map, ["region"]);
    clock.update(label, (draft) => Object.assign(draft, piece));
    song.update(label, ({ children }) => {
      moveMusic(children, from, by);
      // The end line moves by the bars too: held by an empty clip at the end, else a new one on the first instrument, so no note plays longer.
      const rows = children.filter(isRow);
      const clips = rows.flatMap((row) => row.children.flatMap((child) => (child.tool === "timeline" ? [child.state] : [])));
      const reach = Math.max(0, ...clips.map((clip) => clip.start + lengthOf(clip)));
      const ends = [...clips.map((clip) => clip.start + lengthOf(clip)), ...audio.map(({ start, length }) => (retimed(start, from, by) ?? from) + length)];
      if (ends.length > 0 && endLine(ends, piece.tempo_map.time_signatures) >= last + by) return;
      const empty = clips.find((clip) => clip.start + lengthOf(clip) === reach && clip.notes.length === 0 && [clip.pedal, clip.bend, clip.mod_wheel, clip.pressure].every((points) => !points?.length) && Object.keys(clip.controllers ?? {}).length === 0);
      const first = children.find((child) => child.tool === "track");
      if (empty) empty.length = last + by - empty.start;
      else if (first?.tool === "track") first.children.push({ name: freeName(first.children.map((child) => child.name), "music"), tool: "timeline", state: { start: reach, length: last + by - reach, notes: [] } });
    });
  };
  return {
    /** Where the piece ends: the final bar line. */
    end,
    /** Bar `bar` alone chosen, or with `extend` the bars from where the choice started to it. */
    choose(bar: number, extend = false) {
      memory.bars = { from: extend && memory.bars ? memory.bars.from : bar, to: bar };
      [memory.mark, memory.notes] = [null, []];
    },
    /** Bar `bar` chosen for its menu, unless it is one of the chosen bars. */
    chooseForMenu(bar: number) {
      const chosen = chosenBars(song);
      if (!chosen || bar < chosen.first || bar > chosen.last) this.choose(bar);
    },
    /** The bars from `from` to `to`, either way, chosen. */
    chooseAcross(from: number, to: number) {
      memory.bars = { from, to };
      [memory.mark, memory.notes] = [null, []];
    },
    /** `count` bars of the time signature of bar `bar` added before it, or after it when `after`; all after them moves later, the chosen bars with them. */
    addBars(bar: number, count = 1, after = false) {
      bars(count === 1 ? "Add a bar" : "Add bars", bar, count, after);
      const chosen = chosenBars(song);
      if (chosen && (after ? bar < chosen.first : bar <= chosen.first)) memory.bars = { from: chosen.first + count, to: chosen.last + count };
    },
    /** Bars `first` to `last` taken out with what is in them; all after them moves earlier. A bar stays. */
    removeBars(first: number, last: number): boolean {
      if (first === 1 && tickOfBar(runs(), last + 1) >= end()) return false;
      for (let bar = last; bar >= first; bar--) bars(first === last ? "Remove a bar" : "Remove bars", bar, -1);
      memory.bars = null;
      return true;
    },
    /** The notes in bars `first` to `last` taken out, and one ringing into them cut short; the bars stay. */
    clearNotes(first: number, last: number) {
      const [from, to] = [tickOfBar(runs(), first), tickOfBar(runs(), last + 1)];
      song.update("Clear the notes", ({ children }) => clearNotes(children, from, to));
    },
    /** The tempo at the start of bar `bar`, typed where it stands; nothing changes until it is. */
    addTempo(bar: number) {
      memory.typing = { kind: "tempo", tick: tickOfBar(runs(), bar) };
    },
    /** A rit. or an accel. over bars `first` to `last`, to where it goes, typed where it stands; nothing changes until it is. */
    addGlide(first: number, last = first) {
      memory.typing = { kind: "target", tick: tickOfBar(runs(), first), end: tickOfBar(runs(), last + 1) };
    },
    /**
     * What was typed on the mark `typing` names, as one undo step: a tempo, where a rit. or an
     * accel. goes, or a time signature. Nothing typed takes the mark out; Escape, or words the
     * clock does not take, leave it as it was.
     */
    typed(typing: Typing, text: string | null) {
      if (sameTyping(memory.typing, typing)) memory.typing = null;
      if (text === null) return;
      if (typing.kind === "signature") return this.signature(typing.bar, typing.last, text);
      const { tick } = typing;
      const was = clock.tempo_map.tempo_changes.find((change) => change.tick === tick);
      if (text.trim() === "") {
        if (typing.kind === "target") {
          if (was?.to !== undefined) this.removeGlide(tick);
        } else if (was && tick > 0) changesOf("Remove a tempo mark", (changes) => needed(changes.filter((change) => change.tick !== tick)));
        return;
      }
      const number = Number(text.replace(",", "."));
      if (!Number.isFinite(number)) return;
      const bpm = bpmOf(number);
      if (typing.kind === "tempo") {
        changesOf(was ? "Change the tempo" : "Add a tempo mark", (changes) =>
          needed(was ? changes.map((change) => (change.tick === tick ? { ...change, bpm } : change)) : inserted(changes, { tick, bpm })));
        return;
      }
      const starts = was?.bpm ?? playingAt(clock.tempo_map, tick);
      changesOf(was?.to !== undefined ? "Change where the tempo goes" : bpm < starts ? "Add a rit." : "Add an accel.", (changes) => {
        const all = was ? changes : inserted(changes, { tick, bpm: starts });
        const at = all.find((change) => change.tick === tick)!;
        const next = all.find((change) => change.tick > tick);
        // The change that held where it went holds where it goes now; a new one goes to the end of its bars, or to the next change before it.
        if (next && next.to === undefined && at.to !== undefined && next.bpm === at.to) next.bpm = bpm;
        const stop = typing.end ?? barAt(runs(), tick).end;
        if (!was?.to && (!next || next.tick > stop)) all.push({ tick: stop, bpm });
        if (bpm === at.bpm) delete at.to;
        else at.to = bpm;
        return needed(all.sort((a, b) => a.tick - b.tick));
      });
    },
    /** The rit. or accel. from `tick` taken out: the tempo holds, and a change that only held where it went goes too. */
    removeGlide(tick: number) {
      changesOf("Remove a rit.", (changes) => {
        const index = changes.findIndex((change) => change.tick === tick);
        const [at, next] = [changes[index], changes[index + 1]];
        if (!at?.to) return changes;
        const kept = next && next.to === undefined && next.bpm === at.to ? changes.filter((change) => change !== next) : changes;
        delete at.to;
        return needed(kept);
      });
    },
    /** The fermata at `tick` taken out. */
    removeFermata(tick: number) {
      clock.update("Remove a fermata", (piece) => (piece.tempo_map.fermatas = (piece.tempo_map.fermatas ?? []).filter((each) => each.tick !== tick)));
    },
    /** The fermata over the last beat of bar `bar`: added holding it twice as long, or taken out. */
    toggleFermata(bar: number) {
      const tick = lastBeat(bar);
      const had = (clock.tempo_map.fermatas ?? []).some((each) => each.tick === tick);
      clock.update(had ? "Remove a fermata" : "Add a fermata", (piece) => {
        const rest = (piece.tempo_map.fermatas ?? []).filter((each) => each.tick !== tick);
        piece.tempo_map.fermatas = had ? rest : [...rest, { tick, hold: HOLD }].sort((a, b) => a.tick - b.tick);
      });
    },
    /** How many times as long the fermata at `tick` holds its beat. */
    hold(tick: number, hold: number) {
      clock.update("Change a fermata", (piece) => (piece.tempo_map.fermatas = (piece.tempo_map.fermatas ?? []).map((each) => (each.tick === tick ? { ...each, hold } : each))));
    },
    /**
     * Bars `first` to `last` in the time signature `text` says, or with nothing typed in the one of
     * the bar before them: each bar keeps its notes on their beats and gains silent beats at its
     * end or loses its last ones, everything after moving; the bar after them keeps its own.
     */
    signature(first: number, last: number, text: string | null) {
      if (text === null) return;
      const signature = text.trim() === "" ? (first > 1 ? signatureOf(runs(), first - 1) : null) : signatureFrom(text);
      if (signature === null) return;
      if (Array.from({ length: last - first + 1 }, (_, index) => signatureOf(runs(), first + index)).every((each) => each === signature)) return;
      const { tempo_map, moves } = inSignature(clock.tempo_map, first, last, signature, end());
      const label = "Change the time signature";
      clock.update(label, (piece) => (piece.tempo_map = tempo_map));
      song.update(label, ({ children }) => moves.forEach(([from, by]) => moveMusic(children, from, by)));
    },
    /** The notes of `track` at `tick`, open in the editor below: of the clip there, or of a new one from that bar to the end of the piece. */
    openMusic(track: string, tick: number) {
      const owner = song.children.find((child) => child.name === track);
      if (owner?.tool !== "track" || tick >= end()) return;
      const clips = owner.children.flatMap((child) => (child.tool === "timeline" ? [child] : []));
      const here = clips.find((clip) => clip.state.start <= tick && tick < clip.state.start + lengthOf(clip.state));
      let key = here ? `${track}/${here.name}` : undefined;
      if (!key) {
        // From its bar, or where a clip in it ends, as one does where a bar grew.
        const start = clips.length === 0 ? 0 : Math.max(barAt(runs(), tick).start, ...clips.map((clip) => clip.state.start + lengthOf(clip.state)).filter((each) => each <= tick));
        const later = clips.map((clip) => clip.state.start).filter((each) => each > start);
        key = addClip(song, track, start, Math.min(end(), ...later) - start);
      }
      if (!key) return;
      choose(song, [key]);
      memory.below = { kind: "editor", clip: key };
      memory.notes = [];
    },
  };
};
