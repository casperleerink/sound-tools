//! Both kinds of clip as the timeline edits them, and what a drag does to an audio clip. Pure,
//! no GPUI: the view finds the clip and the pointer, these functions say what the clip becomes.
//!
//! A note clip and an audio clip share selecting, moving, copying, pasting and deleting, so the
//! timeline edits an [`AnyClip`]. What differs is kept here: an audio clip has no length of its
//! own, because it plays at the speed of its file, so where it ends comes from its file and the
//! tempo; and each kind goes on its own kind of track only.
//!
//! An audio clip's edges trim the file, keeping the sound where it is in time: the left edge
//! moves the start and `file_start_seconds` together. Its fades and gain are values of its
//! record, dragged from their handles and turned with their knobs.

use sound_core::{Changes, Clock, InstanceId, Project, Ticks};
use sound_media::{Cached, Info};
use sound_notes::Clip;

use super::layout::shifted;
use crate::{AudioClip, TrackKind};

/// A clip of either kind, by value.
#[derive(Clone, Debug, PartialEq)]
pub enum AnyClip {
    Notes(Clip),
    Audio(AudioClip),
}

impl AnyClip {
    /// The clip the project has at `id`, of either kind.
    pub fn read(project: &Project, id: &InstanceId) -> Option<Self> {
        if let Some(clip) = project.resolve::<Clip>(id) {
            return project.state(&clip).cloned().map(Self::Notes);
        }
        let clip = project.resolve::<AudioClip>(id)?;
        project.state(&clip).cloned().map(Self::Audio)
    }

    pub fn start(&self) -> Ticks {
        match self {
            Self::Notes(clip) => clip.start,
            Self::Audio(clip) => clip.start,
        }
    }

    pub fn with_start(self, start: Ticks) -> Self {
        match self {
            Self::Notes(clip) => Self::Notes(Clip { start, ..clip }),
            Self::Audio(clip) => Self::Audio(AudioClip { start, ..clip }),
        }
    }

    /// The kind of track that plays it.
    pub fn kind(&self) -> TrackKind {
        match self {
            Self::Notes(_) => TrackKind::Instrument,
            Self::Audio(_) => TrackKind::Audio,
        }
    }

    /// Where it ends on the timeline, see [`shown_end`] for an audio clip.
    pub fn end(&self, project: &Project) -> Ticks {
        match self {
            Self::Notes(clip) => clip.end(),
            Self::Audio(clip) => shown_end(project, clip),
        }
    }

    /// Puts it at `id`, a new clip or over the one that is there.
    pub fn write(self, changes: &mut Changes, id: InstanceId) {
        match self {
            Self::Notes(clip) => {
                changes.create(id, clip);
            }
            Self::Audio(clip) => {
                changes.create(id, clip);
            }
        }
    }
}

/// Where an audio clip ends on the timeline. It plays at the speed of its file, so this depends
/// on the file and the tempo. When the file is not there the clip still needs a place to be seen,
/// selected and deleted: as long as its trim says, or one bar when it plays to the end of a file
/// nobody can measure. The same while nothing knows yet what the file is: this never looks at
/// the disk, because the thread that draws calls it, see [`sound_media::cached`].
pub fn shown_end(project: &Project, clip: &AudioClip) -> Ticks {
    let clock = project.clock();
    if let Cached::Plays(file) = sound_media::cached(project.assets(), &clip.asset) {
        return clip.end(Some(&file), clock).max(clip.start + Ticks(1));
    }
    match clip.file_end_seconds {
        Some(end) => {
            let seconds = clock.seconds_of(clip.start) + (end - clip.file_start_seconds);
            clock.tick_at_seconds(seconds).max(clip.start + Ticks(1))
        }
        None => {
            let bar = project
                .project_file()
                .tempo_map
                .time_signature()
                .ticks_per_bar();
            clip.start + Ticks(bar)
        }
    }
}

/// Seconds from the start of the project to a tick, through the clock.
fn seconds(clock: &Clock, tick: Ticks) -> f64 {
    clock.seconds_of(tick)
}

