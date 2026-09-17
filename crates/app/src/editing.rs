use sound_core::{Error, Result};
use sound_daw::arrangement::{Arrangement, BEAT, Clip, Note, Track, validate_arrangement};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub track: usize,
    pub clip: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Edit {
    AddTrack,
    DeleteTrack(usize),
    AddClip(usize),
    DeleteClip(Selection),
    DuplicateClip(Selection),
    MoveClip(Selection, i8),
    Transpose(Selection, i8),
    Mute(usize),
    Solo(usize),
    Gain(usize, f32),
    Pan(usize, f32),
    Filter(usize),
    Delay(usize),
    Reverb(usize),
}

pub fn selected_clip(arrangement: &Arrangement, selection: Selection) -> Option<&Clip> {
    arrangement
        .tracks
        .get(selection.track)?
        .clips
        .get(selection.clip)
}

pub fn reconcile_selection(
    displayed: &Arrangement,
    live: &Arrangement,
    selection: Option<Selection>,
) -> Option<Selection> {
    selection.filter(|selected| displayed == live && selected_clip(live, *selected).is_some())
}

fn track_mut(arrangement: &mut Arrangement, index: usize) -> Result<&mut Track> {
    arrangement
        .tracks
        .get_mut(index)
        .ok_or_else(|| Error("Track no longer exists".into()))
}

fn clip_mut(arrangement: &mut Arrangement, selection: Selection) -> Result<&mut Clip> {
    track_mut(arrangement, selection.track)?
        .clips
        .get_mut(selection.clip)
        .ok_or_else(|| Error("Clip no longer exists".into()))
}

fn end(clip: &Clip) -> Result<u64> {
    clip.start
        .checked_add(clip.length)
        .ok_or_else(|| Error("Clip exceeds the frame range".into()))
}

fn track_end(track: &Track) -> Result<u64> {
    track
        .clips
        .iter()
        .try_fold(0, |last, clip| Ok(last.max(end(clip)?)))
}

pub fn validate(arrangement: &Arrangement) -> Result<()> {
    for track in &arrangement.tracks {
        for clip in &track.clips {
            end(clip)?;
            for note in &clip.notes {
                note.start
                    .checked_add(note.length)
                    .and_then(|end| clip.start.checked_add(end))
                    .ok_or_else(|| Error("Note exceeds the frame range".into()))?;
            }
        }
    }
    validate_arrangement(arrangement)
}

