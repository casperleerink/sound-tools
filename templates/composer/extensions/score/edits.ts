// What the score does to the piece where the composer points at it: bars added and taken out,
// tempo marks, rit. and accel., fermatas and time signatures typed on the music, and an
// instrument's notes opened. Each is one undo step, the tempo map and the records together.
import { tempoSteps, type TempoMap } from "../sdk";
import { addClip, choose } from "../song/clips";
import { changeBars, FASTEST, SLOWEST, withoutSignature, withSignature } from "../song/marks";
import { isRow, type Song } from "../song/model";
import { barAt, tickOfBar } from "../song/time";
import { PLANE } from "../song/view";
import { AFTER, moveMusic, pieceEnd, signatureOf, ticksOfSignature, withBars, type Item } from "./bars";

type Changes = TempoMap["tempo_changes"];
/** A tempo map as the clock gives it, to read. */
type Read = Parameters<typeof tempoSteps>[0];

/** How much slower a new rit. ends than it starts: three quarters, as scores mostly mean it. */
const RIT = 0.75;
/** How long a new rit. is when nothing after it ends it, in bars. */
const RIT_BARS = 2;
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
const playingAt = (map: Read, tick: number) => bpmOf(tempoSteps({ ...map, fermatas: [] }).findLast((step) => step.tick <= tick)?.bpm ?? 120);