/// The shortest an edge drag makes a clip, in ticks: one `unit`, or what it was when shorter.
fn shortest(length: Ticks, unit: Ticks) -> u64 {
    unit.0.min(length.0)
}

/// The clip with its left edge moved by `delta` ticks. The sound stays where it is in time, so
/// the start of the file that plays moves with the edge. It stops at the start of the file, at
/// tick 0, and one `unit` before the right edge.
pub fn trimmed_left(
    origin: &AudioClip,
    file: &Info,
    clock: &Clock,
    delta: i64,
    unit: Ticks,
) -> AudioClip {
    let end = origin.end(Some(file), clock);
    let length = end.saturating_sub(origin.start);
    // The tick where the file itself starts, which the edge cannot pass.
    let file_start = seconds(clock, origin.start) - origin.file_start_seconds;
    let earliest = clock.tick_at_seconds(file_start.max(0.0));
    let latest = end.saturating_sub(Ticks(shortest(length, unit)));
    let start =
        shifted(origin.start, delta).clamp(earliest.min(origin.start), latest.max(origin.start));
    let moved = seconds(clock, start) - seconds(clock, origin.start);
    let trimmed = AudioClip {
        start,
        file_start_seconds: (origin.file_start_seconds + moved).max(0.0),
        ..origin.clone()
    };
    fitted(trimmed, file)
}

/// The clip with its right edge moved by `delta` ticks: where the file stops playing. It stops
/// at the end of the file, where the end is written as the end of the file again, and one
/// `unit` after the left edge.
pub fn trimmed_right(
    origin: &AudioClip,
    file: &Info,
    clock: &Clock,
    delta: i64,
    unit: Ticks,
) -> AudioClip {
    let end = origin.end(Some(file), clock);
    let length = end.saturating_sub(origin.start);
    let whole = AudioClip {
        file_end_seconds: None,
        ..origin.clone()
    };
    let latest = whole.end(Some(file), clock);
    let earliest = origin.start + Ticks(shortest(length, unit));
    let next = shifted(end, delta).clamp(earliest.min(end), latest.max(end));
    if next >= latest {
        return fitted(whole, file);
    }
    let played = seconds(clock, next) - seconds(clock, origin.start);
    let trimmed = AudioClip {
        file_end_seconds: Some(origin.file_start_seconds + played),
        ..origin.clone()
    };
    fitted(trimmed, file)
}

/// The clip with its fades inside what it plays after a trim, by the rule of the fade limits:
/// the fade in no longer than the clip, the fade out no longer than what the fade in leaves.
pub fn fitted(clip: AudioClip, file: &Info) -> AudioClip {
    let played = played_ms(&clip, file);
    let fade_in_ms = clip.fade_in_ms.min(played);
    let fade_out_ms = clip.fade_out_ms.min(played - fade_in_ms);
    AudioClip {
        fade_in_ms,
        fade_out_ms,
        ..clip
    }
}

/// The shortest part of a file the Start and End of the Clip card leave a clip, in seconds.
pub const SHORTEST_SECONDS: f64 = 0.01;

/// The clip playing its file from `seconds` on, as the start line of the Clip card and its
/// Start knob set it. As the left edge on the timeline, it keeps the sound where it is in time:
/// the clip starts that much later or earlier. It stops at tick 0 and short of the end.
///
/// The clip starts on a tick, so the tick is found first and the start in the file follows
/// from it, as for the left edge: the sound stays exactly in place. Every move of a drag gives
/// `origin`, the clip as it was when the drag began, so nothing adds up over the moves.
pub fn with_file_start(origin: &AudioClip, file: &Info, clock: &Clock, seconds: f64) -> AudioClip {
    let end = origin.file_end_seconds.unwrap_or_else(|| file.seconds());
    // Where the start of the file is on the timeline, which a trim does not move.
    let place = clock.seconds_of(origin.start) - origin.file_start_seconds;
    // The earliest the file can start and still begin at tick 0 or after.
    let earliest = (-place).max(0.0);
    let latest = (end - SHORTEST_SECONDS).max(earliest);
    let seconds = seconds.clamp(earliest, latest);
    let at = |tick: Ticks| clock.seconds_of(tick) - place;
    let mut start = clock.tick_at_seconds(place + seconds);
    // The tick is the first at or after the time, so it may be just past the latest start.
    while start > Ticks(0) && at(start) > latest {
        start = Ticks(start.0 - 1);
    }
    let trimmed = AudioClip {
        start,
        file_start_seconds: at(start).max(0.0),
        ..origin.clone()
    };
    fitted(trimmed, file)
}

