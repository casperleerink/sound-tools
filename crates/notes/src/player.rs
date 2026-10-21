//! Plays notes: the notes on the timeline, from one immutable snapshot of them, and patterns a
//! performance launches, from one immutable set of them. The control side makes both; the
//! processor sends their notes from the transport tick range on the audio thread.
//!
//! The rule that shapes this file: a note that started always gets its off. The processor keeps
//! a fixed list of the notes it started, with the tick of their off. Offs come from that list
//! and never from the snapshot, so they arrive also when the clip changed, moved or went away
//! while the note sounded, or another pattern was launched. The pedal and the lanes work the
//! same way: the processor keeps what it sent and sends what the snapshot wants when that is
//! something else.

use std::ops::Range;
use std::sync::Arc;

use sound_core::{
    EventOutput, EventOutputs, Ports, PrepareConfig, ProcessContext, Processor, Ticks, Transport,
    Watch,
};

use crate::{
    Expression, Launch, NoteEvent, NoteTimeline, Pattern, Pedal, Pitch, Placing, Velocity,
};

/// How long a preview note sounds. Its off comes from the processor after this time, so no
/// interface can leave one sounding.
pub const PREVIEW_SECONDS: f32 = 0.3;

/// How many notes one player can hold at a time. A note that would be one more is not played
/// and counts in `EngineStatus::event_overflows`. The synth has 16 voices.
pub const HELD_CAPACITY: usize = 128;

/// What [`PlayerWatches`] show for no pattern.
pub const NO_PATTERN: f32 = -1.0;

/// What [`PlayerWatches::queued`] shows for a stop that has not landed yet.
pub const QUEUED_STOP: f32 = -2.0;

/// What the control side sends to a [`NotePlayer`].
pub enum NotePlayerUpdate {
    /// The notes on the timeline, from the behaviour on every change.
    Timeline(Arc<NoteTimeline>),
    /// The patterns a [`Launch`] picks from by index. A pattern that plays goes on at its
    /// phase, and its held notes follow the new set as they follow a new timeline.
    Patterns(Arc<Vec<Pattern>>),
    /// Takes the place of a launch that has not landed yet.
    Launch(Launch),
    /// Puts a pattern on the song from the next block on: the notes it holds end there.
    Place(Placing),
    /// A note from an interface that sounds now, for [`PREVIEW_SECONDS`], also while the
    /// project does not play. A new one ends the one before it.
    Preview { pitch: Pitch, velocity: Velocity },
}

/// Where a [`NotePlayer`] shows what it plays, for a view or a page. A pattern shows as its
/// index, and none as [`NO_PATTERN`]. Set every block.
#[derive(Clone, Debug, Default)]
pub struct PlayerWatches {
    /// The launched pattern.
    pub playing: Watch,
    /// The pattern of a launch that has not landed yet, or [`QUEUED_STOP`].
    pub queued: Watch,
    /// How far the launched pattern is into its round, from 0 up to 1. 0 when none plays.
    pub phase: Watch,
}

/// The preview note that sounds, and for how many more frames.
#[derive(Copy, Clone)]
struct Previewed {
    pitch: Pitch,
    frames_left: usize,
}

/// A note this processor started and has not ended yet.
#[derive(Copy, Clone)]
struct Held {
    start: Ticks,
    end: Ticks,
    pitch: Pitch,
    from: Source,
}

/// Where a held note came from, which is what it follows on a swap.
#[derive(Copy, Clone)]
enum Source {
    Timeline,
    /// The note that starts at `start` in the pattern of index `pattern`.
    Pattern {
        pattern: usize,
        start: Ticks,
    },
    /// A pattern note that a launch ended and whose off did not fit yet: it follows nothing.
    Cut,
}

/// The pattern that plays and the tick it was launched on.
#[derive(Copy, Clone)]
struct Launched {
    pattern: usize,
    anchor: Ticks,
    /// Placed on the song: the anchor stays through seeks, and nothing plays before it.
    placed: bool,
    /// One round from the anchor, and nothing before it.
    once: bool,
}