/** The changes without one that adds nothing: after tick 0, the tempo that already plays, with no move to start or end. */
const needed = (changes: Changes): Changes =>
  changes.filter((change, index) => {
    const before = changes[index - 1];
    return !before || change.to !== undefined || before.to !== undefined || before.bpm !== change.bpm;
  });

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
  const end = () => pieceEnd(song.children.filter(isRow), runs());
  const changesOf = (label: string, change: (changes: Changes) => Changes) =>
    clock.update(label, (piece) => (piece.tempo_map.tempo_changes = change(piece.tempo_map.tempo_changes.map((each) => ({ ...each })))));
  /** The tick of the beat a fermata at the end of bar `bar` holds: its last. */
  const lastBeat = (bar: number) => {
    const { end: next } = barAt(runs(), tickOfBar(runs(), bar));
    const [, unit] = signatureOf(runs(), bar).split("/").map(Number);
    return next - 3840 / (unit ?? 4);
  };
  /** Adds `count` bars before `bar`, or takes `-count` out from it, with all after them: one undo step, the whole piece shown again. */
  const bars = (label: string, bar: number, count: number) => {
    const from = tickOfBar(runs(), bar);
    const by = count * ticksOfSignature(signatureOf(runs(), bar));
    clock.update(label, (piece) => Object.assign(piece, withBars(piece, bar, count)));
    song.update(label, ({ children }) => moveMusic(children.filter(isRow) as Item[], from, by));
    song.show(PLANE, { x: [0, end() + AFTER] });
  };
  return {
    /** Where the piece ends: the final bar line. */
    end,
    /** `count` bars of the time signature there added before bar `bar`; all after them moves later. */
    addBars: (bar: number, count = 1) => bars(count === 1 ? "Add a bar" : "Add bars", bar, count),
    /** Bar `bar` taken out with what is in it; all after it moves earlier. The last bar stays. */
    removeBar(bar: number): boolean {
      if (tickOfBar(runs(), 2) >= end()) return false;
      bars("Remove a bar", bar, -1);
      return true;
    },
    /** The tempo at the start of bar `bar` typed where it stands: a change there, playing the tempo that plays, until it is typed. */
    addTempo(bar: number) {
      const tick = tickOfBar(runs(), bar);
      if (!clock.tempo_map.tempo_changes.some((change) => change.tick === tick)) {
        const bpm = playingAt(clock.tempo_map, tick);
        changesOf("Add a tempo mark", (changes) => [...changes, { tick, bpm }].sort((a, b) => a.tick - b.tick));
      }
      memory.typing = { kind: "tempo", tick };
    },
    /** A rit. from bar `bar` to three quarters of the tempo there, ending at the next change, or two bars on; where it goes is then typed. */
    addGlide(bar: number) {
      const tick = tickOfBar(runs(), bar);
      const bpm = playingAt(clock.tempo_map, tick);
      const to = bpmOf(bpm * RIT);
      changesOf("Add a rit.", (changes) => {
        const at = changes.find((change) => change.tick === tick);
        const all = at ? changes.map((change) => (change === at ? { ...change, to } : change)) : [...changes, { tick, bpm, to }];
        if (!all.some((change) => change.tick > tick)) all.push({ tick: tickOfBar(runs(), bar + RIT_BARS), bpm: to });
        return all.sort((a, b) => a.tick - b.tick);
      });
      memory.typing = { kind: "target", tick };
    },
    /**
     * What was typed on the mark being typed: a tempo, where a rit. or an accel. goes, or a time
     * signature. Nothing typed takes the mark out; words the clock does not take leave it as it was.
     */
    typed(text: string | null) {
      const typing = memory.typing;
      memory.typing = null;
      if (!typing) return;
      if (typing.kind === "signature") return this.signature(typing.bar, text);
      const number = text === null ? NaN : Number(text.replace(",", "."));
      const { tick } = typing;
      if (typing.kind === "tempo") {
        if (text !== null && text.trim() === "" && tick > 0) return changesOf("Remove a tempo mark", (changes) => changes.filter((change) => change.tick !== tick));
        const bpm = Number.isFinite(number) ? bpmOf(number) : null;
        changesOf("Change the tempo", (changes) => needed(changes.map((change) => (change.tick === tick && bpm !== null ? { ...change, bpm } : change))));
        return;
      }
      if (text !== null && text.trim() === "") return this.removeGlide(tick);
      if (!Number.isFinite(number)) return;
      const to = bpmOf(number);
      changesOf("Change where the tempo goes", (changes) => {
        const index = changes.findIndex((change) => change.tick === tick);
        const [at, next] = [changes[index], changes[index + 1]];
        if (!at) return changes;
        // The change that held where it went holds where it goes now.
        if (next && next.to === undefined && next.bpm === at.to) next.bpm = to;
        if (to === at.bpm) delete at.to;
        else at.to = to;
        return needed(changes);
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
    /** Bar `bar` and those after it, to the next change, in the time signature `text` says; nothing typed takes the change out. Notes stay where they are. */
    signature(bar: number, text: string | null) {
      if (text === null) return;
      const signature = signatureFrom(text);
      const firsts = changeBars(runs());
      if (text.trim() === "" && bar > 1 && firsts.includes(bar)) {
        clock.update("Remove a time signature change", (piece) => (piece.tempo_map.time_signatures = withoutSignature(piece.tempo_map.time_signatures, bar)));
        return;
      }
      if (signature === null || signature === signatureOf(runs(), bar)) return;
      clock.update("Change the time signature", (piece) => {
        let all = withSignature(piece.tempo_map.time_signatures, bar);
        all = all.map((run, index) => (changeBars(all)[index] === bar ? { ...run, signature } : run));
        // A change to the time signature of the bars around it is no change.
        for (const each of [bar, ...changeBars(all).filter((first) => first > bar).slice(0, 1)]) {
          const index = changeBars(all).indexOf(each);
          if (index > 0 && all[index - 1]?.signature === all[index]?.signature) all = withoutSignature(all, each);
        }
        piece.tempo_map.time_signatures = all;
      });
    },
    /** The notes of `track` at `tick`, open in the editor below: of the clip there, or of a new one from that bar to the end of the piece. */
    openMusic(track: string, tick: number) {
      const owner = song.children.find((child) => child.name === track);
      if (owner?.tool !== "track") return;
      const clips = owner.children.flatMap((child) => (child.tool === "timeline" ? [child] : []));
      const here = clips.find((clip) => clip.state.start <= tick && tick < clip.state.start + (clip.state.length ?? Infinity));
      let key = here ? `${track}/${here.name}` : undefined;
      if (!key) {
        const start = clips.length === 0 ? 0 : barAt(runs(), tick).start;
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
