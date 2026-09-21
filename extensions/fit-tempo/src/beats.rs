//! The beat finder: where the beats of a freely played take are.
//!
//! Plain Rust, no model and no heavy dependency, and the same bytes for the same take on every
//! run. It is a small version of the dynamic-programming beat tracker that Daniel Ellis
//! described in 2007, on the exact note times of a MIDI take instead of on an audio onset
//! envelope, with the beat period estimated again every second so that a piece that slows down
//! or changes tempo is followed and not resisted.
//!
//! Four steps:
//!
//! 1. **Onsets.** Note ons within [`CHORD_US`] of each other are one onset, at their mean time,
//!    with a weight that grows with how many notes it holds. A chord is one strong onset; an
//!    arpeggio spread wider than that is not.
//! 2. **The period.** An onset envelope on a grid of [`FRAME_HZ`] frames a second, and a comb
//!    over its autocorrelation: the score of a period is the correlation at that lag plus a
//!    part of the correlation at two, three and four times it. A log-normal prior around
//!    [`PRIOR_CENTRE_SECONDS`] picks between a period and its double, which is a choice no
//!    method can make from the timing alone. The same comb over a window of
//!    [`TEMPO_WINDOW_SECONDS`] every [`TEMPO_STEP_SECONDS`] gives the period at each moment.
//! 3. **The beats.** One pass of dynamic programming over the frames: the best beat sequence
//!    is the one that lands on strong onsets and keeps its steps near the local period. The
//!    cost of a step is [`TIGHTNESS`] times the square of the log of how far the step is from
//!    that period, so a small rubato is cheap and a skipped beat is not.
//! 4. **Snapping.** Each beat moves to the onset within [`SNAP_US`] of it, so a beat is on the
//!    note the hand played and not on the 5 ms grid the search used.
//!
//! What it is wrong about is in `README.md` and in the agent doc: it is the octave, the first
//! downbeat and the time signature, which is exactly what an agent corrects in the fit record.

use sound_notes::RawEvent;

/// Note ons this far apart are one onset. A chord is played in about 30 ms.
pub const CHORD_US: u64 = 40_000;

/// The grid the search runs on: 5 ms a frame. Snapping puts the beats back on exact times.
pub const FRAME_HZ: u64 = 200;

/// The periods the comb looks at, as beats per minute.
const MIN_SEARCH_BPM: f64 = 40.0;
const MAX_SEARCH_BPM: f64 = 208.0;

/// Where the prior over periods sits, in seconds: 120 bpm.
pub const PRIOR_CENTRE_SECONDS: f64 = 0.5;
/// How wide it is, in octaves. Wide, because it only has to decide between a period, its half
/// and its double.
const PRIOR_OCTAVES: f64 = 1.0;
/// The prior of a window around the period of the whole take. Narrower, so that the octave a
/// take is read in stays the same from start to end: one octave error over the whole take is
/// one field for an agent to correct, a different one per section is not.
const WINDOW_PRIOR_OCTAVES: f64 = 0.5;

/// How long a window is when the period is measured again, and how often that happens.
const TEMPO_WINDOW_SECONDS: f64 = 5.0;
const TEMPO_STEP_SECONDS: f64 = 1.0;

/// How dearly a step away from the local period costs. Measured against the generated takes of
/// `tests/`: below about 10 the search wanders off a beat that has no note on it, and above
/// about 60 it will not follow a ritardando.
const TIGHTNESS: f64 = 20.0;

/// A beat moves to an onset this close to it.
pub const SNAP_US: u64 = 30_000;

/// How much a low note counts for over the middle of the take, at an octave below it. A piano
/// player's left hand marks the beat, so the bass of a group says more about where the beat is
/// than the notes above it. Without this, playing in which every beat is subdivided evenly,
/// such as an arpeggio, has nothing to tell the beat from the notes between them.
const BASS_WEIGHT: f64 = 2.0;

/// Notes that were played together, as one moment with a weight.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Onset {
    pub time_us: u64,
    pub weight: f64,
    /// The lowest note of the group, which decides part of the weight.
    pub lowest: u8,
}

