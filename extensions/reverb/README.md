# reverb

The built-in reverb effect. It has one tool, `reverb`, which goes in an effect slot of a track like an effect plugin: an `audio` input and an `audio` output, the port names of an effect in `crates/notes`. The arrangement finds it by those ports and knows nothing else of it.

It follows the pattern of the Filter, ARCHITECTURE.md "Built-in effects".

Enable it in `project.json` under `extensions` as `"reverb"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, its range and default, and starting points for sounds. The runtime writes it into every project as `agent-docs/reverb.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples, so it is the single source for the format.

The reverb owns no children, so an instance is one file: `<name>.json` in a track folder, named in the track's `effects`. A field you leave out takes its default. The runtime writes every field when it saves. An unknown field or a value out of range does not load. The instance then keeps its last valid state, and the problem names the file and the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `pre_delay_ms` | Pre-delay | 0.5 to 250 | 20 | logarithmic | `20 ms` |
| `decay_seconds` | Decay | 0.2 to 60 | 2 | logarithmic | `500 ms`, `2.4 s` |
| `size` | Size | 0 to 1 | 0.5 | linear | `50%` |
| `damping` | Damping | 0 to 1 | 0.5 | linear | `50%` |
| `diffusion` | Diffusion | 0 to 1 | 0.7 | linear | `70%` |
| `low_cut_hz` | Low cut | 20 to 20000 | 100 | logarithmic | `100 Hz` |
| `high_cut_hz` | High cut | 20 to 20000 | 8000 | logarithmic | `8 kHz` |
| `width` | Width | 0 to 1 | 1 | linear | `100%` |
| `mix` | Mix | 0 to 1 | 0.3 | linear | `30%` |

`freeze` is `true` or `false`. The numbers are `Parameter` constants next to `ReverbState` (`sound_core::Parameter`), and `validate`, `Default`, the knobs, the reset and a test of both docs read them.

## How it sounds

A feedback delay network of sixteen lines (Jot and Chaigne, 1991). Why this one and not a plate of Dattorro's: its decay time is a formula and not a tuning, so the tail meets the setting at every size; its mixing matrix loses no energy, so freeze is a gain of exactly 1; and the damping is the same formula at a second frequency. It is well known, small, and needs no modulation or randomness.

