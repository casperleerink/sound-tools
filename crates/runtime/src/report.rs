//! The report of `--analyze`: which rows it has and where they start, the table of what was
//! measured in them, the notes and the pitch moment by moment.

use sound_core::{Clock, Frames, Ticks, TimeSignatures};

use sound_media::analysis::{BANDS, Events, MeasuredPitch, Measures, Pitch, nearest_note};

/// At most this many rows: enough to see the shape of a piece, few enough to read at once.
const MOST_ROWS: u64 = 32;

/// What was measured, and how a place in it is told.
pub enum Timeline {
    /// A render of a project from `from` to `to`, with the tail after `to` when `tail`.
    Project {
        clock: Clock,
        from: Ticks,
        to: Ticks,
        tail: bool,
    },
    /// An audio file from `from` to `to` seconds.
    File {
        sample_rate: u32,
        from: f64,
        to: f64,
    },
}

/// How far apart the rows of a project are.
#[derive(Clone, Copy, Debug)]
enum Step {
    Ticks(u64),
    Bars(usize),
}

/// One row: where it starts, in frames from the start of what was measured, and the cells that
/// tell where that is.
struct Row {
    start: u64,
    at: Vec<String>,
}

impl Timeline {
    /// Where each row starts, in frames from the start, for [`sound_media::analysis::Meter::new`].
    pub fn row_starts(&self) -> Vec<u64> {
        self.rows().1.iter().map(|row| row.start).collect()
    }

    /// The rows of the same size that cover the span in at most [`MOST_ROWS`], with the name of
    /// that size. A project counts in parts of beats or in bars, sized by the time signature at
    /// `from`; a file in seconds.
    fn rows(&self) -> (String, Vec<Row>) {
        match self {
            Self::Project {
                clock,
                from,
                to,
                tail,
            } => {
                let signatures = clock.tempo_map().time_signatures();
                let signature = signatures.bar_at(*from).signature;
                let (beat, bar) = (signature.ticks_per_beat(), signature.ticks_per_bar());
                let parts = [
                    (
                        beat / 4,
                        Step::Ticks(beat / 4),
                        "a quarter beat".to_string(),
                    ),
                    (beat / 2, Step::Ticks(beat / 2), "half a beat".to_string()),
                    (beat, Step::Ticks(beat), "a beat".to_string()),
                    (bar, Step::Bars(1), "a bar".to_string()),
                ];
                let bars = (1..).map_while(|power| {
                    let count = 1_usize.checked_shl(power)?;
                    let size = bar.checked_mul(u64::try_from(count).ok()?)?;
                    Some((size, Step::Bars(count), format!("{count} bars")))
                });
                let span = to.0 - from.0;
                // The size is a guess from the bar at `from`: later bars may be shorter, and a
                // start off the grid makes a row more. So the rows are counted.
                let fits = |(size, step, name): (u64, Step, String)| {
                    if span.div_ceil(size.max(1)) > MOST_ROWS {
                        return None;
                    }
                    let ticks = row_ticks(signatures, *from, *to, step, size);
                    (ticks.len() as u64 <= MOST_ROWS).then_some((name, ticks))
                };
                let (name, ticks) = parts
                    .into_iter()
                    .chain(bars)
                    .find_map(fits)
                    .unwrap_or(("the whole".to_string(), vec![*from]));
                let start = clock.frame_of(*from).0;
                let mut rows: Vec<Row> = ticks
                    .into_iter()
                    .map(|tick| Row {
                        start: clock.frame_of(tick).0 - start,
                        at: project_cells(clock, tick).to_vec(),
                    })
                    .collect();
                if *tail {
                    let seconds = clock.seconds_of(*to);
                    rows.push(Row {
                        start: clock.frame_of(*to).0 - start,
                        at: vec!["tail".to_string(), String::new(), time(seconds)],
                    });
                }
                (format!("rows of {name}"), rows)
            }
            Self::File {
                sample_rate,
                from,
                to,
            } => {
                // In tenths of a second, so that the sizes are whole numbers.
                let parts = [1, 2, 5, 10, 20, 50, 100, 150, 300, 600];
                let longer = (1..).map(|power| 600 << power);
                let span = ((to - from) * 10.0).ceil() as u64;
                let sizes = parts.into_iter().chain(longer).map(|size| (size, ()));
                let size = shortest(sizes, span).map_or(span, |(size, ())| size);
                let rows = (0..span.max(1))
                    .step_by(size.max(1) as usize)
                    .map(|tenths| {
                        let offset = tenths as f64 / 10.0;
                        Row {
                            start: (offset * f64::from(*sample_rate)).round() as u64,
                            at: vec![time(from + offset)],
                        }
                    });
                (format!("rows of {} s", size as f64 / 10.0), rows.collect())
            }
        }
    }