/// The note ons of a take as onsets, in order. Times are the take's own, in microseconds.
pub fn onsets(events: &[RawEvent]) -> Vec<Onset> {
    let mut played: Vec<(u64, u8)> = events
        .iter()
        .filter_map(|event| match event {
            RawEvent::On { time_us, pitch, .. } => Some((*time_us, *pitch)),
            _ => None,
        })
        .collect();
    played.sort_unstable();
    let mut onsets: Vec<Onset> = Vec::new();
    let mut group: Vec<(u64, u8)> = Vec::new();
    for note in played {
        if group
            .first()
            .is_some_and(|first| note.0 - first.0 > CHORD_US)
        {
            onsets.push(merge(&group));
            group.clear();
        }
        group.push(note);
    }
    if !group.is_empty() {
        onsets.push(merge(&group));
    }
    weigh_the_bass(&mut onsets);
    onsets
}

/// One onset from the notes of a chord: the mean of their times, so that the jitter of a hand
/// is averaged away, and a weight that grows with the count but flattens, so one loud chord
/// cannot decide the whole grid.
fn merge(group: &[(u64, u8)]) -> Onset {
    let sum: u128 = group.iter().map(|note| u128::from(note.0)).sum();
    Onset {
        time_us: (sum / group.len() as u128) as u64,
        weight: 1.0 + (group.len() as f64).ln(),
        lowest: group.iter().map(|note| note.1).min().unwrap_or(0),
    }
}

/// Gives every onset below the middle of the take a part of [`BASS_WEIGHT`] more, at most a
/// whole octave's worth.
fn weigh_the_bass(onsets: &mut [Onset]) {
    let mut lowest: Vec<u8> = onsets.iter().map(|onset| onset.lowest).collect();
    lowest.sort_unstable();
    let Some(middle) = lowest.get(lowest.len() / 2).copied() else {
        return;
    };
    for onset in onsets {
        let below = f64::from(middle.saturating_sub(onset.lowest)).min(12.0) / 12.0;
        onset.weight *= 1.0 + BASS_WEIGHT * below;
    }
}

/// The beats of a take, in microseconds from the start of the recording, in order.
///
/// Empty when the take has fewer than [`MIN_ONSETS`] onsets: there is nothing to find a period
/// in, and a grid guessed from two notes would be worse than no fit.
pub fn find_beats(events: &[RawEvent]) -> Vec<u64> {
    let onsets = onsets(events);
    if onsets.len() < MIN_ONSETS {
        return Vec::new();
    }
    let envelope = envelope(&onsets);
    let Some(period) = best_period(
        &envelope,
        0,
        envelope.len(),
        PRIOR_CENTRE_SECONDS,
        PRIOR_OCTAVES,
    ) else {
        return Vec::new();
    };
    let local = local_periods(&envelope, period);
    let frames = track(&envelope, &local);
    let beats = frames
        .iter()
        .map(|frame| *frame as u64 * 1_000_000 / FRAME_HZ);
    let beats = fill(beats.collect::<Vec<u64>>());
    let beats = snap(beats, &onsets);
    cover(beats, &onsets)
}

/// Fewer onsets than this and there is no period to find.
pub const MIN_ONSETS: usize = 8;

/// The onsets on the search grid. Frame 0 is the start of the recording.
fn envelope(onsets: &[Onset]) -> Vec<f64> {
    let last = onsets.last().map_or(0, |onset| onset.time_us);
    let frames = (last * FRAME_HZ / 1_000_000) as usize + 1;
    let mut envelope = vec![0.0; frames];
    for onset in onsets {
        let frame = (onset.time_us * FRAME_HZ / 1_000_000) as usize;
        if let Some(value) = envelope.get_mut(frame) {
            *value += onset.weight;
        }
    }
    envelope
}

/// The lags the comb looks at, in frames, from the fastest to the slowest tempo.
fn lags() -> std::ops::RangeInclusive<usize> {
    let of = |bpm: f64| (60.0 / bpm * FRAME_HZ as f64).round() as usize;
    of(MAX_SEARCH_BPM)..=of(MIN_SEARCH_BPM)
}