- The path: the input through a low cut and a high cut, one-pole filters of 6 dB per octave in their trapezoidal form; the pre-delay; four short allpass diffusers per channel, 2 to 5 ms, whose factor is `0.7 × diffusion`; then the sixteen lines. The mix of the dry sound and the reverb comes last. Stereo: the left and the right go into every line with the signs of two rows of a Hadamard matrix, and come out with the same signs, so the two sides of the tail are different. Width is mid and side on the tail only.
- The lines are 37 to 97 ms long at size 1 and a tenth of that at size 0, on a ratio (`0.1^(1 - size)`), each rounded to the nearest prime number of frames. Every line is mixed back into every other through the Hadamard matrix of 16 over 4: each line gets a quarter of every line, itself included, with a sign. It is orthogonal, so it keeps the energy exactly, and a quarter is exact in floating point. A Householder matrix, tried first, gives each line 7/8 of itself back and fills in slower: the echo density from 50 to 100 ms is 0.64 of noise with it and 0.73 with the Hadamard. The first time each line comes out is an early reflection. What comes out is what the lines hold; the loss of a pass is on the way back in, after the mix, on what goes into each line.
- Decay: each line loses `60 dB × length / decay` on each pass, so the whole tail falls by 60 dB in the decay time, whatever the size. The loss is a one-pole low pass whose gain at 0 Hz is that, and whose gain at 5 kHz (`DAMPED_HZ`) is what falls by 60 dB in `1 - 0.9 × damping` of the decay time: damping 0 is as long, 0.5 is 55 %, 1 is a tenth. `high_decay_seconds` gives it. The pole is capped at 0.99, which takes at most about 36 dB more from the highs per pass; only a short decay in a large room at full damping reaches it.
- Freeze glides every line to a gain of exactly 1 and no damping, and the input into the lines to 0, over 20 ms. The matrix keeps the energy, so the tail holds; rounding moves it by 0.02 dB in a minute. Because the tail is what the lines hold, a freeze keeps the tail it froze. The loss used to be on the way out, and the level of the tail made up for it; a freeze took the loss away and kept the make-up, so a short decay froze up to 80 times full scale. During the glide in, with no damping, what is written into the lines is at most what they held: the loop keeps more as the input gives less. With damping it can be more for the 20 ms of the glide, as the damping filter lets go: up to about 1 dB, measured +0.74 dB at size 1, a decay of 0.5 s and damping 1. The dry sound passes as before. A change of size while frozen waits until freeze ends, because every fade between taps loses a little of the tail and nothing fills it again: a drag of 60 moves would take 43 dB off.
- Every change glides over 20 ms (`RAMP_SECONDS`): the decay on a ratio, the cuts on a ratio, damping, diffusion, width, mix and freeze. Size and pre-delay do not move a read position, which would bend the pitch: the read fades from the old tap to the new one over 20 ms. A new value during a fade waits for its end, so a drag is a row of fades, each from where the last one ended. No edit clicks.
- All delay memory is allocated when the reverb is made, for 48 kHz, and again in `prepare` for another sample rate: for the largest size, the longest pre-delay and the diffusers. `process` never allocates, and a change of size or pre-delay only moves where it reads.
- Level: what goes into the lines is scaled by the loss of the loop. At each of eight frequencies from 0.5 to 7.5 kHz, the lines hold the power of the input over the part a pass loses, and give out what they hold; the input is scaled by the root of that, averaged, so noise comes out of the reverb alone at its own level at every size, decay and damping. So `mix` means the same at every setting. The factor is on the way in, not on the way out, so a change of decay only changes how fast the lines fill and drain: turning a long tail short drains it and never makes it louder. With the factor on the way out, a decay turned from 60 s to 0.2 s after 20 s of full scale noise peaked at 23 times full scale.
- It is linear and stable at every setting, also while they move: the loss of every line is at most 1 at every frequency, and 1 only when frozen. What goes into the reverb is held to +36 dBFS, and a sample that is not a number is silence to it. The dry sound is passed as it came, so mix 0 is the input to the bit, whatever it is.
- No modulation and no randomness, so a render, which makes the reverb new, is the same every time, to the byte, and so is a session from the same start.
- No latency: `Processor::latency` stays 0. The first reflection comes after the pre-delay, the diffusers (about 13 ms) and the shortest line.
- When the input is silent and nothing in the lines is louder than -180 dB for longer than the longest way through, it does no work and its output is silent.
- Nothing allocates, locks or makes a system call in `process`; the tests run under the realtime sanitizer.

## The card

`view::register(views, devices)` registers `ReverbView` as the card of `reverb` (`Views::register_card`) and says the card of a reverb is called `Reverb`. The runtime offers `Reverb` in the control that adds an effect.

The rack gives the view a `CardFrame`, the picker of the slot as the title, the power icon and the close icon, and the view draws the whole card, 352 pt wide as DESIGN.md gives it, with its expand icon:

- The display, 200 pt: the decay in time. The pre-delay and the decay each have a zone across on the travel of their knob. The tail is a line from full level at the end of the pre-delay to the floor, 60 dB down, at the end of the decay; the highs are a dashed line that reaches the floor at their part of the way; the early reflections are thin lines under the tail after its start, wider apart in a larger room. The hollow handle at the start drags the pre-delay, the handle at the end the decay. The line under it: `Pre-delay 20 ms · Decay 2 s`.
- Size and Width, Damping and Mix, as knobs in two columns.
- Behind expand: Low cut over Diffusion, High cut over Pre-delay, Freeze over Decay.

Editing, the same rules as every control on saved state (`sound_ui::ControlEdit`):

- A drag of a knob or a handle is one gesture and one undo step: "Change size", "Change damping", "Change width", "Change mix", "Change low cut", "Change high cut", "Change diffusion", "Change pre-delay", "Change decay". A handle has the step of its knob. The file is written once, at the end. Escape cancels.
- A click on Freeze is one step, "Change freeze". A double click or backspace on a knob sets its default, a double click on a handle sets its value's.
- The view keeps no copy of the state, so an outside edit shows at once, also during a drag. Whether the card is expanded is the view's own interface state and is not saved.
- The power icon bypasses the slot, as for every effect: "Turn off Reverb" is saved on the track, not in this record. A bypassed reverb leaves the chain, so its tail stops at once and does not ring out. The switch is hard, so on a loud tail it may click; that is the slot's bypass, the same for every effect.