pub fn apply(source: &Arrangement, edit: Edit) -> Result<Arrangement> {
    validate(source)?;
    let mut next = source.clone();
    match edit {
        Edit::AddTrack => {
            let name = (1..=source.tracks.len().saturating_add(1))
                .map(|number| format!("Track {number}"))
                .find(|name| source.tracks.iter().all(|track| &track.name != name))
                .ok_or_else(|| Error("No available track name".into()))?;
            next.tracks.push(Track {
                name,
                ..Track::default()
            });
        }
        Edit::DeleteTrack(index) => {
            track_mut(&mut next, index)?;
            next.tracks.remove(index);
        }
        Edit::AddClip(index) => {
            let track = track_mut(&mut next, index)?;
            track.clips.push(Clip {
                start: track_end(track)?,
                length: BEAT * 4,
                notes: vec![Note {
                    start: 0,
                    length: BEAT / 2,
                    key: 60,
                    velocity: 100,
                }],
            });
        }
        Edit::DeleteClip(selection) => {
            clip_mut(&mut next, selection)?;
            track_mut(&mut next, selection.track)?
                .clips
                .remove(selection.clip);
        }
        Edit::DuplicateClip(selection) => {
            let mut clip = clip_mut(&mut next, selection)?.clone();
            let track = track_mut(&mut next, selection.track)?;
            clip.start = track_end(track)?;
            track.clips.push(clip);
        }
        Edit::MoveClip(selection, beats) => {
            let clip = clip_mut(&mut next, selection)?;
            let distance = BEAT * u64::from(beats.unsigned_abs());
            clip.start = if beats < 0 {
                clip.start.checked_sub(distance)
            } else {
                clip.start.checked_add(distance)
            }
            .ok_or_else(|| Error("Cannot move outside the frame range".into()))?;
            track_mut(&mut next, selection.track)?
                .clips
                .sort_by_key(|clip| clip.start);
        }
        Edit::Transpose(selection, semitones) => {
            for note in &mut clip_mut(&mut next, selection)?.notes {
                let key = i16::from(note.key) + i16::from(semitones);
                if !(0..=127).contains(&key) {
                    return Err(Error("Transpose would exceed MIDI keys 0..127".into()));
                }
                note.key = key as u8;
            }
        }
        Edit::Mute(index) => {
            let track = track_mut(&mut next, index)?;
            track.muted = !track.muted;
        }
        Edit::Solo(index) => {
            let track = track_mut(&mut next, index)?;
            track.soloed = !track.soloed;
        }
        Edit::Gain(index, delta) => {
            if !delta.is_finite() {
                return Err(Error("Gain change must be finite".into()));
            }
            let track = track_mut(&mut next, index)?;
            track.gain = (track.gain + delta).clamp(0.0, 2.0);
        }
        Edit::Filter(index) => {
            let fx = &mut track_mut(&mut next, index)?.fx;
            fx.filter = fx
                .filter
                .is_none()
                .then_some(sound_daw::FilterSettings { cutoff: 4_000.0 });
        }
        Edit::Delay(index) => {
            let fx = &mut track_mut(&mut next, index)?.fx;
            fx.delay = fx.delay.is_none().then_some(sound_daw::DelaySettings {
                seconds: 0.25,
                feedback: 0.3,
                mix: 0.2,
            });
        }
        Edit::Reverb(index) => {
            let fx = &mut track_mut(&mut next, index)?.fx;
            fx.reverb = fx
                .reverb
                .is_none()
                .then_some(sound_daw::ReverbSettings { mix: 0.2 });
        }
        Edit::Pan(index, delta) => {
            if !delta.is_finite() {
                return Err(Error("Pan change must be finite".into()));
            }
            let track = track_mut(&mut next, index)?;
            track.pan = (track.pan + delta).clamp(-1.0, 1.0);
        }
    }
    validate(&next)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrangement() -> Arrangement {
        apply(
            &apply(&Arrangement::default(), Edit::AddTrack).unwrap(),
            Edit::AddClip(0),
        )
        .unwrap()
    }

    const SELECTED: Selection = Selection { track: 0, clip: 0 };

    #[test]
    fn selection_preserves_clips_and_resets_on_live_changes() {
        let displayed = arrangement();
        let live = displayed.clone();
        assert_eq!(
            reconcile_selection(&displayed, &live, Some(SELECTED)),
            Some(SELECTED)
        );
        assert_eq!(live.tracks[0].clips.len(), 1);
        assert_eq!(live, displayed);
        let deleted = apply(&live, Edit::DeleteClip(SELECTED)).unwrap();
        assert_eq!(
            reconcile_selection(&displayed, &deleted, Some(SELECTED)),
            None
        );
        let replaced = apply(&live, Edit::Transpose(SELECTED, 1)).unwrap();
        assert_eq!(
            reconcile_selection(&displayed, &replaced, Some(SELECTED)),
            None
        );
        let stale = Selection {
            track: 99,
            clip: 99,
        };
        assert_eq!(reconcile_selection(&live, &live, Some(stale)), None);
    }

    #[test]
    fn new_track_names_are_unique_even_after_deletion() {
        let first = arrangement();
        let second = apply(&first, Edit::AddTrack).unwrap();
        let deleted = apply(&second, Edit::DeleteTrack(0)).unwrap();
        let added = apply(&deleted, Edit::AddTrack).unwrap();
        assert_eq!(added.tracks[0].name, "Track 2");
        assert_eq!(added.tracks[1].name, "Track 1");
        validate(&added).unwrap();
    }

    #[test]
    fn added_and_duplicated_clips_append_without_overlap() {
        let first = arrangement();
        let second = apply(&first, Edit::AddClip(0)).unwrap();
        let third = apply(&second, Edit::DuplicateClip(SELECTED)).unwrap();
        assert_eq!(
            third.tracks[0]
                .clips
                .iter()
                .map(|clip| clip.start)
                .collect::<Vec<_>>(),
            vec![0, BEAT * 4, BEAT * 8]
        );
        assert_eq!(
            third.tracks[0].clips[2].notes,
            first.tracks[0].clips[0].notes
        );
        let deleted = apply(&third, Edit::DeleteClip(SELECTED)).unwrap();
        assert_eq!(deleted.tracks[0].clips.len(), 2);
        assert_eq!(deleted.tracks[0].clips[0].start, BEAT * 4);
    }

    #[test]
    fn move_checks_zero_overlap_and_overflow() {
        let first = arrangement();
        assert!(apply(&first, Edit::MoveClip(SELECTED, -1)).is_err());
        let moved = apply(&first, Edit::MoveClip(SELECTED, 1)).unwrap();
        assert_eq!(moved.tracks[0].clips[0].start, BEAT);
        assert_eq!(apply(&moved, Edit::MoveClip(SELECTED, -1)).unwrap(), first);
        let adjacent = apply(&first, Edit::AddClip(0)).unwrap();
        assert!(apply(&adjacent, Edit::MoveClip(SELECTED, 1)).is_err());
        let mut far = first.clone();
        far.tracks[0].clips[0].start = u64::MAX - BEAT * 4;
        assert!(apply(&far, Edit::MoveClip(SELECTED, 1)).is_err());
        assert!(apply(&far, Edit::AddClip(0)).is_err());
        assert!(apply(&far, Edit::DuplicateClip(SELECTED)).is_err());
    }

    #[test]
    fn transpose_is_atomic_and_obeys_midi_boundaries() {
        let mut first = arrangement();
        let up = apply(&first, Edit::Transpose(SELECTED, 1)).unwrap();
        assert_eq!(up.tracks[0].clips[0].notes[0].key, 61);
        assert_eq!(apply(&up, Edit::Transpose(SELECTED, -1)).unwrap(), first);
        for (key, delta) in [(0, -1), (127, 1)] {
            first.tracks[0].clips[0].notes[0].key = key;
            let original = first.clone();
            assert!(apply(&first, Edit::Transpose(SELECTED, delta)).is_err());
            assert_eq!(first, original);
        }
    }

    #[test]
    fn stale_indices_and_invalid_ranges_return_errors() {
        let mut first = arrangement();
        let stale = Selection { track: 0, clip: 99 };
        for edit in [
            Edit::DeleteTrack(99),
            Edit::AddClip(99),
            Edit::DeleteClip(stale),
            Edit::DuplicateClip(stale),
            Edit::MoveClip(stale, 1),
            Edit::Transpose(stale, 1),
        ] {
            assert!(apply(&first, edit).is_err());
        }
        first.tracks[0].clips[0].notes[0].start = u64::MAX;
        assert!(apply(&first, Edit::AddTrack).is_err());
    }

    #[test]
    fn effects_toggle_independently_with_valid_defaults() {
        let first = apply(&arrangement(), Edit::AddTrack).unwrap();
        let mut next = first.clone();
        for edit in [Edit::Filter(1), Edit::Delay(1), Edit::Reverb(1)] {
            next = apply(&next, edit).unwrap();
            validate(&next).unwrap();
            assert_eq!(next.tracks[0], first.tracks[0]);
        }
        let fx = next.tracks[1].fx;
        assert_eq!(fx.filter.unwrap().cutoff, 4_000.0);
        assert_eq!(fx.delay.unwrap().seconds, 0.25);
        assert_eq!(fx.reverb.unwrap().mix, 0.2);
        for edit in [Edit::Filter(1), Edit::Delay(1), Edit::Reverb(1)] {
            next = apply(&next, edit).unwrap();
        }
        assert_eq!(next, first);
        for edit in [Edit::Filter(99), Edit::Delay(99), Edit::Reverb(99)] {
            assert!(apply(&first, edit).is_err());
        }
    }

    #[test]
    fn mixer_edits_validate_and_clamp() {
        let first = arrangement();
        assert_eq!(
            apply(&first, Edit::Gain(0, -9.0)).unwrap().tracks[0].gain,
            0.0
        );
        assert_eq!(
            apply(&first, Edit::Gain(0, 9.0)).unwrap().tracks[0].gain,
            2.0
        );
        assert_eq!(
            apply(&first, Edit::Pan(0, -9.0)).unwrap().tracks[0].pan,
            -1.0
        );
        assert_eq!(apply(&first, Edit::Pan(0, 9.0)).unwrap().tracks[0].pan, 1.0);
        assert!(apply(&first, Edit::Gain(0, f32::NAN)).is_err());
        assert!(apply(&first, Edit::Pan(0, f32::INFINITY)).is_err());
        assert!(apply(&first, Edit::Mute(0)).unwrap().tracks[0].muted);
        assert!(apply(&first, Edit::Solo(0)).unwrap().tracks[0].soloed);
    }
}
