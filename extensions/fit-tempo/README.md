# fit-tempo

Fits the project's tempo map to a freely played take, so the grid follows the playing and the take sounds exactly as it did. Play freely, and the grid comes to you.

One record, `state/fit-tempo.json`, holds the inputs. Everything else is computed from it every time it changes: the tempo map in `project.json`, and the ticks of the notes and the pedal in the take's clip. `agent-doc.md` is what an agent reads; this file is for the people who build on it.

```rust
let mut changes = Changes::new();
fit_tempo::fit_take(project, &mut changes, &clip)?;      // the clip must have a `take`
project.commit(fit_tempo::FIT_LABEL, changes)?;          // one undo step: fit, map and clip

let fit = fit_tempo::fit_of(project);                    // None when the project has no fit
fit_tempo::set_steadiness(project, &mut changes, 0.5);   // 0 as played, 1 one tempo
```

## The record

```json
{"tool": "fit-tempo", "state": {"take": "take-1", "first_downbeat_us": 0, "beat": "normal", "steadiness": 0.0}}
```

- `take`: the raw take under `assets/takes/`, written by a recording. The clip that names the same take in its own `take` field is the one the fit writes into.
- `first_downbeat_us`: which moment of the take is beat 1 of a bar, in microseconds from the start of the recording. The beat nearest to it lands on a bar line. `0` is the first beat found.
- `beat`: `half`, `normal` or `double`.
- `steadiness`: 0 to 1.

The tool sits at the top of `state/` (`Place::Root`). One fit per project: a second record changes nothing and says so in `problems.txt`.

A project made before this step does not list `fit-tempo` in `extensions` in `project.json`, and fitting it fails with `tool "fit-tempo" is not registered, or its extension is not enabled in project.json`. Add `"fit-tempo"` to that list and open the project again. Nothing rewrites it: turning an extension on is the composer's edit, as "Project storage" decides.

## How a change reaches the tempo map

The tool registers a **derive**, which is the core's way for a record to decide state of its own (`ToolRegistration::derive`, see the [core README](../../crates/core/README.md)). The derive runs inside the same state application as the change that asked for it, so a change of the fit record, from the window or from a file an agent wrote, and a change of the project's time signature, rewrite the tempo map and the clip as one group, one engine batch and one undo step. It does not run while the project loads, nor for undo, redo or a cancel: the files and the undo step already hold what it would compute, so a read-only open never writes.

A correction makes the clip's notes again from the raw take, so it is exact however many times it is corrected. **Hand edits to that clip made before a correction are lost.** One undo brings them back with the rest of the step.

Steadiness never touches the clip. It only rewrites the tempo map, so going back to 0 gives the fitted map again, byte for byte.

## The beat finder, and what it gets wrong

`src/beats.rs`. A small version of the dynamic-programming beat tracker Daniel Ellis described in 2007, on the exact note times of a MIDI take instead of on an audio onset envelope, with the beat period measured again every second so that a piece that slows down is followed and not resisted. Plain Rust, no model file, no dependency beyond the note contract, and the same bytes for the same take on every run.

1. **Onsets.** Note ons within 40 ms are one onset at their mean time. A chord weighs more than one note, and an onset whose lowest note is below the middle of the take weighs up to three times as much: a piano player's left hand marks the beat, and without that, playing in which every beat is subdivided evenly has nothing to tell the beat from the notes between them.
2. **The period.** A comb over the autocorrelation of an onset envelope at 200 frames a second, with a log-normal prior around 120 bpm. The same comb over a five-second window every second gives the period at each moment.
3. **The beats.** One pass of dynamic programming: the best beat sequence lands on strong onsets and keeps its steps near the local period, at a cost of 20 times the square of the log of how far a step is from it.
4. **Filling and snapping.** A step that is a whole multiple of the steps around it gets the beats it is missing. Each beat then moves to the onset within 30 ms of it, so a beat is on the note the hand played and not on the 5 ms grid the search used.

Measured against generated takes with a known tempo curve (`tests/`), sixteen bars each with the timing jitter of a hand:

| Kind of playing | Beats within 30 ms | Worst |
| --- | --- | --- |
| Steady 4/4, 3/4 and 6/8, chords on the beat | all | 12 ms |
| Slow rubato, ±12 % | all | 14 ms |
| Ritardando, 110 to 70 bpm | all | 12 ms |
| Syncopated, with silent beats | all | 15 ms |
| Arpeggiated, four notes a beat | all | 20 ms |
| A loose hand, ±35 ms of jitter | all | 29 ms |
| A sudden tempo change, 92 to 128 bpm | all but one | 182 ms |

What it cannot know from the timing alone is the octave, where a bar begins and how many beats a bar has. Those are the three fields of the record, and correcting them is the agent's part. It is also weaker at a sudden change of tempo, where the beat at the change lands about a fifth of a beat out because the period is measured over a window that holds both tempos, and it needs at least eight chords or notes to find anything at all.

## The grid

`src/grid.rs`.

- One tempo step per beat. A beat that the step before it already lands right shares that step, so a run of evenly spaced beats is one step.
- The first downbeat lands on a bar line. The bars before it hold the beats played before it, the pickup, plus the silence in front of the take: that silence gets as many bars as fit in it at the tempo the take begins with, and never fewer than the pickup needs. So a composer who presses record and then waits gets bars of about the right length in front of the playing, not one bar stretched over the wait.
- A take that begins the moment recording starts, with no silence at all, has nowhere to put the bar in front of its first downbeat. The fit still follows the playing and says so in `problems.txt`: "4 of 91 beats are too far apart or too close together for a tempo between 10 and 1000 bpm". Leave a beat of silence before playing, or move `first_downbeat_us` later.
- Every tempo is chosen against the frame the beat has to land on, not against the length of the beat before it, so the rounding of a tempo to 0.001 bpm does not add up over a long take. Measured over 1520 beats: every beat within two frames, 42 µs, of where it belongs.
- The frames are simulated exactly as `Clock` computes them, at `FIT_SAMPLE_RATE` (48 kHz), because a tempo map is saved without a sample rate and building it against the device of the moment would make one fit two different files on two machines. At another sample rate the clock rounds each tempo change down to a whole frame, so a map with many steps drifts slowly against the take. Fitting again on that machine takes it away.
- Steadiness moves every beat towards even spacing between the first and the last. At 100 % every beat is the same length to within four frames, 83 µs, and every step of the map holds the same tempo to within a hundredth of a bpm.

## Costs

Measured on an Apple Silicon laptop, dev profile, on a take of ten minutes: 4560 notes, 1520 beats.

- The fit itself takes 0.74 s, on the thread that draws. It is paid once per fit and once per correction; a steadiness change costs nothing, because the beats are remembered.
- `project.json` grows from 210 bytes to 60 kB.
- One mouse move of a steadiness drag takes 1.6 ms, which is inside a display frame.
- One tick to frame costs 3.2 ns with one tempo change and 22.2 ns with 1520: a lookup is a binary search, so it grows with the logarithm of the number of changes and not with the number.
- On the real device at 44.1 kHz, playing that project: 0 xruns, 0 late callbacks, slowest callback 137 µs, against 109 µs for the same project with one tempo change.
- The click needs nothing of a fit: it holds no tempo map and asks the transport which ticks each block covers, so it lands on the beats of a fitted grid like any other. `the_click_follows_a_fitted_grid` holds that, within one frame.

## Checks

```sh
cargo nextest run -p fit-tempo
cargo nextest run -p fit-tempo --run-ignored only --no-capture   # the bound, per kind of playing
cargo nextest run -p runtime -E 'test(fit::)'                    # the whole path, with undo
cargo nextest run -p runtime --run-ignored only -E 'test(a_long_take)' --no-capture
```

## Not built

Tempo ramps; time signature changes inside a piece; steadiness per section or per bar; more than one fit per project; fitting a take that is not the first in the project to name its file; a way to remove a fit other than undo or deleting the record; fitting audio; a tempo lane or any view of the fitted grid beyond the tempo the transport shows; and a fit that runs off the thread that draws.