/// The best period in frames over `envelope[from..to]`, or `None` when the stretch is too short
/// to hold one.
fn best_period(
    envelope: &[f64],
    from: usize,
    to: usize,
    centre_seconds: f64,
    octaves: f64,
) -> Option<f64> {
    let window = envelope.get(from..to)?;
    let mut best: Option<(f64, usize)> = None;
    for lag in lags() {
        if lag * 2 >= window.len() {
            break;
        }
        // The comb: a period explains the onsets at one, two, three and four times itself.
        let comb: f64 = [(1, 1.0), (2, 0.5), (3, 0.25), (4, 0.25)]
            .into_iter()
            .map(|(multiple, weight)| weight * correlation(window, lag * multiple))
            .sum();
        let seconds = lag as f64 / FRAME_HZ as f64;
        let distance = (seconds / centre_seconds).log2() / octaves;
        let score = comb * (-0.5 * distance * distance).exp();
        if best.is_none_or(|(highest, _)| score > highest) {
            best = Some((score, lag));
        }
    }
    best.map(|(_, lag)| lag as f64)
}

/// The autocorrelation at one lag, per frame, so that lags of different lengths compare.
fn correlation(window: &[f64], lag: usize) -> f64 {
    let Some(overlap) = window.len().checked_sub(lag).filter(|it| *it > 0) else {
        return 0.0;
    };
    let sum: f64 = (0..overlap).map(|n| window[n] * window[n + lag]).sum();
    sum / overlap as f64
}

/// The period at every frame, from a window of [`TEMPO_WINDOW_SECONDS`] measured every
/// [`TEMPO_STEP_SECONDS`] and joined by a straight line in between.
fn local_periods(envelope: &[f64], overall: f64) -> Vec<f64> {
    let half = (TEMPO_WINDOW_SECONDS * FRAME_HZ as f64 / 2.0) as usize;
    let step = (TEMPO_STEP_SECONDS * FRAME_HZ as f64) as usize;
    let centre_seconds = overall / FRAME_HZ as f64;
    let mut measured: Vec<(usize, f64)> = Vec::new();
    let mut centre = 0;
    while centre < envelope.len() {
        let from = centre.saturating_sub(half);
        let to = (centre + half).min(envelope.len());
        let period = best_period(envelope, from, to, centre_seconds, WINDOW_PRIOR_OCTAVES);
        measured.push((centre, period.unwrap_or(overall)));
        centre += step;
    }
    if measured.is_empty() {
        return vec![overall; envelope.len()];
    }
    (0..envelope.len())
        .map(|frame| between(&measured, frame, overall))
        .collect()
}

/// The measured period at `frame`, on the line between the two windows around it.
fn between(measured: &[(usize, f64)], frame: usize, fallback: f64) -> f64 {
    let after = measured.partition_point(|(centre, _)| *centre <= frame);
    match (
        after.checked_sub(1).and_then(|it| measured.get(it)),
        measured.get(after),
    ) {
        (Some((left, a)), Some((right, b))) if right > left => {
            let part = (frame - left) as f64 / (right - left) as f64;
            a + (b - a) * part
        }
        (Some((_, a)), _) => *a,
        (None, Some((_, b))) => *b,
        (None, None) => fallback,
    }
}

/// The dynamic programming pass: the frames of the best beat sequence, in order.
fn track(envelope: &[f64], local: &[f64]) -> Vec<usize> {
    let frames = envelope.len();
    let mut score = vec![f64::NEG_INFINITY; frames];
    let mut previous = vec![usize::MAX; frames];
    for frame in 0..frames {
        let period = local[frame].max(1.0);
        let earliest = frame.saturating_sub((period * 2.0) as usize);
        let latest = frame.saturating_sub((period * 0.5).max(1.0) as usize);
        let mut best = f64::NEG_INFINITY;
        let mut best_frame = usize::MAX;
        for candidate in earliest..=latest.min(frame.saturating_sub(1)) {
            if score[candidate] == f64::NEG_INFINITY {
                continue;
            }
            let step = (frame - candidate) as f64 / period;
            let logarithm = step.ln();
            let total = score[candidate] - TIGHTNESS * logarithm * logarithm;
            if total > best {
                best = total;
                best_frame = candidate;
            }
        }
        if best_frame == usize::MAX {
            // A sequence may begin anywhere in the first two beats and nowhere later, else the
            // search would start fresh at every onset and pay no cost at all.
            if (frame as f64) < period * 2.0 {
                score[frame] = envelope[frame];
            }
            continue;
        }
        score[frame] = envelope[frame] + best;
        previous[frame] = best_frame;
    }

    // The last beat is the best end within one period of the end of the take.
    let period = local.last().copied().unwrap_or(1.0).max(1.0);
    let from = frames.saturating_sub(period as usize + 1);
    let end = (from..frames)
        .filter(|frame| score[*frame] > f64::NEG_INFINITY)
        .max_by(|a, b| score[*a].total_cmp(&score[*b]));
    let Some(end) = end else {
        return Vec::new();
    };
    let mut beats = vec![end];
    let mut frame = end;
    while previous[frame] != usize::MAX {
        frame = previous[frame];
        beats.push(frame);
    }
    beats.reverse();
    beats
}