impl Launched {
    /// Whether the note that starts at `start` of the song plays, in a round of `length`.
    fn plays_at(&self, start: Ticks, length: Ticks) -> bool {
        let from_anchor = self.placed || self.once;
        (!from_anchor || start >= self.anchor) && (!self.once || start < self.anchor + length)
    }

    /// Whether a note of a pattern of `length` may start or end in `range`.
    fn plays_in(&self, range: &Range<Ticks>, length: Ticks) -> bool {
        if !self.placed && !self.once {
            return true;
        }
        let end = match self.once {
            true => self.anchor + length,
            false => Ticks(u64::MAX),
        };
        range.start <= end && self.anchor < range.end
    }

    /// After ranges up to `end`, the tick before which no note of a pattern of `length` plays,
    /// see [`Self::plays_in`]: `Ticks::MAX` when its round is behind. `None` while it plays.
    fn quiet_until(&self, end: Ticks, length: Ticks) -> Option<Ticks> {
        if !self.placed && !self.once {
            return None;
        }
        let stop = match self.once {
            true => self.anchor + length,
            false => Ticks::MAX,
        };
        if end > stop {
            Some(Ticks::MAX)
        } else if self.anchor >= end {
            Some(self.anchor)
        } else {
            None
        }
    }

    /// Whether it sounds at `heard`, for the watches.
    fn sounds_at(&self, heard: Ticks, length: Ticks) -> bool {
        let started = !self.placed || heard >= self.anchor;
        started && (!self.once || heard < self.anchor + length)
    }
}

pub struct NotePlayer {
    timeline: Arc<NoteTimeline>,
    /// A new timeline came since the last block.
    timeline_swapped: bool,
    patterns: Arc<Vec<Pattern>>,
    patterns_swapped: bool,
    launched: Option<Launched>,
    /// A launch that has not landed yet.
    queued: Option<Launch>,
    /// A placing that came since the last block.
    placing: Option<Placing>,
    watches: Option<PlayerWatches>,
    /// Never grows past [`HELD_CAPACITY`], so it never allocates on the audio thread.
    held: Vec<Held>,
    /// A preview that came since the last block.
    preview_wanted: Option<(Pitch, Velocity)>,
    previewed: Option<Previewed>,
    preview_frames: usize,
    /// Where this player last put the pedal of its instrument. Every block compares it with
    /// the snapshot, so a seek into a held pedal, and an edit that removes one, both arrive
    /// with no case of their own.
    sent_pedal: Pedal,
    /// Where this player last put the wheels and the pressure, compared with the lanes of
    /// the snapshot every block as the pedal is.
    sent_expression: Expression,
    /// See [`Processor::rests_until`].
    rests_until: Option<Ticks>,
}

impl NotePlayer {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);

    /// A player that shows what it plays in `watches`.
    pub fn watched(watches: PlayerWatches) -> Self {
        let mut player = Self::default();
        player.show(Some(watches));
        player
    }

    /// Shows what it plays in `watches` from now on, or nothing: a player that shows nothing
    /// can rest.
    pub fn show(&mut self, watches: Option<PlayerWatches>) {
        if let Some(watches) = &watches {
            watches.playing.set(NO_PATTERN);
            watches.queued.set(NO_PATTERN);
            watches.phase.set(0.0);
        }
        self.watches = watches;
    }
}

impl Default for NotePlayer {
    fn default() -> Self {
        Self {
            timeline: Arc::default(),
            timeline_swapped: false,
            patterns: Arc::default(),
            patterns_swapped: false,
            launched: None,
            queued: None,
            placing: None,
            watches: None,
            held: Vec::with_capacity(HELD_CAPACITY),
            preview_wanted: None,
            previewed: None,
            preview_frames: 0,
            sent_pedal: Pedal::UP,
            sent_expression: Expression::REST,
            rests_until: None,
        }
    }
}

