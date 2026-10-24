//! The notes in the moments of a sound, and the phrases they make. A note starts where new sound
//! comes in, such as an attack, a syllable or a change of pitch, and ends where the next starts
//! or where it fades. A phrase is notes with no long silence between them.
//!
//! What it reports is in the sound: where and how high. Which key, which beat and which bar is
//! the composer's to say.

use super::moments::Moment;
use super::pitch::Pitch;

/// A moment is silence under the noise of a recording by this, in dB, the noise being the level
/// of its quietest twentieth…
const ABOVE_NOISE: f64 = 10.0;
const NOISE_SHARE: f64 = 0.05;
/// …but under the loudest moment by no less than the first and no more than the second, and
/// never under the floor.
const UNDER_LOUDEST: (f64, f64) = (20.0, 40.0);
const SILENCE_FLOOR: f64 = -70.0;
/// A note ends where it falls this far under its loudest moment, in dB.
const FADE: f64 = 30.0;
/// A rise starts a note when it is the highest within this many moments either way…
const PEAK_REACH: usize = 3;
/// …and above the mean of the rises this many moments either way, by this share of the
/// highest rise: the thresholds of librosa's onset detection.
const MEAN_REACH: usize = 10;
const ABOVE_MEAN: f64 = 0.07;
/// A note that starts in silence must sound within this many moments.
const SOUNDS_WITHIN: usize = 5;
/// A silence this long, in seconds, splits two phrases.
const PAUSE_SECONDS: f64 = 0.25;
/// The pitch at each end of a note is the middle of this many moments, for its glide…
const END_MOMENTS: usize = 3;
/// …leaving out this many at each end, whose pitch hears the note next to it: a moment's pitch
/// hears 25 ms either side.
const EDGE_MOMENTS: usize = 3;
/// A note sounds for at least this many moments: a click is no note.
const SHORTEST: usize = 2;
/// The fewest notes with a pitch, and gaps between starts, that a tuning and a gap are told from.
const FEWEST: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// In frames from the start, the end after the last frame.
    pub start: u64,
    pub end: u64,
    /// The level of its loudest moment, in dB.
    pub level: f64,
    pub pitch: Option<NotePitch>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotePitch {
    /// The middle pitch, a MIDI note number with its fraction.
    pub note: f64,
    /// From its start to its end, in cents.
    pub glide: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phrase {
    /// In frames from the start, the end after the last frame.
    pub start: u64,
    pub end: u64,
    pub notes: usize,
}

/// How far the pitched notes sit from the notes of equal temperament.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// In cents, from -50 to 50.
    pub cents: f64,
    /// How far the notes spread around it, in cents: near 0 for an instrument in tune with
    /// itself, 20 or more for a voice that speaks.
    pub spread: f64,
}

/// The notes of a sound and what they have in common.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Events {
    pub notes: Vec<Note>,
    pub phrases: Vec<Phrase>,
    pub tuning: Option<Tuning>,
    /// The middle time from one start to the next inside a phrase, in frames.
    pub gap: Option<u64>,
}

impl Events {
    pub(super) fn find(moments: &[Moment], rate: f64) -> Self {
        let notes = notes(moments);
        let pause = (PAUSE_SECONDS * rate) as u64;
        let mut phrases: Vec<Phrase> = Vec::new();
        let mut gaps = Vec::new();
        let mut previous: Option<&Note> = None;
        for note in &notes {
            match phrases.last_mut() {
                Some(phrase) if note.start < phrase.end + pause => {
                    gaps.extend(previous.map(|previous| note.start - previous.start));
                    phrase.end = phrase.end.max(note.end);
                    phrase.notes += 1;
                }
                _ => phrases.push(Phrase {
                    start: note.start,
                    end: note.end,
                    notes: 1,
                }),
            }
            previous = Some(note);
        }
        Self {
            tuning: tuning(&notes),
            gap: (gaps.len() >= FEWEST).then(|| middle(gaps)).flatten(),
            notes,
            phrases,
        }
    }
}