/// The clip playing its file up to `seconds`, as the end line and the End knob set it. The end
/// of the file, or within a frame of it, is written as the end of the file.
pub fn with_file_end(clip: &AudioClip, file: &Info, seconds: f64) -> AudioClip {
    let seconds = seconds.max(clip.file_start_seconds + SHORTEST_SECONDS);
    let frame = 1.0 / f64::from(file.sample_rate.max(1));
    let file_end_seconds = (seconds < file.seconds() - frame).then_some(seconds);
    let trimmed = AudioClip {
        file_end_seconds,
        ..clip.clone()
    };
    fitted(trimmed, file)
}

/// How long a clip plays, in milliseconds.
pub fn played_ms(clip: &AudioClip, file: &Info) -> f32 {
    let end = clip.file_end_seconds.unwrap_or_else(|| file.seconds());
    ((end - clip.file_start_seconds).max(0.0) * 1000.0) as f32
}

/// A fade in of `ms` for this clip: from none to what the clip has left after its fade out.
pub fn fade_in(clip: &AudioClip, file: &Info, ms: f32) -> f32 {
    let room = (played_ms(clip, file) - clip.fade_out_ms).max(0.0);
    ms.round().clamp(0.0, room)
}

/// A fade out of `ms` for this clip, the same way.
pub fn fade_out(clip: &AudioClip, file: &Info, ms: f32) -> f32 {
    let room = (played_ms(clip, file) - clip.fade_in_ms).max(0.0);
    ms.round().clamp(0.0, room)
}

/// The gains a knob, a handle and a key give a clip: from -48 to +24 dB. A record may say less,
/// down to `-inf`; a control that moves it starts from the bottom of this.
pub const GAIN_DB: (f32, f32) = (-48.0, AudioClip::MAX_GAIN_DB);
/// How far alt-up and alt-down move the gain of the selected clips.
pub const GAIN_KEY_STEP_DB: f32 = 1.0;
/// Points of a drag of the gain handle for the whole range, as a knob has.
pub const GAIN_TRAVEL: f32 = 200.0;

/// A gain moved by `db` from `from`, to a tenth of a decibel and inside [`GAIN_DB`]. A gain
/// under the range, such as `-inf`, stays where it is when it is moved down.
pub fn gain_moved(from: f32, db: f32) -> f32 {
    let (bottom, top) = GAIN_DB;
    if from < bottom && db <= 0.0 {
        return from;
    }
    let from = from.max(bottom);
    ((from + db) * 10.0)
        .round()
        .clamp(bottom * 10.0, top * 10.0)
        / 10.0
}

/// The label of a gain: `-6 dB`, `+3.5 dB`, `0 dB`, `-inf`.
pub fn gain_label(db: f32) -> String {
    if db == f32::NEG_INFINITY {
        return "-inf".to_string();
    }
    let short = sound_ui::components::knob::short(db);
    match db > 0.0 {
        true => format!("+{short} dB"),
        false => format!("{short} dB"),
    }
}

/// A time as a knob and a label show it: `420 ms`, `1.2 s`.
pub fn time_label(ms: f32) -> String {
    let short = sound_ui::components::knob::short;
    match ms < 1000.0 {
        true => format!("{} ms", short(ms)),
        false => format!("{} s", short(ms / 1000.0)),
    }
}

#[cfg(test)]
mod tests {
    use sound_core::{Tempo, TempoMap, TimeSignature};
    use sound_media::{AudioAsset, Container};

    use super::*;

    const RATE: u32 = 48_000;

    /// 120 bpm: a quarter of 960 ticks is half a second.
    fn clock() -> Clock {
        let tempo_map = TempoMap::constant(
            TimeSignature::new(4, 4).unwrap(),
            Tempo::from_bpm(120.0).unwrap(),
        );
        Clock::new(tempo_map, RATE)
    }

