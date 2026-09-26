# compressor

The built-in compressor effect. It has one tool, `compressor`, which goes in an effect slot of a track like an effect plugin: an `audio` input and an `audio` output, the port names of an effect in `crates/notes`. The arrangement finds it by those ports and knows nothing else of it.

It follows the pattern of the built-in effects, which ARCHITECTURE.md, "Built-in effects", describes and the filter shows first.

Enable it in `project.json` under `extensions` as `"compressor"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and starting points for sounds. The runtime writes it into every project as `agent-docs/compressor.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples, so it is the single source for the format.

The compressor owns no children, so an instance is one file: `<name>.json` in a track folder, named in the track's `effects`. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field, a value out of range or a lookahead that is not 0, 1 or 10 does not load. The instance then keeps its last valid state, and the problem names the file and the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `threshold_db` | Threshold | -60 to 0 | -18 | linear | `-18 dB` |
| `ratio` | Ratio | 1 to 100 | 4 | logarithmic | `4:1` |
| `attack_ms` | Attack | 0.1 to 300 | 10 | logarithmic | `10 ms` |
| `release_ms` | Release | 1 to 3000 | 120 | logarithmic | `120 ms`, `1.2 s` |
| `knee_db` | Knee | 0 to 18 | 6 | linear | `6 dB` |
| `makeup_db` | Makeup | 0 to 24 | 0 | linear | `3 dB` |
| `mix` | Mix | 0 to 1 | 1 | linear | `100%` |

`lookahead_ms` (`0`, `1`, `10`) is an enum, so a wrong one cannot be written in code; in a file it is a problem that names the field. The numbers are `Parameter` constants next to `CompressorState` (`sound_core::Parameter`), and `validate`, `Default`, the knobs, the reset and a test of both docs read them.

## How it sounds

- The layout of Giannoulis, Massberg and Reiss (2012): a peak detector, a gain computer on the level in dB, then attack and release on the reduction in dB, then the gain on the sound. Both channels share one level and one gain, so the stereo image stays where it is.
- The level is the largest sample of either channel in the last 10 to 11 ms (`HOLD_SECONDS`). A steady tone from 50 Hz up has the same peak in every such stretch, so its level is still and the gain does not wobble with the wave. The level of a sine is its amplitude, to within 0.02 dB up to 1 kHz at 48 kHz.
- The gain computer: nothing under the knee, `(level - threshold) (1 - 1 / ratio)` dB off above it, and a quadratic bend of `knee_db` width centred on the threshold between, which meets both without a corner. `reduction_db(state, level_db)` is this curve.
- Attack and release are one-pole glides of the reduction in dB: after `attack_ms` a step up is 63 % of the way, after `release_ms` a step down is. The release starts when the loud part has left the 10 ms of the detector, so a release is heard 10 to 11 ms later than its time. Changing attack or release does not glide; it changes how fast the reduction moves, which cannot click.
- Makeup is a gain after the reduction. Mix is `dry + mix (compressed - dry)`, and the dry sound goes through the same lookahead, so the two are in phase and a steady tone's gain is a sum of gains. `static_gain_db(state, level_db)` is the gain of the whole compressor for a steady sound at that peak level once it has settled: the tests hold the measured sound to it.
- Lookahead delays the sound by 0, 1 or 10 ms while the detector hears it undelayed, so the reduction starts before a peak arrives. The delay is `Processor::latency`, so the app starts the tracks before it earlier and everything stays in time. A new lookahead fades from the old delay to the new one over 20 ms; the tracks before it jump once, as for any latency change (ARCHITECTURE.md, "Latency compensation").
- Every other change glides over 20 ms (`RAMP_SECONDS`): threshold, ratio as `1 - 1 / ratio`, knee, makeup and mix. No edit clicks.
- Stable at every setting: the gain is at most +24 dB, input louder than +36 dBFS is held there, a sample that is not a number or smaller than 1e-30 is silence, and the reduction settles to exactly where it goes once it is within 0.0001 dB, so nothing becomes infinite or so small that the processor slows down.
- When the input is silent, the silence has left the detector and the lookahead, and the reduction has let go, it does no work and its output is silent.
- Nothing allocates, locks or makes a system call in `process`; the tests run under the realtime sanitizer.

## The card

`view::register(views, devices)` registers `CompressorView` as the card of `compressor` (`Views::register_card`) and says the card of a compressor is called `Compressor`. The runtime offers `Compressor` in the control that adds an effect.

The rack gives the view a `CardFrame`, the picker of the slot as the title and the close icon, and the view draws the whole card, 288 pt wide as DESIGN.md gives it, with its expand icon:

- The display, 136 pt: the transfer curve, input across and output up, both -60 to 0 dBFS, square so that a ratio of 1 is the diagonal. It is the reduction only; makeup and mix are not in it. A line at the threshold. A handle on the curve at the threshold: sideways is threshold. A hollow handle at its right end, at 0 dBFS in: up and down is ratio. The level now as a green dot on the curve, and the gain reduction as a bar from the top at the right edge, 0 to 24 dB. The line under it: the gain reduction now, `GR -6.8 dB`.
- Threshold and Attack, Ratio and Release, as knobs in two columns.
- Behind expand: Knee and Makeup, then Mix and Lookahead as a select of `0`, `1` and `10`, with `ms` on its value line. Three segments are 75 pt wide and do not fit in a 56 pt cell; a select is what DESIGN.md gives a list that does not fit as segments.

The level and the reduction come from the audio thread without a lock or an allocation: the processor records the largest of each block in two `sound_core::Peaks` (`Meters`), which the behaviour declares as `level` and `reduction`. The view takes them once per poll of the session (`Project::peaks`) and draws again only when the reading moves by a quarter point or a tenth of a dB, so a card at rest asks for no frame.

Editing, the same rules as every control on saved state (`sound_ui::ControlEdit`):

- A drag of a knob or a handle is one gesture and one undo step: "Change threshold", "Change ratio", "Change attack", "Change release", "Change knee", "Change makeup", "Change mix". The file is written once, at the end. Escape cancels.
- A pick in the lookahead select is one step, "Change lookahead". A double click or backspace on a knob sets its default, a double click on a handle sets its value's.
- The view keeps no copy of the state, so an outside edit shows at once, also during a drag. Whether the card is expanded is the view's own interface state and is not saved.

## Checks

```sh
cargo nextest run -p compressor
RTSAN_ENABLE=1 cargo nextest run -p compressor                               # with the realtime sanitizer
cargo nextest run -p runtime --test projects compressor                      # in a real project: files, reopen, renders
cargo nextest run -p runtime --test window compressor                        # the card, with a simulated mouse and keys
cargo nextest run -p compressor --run-ignored only realtime_ratio --no-capture   # speed of 100 compressors
```

Measured September 26, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline, while other builds ran:

- The gain of a steady sine is the static gain within 0.0000 dB (printed to four places) for every threshold, ratio and knee of the test, from -60 to +6 dBFS in, at 50 Hz to 15 kHz. By hand: threshold -20 dB, 4:1, hard knee gives 0, -3, -9 and -15 dB at -20, -16, -8 and 0 dBFS in.
- A step from -60 to 0 dBFS reaches 63 % of its reduction in `attack_ms` plus at most one frame, for 0.1 to 300 ms. Back down, the reduction holds 10.00 ms and then reaches 63 % of the way in `release_ms` plus at most one frame, for 1 ms to 3 s.
- 100 compressors, each fed by its own noise source, with 1 ms of lookahead, render 6.7 times faster than realtime.
