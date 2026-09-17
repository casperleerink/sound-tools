use serde::{Deserialize, Serialize};
use sound_core::{Error, Result};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub start: u64,
    pub length: u64,
    pub key: u8,
    pub velocity: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteEvent {
    pub frame: u64,
    pub kind: NoteKind,
    pub key: u8,
    pub velocity: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoteKind {
    On,
    Off,
}

pub fn validate_note(note: &Note) -> Result<()> {
    if note.length == 0 || note.key > 127 || note.velocity == 0 || note.velocity > 127 {
        return Err(Error(
            "Notes need a positive length, key 0..127 and velocity 1..127".into(),
        ));
    }
    note.start
        .checked_add(note.length)
        .ok_or_else(|| Error("Note end exceeds the frame range".into()))?;
    Ok(())
}

pub fn validate_notes(notes: &[Note]) -> Result<()> {
    let mut previous_end = 0u64;
    for note in notes {
        validate_note(note)?;
        if note.start < previous_end {
            return Err(Error(
                "Notes within one clip must not overlap and must stay sorted".into(),
            ));
        }
        previous_end = note
            .start
            .checked_add(note.length)
            .ok_or_else(|| Error("Note end exceeds the frame range".into()))?;
    }
    Ok(())
}

pub fn note_events(notes: &[Note], clip_start: u64) -> Vec<NoteEvent> {
    let mut events = Vec::with_capacity(notes.len() * 2);
    for note in notes {
        let Some(on) = clip_start.checked_add(note.start) else {
            continue;
        };
        let Some(off) = on.checked_add(note.length) else {
            continue;
        };
        if validate_note(note).is_err() {
            continue;
        }
        events.push(NoteEvent {
            frame: on,
            kind: NoteKind::On,
            key: note.key,
            velocity: note.velocity,
        });
        events.push(NoteEvent {
            frame: off,
            kind: NoteKind::Off,
            key: note.key,
            velocity: 0,
        });
    }
    events.sort_by_key(|event| (event.frame, event.kind == NoteKind::On));
    events
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub start: u64,
    pub length: u64,
    pub notes: Vec<Note>,
}

pub fn validate_clip(clip: &Clip) -> Result<()> {
    if clip.length == 0 {
        return Err(Error("Clips need a positive length".into()));
    }
    clip.start
        .checked_add(clip.length)
        .ok_or_else(|| Error("Clip end exceeds the frame range".into()))?;
    validate_notes(&clip.notes)?;
    if clip.notes.last().is_some_and(|note| {
        note.start
            .checked_add(note.length)
            .is_none_or(|end| end > clip.length)
    }) {
        return Err(Error("Clip notes must stay inside the clip".into()));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub name: String,
    pub instrument: String,
    #[serde(default)]
    pub sampler: Option<crate::sampler::SamplerConfig>,
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub soloed: bool,
    #[serde(default)]
    pub fx: FxChain,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct FilterSettings {
    pub cutoff: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DelaySettings {
    pub seconds: f32,
    pub feedback: f32,
    pub mix: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReverbSettings {
    pub mix: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct FxChain {
    #[serde(default)]
    pub filter: Option<FilterSettings>,
    #[serde(default)]
    pub delay: Option<DelaySettings>,
    #[serde(default)]
    pub reverb: Option<ReverbSettings>,
}

impl FxChain {
    pub fn validate(&self) -> Result<()> {
        if let Some(filter) = &self.filter
            && !(20.0..=20_000.0).contains(&filter.cutoff)
        {
            return Err(Error("Filter cutoff must be 20..20000 Hz".into()));
        }
        if let Some(delay) = &self.delay
            && (!(0.001..=4.0).contains(&delay.seconds)
                || !(0.0..=0.95).contains(&delay.feedback)
                || !(0.0..=1.0).contains(&delay.mix))
        {
            return Err(Error(
                "Delay needs seconds 0.001..4, feedback 0..0.95 and mix 0..1".into(),
            ));
        }
        if let Some(reverb) = &self.reverb
            && !(0.0..=1.0).contains(&reverb.mix)
        {
            return Err(Error("Reverb mix must be 0..1".into()));
        }
        Ok(())
    }
}

impl Default for Track {
    fn default() -> Self {
        Self {
            name: "Track".into(),
            instrument: "daw.synth".into(),
            sampler: None,
            gain: 0.8,
            pan: 0.0,
            muted: false,
            soloed: false,
            fx: FxChain::default(),
            clips: Vec::new(),
        }
    }
}

pub fn validate_track(track: &Track) -> Result<()> {
    if track.name.is_empty()
        || !(-1.0..=1.0).contains(&track.pan)
        || !(0.0..=2.0).contains(&track.gain)
    {
        return Err(Error("Tracks need a name, gain 0..2 and pan -1..1".into()));
    }
    match track.instrument.as_str() {
        "daw.synth" => {}
        "daw.sampler" if track.sampler.is_some() => {}
        "daw.sampler" => {
            return Err(Error("Sampler track needs a sample configuration".into()));
        }
        instrument => return Err(Error(format!("Unsupported instrument: {instrument}"))),
    }
    if let Some(config) = &track.sampler {
        config.validate()?;
    }
    track.fx.validate()?;
    let mut previous_end = 0u64;
    for clip in &track.clips {
        validate_clip(clip)?;
        if clip.start < previous_end {
            return Err(Error(
                "Track clips must not overlap and must stay sorted".into(),
            ));
        }
        previous_end = clip
            .start
            .checked_add(clip.length)
            .ok_or_else(|| Error("Clip end exceeds the frame range".into()))?;
    }
    Ok(())
}

pub const FRAME_RATE: f64 = 48_000.0;
pub const BEAT: u64 = 24_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Arrangement {
    pub tracks: Vec<Track>,
}

pub fn validate_arrangement(arrangement: &Arrangement) -> Result<()> {
    let mut names = std::collections::BTreeSet::new();
    for track in &arrangement.tracks {
        validate_track(track)?;
        if !names.insert(track.name.as_str()) {
            return Err(Error("Track names must be unique".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_validators_reject_overflow_at_every_level() {
        let mut note = Note {
            start: u64::MAX,
            length: 1,
            key: 60,
            velocity: 100,
        };
        assert!(validate_note(&note).is_err());
        assert!(validate_notes(&[note]).is_err());
        let mut clip = Clip {
            start: 0,
            length: u64::MAX,
            notes: vec![note],
        };
        assert!(validate_clip(&clip).is_err());
        note.start -= 1;
        assert!(validate_note(&note).is_ok());
        clip.notes = vec![note];
        assert!(validate_clip(&clip).is_ok());
        clip.start = 1;
        assert!(validate_clip(&clip).is_err());
        let mut track = Track {
            clips: vec![clip],
            ..Track::default()
        };
        assert!(validate_track(&track).is_err());
        assert!(
            validate_arrangement(&Arrangement {
                tracks: vec![track.clone()]
            })
            .is_err()
        );
        track.clips[0].start = 0;
        assert!(
            validate_arrangement(&Arrangement {
                tracks: vec![track]
            })
            .is_ok()
        );
    }

    #[test]
    fn public_note_events_skip_overflow_without_partial_note_pairs() {
        for (start, length, clip_start) in
            [(u64::MAX, 1, 0), (1, 1, u64::MAX), (0, 2, u64::MAX - 1)]
        {
            assert!(
                note_events(
                    &[Note {
                        start,
                        length,
                        key: 60,
                        velocity: 100
                    }],
                    clip_start
                )
                .is_empty()
            );
        }
        let events = note_events(
            &[Note {
                start: 0,
                length: 1,
                key: 60,
                velocity: 100,
            }],
            u64::MAX - 1,
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].frame, u64::MAX);
    }

    #[test]
    fn rejects_overlapping_clips_and_notes() {
        let mut track = Track::default();
        track.clips.push(Clip {
            start: 0,
            length: 1000,
            notes: vec![Note {
                start: 0,
                length: 100,
                key: 60,
                velocity: 100,
            }],
        });
        track.clips.push(Clip {
            start: 500,
            length: 1000,
            notes: vec![],
        });
        assert!(validate_track(&track).is_err());
        track.clips[1].start = 1000;
        assert!(validate_track(&track).is_ok());
        track.clips[0].notes.push(Note {
            start: 50,
            length: 100,
            key: 62,
            velocity: 100,
        });
        assert!(validate_track(&track).is_err());
    }

    #[test]
    fn events_interleave_offs_before_ons_at_the_same_frame() {
        let notes = vec![
            Note {
                start: 0,
                length: 100,
                key: 60,
                velocity: 100,
            },
            Note {
                start: 100,
                length: 50,
                key: 62,
                velocity: 90,
            },
        ];
        let events = note_events(&notes, 1000);
        assert_eq!(
            events[0],
            NoteEvent {
                frame: 1000,
                kind: NoteKind::On,
                key: 60,
                velocity: 100
            }
        );
        assert_eq!(
            events[1],
            NoteEvent {
                frame: 1100,
                kind: NoteKind::Off,
                key: 60,
                velocity: 0
            }
        );
        assert_eq!(
            events[2],
            NoteEvent {
                frame: 1100,
                kind: NoteKind::On,
                key: 62,
                velocity: 90
            }
        );
        assert_eq!(
            events[3],
            NoteEvent {
                frame: 1150,
                kind: NoteKind::Off,
                key: 62,
                velocity: 0
            }
        );
    }
}