impl NotePlayer {
    /// Whether a block over `transport` would send nothing and change nothing: no note held or
    /// due, no edit, launch or preview to take in, the pedal up and the lanes at rest. A song of
    /// many timelines then pays almost nothing for those that are not playing.
    fn idle(&self, transport: &Transport<'_>) -> bool {
        let range = &transport.tick_range;
        let pattern_plays = self.launched.is_some_and(|launched| {
            let length = self.patterns.get(launched.pattern);
            length.is_some_and(|pattern| launched.plays_in(range, pattern.length().ticks()))
        });
        !transport.jumped
            && !transport.stopped_playing
            && self.quiet()
            && !pattern_plays
            && !self.timeline.plays_in(range)
    }

    /// Nothing held, nothing to take in, nothing shown, the pedal up and the lanes at rest.
    fn quiet(&self) -> bool {
        self.held.is_empty()
            && self.placing.is_none()
            && self.queued.is_none()
            && !self.timeline_swapped
            && !self.patterns_swapped
            && self.preview_wanted.is_none()
            && self.previewed.is_none()
            && self.watches.is_none()
            && self.sent_pedal == Pedal::UP
            && self.sent_expression == Expression::REST
    }

    /// After a block whose range ended at `end`, the tick before which it stays idle.
    fn quiet_until(&self, end: Ticks) -> Option<Ticks> {
        if !self.quiet() {
            return None;
        }
        let pattern = match self.launched {
            Some(launched) => match self.patterns.get(launched.pattern) {
                Some(pattern) => launched.quiet_until(end, pattern.length().ticks())?,
                None => Ticks::MAX,
            },
            None => Ticks::MAX,
        };
        Some(self.timeline.quiet_until(end)?.min(pattern))
    }
}

impl Processor for NotePlayer {
    type Update = NotePlayerUpdate;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.preview_frames = (config.sample_rate as f32 * PREVIEW_SECONDS) as usize;
    }

    fn update(&mut self, update: &mut NotePlayerUpdate) {
        // An old snapshot rides back to the control thread inside the update.
        match update {
            NotePlayerUpdate::Timeline(timeline) => {
                std::mem::swap(&mut self.timeline, timeline);
                self.timeline_swapped = true;
            }
            NotePlayerUpdate::Patterns(patterns) => {
                std::mem::swap(&mut self.patterns, patterns);
                self.patterns_swapped = true;
            }
            NotePlayerUpdate::Launch(launch) => self.queued = Some(*launch),
            NotePlayerUpdate::Place(placing) => self.placing = Some(*placing),
            NotePlayerUpdate::Preview { pitch, velocity } => {
                self.preview_wanted = Some((*pitch, *velocity));
            }
        }
    }

    fn rests_until(&self) -> Option<Ticks> {
        self.rests_until
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if !self.idle(&context.transport) {
            self.play_block(context);
        }
        self.rests_until = self.quiet_until(context.transport.tick_range.end);
    }
}

