# eq

The built-in EQ effect. It has one tool, `eq`, which goes in an effect slot of a track like an effect plugin: an `audio` input and an `audio` output, the port names of an effect in `crates/notes`. The arrangement finds it by those ports and knows nothing else of it.

It follows the pattern of the Filter, see ARCHITECTURE.md, "Built-in effects".

Enable it in `project.json` under `extensions` as `"eq"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and starting points. The runtime writes it into every project as `agent-docs/eq.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples, so it is the single source for the format.

The EQ owns no children, so an instance is one file: `<name>.json` in a track folder, named in the track's `effects`. `bands` is a list of at most four bands, band 1 first. A band or a field you leave out takes the default of that band. The runtime writes every field of every band when it saves. An unknown field, a value out of range, an unknown shape or a fifth band does not load. The instance then keeps its last valid state, and the problem names the file, the band and the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `shape` | Shape | a select | `"low_shelf"`, `"bell"`, `"bell"`, `"high_shelf"` | | `Bell` |
| `frequency_hz` | Freq | 20 to 20000 | 100, 400, 2000, 8000 | logarithmic | `900 Hz`, `1.2 kHz` |
| `gain_db` | Gain | -15 to 15 | 0 | linear, from the middle | `-4.5 dB` |
| `q` | Q | 0.1 to 18 | 0.71 | logarithmic | `0.71`, `2` |
| `output_gain_db` | Output | -12 to 12 | 0 | linear, from the middle | `0 dB` |

`on` is `true` or `false`, `true` by default. `shape` (`low_cut`, `low_shelf`, `bell`, `notch`, `high_shelf`, `high_cut`) is an enum, so a wrong one cannot be written in code. The numbers are `Parameter` constants next to `EqState` (`sound_core::Parameter`): `FREQUENCIES`, one per band because each band starts at its own frequency, `GAIN`, `Q` and `OUTPUT_GAIN`. `validate`, `Default`, the knobs, the reset and a test of both docs read them.

Four bands, not eight: DESIGN.md draws four, a cut and two or three moves are what most tracks need, four numbered handles stay apart on the 312 pt display of a laptop, and the file stays short for an agent. A list with fewer bands loads, so more bands later would not break a saved project.

## How it sounds

- Four bands one after the other, then the output gain. Stereo: each channel has its own memory and the same settings.
- Each band is one state variable filter in its trapezoidal form (Simper, 2013), the same as a section of the Filter, and every shape is a mix of its input, band pass and low pass with its own factors (Simper, 2016). A bell is exactly its gain at its frequency, a shelf is half of its gain there and all of it far on its side, a cut is 3 dB down at its frequency at Q 0.71 and falls 12 dB per octave, and a notch takes its frequency out completely. The frequencies are bent the way the trapezoidal form bends them, so a bell or a shelf is where it is set up to the top of the range, and narrower than an analog one close to the Nyquist frequency.
- A band at 0 dB, or off, is the input, exactly: the default EQ changes no sample.
- The EQ is linear and stable at every frequency, gain and Q, also while they move. Input louder than +36 dBFS is held there and a sample that is not a number is taken as silence, so nothing that comes in can make it infinite. The frequency stays under 45 % of the sample rate: 19.8 kHz at 44.1 kHz.
- Every change glides over 20 ms (`RAMP_SECONDS`): the frequency in octaves, the gain in dB, the Q on a ratio, the output gain, and also the shape and on or off. A band makes every shape from one memory, so a new shape is a glide of the weight of each: between two shapes a band takes the mean of their factors, on a log scale for its cutoff and damping. On and off is a glide towards the input. Nothing is a switch, and no edit clicks.
- The factors of a band are worked out once per 16 frames while it moves, and not at all while it rests.
- No latency: `Processor::latency` stays 0.
- When the input is silent and every band has rung out, it does no work and its output is silent.
- Nothing allocates, locks or makes a system call in `process`; the tests run under the realtime sanitizer.

`response(state, hz, sample_rate)` is the exact gain of the processor at a frequency once every change has arrived, and `band_response` that of one band. The card draws it, and the tests hold the measured sound to it.

## The card

`view::register(views, devices)` registers `EqView` as the card of `eq` (`Views::register_card`) and says the card of an EQ is called `EQ`. The runtime offers `EQ` in the control that adds an effect.

The rack gives the view a `CardFrame`, the picker of the slot as the title and the close icon, and the view draws the whole card, 464 pt wide as DESIGN.md gives it, with its expand icon:

- The display, 312 pt: the summed curve from 20 Hz to 20 kHz, -18 to +18 dB, with the scale `100 · 1k · 10k` under it. A numbered handle per band: sideways is frequency, up and down is gain. A cut or a notch has no gain, so its handle sits on the 0 dB line and moves only sideways. The selected band has a full dot, the others a ring, and a band that is off is dimmed.
- A press on a handle selects its band. The keys 1 to 4 do too, from any control of the card that has the focus.
- Freq and Q, Gain and Shape of the selected band, in two columns. The line under Shape says which band it is. Gain is dimmed for a cut or a notch.
- Behind expand: the on and off of each band, then Output.

Editing, the same rules as every control on saved state (`sound_ui::ControlEdit`):

- A drag of a knob is one gesture and one undo step: "Change frequency", "Change gain", "Change Q", "Change output". A drag of a handle is one step for both of its values, "Change frequency and gain", or "Change frequency" for a cut or a notch. The file is written once, at the end. Escape cancels.
- A shape is one step, "Change shape". On and off is one step, "Turn band on" or "Turn band off". A double click or backspace on a knob sets its default, a double click on a handle sets both of its; the default frequency is that of the band.
- The view keeps no copy of the state, so an outside edit shows at once, also during a drag. Which band is selected and whether the card is expanded are the view's own interface state and are not saved.

## Checks

```sh
cargo nextest run -p eq
RTSAN_ENABLE=1 cargo nextest run -p eq                               # with the realtime sanitizer
cargo nextest run -p runtime --test projects eq                      # in a real project: files, reopen, renders
cargo nextest run -p runtime --test window eq                        # the card, with a simulated mouse and keys
cargo nextest run -p eq --run-ignored only realtime_ratio --no-capture   # speed of 100 EQs
```