    fn headings(&self) -> Vec<&'static str> {
        match self {
            Self::Project { .. } => vec!["tick", "bar", "time"],
            Self::File { .. } => vec!["time"],
        }
    }

    /// Where `frame`, from the start of what was measured, is.
    fn place(&self, frame: u64) -> String {
        match self.cells(frame).as_slice() {
            [tick, bar, time] => format!("{time} (bar {bar}, tick {tick})"),
            cells => cells.join(" "),
        }
    }

    /// The cells of [`Self::headings`] that tell where `frame` is.
    fn cells(&self, frame: u64) -> Vec<String> {
        match self {
            Self::Project { clock, from, .. } => {
                let tick = clock.tick_at(Frames(clock.frame_of(*from).0 + frame));
                project_cells(clock, tick).to_vec()
            }
            Self::File { from, .. } => vec![time(from + self.seconds(frame))],
        }
    }

    /// `frames` in seconds.
    fn seconds(&self, frames: u64) -> f64 {
        let rate = match self {
            Self::Project { clock, .. } => clock.sample_rate(),
            Self::File { sample_rate, .. } => *sample_rate,
        };
        frames as f64 / f64::from(rate)
    }
}

/// Where the rows of a project start: at `from`, then every `step` on the grid of the bar that
/// `from` is in, or on every `count`th bar line. A first row shorter than half a row joins the
/// next, so that it is long enough to measure.
fn row_ticks(
    signatures: &TimeSignatures,
    from: Ticks,
    to: Ticks,
    step: Step,
    size: u64,
) -> Vec<Ticks> {
    let later: Vec<Ticks> = match step {
        Step::Ticks(ticks) => {
            let ticks = ticks.max(1);
            let bar = signatures.bar_at(from).start.0;
            let first = bar + (from.0 - bar).div_ceil(ticks) * ticks;
            (first..to.0).step_by(ticks as usize).map(Ticks).collect()
        }
        Step::Bars(count) => {
            let lines = signatures.bars_from(from).skip(count).step_by(count);
            lines
                .map(|bar| bar.start)
                .take_while(|tick| *tick < to)
                .collect()
        }
    };
    let later = later
        .into_iter()
        .skip_while(|tick| tick.0 < from.0 + size / 2);
    std::iter::once(from).chain(later).collect()
}

/// The tick, the bar and the time of `tick`: the cells of a row of a project.
fn project_cells(clock: &Clock, tick: Ticks) -> [String; 3] {
    let bar = clock.tempo_map().time_signatures().bar_beat_of(tick);
    [
        tick.0.to_string(),
        bar.to_string(),
        time(clock.seconds_of(tick)),
    ]
}

/// The first of `sizes` that covers `span` in at most [`MOST_ROWS`] rows.
fn shortest<Name>(sizes: impl Iterator<Item = (u64, Name)>, span: u64) -> Option<(u64, Name)> {
    let mut sizes = sizes.filter(|(size, _)| *size > 0);
    sizes.find(|(size, _)| span.div_ceil(*size) <= MOST_ROWS)
}