## Known gaps

- A freeze over a short decay in a large room rises by up to 1.5 dB, although the frozen loop keeps its energy exactly. The likely cause, not proven: the tail of a short decay is mostly the first pass through the lines, and a frozen tail is the sum of many passes, where paths through the same lines in another order arrive together. The Householder matrix gives the same rise, and so does diffusion 0.
- With damping, the 20 ms glide into freeze can write up to about 1 dB more into the lines than they held, as the damping filter lets go.
- The bypass of the slot is a hard switch, so on a loud tail it may click.

## Checks

```sh
cargo nextest run -p reverb --no-capture                                 # prints the measured decay times
RTSAN_ENABLE=1 cargo nextest run -p reverb                               # with the realtime sanitizer
cargo nextest run -p runtime --test projects reverb                      # in a real project: files, reopen, renders
cargo nextest run -p runtime --test window reverb                        # the card, with a simulated mouse and keys
cargo nextest run -p reverb --run-ignored only realtime_ratio --no-capture   # speed of 20 reverbs
```

Measured September 26, 2026 on an Apple Silicon laptop, dev profile with `opt-level = 3`, 48 kHz, offline:

- Decay: the time an impulse takes to fall by 60 dB (T30 of Schroeder's energy decay curve, fitted from -5 to -35 dB) is within 2.3 % of the setting for decays of 0.2 (the shortest), 0.3, 0.5, 1, 2, 5, 10, 20 and 60 s (the longest) at sizes 0, 0.5 and 1; the worst is the shortest decay in the largest room. At 44.1 and 96 kHz it is within 0.3 % for 2 s. The tests hold it within 4 %.
- Damping: measured on a third of an octave, the decay at 200 Hz is within 2.4 % of the setting at damping 0 and 0.5, and at 5 kHz within 10.4 % of `high_decay_seconds` at damping 0, 0.5 and 1. At damping 1 the one-pole takes a little from 200 Hz too: it dies 11 % sooner at a decay of 1 s. The band is wide where the loss rises fast, so the 5 kHz figure is the measurement's more than the reverb's; the loss of a line at 5 kHz is exact to 0.001 dB.
- Freeze: over a minute with loud noise still coming in, the level of the held tail stays within 0.07 dB, also after a change of size half way; with silence coming in it moves by 0.03 dB; a drag of the size with 60 moves in a second changes it by 0.03 dB. Frozen while full scale noise plays, at every corner of size (0, 0.5, 1), decay (0.2, 2, 60 s) and damping (0, 1), the level of the held tail is within 1.5 dB of the tail before it and its peak at most 1.9 dB higher; the tests hold 2 and 2.5 dB. Why it rises at all is a known gap, above. Before the fix, the same case froze at 80 times full scale. Switched on and off every 37, 480 and 4800 frames for 3 s while full scale noise plays, at four corners, the peak is at most 1.0 dB over the peak while it plays. A decay turned from 60 s to 0.2 s after 20 s of full scale noise peaks at 2.44, under the 3.04 of the long tail, and so does a freeze, a change of decay while frozen and its end, at 2.45. Every sample is a normal number or 0. Off again, it is 64 dB down 5 to 6 s later at a decay of 5 s.
- Level: full scale noise for 30 s at every corner of size and decay (0 and 1, 0.2 and 60 s) comes out within -0.9 and +0.7 dB of the defaults, and peaks at 3.21 at most.
- No edit steps the sound: the largest step in the 30 ms after an edit of any field is at most 1.04 times the largest step of the reverb before and after it, and a drag of size and pre-delay 1.16 times; with the glides off, a change of size steps 8.4 times.
- Speed: 20 reverbs, each fed by its own noise, render 10.2 times faster than realtime, also with the size of every one moving all the time: about 0.5 % of one core for one reverb.