fn notes(moments: &[Moment]) -> Vec<Note> {
    let mut levels: Vec<f64> = moments
        .iter()
        .map(|moment| moment.level.unwrap_or(f64::NEG_INFINITY))
        .collect();
    levels.sort_by(f64::total_cmp);
    let (Some(noise), Some(loudest)) = (
        levels.get((levels.len() as f64 * NOISE_SHARE) as usize),
        levels.last().filter(|loudest| loudest.is_finite()),
    ) else {
        return Vec::new();
    };
    let (closest, farthest) = UNDER_LOUDEST;
    let silence = (noise + ABOVE_NOISE)
        .clamp(loudest - farthest, loudest - closest)
        .max(SILENCE_FLOOR);
    let level = |index: usize| {
        let level = moments.get(index).and_then(|moment| moment.level);
        level.filter(|level| *level > silence)
    };
    let starts = starts(moments, &level);
    let mut notes = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let next = starts.get(index + 1).copied().unwrap_or(moments.len());
        let mut loudest: Option<f64> = None;
        let (mut end, mut sounding) = (next, 0);
        for at in *start..next {
            match level(at) {
                Some(now) if loudest.is_none_or(|peak| now >= peak - FADE) => {
                    loudest = Some(loudest.map_or(now, |peak| peak.max(now)));
                    sounding += 1;
                }
                // A rise comes a moment before the sound is loud enough to hear.
                None if loudest.is_none() => {}
                _ => {
                    end = at;
                    break;
                }
            }
        }
        let (Some(level), Some(first)) = (loudest, moments.get(*start)) else {
            continue;
        };
        if sounding < SHORTEST {
            continue;
        }
        let after = |index: usize| moments.get(index).map(|moment| moment.frame);
        let last = moments
            .get(end.saturating_sub(1))
            .map_or(first.frame, |moment| moment.frame);
        notes.push(Note {
            start: first.frame,
            end: after(end).unwrap_or(last + hop(moments)),
            level,
            pitch: pitch(moments.get(*start..end).unwrap_or_default()),
        });
    }
    notes
}

/// The frames from one moment to the next.
fn hop(moments: &[Moment]) -> u64 {
    match moments {
        [first, second, ..] => second.frame - first.frame,
        _ => 0,
    }
}

/// The moments where a note starts: a peak of the rise, where the sound soon sounds. A sound
/// that comes in too slowly to rise anywhere starts where it is first heard, and so does one
/// whose rise peaks just after that, such as at the click of a short note's end.
fn starts(moments: &[Moment], level: &impl Fn(usize) -> Option<f64>) -> Vec<usize> {
    let rises: Vec<f64> = moments.iter().map(|moment| moment.rise).collect();
    let highest = rises.iter().copied().fold(0.0, f64::max);
    let around = |at: usize, reach: usize| {
        let from = at.saturating_sub(reach);
        rises
            .get(from..(at + reach + 1).min(rises.len()))
            .unwrap_or_default()
    };
    let mut starts: Vec<usize> = Vec::new();
    for (at, rise) in rises.iter().enumerate() {
        let near = around(at, MEAN_REACH);
        let mean = near.iter().sum::<f64>() / near.len().max(1) as f64;
        let peak = around(at, PEAK_REACH).iter().all(|other| other <= rise);
        let sounds = (at..at + SOUNDS_WITHIN).any(|at| level(at).is_some());
        let apart = starts.last().is_none_or(|last| at > last + PEAK_REACH);
        if peak && *rise > 0.0 && *rise >= mean + ABOVE_MEAN * highest && sounds && apart {
            starts.push(at);
        }
    }
    let mut heard = Vec::new();
    for at in 0..moments.len() {
        let comes_in = level(at).is_some() && (at == 0 || level(at - 1).is_none());
        let rose = starts
            .iter()
            .any(|start| *start <= at && at - start < SOUNDS_WITHIN);
        if !comes_in || rose {
            continue;
        }
        let later = starts
            .iter_mut()
            .find(|start| (at..at + SOUNDS_WITHIN).contains(*start));
        match later {
            Some(later) => *later = at,
            None => heard.push(at),
        }
    }
    starts.extend(heard);
    starts.sort_unstable();
    starts
}

/// The middle pitch of a note and its glide, when most of it has a clear pitch.
fn pitch(moments: &[Moment]) -> Option<NotePitch> {
    let sounding = moments
        .iter()
        .filter(|moment| !matches!(moment.pitch, Pitch::Quiet));
    let notes = |moments: &[Moment]| -> Vec<f64> {
        let notes = moments.iter().filter_map(|moment| match moment.pitch {
            Pitch::Note(note) => Some(note),
            Pitch::Quiet | Pitch::Unclear => None,
        });
        notes.collect()
    };
    let all = notes(moments);
    if all.len() < END_MOMENTS || 2 * all.len() < sounding.count() {
        return None;
    }
    let inner = moments
        .len()
        .checked_sub(EDGE_MOMENTS)
        .and_then(|end| moments.get(EDGE_MOMENTS..end));
    let ends = Some(notes(inner.unwrap_or_default()))
        .filter(|inner| inner.len() >= END_MOMENTS)
        .unwrap_or_else(|| all.clone());
    let first = middle(ends.iter().take(END_MOMENTS).copied())?;
    let last = middle(ends.iter().rev().take(END_MOMENTS).copied())?;
    Some(NotePitch {
        note: middle(all)?,
        glide: 100.0 * (last - first),
    })
}