/// How many beats on each side of a step decide what a beat of that moment is worth.
const NEIGHBOURS: usize = 4;

/// Fills a hole: a step that is a whole multiple of the steps around it gets the beats it is
/// missing, evenly spaced.
///
/// The search takes a step of two beats now and then, where the period it measured for that
/// moment came out too long. One beat missing in the middle of a take is a whole beat of error
/// and is worth this much: the steps on either side say what the step should have been.
fn fill(beats: Vec<u64>) -> Vec<u64> {
    let steps: Vec<u64> = beats.windows(2).map(|pair| pair[1] - pair[0]).collect();
    if steps.len() < NEIGHBOURS {
        return beats;
    }
    let mut filled = vec![beats[0]];
    for (index, step) in steps.iter().enumerate() {
        let parts = (*step as f64 / neighbour_step(&steps, index))
            .round()
            .max(1.0) as u64;
        for part in 1..=parts {
            filled.push(beats[index] + step * part / parts);
        }
    }
    filled
}

/// What a step around `index` usually is: the middle of the [`NEIGHBOURS`] on each side, which
/// follows a tempo that changes while the take runs. The step itself is left out, so a hole
/// does not decide what a hole is.
fn neighbour_step(steps: &[u64], index: usize) -> f64 {
    let from = index.saturating_sub(NEIGHBOURS);
    let to = (index + NEIGHBOURS + 1).min(steps.len());
    let mut around: Vec<u64> = (from..to)
        .filter(|it| *it != index)
        .map(|it| steps[it])
        .collect();
    around.sort_unstable();
    around
        .get(around.len() / 2)
        .map_or(steps[index] as f64, |step| *step as f64)
}

/// Moves every beat onto the onset within [`SNAP_US`] of it, keeping the beats in order.
fn snap(beats: Vec<u64>, onsets: &[Onset]) -> Vec<u64> {
    let mut snapped: Vec<u64> = Vec::with_capacity(beats.len());
    for beat in beats {
        let nearest = onsets
            .iter()
            .filter(|onset| onset.time_us.abs_diff(beat) <= SNAP_US)
            .min_by_key(|onset| onset.time_us.abs_diff(beat));
        let time = nearest.map_or(beat, |onset| onset.time_us);
        // Two beats never share a moment: a grid with two beats on one tick has no tempo.
        match snapped.last() {
            Some(last) if time <= *last => snapped.push(beat.max(last + 1)),
            _ => snapped.push(time),
        }
    }
    snapped
}

/// Extends the grid at both ends so that every note of the take is inside it. The search may
/// start after the first note and end before the last, and a note outside the grid would land
/// before the first beat or after the last tempo change.
fn cover(mut beats: Vec<u64>, onsets: &[Onset]) -> Vec<u64> {
    let (Some(first), Some(last)) = (onsets.first(), onsets.last()) else {
        return beats;
    };
    if beats.len() < 2 {
        return beats;
    }
    let step_at_start = beats[1] - beats[0];
    while beats[0] > first.time_us {
        let earlier = beats[0].saturating_sub(step_at_start);
        beats.insert(0, earlier);
        if earlier == 0 {
            break;
        }
    }
    let count = beats.len();
    let step_at_end = beats[count - 1] - beats[count - 2];
    while *beats.last().unwrap_or(&u64::MAX) < last.time_us {
        let later = beats.last().copied().unwrap_or(0) + step_at_end;
        beats.push(later);
    }
    beats
}