impl NotePlayer {
    fn play_block(&mut self, context: &mut ProcessContext<'_>) {
        let ProcessContext {
            transport,
            event_outputs,
            ..
        } = context;
        let range = transport.tick_range.clone();
        let Self {
            timeline,
            timeline_swapped,
            patterns,
            patterns_swapped,
            launched,
            queued,
            placing,
            watches,
            held,
            preview_wanted,
            previewed,
            preview_frames,
            sent_pedal,
            sent_expression,
            rests_until: _,
        } = self;
        let mut sender = Sender {
            event_outputs,
            full: false,
        };

        if transport.jumped || transport.stopped_playing {
            // After this block the range is empty or somewhere else, so no off of the list
            // would come. The buffer is still empty here, so the event fits.
            sender.send(0, NoteEvent::AllOff);
            held.clear();
            *previewed = None;
            // `AllOff` puts the pedal of the instrument up and its lanes at rest as well.
            *sent_pedal = Pedal::UP;
            *sent_expression = Expression::REST;
            // A seek while stopped moves where play starts, so a launch there moves with it. A
            // pause keeps the anchor, so play from the same spot keeps the phase.
            if transport.jumped
                && !transport.playing
                && let Some(launched) = launched.as_mut().filter(|launched| !launched.placed)
            {
                launched.anchor = transport.heard_tick;
            }
        }
        // The pedal, before the notes of this block: an off at the first tick of the block must
        // see the pedal the clips ask for. Only while playing, because the pedal of a stopped
        // project would hold what a keyboard plays into the same instrument.
        if transport.playing {
            let wanted = timeline.pedal_before(range.start);
            if wanted != *sent_pedal && sender.send(0, NoteEvent::Pedal(wanted)) {
                *sent_pedal = wanted;
            }
            for change in timeline.pedal_starting_in(&range) {
                let Some(offset) = transport.offset_of(change.start) else {
                    continue;
                };
                if sender.send(offset, NoteEvent::Pedal(change.value)) {
                    *sent_pedal = change.value;
                }
            }
        }

        // The lanes, once per block and only what moved, at its start and before its notes. The
        // value is the one at the last tick of the block, so a note that starts in the block
        // starts where the lanes are going, also at the start of a clip. A seek, a play from
        // the middle of a clip and the end of a clip need no case of their own, as for the
        // pedal. Only while playing, for the same reason as the pedal.
        if transport.playing && range.start < range.end {
            let wanted = timeline.expression_at(Ticks(range.end.0 - 1));
            for event in sent_expression.moves_to(wanted) {
                if sender.send(0, event) {
                    sent_expression.follow(event);
                }
            }
        }

        // The preview after the pedal and the lanes, so it starts where they stand too.
        preview(
            preview_wanted,
            previewed,
            *preview_frames,
            context.frames,
            &mut sender,
        );

        // A held note follows the new snapshot: it takes its new end, or it ends now when its
        // note is gone or moved. A note the edit did not touch is found with the same end, so
        // nothing is sent for it: a swap neither cuts nor starts it again.
        let timeline_swapped = std::mem::take(timeline_swapped);
        let patterns_swapped = std::mem::take(patterns_swapped);
        if timeline_swapped || patterns_swapped {
            let mut index = 0;
            while let Some(note) = held.get_mut(index) {
                let end = match note.from {
                    Source::Timeline if timeline_swapped => timeline.end_of(note.start, note.pitch),
                    Source::Pattern { pattern, start } if patterns_swapped => patterns
                        .get(pattern)
                        .and_then(|pattern| pattern.end_of(start, note.pitch))
                        .map(|end| note.start + end.saturating_sub(start)),
                    _ => {
                        index += 1;
                        continue;
                    }
                };
                match end {
                    Some(end) if end > range.start => {
                        note.end = end;
                        index += 1;
                    }
                    // Removed: the note that took its place at `index` is looked at next.
                    _ if end_note(held, index, 0, &mut sender) => {}
                    _ => index += 1,
                }
            }
        }

        if let Some(placing) = placing.take() {
            cut_patterns(range.start, held, transport, &mut sender);
            *launched = placing.pattern.map(|pattern| Launched {
                pattern,
                anchor: placing.at,
                placed: true,
                once: placing.once,
            });
            *queued = None;
        }

        // A launch lands on its point inside this block. While the project does not play it
        // lands at once, anchored where play will start: the tick the device stands on, not
        // the range, which is ahead of it by the latency after this player.
        let landing = queued.and_then(|launch| {
            if !transport.playing {
                return Some((launch, transport.heard_tick));
            }
            let time_signatures = transport.clock.tempo_map().time_signatures();
            let at = (launch.quantize).point_from(range.start.max(launch.after), time_signatures);
            (at < range.end).then_some((launch, at))
        });
        let playing = |launched: Option<Launched>| {
            launched.and_then(|launched| Some((patterns.get(launched.pattern)?, launched)))
        };
        let before = landing.map_or(range.end, |(_, at)| at);
        let pattern = playing(*launched);
        play(
            range.start..before,
            timeline,
            pattern,
            held,
            transport,
            &mut sender,
        );
        if let Some((launch, at)) = landing {
            cut_patterns(at, held, transport, &mut sender);
            *launched = launch.pattern.map(|pattern| Launched {
                pattern,
                anchor: at,
                placed: false,
                once: launch.once,
            });
            *queued = None;
            let pattern = playing(*launched);
            play(
                at..range.end,
                timeline,
                pattern,
                held,
                transport,
                &mut sender,
            );
        }

        if let Some(watches) = watches {
            let shown = |pattern: Option<usize>| pattern.map_or(NO_PATTERN, |index| index as f32);
            // A launch of a pattern the set does not have shows, so a view sees what it asked.
            let sounding = launched.filter(|launched| {
                let length = patterns
                    .get(launched.pattern)
                    .map(|pattern| pattern.length());
                length.is_none_or(|length| launched.sounds_at(transport.heard_tick, length.ticks()))
            });
            watches
                .playing
                .set(shown(sounding.map(|launched| launched.pattern)));
            watches.queued.set(match queued {
                Some(Launch { pattern: None, .. }) => QUEUED_STOP,
                launch => shown(launch.and_then(|launch| launch.pattern)),
            });
            let phase = playing(sounding).map(|(pattern, launched)| {
                // Before the anchor, as a launch is while the latency after the player passes.
                let heard = transport.heard_tick.max(launched.anchor);
                let phase = pattern.phase(launched.anchor, heard);
                (phase.0 as f64 / pattern.length().ticks().0 as f64) as f32
            });
            watches.phase.set(phase.unwrap_or(0.0));
        }
    }
}