fn value(value: Option<f64>) -> String {
    value.map_or("-".to_string(), |value| format!("{value:.1}"))
}

/// Writes the whole report to a new file in the temporary folder and gives what to print: the
/// lines of the whole, where the file is, and the line each part of it starts on. The file can
/// be long, so an agent reads only the part it needs.
pub fn write(timeline: &Timeline, measures: &Measures) -> anyhow::Result<String> {
    let mut summary = vec![whole(timeline, measures)];
    if let Some(frame) = measures.not_a_number {
        summary.push(format!(
            "samples that are not a number from {}, measured as silence",
            timeline.place(frame)
        ));
    }
    summary.push(events(timeline, &measures.events));
    let mut file = summary.join("\n");
    let mut starts = Vec::new();
    for (heading, table) in parts(timeline, measures) {
        file.push_str("\n\n");
        starts.push(format!("  line {}: {heading}", file.lines().count() + 1));
        file.push_str(&format!("{heading}\n{table}"));
    }
    file.push('\n');
    let (mut written, path) = tempfile::Builder::new()
        .prefix("sound-tools-analysis-")
        .suffix(".txt")
        .tempfile()?
        .keep()?;
    std::io::Write::write_all(&mut written, file.as_bytes())?;
    summary.push(format!("the whole report is in {}:", path.display()));
    summary.extend(starts);
    Ok(summary.join("\n"))
}

/// The line of the whole: loudness, peak and pitch.
fn whole(timeline: &Timeline, measures: &Measures) -> String {
    format!(
        "loudness {} LUFS, loudest 400 ms {} LUFS, true peak {} dBTP{}, {}",
        value(measures.integrated),
        value(measures.max_momentary),
        value(measures.true_peak.map(|(peak, _)| peak)),
        measures
            .true_peak
            .map_or(String::new(), |(_, frame)| format!(
                " at {}",
                timeline.place(frame)
            )),
        measures
            .pitch
            .map_or("no single pitch".to_string(), |pitch| format!(
                "pitch {}, drift {}",
                name(pitch.note),
                drift(pitch)
            )),
    )
}

/// The parts of the file after the lines of the whole, each a heading and a table.
fn parts(timeline: &Timeline, measures: &Measures) -> Vec<(String, String)> {
    let (size, rows) = rows(timeline, measures);
    let mut parts = vec![(size, rows)];
    if !measures.events.notes.is_empty() {
        let (phrases, notes) = notes(timeline, &measures.events);
        parts.push(("phrases".to_string(), phrases));
        parts.push(("notes".to_string(), notes));
    }
    parts.push((
        "pitch every 10 ms".to_string(),
        pitch_by_moment(timeline, measures),
    ));
    parts
}

/// The size of the rows, and what each measures.
fn rows(timeline: &Timeline, measures: &Measures) -> (String, String) {
    let (size, rows) = timeline.rows();

    let mut after = vec!["LUFS", "max", "peak"];
    after.extend(BANDS.map(|(name, _)| name));
    after.extend(["width", "pitch", "drift"]);
    let mut table = vec![headings(timeline, &[], &after)];
    for (row, measured) in rows.into_iter().zip(&measures.rows) {
        let mut cells = row.at;
        cells.extend([
            value(measured.loudness),
            value(measured.max_momentary),
            value(measured.true_peak),
        ]);
        cells.extend(measured.bands.map(value));
        cells.push(
            measured
                .width
                .map_or("-".to_string(), |width| format!("{width:.0}%")),
        );
        cells.push(
            measured
                .pitch
                .map_or("-".to_string(), |pitch| name(pitch.note)),
        );
        cells.push(measured.pitch.map_or("-".to_string(), drift));
        table.push(cells);
    }
    (size, aligned(&table))
}

