# sampler

The bundled Sampler. It has one tool, `sampler`: one audio file played across the keyboard, with a root key, a start and an end in the file, an envelope, velocity to volume and a gain. It goes in the `instrument` slot of a track like the synth, plays the note events of the `sound-notes` contract (`crates/notes/README.md`) and reads its file through `sound-media` (`crates/media/README.md`). It depends on no other extension.

It follows the pattern of the built-in effects, ARCHITECTURE.md "Built-in effects": its own card with a picture of what it does, its parameters in the record, and an agent doc. What it settled is in ARCHITECTURE.md, "The Sampler".

Enable it in `project.json` under `extensions` as `"sampler"`. A new project has it.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, and starting points. The runtime writes it into every project as `agent-docs/sampler.md`, listed in the map `AGENTS.md`, and a test of the runtime loads its examples.

The sampler owns no children, so an instance is one file, `instrument.json` in a track folder. A field you leave out takes its default, so `"state": {}` is an empty sampler. The runtime writes every field when it saves, except `sample` and `end_seconds` while they are not set. An unknown field, a value out of range, a `root` that is no note number or a `sample` that is no file name under `assets/audio/` does not load, and the problem names the field.

| Field | Knob | Range | Default | Travel | Shown as |
| --- | --- | --- | --- | --- | --- |
| `sample` | the display | a file name under `assets/audio/` | none | | `kalimba.wav` |
| `root` | Root | 0 to 127 | 60 | linear, whole notes | `C4` |
| `start_seconds` | Start | 0 to the length of the file | 0 | linear | `12 ms`, `1.18 s` |
| `end_seconds` | End | after `start_seconds` | the end of the file | linear | `1.18 s` |
| `attack_seconds` | Attack | 0.001 to 10 | 0.002 | logarithmic | `2 ms` |
| `decay_seconds` | Decay | 0.001 to 10 | 0.4 | logarithmic | `400 ms` |
| `sustain` | Sustain | 0 to 1 | 1 | linear | `55%` |
| `release_seconds` | Release | 0.001 to 10 | 0.3 | logarithmic | `300 ms` |
| `velocity_to_volume` | Velocity | 0 to 1 | 0.5 | linear | `50%` |
| `gain_db` | Gain | -48 to 24 | 0 | linear | `0 dB`, `-6 dB` |

The numbers are `Parameter` constants next to `SamplerState` (`sound_core::Parameter`): `ROOT`, `ATTACK`, `DECAY`, `SUSTAIN`, `RELEASE`, `VELOCITY` and `GAIN`, all in `PARAMETERS`. `validate`, `Default`, the knobs, their reset and a test of both docs read them. Start and end are seconds of the file, as for an audio clip: their range is the length of the file, which the record does not know, so they are checked against each other and the behaviour says when a start is past the end of the file.

## How it plays

- A key plays the file at `2^((key - root) / 12)` times its speed, times the rate of the file over the rate of the engine, through `sound_media::Varispeed`: a windowed sinc of 32 taps, the same every time. So a file at 44.1 kHz in an engine at 48 kHz plays at its pitch, and at the root from a file at the engine's rate a note is the file itself, sample for sample. Above the root, and from a file at a higher rate than the engine, the filter is stretched with the step so nothing folds back: what would is 80 dB down or more, up to a step of 8. A voice costs 0.14 % of a core at the root and 0.6 % an octave up, 2.3 % three octaves up, in the dev profile (`performance::`, run by hand).
- 16 voices. The 17th note takes over the quietest released voice, or the oldest held one, which fades out over 5 ms next to the new note in one of 8 more slots, so a takeover does not click.
- A note plays from `start_seconds`. It stops at `end_seconds` or at the end of the file, with a ramp of 2 ms before it so a sound cut in the middle does not click, or when its release has ended. There is no loop.
- The envelope is `sound_core::Envelope`, which the synth uses too: the attack reaches full level in its time, the decay comes to within 0.1 % of the way to the sustain level in its time, and the release reaches silence in its time from full level, sooner from a lower one. A held note with `sustain` 0 ends after its decay.
- Velocity to volume: a note plays at `1 - v + v * (velocity / 127)^2` of full level, `v` being `velocity_to_volume`.
- The sustain pedal holds notes as in the synth. `AllOff` releases everything and puts the pedal up.
- An edit applies while notes sound: `gain_db` glides over 20 ms, the envelope applies at once to held notes, and root, start, end and velocity to volume apply from the next note. A new file fades out the notes of the old one over 5 ms.
- A sampler with no voice in use does no work.

## Loading, and the audio thread

The behaviour reads the file on the control side with `sound_media::load`, the first time anything names it, and sends it to the processor in its update as an `Arc<Audio>`. A file the window just imported is already in memory, so the thread that draws reads nothing. The processor keeps the sample and the one before it, for notes that fade out; the one before that goes back to the control side inside the update, and is let go of there. So nothing reads a file on the audio thread and no sample is freed there. `loading::a_new_file_while_notes_sound_fades_them_out_and_the_old_one_goes_back` shows it, and runs under the realtime sanitizer in CI.

A file that is not there is a problem on the record, in `problems.txt` and on the card, and the Sampler is silent. It plays when the file arrives: the tool asks with `rebinds_on_assets("audio")` to run its behaviour again when a file under `assets/audio/` changes.

## The view

`view::register(views, devices)` registers `SamplerView` as the card of `sampler` and names it "Sampler". The rack gives it a `CardFrame`, the picker of the instrument slot as its title, as for the synth.

The card is 464 pt, 649 expanded, as DESIGN.md draws it:

- The display is the waveform display of `sound-ui` with the whole file: the start and end lines drag the start and end, the envelope is drawn over it in the time of the file from the start line (the attack peak drags sideways, the decay corner sideways and up for the sustain), and a green line is where the last note is in the file. The line under it: `kalimba.wav · A 2 ms · D 400 ms · S 55%`. The release comes after the key is let go, which is no place in the file, so it has a knob and no handle.
- Shown: Root over Release, Velocity over Gain. Behind expand: Start over End, Attack over Decay, Sustain.
- With no file: `Drop an audio file here` and a `Choose file` button, which opens the file panel of macOS and is the way from the keys. A file that is not there says `kalimba.wav is missing` over the same button.
- A file dragged over the display shows the 2 pt lavender ring and `Drop to load the file`, or `Drop to replace the file` over a Sampler that has one. A drop or a choice copies the file into `assets/audio/` on a background thread (`sound_media::import`) and sets `sample` to it as one undo step, "Load sample". A file that does not play is not copied and the notice says why.
- Every knob and handle is one gesture or one commit through `ControlEdit`, named after its knob: "Change root", "Change start", "Change attack", "Change decay and sustain" for the corner. The Root knob moves in whole notes; its arrow keys step one semitone.

## Ports

| Name | Kind |
| --- | --- |
| `notes` | event input carrying `sound_notes::NoteEvent` |
| `audio` | audio output, stereo. A mono file plays on both channels, a stereo file on its own two |

## Checks

```sh
cargo nextest run -p sampler --no-capture                     # pitch, envelope, velocity, clicks, loading
cargo nextest run -p runtime --test projects sampler          # a real track: live edits, reopen
cargo nextest run -p runtime --test window sampler            # the card, with a simulated mouse and keys
RTSAN_ENABLE=1 cargo nextest run -p sampler                   # with the realtime sanitizer
```