    /// Four seconds of a file at 48 kHz.
    fn file() -> Info {
        Info {
            frames: 4 * u64::from(RATE),
            channels: 1,
            sample_rate: RATE,
            container: Container::Wav,
        }
    }

    fn clip(start: u64, from: f64, to: Option<f64>) -> AudioClip {
        AudioClip {
            file_start_seconds: from,
            file_end_seconds: to,
            ..AudioClip::new(AudioAsset::new("voice.wav").unwrap(), Ticks(start))
        }
    }

    const UNIT: Ticks = Ticks(240);

    #[test]
    fn the_left_edge_trims_the_file_and_keeps_the_sound_in_place() {
        let (clock, file) = (clock(), file());
        // A clip at 2 s (tick 3840) that plays the file from 1 s.
        let origin = clip(3840, 1.0, None);
        assert_eq!(origin.end(Some(&file), &clock), Ticks(3840 + 5760));
        // In by a quarter: half a second later, and the file from 1.5 s.
        let trimmed = trimmed_left(&origin, &file, &clock, 960, UNIT);
        assert_eq!(trimmed.start, Ticks(4800));
        assert_eq!(trimmed.file_start_seconds, 1.5);
        assert_eq!(
            trimmed.end(Some(&file), &clock),
            origin.end(Some(&file), &clock)
        );
        // Out past the start of the file: it stops where the file starts, at 1 s, tick 1920.
        let out = trimmed_left(&origin, &file, &clock, -100_000, UNIT);
        assert_eq!((out.start, out.file_start_seconds), (Ticks(1920), 0.0));
        // In past the right edge: it stops one unit before it.
        let most = trimmed_left(&origin, &file, &clock, 100_000, UNIT);
        assert_eq!(most.start, Ticks(3840 + 5760 - 240));
        assert_eq!(trimmed_left(&origin, &file, &clock, 0, UNIT), origin);
    }

    #[test]
    fn the_left_edge_stops_at_tick_zero() {
        let (clock, file) = (clock(), file());
        let origin = clip(480, 2.0, None);
        let out = trimmed_left(&origin, &file, &clock, -100_000, UNIT);
        assert_eq!(out.start, Ticks(0));
        assert_eq!(out.file_start_seconds, 1.75);
    }

    #[test]
    fn the_right_edge_trims_the_end_and_stops_at_the_end_of_the_file() {
        let (clock, file) = (clock(), file());
        let origin = clip(0, 0.0, None);
        // In by a bar: two seconds.
        let trimmed = trimmed_right(&origin, &file, &clock, -3840, UNIT);
        assert_eq!(trimmed.file_end_seconds, Some(2.0));
        assert_eq!(trimmed.end(Some(&file), &clock), Ticks(3840));
        // Out again past the end of the file: the whole file, written as nothing.
        assert_eq!(
            trimmed_right(&trimmed, &file, &clock, 100_000, UNIT),
            origin
        );
        // Never shorter than a unit.
        let short = trimmed_right(&origin, &file, &clock, -100_000, UNIT);
        assert_eq!(short.end(Some(&file), &clock), Ticks(240));
    }

    #[test]
    fn the_start_and_end_of_the_card_trim_as_the_edges_do() {
        let (clock, file) = (clock(), file());
        // At 2 s, playing the file from 1 s.
        let origin = clip(3840, 1.0, None);
        let later = with_file_start(&origin, &file, &clock, 1.5);
        assert_eq!((later.start, later.file_start_seconds), (Ticks(4800), 1.5));
        // Not before the start of the file, and not so early the clip would start before 0.
        let earliest = with_file_start(&origin, &file, &clock, -1.0);
        assert_eq!(
            (earliest.start, earliest.file_start_seconds),
            (Ticks(1920), 0.0)
        );
        let near_zero = clip(480, 2.0, None);
        let most = with_file_start(&near_zero, &file, &clock, 0.0);
        assert_eq!((most.start, most.file_start_seconds), (Ticks(0), 1.75));
        // The end: short of the start, and the end of the file is no end at all.
        assert_eq!(
            with_file_end(&origin, &file, 3.0).file_end_seconds,
            Some(3.0)
        );
        assert_eq!(with_file_end(&origin, &file, 4.0).file_end_seconds, None);
        assert_eq!(
            with_file_end(&origin, &file, 0.5).file_end_seconds,
            Some(1.01)
        );
    }