/// The tuning the pitched notes share, weighted by their length: the mean of their offsets
/// from the nearest note and their spread, taken around a circle so that -49 and 49 cents are
/// close.
fn tuning(notes: &[Note]) -> Option<Tuning> {
    let pitched = notes
        .iter()
        .filter_map(|note| Some((note.pitch?.note, note.end - note.start)));
    let (mut cosine, mut sine, mut weight, mut count) = (0.0, 0.0, 0.0, 0);
    for (note, length) in pitched {
        let angle = std::f64::consts::TAU * (note - note.round());
        let length = length as f64;
        cosine += length * angle.cos();
        sine += length * angle.sin();
        weight += length;
        count += 1;
    }
    let cents = |angle: f64| 100.0 * angle / std::f64::consts::TAU;
    // The circular standard deviation.
    let agreement = (cosine * cosine + sine * sine).sqrt() / weight;
    (count >= FEWEST).then(|| Tuning {
        cents: cents(sine.atan2(cosine)),
        spread: cents((-2.0 * agreement.ln()).sqrt()),
    })
}

/// The middle of `values`, `None` for none.
fn middle<T: Copy + PartialOrd>(values: impl IntoIterator<Item = T>) -> Option<T> {
    let mut values: Vec<T> = values.into_iter().collect();
    values.sort_by(|one, other| one.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal));
    values.get(values.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::super::{Events, Meter};

    const RATE: f64 = 48_000.0;

    /// A tone of seven harmonics at `note`, a MIDI note number with its fraction, gliding to
    /// `to` over `seconds`, from `phase` on, shaped by `envelope` of the time in it.
    fn tone(
        note: f64,
        to: f64,
        seconds: f64,
        phase: &mut f64,
        envelope: impl Fn(f64) -> f64,
    ) -> Vec<f64> {
        let frames = (seconds * RATE) as usize;
        (0..frames)
            .map(|frame| {
                let time = frame as f64 / RATE;
                let note = note + (to - note) * time / seconds;
                *phase += std::f64::consts::TAU * 440.0 * ((note - 69.0) / 12.0).exp2() / RATE;
                let wave: f64 = (1..8).map(|k| (k as f64 * *phase).sin() / k as f64).sum();
                0.15 * wave * envelope(time)
            })
            .collect()
    }

    fn pluck(time: f64) -> f64 {
        (-4.0 * time).exp() * (time / 0.003).min(1.0)
    }

    fn rest(seconds: f64) -> Vec<f64> {
        vec![0.0; (seconds * RATE) as usize]
    }

    /// White noise from a fixed seed, by xorshift, at `level` dB RMS.
    fn noise(length: usize, level: f64) -> Vec<f64> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let amplitude = 10_f64.powf(level / 20.0) * 3_f64.sqrt() * 2.0;
        (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                amplitude * (state as f64 / u64::MAX as f64 - 0.5)
            })
            .collect()
    }

    fn events(mono: &[f64]) -> Events {
        stereo_events(mono, 1.0)
    }

    /// With `right` times the sound on the right.
    fn stereo_events(mono: &[f64], right: f64) -> Events {
        let mut meter = Meter::new(RATE as u32, vec![]);
        let samples: Vec<f32> = mono
            .iter()
            .flat_map(|value| [*value as f32, (right * value) as f32])
            .collect();
        for piece in samples.chunks(2 * 333) {
            meter.push(piece);
        }
        meter.finish().events
    }

    /// Eighths at 120 BPM, 20 cents sharp, a rest of two, then four more.
    fn melody() -> (Vec<f64>, Vec<(f64, f64)>) {
        let mut sound = Vec::new();
        let mut played = Vec::new();
        let mut phase = 0.0;
        for (index, note) in [60, 62, 64, 65, 0, 0, 72, 71, 69, 67]
            .into_iter()
            .enumerate()
        {
            if note == 0 {
                sound.extend(rest(0.25));
                continue;
            }
            let note = f64::from(note) + 0.2;
            played.push((index as f64 * 0.25, note));
            sound.extend(tone(note, note, 0.25, &mut phase, pluck));
        }
        (sound, played)
    }

    fn assert_played(events: &Events, played: &[(f64, f64)]) {
        assert_eq!(events.notes.len(), played.len(), "{:#?}", events.notes);
        for (note, (start, pitch)) in events.notes.iter().zip(played) {
            let found = note.start as f64 / RATE;
            assert!((found - start).abs() < 0.02, "{found} is not {start}");
            let found = note.pitch.unwrap().note;
            assert!((found - pitch).abs() < 0.05, "{found} is not {pitch}");
        }
    }

    #[test]
    fn notes_start_where_they_play_and_read_their_pitch_their_tuning_and_their_gap() {
        let (sound, played) = melody();
        let events = events(&sound);
        assert_played(&events, &played);
        let plucked = events.notes[0];
        assert!(plucked.pitch.unwrap().glide.abs() < 5.0);
        // A pluck dies away 30 dB in 0.17 s.
        let length = (plucked.end - plucked.start) as f64 / RATE;
        assert!((0.15..0.26).contains(&length), "{length}");
        // The rest of half a second splits the phrases.
        let phrases: Vec<usize> = events.phrases.iter().map(|phrase| phrase.notes).collect();
        assert_eq!(phrases, [4, 4]);
        let tuning = events.tuning.unwrap();
        assert!(
            (tuning.cents - 20.0).abs() < 2.0 && tuning.spread < 2.0,
            "{tuning:?}"
        );
        let gap = events.gap.unwrap() as f64 / RATE;
        assert!((gap - 0.25).abs() < 0.01, "{gap}");
    }

    #[test]
    fn noise_between_the_notes_of_a_recording_is_silence() {
        let (sound, played) = melody();
        // 22 dB under the notes.
        let noisy: Vec<f64> = sound
            .iter()
            .zip(noise(sound.len(), -40.0))
            .map(|(a, b)| a + b)
            .collect();
        let events = events(&noisy);
        assert_played(&events, &played);
        assert_eq!(events.phrases.len(), 2);
    }

    #[test]
    fn a_held_note_ends_where_the_next_pitch_starts() {
        let mut phase = 0.0;
        let mut sound = Vec::new();
        let mut played = Vec::new();
        // One tone that changes pitch, with no fade or break between the notes.
        for (index, note) in [57.0, 59.0, 60.0, 62.0].into_iter().enumerate() {
            played.push((index as f64 * 0.4, note));
            sound.extend(tone(note, note, 0.4, &mut phase, |_| 1.0));
        }
        let events = events(&sound);
        assert_played(&events, &played);
        assert_eq!(events.phrases.len(), 1);
    }

    #[test]
    fn notes_on_channels_of_opposite_polarity_still_start() {
        let mut phase = 0.0;
        let mut sound = Vec::new();
        for note in [57.0, 59.0, 60.0, 62.0] {
            sound.extend(tone(note, note, 0.4, &mut phase, |_| 1.0));
        }
        assert_eq!(stereo_events(&sound, -1.0).notes.len(), 4);
    }

    #[test]
    fn a_note_shorter_than_the_click_of_its_end_is_still_a_note() {
        // A sine at A4 for 40 ms, cut off.
        let mut sound: Vec<f64> = (0..(0.04 * RATE) as usize)
            .map(|frame| 0.3 * (std::f64::consts::TAU * 440.0 * frame as f64 / RATE).sin())
            .collect();
        sound.extend(rest(0.1));
        let events = events(&sound);
        let [note] = events.notes[..] else {
            panic!("{:#?}", events.notes);
        };
        assert!(note.start < (0.01 * RATE) as u64, "{note:?}");
    }

    #[test]
    fn a_glide_is_one_note_with_the_glide_it_has() {
        let mut phase = 0.0;
        let events = events(&tone(57.0, 69.0, 0.6, &mut phase, |_| 1.0));
        let [note] = events.notes[..] else {
            panic!("{:#?}", events.notes);
        };
        let pitch = note.pitch.unwrap();
        assert!((pitch.note - 63.0).abs() < 0.5, "{pitch:?}");
        // Nearly the whole octave: the ends leave out the 30 ms the next note would be in.
        assert!((950.0..=1200.0).contains(&pitch.glide), "{pitch:?}");
    }

    #[test]
    fn hits_and_a_chord_are_notes_with_no_pitch() {
        let mut sound = Vec::new();
        for _ in 0..4 {
            let hit = noise((0.3 * RATE) as usize, -10.0);
            let decay = hit
                .iter()
                .enumerate()
                .map(|(frame, sample)| sample * (-30.0 * frame as f64 / RATE).exp());
            sound.extend(decay);
        }
        sound.extend(rest(0.5));
        let mut chord = vec![0.0; (0.8 * RATE) as usize];
        for note in [60.0, 64.0, 67.0] {
            let tone = tone(note, note, 0.8, &mut 0.0, pluck);
            for (sample, note) in chord.iter_mut().zip(tone) {
                *sample += note;
            }
        }
        sound.extend(chord);
        let events = events(&sound);
        let starts: Vec<f64> = events
            .notes
            .iter()
            .map(|note| note.start as f64 / RATE)
            .collect();
        assert_eq!(starts.len(), 5, "{starts:?}");
        for (found, start) in starts.iter().zip([0.0, 0.3, 0.6, 0.9, 1.7]) {
            assert!((found - start).abs() < 0.02, "{starts:?}");
        }
        assert!(events.notes.iter().all(|note| note.pitch.is_none()));
        assert_eq!(events.tuning, None);
    }
}