/// Starts the notes of the timeline and of the launched pattern that start in `ticks`, and
/// ends the held notes that end before its end.
fn play(
    ticks: Range<Ticks>,
    timeline: &NoteTimeline,
    pattern: Option<(&Pattern, Launched)>,
    held: &mut Vec<Held>,
    transport: &Transport<'_>,
    sender: &mut Sender<'_, '_>,
) {
    let from_timeline = timeline.starting_in(&ticks).iter();
    let mut from_timeline = from_timeline
        .map(|note| (*note, Source::Timeline))
        .peekable();
    let from_pattern = pattern.into_iter().flat_map(|(pattern, launched)| {
        let length = pattern.length().ticks();
        let notes = pattern.starting_in(launched.anchor, ticks.clone());
        let notes = notes.filter(move |(note, _)| launched.plays_at(note.start, length));
        notes.map(move |(note, start)| {
            let pattern = launched.pattern;
            (note, Source::Pattern { pattern, start })
        })
    });
    let mut from_pattern = from_pattern.peekable();
    // In time order, and on one tick the offs before the ons. Else the end of one note would
    // release the next note of the same pitch that starts there.
    loop {
        let pattern_first = match (from_timeline.peek(), from_pattern.peek()) {
            (Some((timeline, _)), Some((pattern, _))) => pattern.start < timeline.start,
            (first, _) => first.is_none(),
        };
        let next = match pattern_first {
            true => from_pattern.next(),
            false => from_timeline.next(),
        };
        let Some((note, from)) = next else {
            break;
        };
        release_before(held, note.start + Ticks(1), transport, sender);
        let Some(offset) = transport.offset_of(note.start) else {
            continue;
        };
        if held.len() >= HELD_CAPACITY {
            sender.event_outputs.count_dropped();
            continue;
        }
        let on = NoteEvent::On {
            pitch: note.pitch,
            velocity: note.velocity,
        };
        // An on that does not fit is not played, so it is not held either.
        if sender.send_or_drop(offset, on) {
            held.push(Held {
                start: note.start,
                end: note.end,
                pitch: note.pitch,
                from,
            });
        }
    }
    release_before(held, ticks.end, transport, sender);
}

