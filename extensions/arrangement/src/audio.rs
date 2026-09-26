//! Audio clips: a stretch of an audio file at a place on the timeline.
//!
//! A clip is placed in ticks and plays at the speed of its file, so a tempo change moves where
//! it starts and never how fast it plays. It names its file by asset name, never by a path,
//! and never changes the file: everything a composer does to a clip is in its record.

use serde::{Deserialize, Serialize};
use sound_core::{Clock, Frames, Place, State, Ticks};
use sound_media::{AudioAsset, Info};
use sound_notes::TRACK_TOOL;

use crate::decibels;
use crate::player::{AudioSnapshot, PlacedAudio};

/// A saved audio clip: `arrangement.audio_clip`, in the folder of an audio track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioClip {
    /// The file, under `assets/audio/`.
    pub asset: AudioAsset,
    /// Where the clip starts in the project.
    pub start: Ticks,
    /// Where in the file the clip starts playing. 0 when left out.
    #[serde(default)]
    pub file_start_seconds: f64,
    /// Where in the file it stops. The end of the file when left out, and a record that
    /// leaves it out is written back without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_end_seconds: Option<f64>,
    /// How much louder or quieter the clip plays than its file. `"-inf"` is silence.
    #[serde(default, with = "decibels")]
    pub gain_db: f32,
    /// A straight line from silence at the start of the clip to its level.
    #[serde(default)]
    pub fade_in_ms: f32,
    /// A straight line from its level to silence at the end of the clip.
    #[serde(default)]
    pub fade_out_ms: f32,
    /// Where clips of one track overlap, the one with the higher layer is heard and the other
    /// is not. Of equal layers the one that starts later, then the later id.
    /// [`crate::add_audio_clip`] gives a new clip a layer above every other clip of its track,
    /// so the newest covers.
    #[serde(default)]
    pub layer: u32,
}

impl AudioClip {
    /// The loudest a clip plays over its file, in decibels. Written here and nowhere else:
    /// `validate` and the docs read it.
    pub const MAX_GAIN_DB: f32 = 24.0;

    /// A clip of the whole file at `start`, at its own level, with no fades.
    pub fn new(asset: AudioAsset, start: Ticks) -> Self {
        Self {
            asset,
            start,
            file_start_seconds: 0.0,
            file_end_seconds: None,
            gain_db: 0.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            layer: 0,
        }
    }

    /// The part of the file that plays, in frames of the file: from, and up to.
    pub fn file_frames(&self, file: &Info) -> (u64, u64) {
        let rate = f64::from(file.sample_rate);
        let frame = |seconds: f64| ((seconds * rate).round().max(0.0) as u64).min(file.frames);
        let from = frame(self.file_start_seconds);
        let to = self.file_end_seconds.map_or(file.frames, frame);
        (from, to.max(from))
    }

    /// How many engine frames the clip plays at `engine_rate`.
    pub fn length(&self, file: &Info, engine_rate: u32) -> u64 {
        let (from, to) = self.file_frames(file);
        sound_media::engine_frames(to - from, file.sample_rate, engine_rate)
    }

    /// The first tick after the clip, under this clock. It depends on the tempo, because the
    /// clip plays at its own speed. The start when the file is not there.
    pub fn end(&self, file: Option<&Info>, clock: &Clock) -> Ticks {
        let Some(file) = file else {
            return self.start;
        };
        let start = clock.frame_of(self.start).0;
        let length = self.length(file, clock.sample_rate());
        clock.tick_at(Frames(start.saturating_add(length)))
    }
}

impl State for AudioClip {
    const TOOL: &'static str = "arrangement.audio_clip";
    /// Only a track plays clips. Anywhere else a clip would load and never sound.
    const PLACE: Place = Place::In(TRACK_TOOL);

    fn validate(&self) -> Result<(), String> {
        let seconds = |field: &str, value: f64| match value.is_finite() && value >= 0.0 {
            true => Ok(()),
            false => Err(format!(
                "{field} must be a number of seconds, 0 or more, not {value}"
            )),
        };
        seconds("file_start_seconds", self.file_start_seconds)?;
        if let Some(end) = self.file_end_seconds {
            seconds("file_end_seconds", end)?;
            if end <= self.file_start_seconds {
                return Err(format!(
                    "file_end_seconds must be after file_start_seconds {}, not {end}",
                    self.file_start_seconds
                ));
            }
        }
        decibels::check("gain_db", self.gain_db, Self::MAX_GAIN_DB)?;
        for (field, value) in [
            ("fade_in_ms", self.fade_in_ms),
            ("fade_out_ms", self.fade_out_ms),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!(
                    "{field} must be a number of milliseconds, 0 or more, not {value}"
                ));
            }
        }
        Ok(())
    }
}

/// The snapshot of the audio clips of one track, and a line for each clip that cannot play.
///
/// `clips` are `(name, clip)` in any order. Reads every file the first time it is named, on
/// this thread, see [`sound_media::load`]. A file that does not play, or a clip that plays
/// none of its file, costs one look at the file after that, see [`sound_media::info`].
pub(crate) fn snapshot<'a>(
    clips: impl IntoIterator<Item = (&'a str, &'a AudioClip)>,
    assets: &sound_core::Assets,
    engine_rate: u32,
) -> (AudioSnapshot, Vec<String>) {
    let mut placed = Vec::new();
    let mut problems = Vec::new();
    for (name, clip) in clips {
        let file = match sound_media::info(assets, &clip.asset) {
            Ok(file) => file,
            Err(sound_media::MediaError::Missing { path }) => {
                problems.push(format!(
                    "the clip {name:?} plays {path}, which is not there. The clip keeps its place and is silent, and the rest plays. Copy the file into assets/audio/ under that name and the clip plays, or correct `asset`"
                ));
                continue;
            }
            Err(error) => {
                problems.push(format!("the clip {name:?} is silent: {error}"));
                continue;
            }
        };
        let length = clip.length(&file, engine_rate);
        if length == 0 {
            problems.push(format!(
                "the clip {name:?} plays nothing: file_start_seconds {} is at or past the end of {}, which is {:.3} s long",
                clip.file_start_seconds,
                clip.asset,
                file.seconds()
            ));
            continue;
        }
        let audio = match sound_media::load(assets, &clip.asset) {
            Ok(audio) => audio,
            Err(error) => {
                problems.push(format!("the clip {name:?} is silent: {error}"));
                continue;
            }
        };
        placed.push((name, clip, audio, length));
    }
    // The clip that covers the others comes last.
    placed.sort_by(|(a_name, a, ..), (b_name, b, ..)| {
        (a.layer, a.start, a_name).cmp(&(b.layer, b.start, b_name))
    });
    let clips = placed.into_iter().map(|(_, clip, audio, length)| {
        let file = audio.info();
        let (origin, _) = clip.file_frames(&file);
        // What the file holds from where the clip starts, which a join may play past its end.
        let rate = file.sample_rate;
        let available = sound_media::engine_frames(file.frames - origin, rate, engine_rate);
        let milliseconds =
            |ms: f32| (f64::from(ms) / 1000.0 * f64::from(engine_rate)).round() as u64;
        PlacedAudio {
            start: clip.start,
            resampler: sound_media::resampler(audio.sample_rate(), engine_rate),
            audio,
            origin,
            length,
            available,
            gain: decibels::amplitude(clip.gain_db),
            fade_in: milliseconds(clip.fade_in_ms),
            fade_out: milliseconds(clip.fade_out_ms),
        }
    });
    (AudioSnapshot::new(clips.collect()), problems)
}
