# filter

The built-in filter effect. It has one tool, `filter`, which goes in an effect slot of a track like an effect plugin: an `audio` input and an `audio` output, the port names of an effect in `crates/notes`. The arrangement finds it by those ports and knows nothing else of it.

It is the first built-in effect, and the pattern the others follow. ARCHITECTURE.md, "Built-in effects", says what the pattern is.

Enable it in `project.json` under `extensions` as `"filter"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and starting points for sounds. The runtime writes it into every project as `agent-docs/filter.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples, so it is the single source for the format.

The filter owns no children, so an instance is one file: `<name>.json` in a track folder, named in the track's `effects`. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field, a value out of range or a slope that is not 12 or 24 does not load. The instance then keeps its last valid state, and the problem names the file and the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `cutoff_hz` | Cutoff | 20 to 20000 | 1000 | logarithmic | `632 Hz`, `1.2 kHz` |
| `resonance` | Resonance | 0 to 1 | 0.2 | linear | `20%` |
| `drive_db` | Drive | 0 to 24 | 0 | linear | `6 dB` |
| `mix` | Mix | 0 to 1 | 1 | linear | `100%` |
| `lfo_rate_hz` | LFO rate | 0.05 to 20 | 1 | logarithmic | `0.5 Hz`, `2 Hz` |
| `lfo_depth_octaves` | LFO depth | 0 to 4 | 0 | linear | `1.5 oct` |

`type` (`low_pass`, `band_pass`, `high_pass`, `notch`) and `slope` (`12`, `24`) are enums, so a wrong one cannot be written in code; in a file it is a problem that names the field. The numbers are `Parameter` constants next to `FilterState` (`sound_core::Parameter`), and `validate`, `Default`, the knobs, the reset and a test of both docs read them.

## How it sounds

- A drive stage, then two sections of the state variable filter in its trapezoidal form (Simper, 2013), then the mix of the filtered and the dry sound. Stereo: each channel has its own memory and the same settings.
- At 12 dB per octave one section plays. At 24 dB the second follows it, and at resonance 0 the two are the sections of a fourth order Butterworth filter. So for both slopes, low and high pass are 3 dB down at the cutoff and flat before it.
- Resonance is in the first section only: it raises its Q from its Butterworth value to 20 on a ratio. The second section has a fixed Q of 0.54, which never boosts, so a change of slope at high resonance glides between the two peaks and never past them. As the Q rises, the low and high pass lose level by `sqrt(q0 / q)`, as an analog ladder loses its bass, so the peak grows by half as many dB as the Q and in a straight line. At the cutoff a low or high pass is -3.01, +4.25 and +11.51 dB at resonance 0, 0.5 and 1 at 12 dB per octave, and -3.01, +2.91 and +8.84 dB at 24; the passband at resonance 1 is 14.5 dB down. The band pass keeps its peak at 0 dB and gets narrower; the notch gets narrower.
- The filter is linear and stable at every cutoff and resonance, also while they move. It never oscillates by itself and nothing blows up. Input louder than +36 dBFS is held there and a sample that is not a number is taken as silence, so nothing that comes in can make it infinite.
- Drive is a gain into one fixed curve: exactly clean up to full scale, then a `tanh` bend that never passes 1.5. So drive 0 leaves every sound up to full scale as it is, drive raises quiet sounds by its gain, and loud ones bend. The curve does not move with the knob, so turning drive up never makes the sound louder on the way than where it ends. `response` leaves drive out; for a quiet sound it is its gain.
- The LFO is a sine on the cutoff in octaves, `lfo_depth_octaves` each way at `lfo_rate_hz`. Its phase starts at 0 when the filter is made, so a render, which makes the filter new, is the same every time. It runs on through stops and seeks and does not start again on play, so two plays in one session may start the wobble at different places. It moves the cutoff between 20 Hz and 20 kHz, and under 45 % of the sample rate.
- Every change glides over 20 ms (`RAMP_SECONDS`): the cutoff in octaves, resonance, drive, mix, LFO depth, and also the type and the slope. Each section makes low, band and high pass from one memory, so a type is a weight of each and a new type is a glide of the weights. Both sections always run, so a new slope is a glide from the first to the second. Nothing is a switch, and no edit clicks.
- The factors of the filter are worked out once per 16 frames while the cutoff, the resonance, the slope or the LFO moves, and not at all while they rest.
- No latency: `Processor::latency` stays 0.
- When the input is silent and the filter has rung out, it does no work and its output is silent.
- Nothing allocates, locks or makes a system call in `process`; the tests run under the realtime sanitizer.

`response(state, hz, sample_rate)` is the exact gain of the processor at a frequency once every change has arrived, without drive. The card draws it, and the tests hold the measured sound to it.

## The card

`view::register(views, devices)` registers `FilterView` as the card of `filter` (`Views::register_card`) and says the card of a filter is called `Filter`. The runtime offers `Filter` in the control that adds an effect.

The rack gives the view a `CardFrame`, the picker of the slot as the title and the close icon, and the view draws the whole card, 352 pt wide as DESIGN.md gives it, with its expand icon:

- The display, 200 pt: the response curve from 20 Hz to 20 kHz, -36 to +18 dB, with the scale `100 · 1k · 10k` under it. The type as segments at its top. One handle at the cutoff: sideways is cutoff, up and down is resonance, placed so that it sits on the peak of a low or high pass.
- Cutoff and Drive, Resonance and Mix, as knobs in two columns.
- Behind expand: Slope as segments, then LFO rate and LFO depth.

Editing, the same rules as every control on saved state (`sound_ui::ControlEdit`):

- A drag of a knob is one gesture and one undo step: "Change cutoff", "Change resonance", "Change drive", "Change mix", "Change LFO rate", "Change LFO depth". A drag of the handle is one step for both of its values, "Change cutoff and resonance". The file is written once, at the end. Escape cancels.
- A click on a type or a slope is one step, "Change filter type" or "Change slope". A double click or backspace on a knob sets its default, a double click on the handle sets both of its.
- The view keeps no copy of the state, so an outside edit shows at once, also during a drag. Whether the card is expanded is the view's own interface state and is not saved.

## Checks

```sh
cargo nextest run -p filter
RTSAN_ENABLE=1 cargo nextest run -p filter                               # with the realtime sanitizer
cargo nextest run -p runtime --test projects filter                      # in a real project: files, reopen, renders
cargo nextest run -p runtime --test window filter                        # the card, with a simulated mouse and keys
cargo nextest run -p filter --run-ignored only realtime_ratio --no-capture   # speed of 100 filters
```

Measured September 26, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline, while other builds ran: 100 filters at 24 dB per octave, each fed by its own noise source, render 7.7 times faster than realtime, and 7.2 times with the LFO of every one moving. The measured response is within 0.0014 dB of `response` at every octave from 125 Hz to 8 kHz for every type, slope and resonance of the test.