    /// A drag of the start line of a hundred moves, each from the clip of the press as the card
    /// gives it, leaves the sound where it was within a frame, wherever it stops.
    #[test]
    fn the_start_of_the_card_keeps_the_sound_in_place_over_a_long_drag() {
        let (clock, file) = (clock(), file());
        let origin = clip(3840, 1.0, None);
        let place = |clip: &AudioClip| clock.seconds_of(clip.start) - clip.file_start_seconds;
        let frame = 1.0 / f64::from(RATE);
        for step in 0..=100 {
            let seconds = 0.3 + 0.0137 * f64::from(step);
            let trimmed = with_file_start(&origin, &file, &clock, seconds);
            assert!(
                (place(&trimmed) - place(&origin)).abs() < frame,
                "{step}: {trimmed:?}"
            );
            assert!(
                (trimmed.file_start_seconds - seconds).abs() < 0.001,
                "{step}"
            );
        }
    }

    #[test]
    fn a_trim_keeps_the_fades_inside_the_clip() {
        let (clock, file) = (clock(), file());
        // Four seconds with a fade in of 3 s and a fade out of 1 s.
        let origin = AudioClip {
            fade_in_ms: 3000.0,
            fade_out_ms: 1000.0,
            ..clip(0, 0.0, None)
        };
        // The end in to 2 s: the fade in takes it all.
        let short = trimmed_right(&origin, &file, &clock, -3840, UNIT);
        assert_eq!((short.fade_in_ms, short.fade_out_ms), (2000.0, 0.0));
        let short = with_file_end(&origin, &file, 2.5);
        assert_eq!((short.fade_in_ms, short.fade_out_ms), (2500.0, 0.0));
        let late = trimmed_left(&origin, &file, &clock, 3840, UNIT);
        assert_eq!((late.fade_in_ms, late.fade_out_ms), (2000.0, 0.0));
        let late = with_file_start(&origin, &file, &clock, 3.5);
        assert!(
            late.fade_in_ms <= 500.0 && late.fade_out_ms == 0.0,
            "{late:?}"
        );
        // The end of the file within a frame is the end of the file.
        let almost = with_file_end(&origin, &file, 4.0 - 0.5 / f64::from(RATE));
        assert_eq!(almost.file_end_seconds, None);
    }

    #[test]
    fn fades_leave_room_for_each_other_and_gain_stays_in_its_range() {
        let file = file();
        let mut origin = clip(0, 1.0, Some(3.0));
        assert_eq!(played_ms(&origin, &file), 2000.0);
        origin.fade_out_ms = 500.0;
        assert_eq!(fade_in(&origin, &file, 420.4), 420.0);
        assert_eq!(fade_in(&origin, &file, 5000.0), 1500.0);
        assert_eq!(fade_in(&origin, &file, -3.0), 0.0);
        origin.fade_in_ms = 1800.0;
        assert_eq!(fade_out(&origin, &file, 900.0), 200.0);
        assert_eq!(gain_moved(0.0, -6.04), -6.0);
        assert_eq!(gain_moved(20.0, 10.0), 24.0);
        assert_eq!(gain_moved(f32::NEG_INFINITY, 1.0), -47.0);
        assert_eq!(gain_moved(f32::NEG_INFINITY, -1.0), f32::NEG_INFINITY);
        assert_eq!(gain_moved(-60.0, -1.0), -60.0);
        assert_eq!(gain_label(-6.0), "-6 dB");
        assert_eq!(gain_label(3.5), "+3.5 dB");
        assert_eq!(gain_label(0.0), "0 dB");
        assert_eq!(time_label(420.0), "420 ms");
        assert_eq!(time_label(1200.0), "1.2 s");
    }
}