/// The tempos, from 50 to 200 BPM, at which notes `gap` seconds apart are a beat or a part of
/// one. The composer knows which it is.
const TEMPO_RANGE: std::ops::RangeInclusive<f64> = 50.0..=200.0;

/// What the notes have in common.
fn events(timeline: &Timeline, events: &Events) -> String {
    let tuning = events.tuning.map_or("-".to_string(), |tuning| {
        let (cents, spread) = (tuning.cents.round() as i64, tuning.spread.round() as i64);
        format!("{cents:+}c ±{spread}c")
    });
    let mut summary = format!(
        "{} notes in {} phrases, tuning {tuning}",
        events.notes.len(),
        events.phrases.len()
    );
    if let Some(gap) = events.gap {
        let gap = timeline.seconds(gap);
        let parts = [
            (1.0, "a beat"),
            (2.0, "half a beat"),
            (4.0, "a quarter beat"),
        ];
        let tempos: Vec<String> = parts
            .into_iter()
            .map(|(parts, name)| (60.0 / (gap * parts), name))
            .filter(|(bpm, _)| TEMPO_RANGE.contains(bpm))
            .map(|(bpm, name)| format!("{name} at {bpm:.0} BPM"))
            .collect();
        summary.push_str(&format!(", starts a median {gap:.3} s apart"));
        if !tempos.is_empty() {
            summary.push_str(&format!(": {}", tempos.join(", ")));
        }
    }
    summary
}

/// The table of the phrases and the table of the notes.
fn notes(timeline: &Timeline, events: &Events) -> (String, String) {
    let seconds = |frames: u64| format!("{:.2}", timeline.seconds(frames));
    let mut table = vec![headings(timeline, &["phrase"], &["length", "notes"])];
    for (number, phrase) in events.phrases.iter().enumerate() {
        let mut cells = vec![(number + 1).to_string()];
        cells.extend(timeline.cells(phrase.start));
        cells.extend([seconds(phrase.end - phrase.start), phrase.notes.to_string()]);
        table.push(cells);
    }
    let phrases = aligned(&table);
    let mut table = vec![headings(
        timeline,
        &[],
        &["length", "level", "pitch", "glide"],
    )];
    for note in &events.notes {
        let mut cells = timeline.cells(note.start);
        cells.extend([
            seconds(note.end - note.start),
            format!("{:.1}", note.level),
            note.pitch.map_or("-".to_string(), |pitch| name(pitch.note)),
            note.pitch.map_or("-".to_string(), |pitch| {
                format!("{:+}c", pitch.glide.round() as i64)
            }),
        ]);
        table.push(cells);
    }
    (phrases, aligned(&table))
}

/// The level and pitch of every moment that is not silence.
fn pitch_by_moment(timeline: &Timeline, measures: &Measures) -> String {
    let mut table = vec![headings(timeline, &[], &["level", "pitch"])];
    for moment in &measures.moments {
        let Some(level) = moment.level else {
            continue;
        };
        let mut cells = timeline.cells(moment.frame);
        cells.push(format!("{level:.1}"));
        cells.push(match moment.pitch {
            Pitch::Note(note) => name(note),
            Pitch::Quiet | Pitch::Unclear => "-".to_string(),
        });
        table.push(cells);
    }
    aligned(&table)
}

/// The headings of a table with `before` and `after` around where its rows are.
fn headings(timeline: &Timeline, before: &[&str], after: &[&str]) -> Vec<String> {
    let at = timeline.headings();
    let headings = before.iter().chain(&at).chain(after);
    headings.map(|heading| heading.to_string()).collect()
}

/// The nearest note to a MIDI note number with its fraction, and how far off it the note is, in
/// cents: `A4+3c`.
fn name(note: f64) -> String {
    let (nearest, cents) = nearest_note(note);
    // The pitch finder looks from 40 Hz to 4 kHz, well inside the notes MIDI has.
    let name = sound_notes::Pitch::nearest(nearest).name();
    format!("{name}{cents:+}c")
}