/// Ends every held note of a pattern at `at`, where a launch lands.
fn cut_patterns(
    at: Ticks,
    held: &mut Vec<Held>,
    transport: &Transport<'_>,
    sender: &mut Sender<'_, '_>,
) {
    for note in held.iter_mut() {
        if let Source::Pattern { .. } = note.from {
            note.end = note.end.min(at);
            note.from = Source::Cut;
        }
    }
    release_before(held, at + Ticks(1), transport, sender);
}

/// The preview note, in engine time, so it sounds while the project does not play. It goes out
/// at the start of the block, before the notes of the timeline.
fn preview(
    wanted: &mut Option<(Pitch, Velocity)>,
    previewed: &mut Option<Previewed>,
    preview_frames: usize,
    frames: usize,
    sender: &mut Sender<'_, '_>,
) {
    let end = |previewed: &mut Option<Previewed>, sender: &mut Sender<'_, '_>| {
        let Some(note) = *previewed else {
            return true;
        };
        let ended = sender.send(0, NoteEvent::Off { pitch: note.pitch });
        if ended {
            *previewed = None;
        }
        ended
    };
    if let Some((pitch, velocity)) = *wanted {
        // An off that does not fit waits for the next block, and the new note with it.
        if !end(previewed, sender) {
            return;
        }
        *wanted = None;
        if sender.send_or_drop(0, NoteEvent::On { pitch, velocity }) {
            *previewed = Some(Previewed {
                pitch,
                frames_left: preview_frames,
            });
        }
    } else if let Some(note) = previewed {
        note.frames_left = note.frames_left.saturating_sub(frames);
        if note.frames_left == 0 {
            end(previewed, sender);
        }
    }
}

/// The event output for one block. Once an event did not fit, the buffer is full for the rest
/// of the block and nothing more is pushed. So the engine counts that one event, and not one
/// more for every later try.
struct Sender<'a, 'b> {
    event_outputs: &'a mut EventOutputs<'b>,
    full: bool,
}

impl Sender<'_, '_> {
    /// For an event that is sent again later when it does not fit: an off.
    fn send(&mut self, offset: usize, event: NoteEvent) -> bool {
        if !self.full {
            self.full = !self.event_outputs.push(NotePlayer::NOTES, offset, event);
        }
        !self.full
    }

    /// For an event that is lost when it does not fit: an on. Each lost one counts once, by
    /// the buffer when it refused the push, else here.
    fn send_or_drop(&mut self, offset: usize, event: NoteEvent) -> bool {
        let refused_before = self.full;
        let sent = self.send(offset, event);
        if refused_before {
            self.event_outputs.count_dropped();
        }
        sent
    }
}

/// Ends the held note at `index` and takes it off the list. Returns false when the off did not
/// fit: the note stays listed and ends in a later block.
fn end_note(
    held: &mut Vec<Held>,
    index: usize,
    offset: usize,
    sender: &mut Sender<'_, '_>,
) -> bool {
    let Some(note) = held.get(index).copied() else {
        return false;
    };
    if sender.send(offset, NoteEvent::Off { pitch: note.pitch }) {
        held.swap_remove(index);
        return true;
    }
    false
}

/// Ends every held note that ends before `limit`, the earliest end first.
fn release_before(
    held: &mut Vec<Held>,
    limit: Ticks,
    transport: &Transport<'_>,
    sender: &mut Sender<'_, '_>,
) {
    loop {
        let due = held.iter().enumerate().filter(|(_, note)| note.end < limit);
        let Some((index, note)) = due.min_by_key(|(_, note)| note.end) else {
            return;
        };
        // An end before this block is an off that did not fit earlier.
        let offset = transport.offset_of(note.end).unwrap_or(0);
        if !end_note(held, index, offset, sender) {
            return;
        }
    }
}