fn drift(pitch: MeasuredPitch) -> String {
    format!("{:.0}c", pitch.drift)
}

/// The cells as columns, each as wide as its widest cell, numbers to the right.
fn aligned(table: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = Vec::new();
    for row in table {
        widths.resize(widths.len().max(row.len()), 0);
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let lines = table.iter().map(|row| {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:>width$}"))
            .collect();
        cells.join("  ")
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// Seconds as minutes and seconds to a hundredth, as a playhead shows them: `1:23.45`.
pub fn time(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).round() as u64;
    let (minutes, rest) = (hundredths / 6_000, hundredths % 6_000);
    format!("{minutes}:{:02}.{:02}", rest / 100, rest % 100)
}

#[cfg(test)]
mod tests {
    use sound_core::{SignatureRun, Tempo, TempoMap, TimeSignature, TimeSignatures};

    use super::*;

    fn clock(bpm: f64) -> Clock {
        let tempo_map = TempoMap::constant(TimeSignature::default(), Tempo::from_bpm(bpm).unwrap());
        Clock::new(tempo_map, 48_000)
    }

    fn rows(timeline: &Timeline) -> (String, Vec<Vec<String>>) {
        let (size, rows) = timeline.rows();
        (size, rows.into_iter().map(|row| row.at).collect())
    }

    #[test]
    fn a_long_span_has_rows_of_bars_and_a_short_one_rows_of_beats() {
        let project = |from, to, tail| Timeline::Project {
            clock: clock(120.0),
            from: Ticks(from),
            to: Ticks(to),
            tail,
        };
        // 90 bars in 23 rows of 4 bars, and the tail.
        let (size, cells) = rows(&project(0, 90 * 3840, true));
        assert_eq!(size, "rows of 4 bars");
        assert_eq!(cells.len(), 24);
        assert_eq!(cells[1], ["15360", "5:1:000", "0:08.00"]);
        assert_eq!(cells[23], ["tail", "", "3:00.00"]);
        // Three bars from bar 3, beat 2: parts of beats, from where it starts.
        let (size, cells) = rows(&project(8640, 8640 + 3 * 3840, false));
        assert_eq!(size, "rows of half a beat");
        assert_eq!(cells.len(), 24);
        assert_eq!(cells[0], ["8640", "3:2:000", "0:04.50"]);
        assert_eq!(cells[1], ["9120", "3:2:480", "0:04.75"]);
        // From off the grid: the next row is on it, unless that leaves a first row shorter
        // than half a row.
        let (_, cells) = rows(&project(8700, 8640 + 3 * 3840, false));
        assert_eq!([&cells[0][0], &cells[1][0]], ["8700", "9120"]);
        let (_, cells) = rows(&project(9000, 8640 + 3 * 3840, false));
        assert_eq!([&cells[0][0], &cells[1][0]], ["9000", "9600"]);
        // 32 half beats from off the grid would be 33 rows.
        let (size, cells) = rows(&project(8700, 8700 + 32 * 480, false));
        assert_eq!((size.as_str(), cells.len()), ("rows of a beat", 17));
    }

    #[test]
    fn rows_of_bars_keep_to_the_bar_lines_after_a_change_of_time_signature() {
        let runs = [("4/4", 8), ("7/8", 1), ("4/4", 100)].map(|(signature, bars)| SignatureRun {
            signature: signature.parse().unwrap(),
            bars: std::num::NonZeroU32::new(bars).unwrap(),
        });
        let signatures = TimeSignatures::new(runs.to_vec()).unwrap();
        let tempo_map = TempoMap::constant(signatures, Tempo::from_bpm(120.0).unwrap());
        let timeline = Timeline::Project {
            clock: Clock::new(tempo_map, 48_000),
            from: Ticks(0),
            to: Ticks(8 * 3840 + 3360 + 31 * 3840),
            tail: false,
        };
        let (size, cells) = rows(&timeline);
        assert_eq!(size, "rows of 2 bars");
        let bars: Vec<&str> = cells.iter().map(|cells| cells[1].as_str()).collect();
        assert_eq!(
            bars[..6],
            [
                "1:1:000", "3:1:000", "5:1:000", "7:1:000", "9:1:000", "11:1:000"
            ]
        );
        assert_eq!(bars.len(), 20);

        // Bars that get shorter than the first: still at most 32 rows.
        let runs = [("4/4", 1), ("2/4", 100)].map(|(signature, bars)| SignatureRun {
            signature: signature.parse().unwrap(),
            bars: std::num::NonZeroU32::new(bars).unwrap(),
        });
        let signatures = TimeSignatures::new(runs.to_vec()).unwrap();
        let tempo_map = TempoMap::constant(signatures, Tempo::from_bpm(120.0).unwrap());
        let timeline = Timeline::Project {
            clock: Clock::new(tempo_map, 48_000),
            from: Ticks(0),
            to: Ticks(3840 + 60 * 1920),
            tail: false,
        };
        let (size, cells) = rows(&timeline);
        assert_eq!((size.as_str(), cells.len()), ("rows of 2 bars", 31));
    }

    #[test]
    fn a_file_has_rows_of_seconds() {
        let file = |from, to| Timeline::File {
            sample_rate: 44_100,
            from,
            to,
        };
        let (size, cells) = rows(&file(0.0, 180.0));
        assert_eq!(size, "rows of 10 s");
        assert_eq!(cells.len(), 18);
        let (size, cells) = rows(&file(60.0, 61.0));
        assert_eq!(size, "rows of 0.1 s");
        assert_eq!(cells[3], ["1:00.30"]);
        let starts = file(60.0, 61.0).row_starts();
        assert_eq!(starts[3], 13_230);
    }

    #[test]
    fn the_printed_lines_say_where_each_part_of_the_file_starts() {
        // Two plucks of A4.
        let samples: Vec<f32> = (0..48_000)
            .flat_map(|frame| {
                let time = f64::from(frame % 24_000) / 48_000.0;
                let value = (std::f64::consts::TAU * 440.0 * time).sin() * (-8.0 * time).exp();
                [value as f32 * 0.5; 2]
            })
            .collect();
        let timeline = Timeline::File {
            sample_rate: 48_000,
            from: 0.0,
            to: 1.0,
        };
        let mut meter = sound_media::analysis::Meter::new(48_000, timeline.row_starts());
        meter.push(&samples);
        let printed = write(&timeline, &meter.finish()).unwrap();
        let path = printed
            .lines()
            .find_map(|line| line.strip_prefix("the whole report is in "))
            .and_then(|path| path.strip_suffix(':'))
            .unwrap();
        let file = std::fs::read_to_string(path).unwrap();
        std::fs::remove_file(path).unwrap();
        let file: Vec<&str> = file.lines().collect();
        let starts: Vec<(usize, &str)> = printed
            .lines()
            .filter_map(|line| line.strip_prefix("  line ")?.split_once(": "))
            .map(|(number, heading)| (number.parse().unwrap(), heading))
            .collect();
        let headings: Vec<&str> = starts.iter().map(|(_, heading)| *heading).collect();
        assert_eq!(
            headings,
            ["rows of 0.1 s", "phrases", "notes", "pitch every 10 ms"]
        );
        for (number, heading) in starts {
            assert_eq!(file[number - 1], heading);
        }
        assert!(file[1].starts_with("2 notes in 1 phrases"), "{}", file[1]);
    }

    #[test]
    fn time_reads_as_a_playhead() {
        assert_eq!(time(83.456), "1:23.46");
        assert_eq!(time(59.999), "1:00.00");
        assert_eq!(time(0.0), "0:00.00");
    }
}
