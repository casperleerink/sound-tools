# drum-pad

The bundled Drum pad. It has one tool, `drum-pad`: an instrument of 16 pads that plays the note events of the `sound-notes` contract (`crates/notes/README.md`). Each pad is a synthesized drum sound of the default kit or a sample, with a volume, a pitch, a decay, a pan and a place in the one choke group. No sample file ships with the product: the kit is made by synthesis in `src/kit.rs`.

Enable it in `project.json` under `extensions` as `"drum-pad"`. A new project enables it. It is picked on the instrument card of a track, as the synth and instrument plugins are.

## The record

[agent-doc.md](agent-doc.md) has the record with every field, the kit and starting points. The runtime writes it into every project as `agent-docs/drums.md`, listed in the map `AGENTS.md`, and a test loads its examples, so it is the single source for the format.

The pads are keyed by their note, `"36"` to `"51"`. A pad left out is the pad of the kit, and a field left out of a pad is that pad's value in the kit. The runtime writes only the pads that differ from the kit, each with all its fields, so the file of a new Drum pad is `{"pads": {}}` and a file says at once what was changed. A pad plays a `sound` of the kit or a `sample` under `assets/audio/`, never both; a record with both does not load and the problem says so, as does a note outside 36 to 51 or a value out of range.

| Field | Knob | Range | Travel | Shown as |
| --- | --- | --- | --- | --- |
| `volume_db` | Volume | -48 to 12 | linear | `0 dB` |
| `pitch_semitones` | Pitch | -24 to 24 | linear, from the middle | `-7 st` |
| `decay_ms` | Decay | 10 to 10000 | logarithmic | `180 ms`, `1.8 s` |
| `pan` | Pan | -1 to 1 | linear, from the middle | `C`, `25L` |

Each pad has its own defaults, the kit. They are written once, in `KIT`, and `PARAMETERS` holds one `sound_core::Parameter` per number and pad with the range and that default. `validate`, `Default`, the knobs, their reset and a test that holds both tables of this file and the agent doc to them read it.

| Note | Pad | Sound | Volume | Pitch | Decay | Pan | Choke |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 36 | Kick | `kick` | 0 | 0 | 600 | 0 | no |
| 37 | Rim | `rim` | 0 | 0 | 70 | 0 | no |
| 38 | Snare | `snare` | 0 | 0 | 350 | 0 | no |
| 39 | Clap | `clap` | 0 | 0 | 380 | 0 | no |
| 40 | Snare 2 | `snare` | -1 | 3 | 240 | 0 | no |
| 41 | Tom 1 | `tom` | 0 | -7 | 800 | 0.35 | no |
| 42 | Hat | `hat` | 0 | 0 | 180 | 0 | yes |
| 43 | Tom 2 | `tom` | 0 | -4 | 720 | 0.25 | no |
| 44 | Pedal hat | `hat` | -3 | -1 | 110 | 0 | yes |
| 45 | Tom 3 | `tom` | 0 | -2 | 650 | 0.1 | no |
| 46 | Open hat | `open_hat` | 0 | 0 | 650 | 0 | yes |
| 47 | Tom 4 | `tom` | 0 | 1 | 600 | -0.05 | no |
| 48 | Tom 5 | `tom` | 0 | 3 | 550 | -0.15 | no |
| 49 | Crash | `crash` | 0 | 0 | 1800 | -0.25 | no |
| 50 | Tom 6 | `tom` | 0 | 6 | 500 | -0.25 | no |
| 51 | Ride | `ride` | 0 | 0 | 2500 | 0.25 | no |

## The sounds

Every sound of a pad is made whole, at the pitch and decay of its pad and the rate of the engine, and played from memory (`src/sounds.rs`). So the audio thread does the same for every pad, reads no file and makes nothing. Sounds are made by one thread of their own, never inside an edit: the behaviour takes a sound that was made before and is still held, which is how an edit of a volume or a pan makes nothing again and two Drum pads with one kit share its sounds, and asks the thread for any other, the latest ask of each pad winning. Until the new sound is made the pad keeps the one it had. Then `take_ready` names the Drum pad, and every loop of the runtime (the window's poll, the headless loop, an offline render) runs its behaviour again with `Project::rebind`, which puts the sound in the kit: it plays from the next hit. An offline render waits for every sound first (`wait_for_sounds`), so it plays what the records say from its first block. A pitch drag on the ride at 10 s, the longest sound, costs one move 6.8 ms with its frame in the window tests, where it cost 110 ms (205 ms at 96 kHz) when the behaviour made the sound. The kit is made in 65 ms after a Drum pad is added or opened, silent until then.

- **The kit** (`src/kit.rs`): the recipes of the analog drum machines, tuned by measuring them against the samples of those machines. A kick is a sine that falls from about 300 Hz to 48 Hz in the first 50 ms, soft-clipped while it is loud, with a short band of noise for the beater. A snare is two drum tones under noise from 1.8 to 9.5 kHz for the snares and a crack of noise at 4 kHz. A clap is four bursts of band-passed noise 11 ms apart and a softer tail. A rim is two short sines at 470 Hz and 1.7 kHz with a click, driven. Hats are six square waves at the frequencies of an analog hat with noise, band-passed high; an open hat holds before it falls. A tom is a sine that drops a third in pitch with a second mode of the head and the thud of the stick. A crash is twelve squares of metal and noise, a splash and a long wash that darkens as it rings; a ride is a quiet bell ping over a wash of metal. The noise is a fixed sequence, so a sound renders to the same bytes every time. Each sound is scaled so its loudest sample is its level (`Sound::level`), which balances the kit.
- **Pitch** tunes a synthesized sound, every frequency of it, and keeps its length. A sample plays faster or slower, as on a sampler: it is played as if it had been recorded at a rate that many semitones higher, through the resampler of audio clips (`sound_media::resampler`), which also takes it to the rate of the engine and filters what would fold back. So a sample at 0 semitones plays at its own pitch at any file rate.
- **Decay** is how long a pad sounds. A synthesized sound is made for it and falls to -60 dB in it. Every hit also fades out over it as `1 - (t / decay)^4`, so it is silent at its end; a sample at a decay longer than itself is hardly touched (2 dB at its end at one and a half times its length, which the window sets). A sample plays at most 10 s (`MAX_SAMPLE_SECONDS`), the longest decay. What is kept of a sample is its render and nothing of its file: 3.8 MB at 48 kHz at most, whatever the file. The file is read whole while the render is made, on the thread that makes sounds, and let go of after. A sample is known by its path, size and modification time, so a file written again under its name is made again at the next edit of the Drum pad. The last 2 ms of every sample fade to silence (`SAMPLE_END_SECONDS`), so a file that ends loud, or one cut at 10 s, ends without a click; every pad ends at zero.
- **A missing file** is a problem on the Drum pad, naming the pad and `assets/audio/<file>`, and that pad is silent. The other pads play. The tool asks `rebinds_on_assets("audio")`, so the pad plays when the file arrives.

## How it plays

- Note 36 plays pad 1, the bottom left, up to note 51, the top right. A note on is a hit on its exact frame. Note offs and the pedal do nothing: a drum is struck and rings out. Velocity sets the level with a square curve, as the synth does: 64 is a quarter of 127.
- One voice per pad sounds at a time. A new hit of a pad fades its last one out over 5 ms (`FADE_SECONDS`), and a hit of a pad of the choke group fades every other pad of the group out the same way: a closed hat cuts the open hat. A stop or a jump of the transport (`AllOff`) fades everything out. 32 voices, so the fading ones never take the place of a sounding one.
- Volume and pan glide over 20 ms (`RAMP_SECONDS`), also while a pad sounds. Sound, pitch and decay apply from the next hit: a hit is one strike, rendered as it was when it was struck.
- Pan has the law of a track: equal power, the middle exactly 1. A synthesized sound is in the middle or, for the clap, the hats and the cymbals, a little wide.
- Nothing is freed on the audio thread. A voice holds its sound by an `Arc`. A sound that an edit replaced while a voice played it goes into a graveyard of 32 places, and the next edit takes it back to the control thread. It is a render, never a file, so what waits there costs little.
- Each pad keeps its `sound_core::Peaks` (`pad-36` to `pad-51`): how loud it sounds, from 0 to 1 of its hit, for the card.

## The view

`view::register(views, devices)` registers `DrumPadView` as the card of `drum-pad`. `view.rs` is the only module here that uses GPUI. The card is 452 pt, 581 pt expanded, as DESIGN.md sets it: the 4 by 4 grid of `sound_ui::components::pad::Pad` as the display, 300 x 140 pt, then Volume and Decay, Pitch and Pan of the selected pad. Behind expand: `Sound`, a select two cells wide with the sounds of the kit, the pad's own file when it plays one, and `Choose file…`; and `Choke`, a toggle.

- A press on a pad selects it and plays it at velocity 100 (`Project::send` of `DrumUpdate::Hit`, not an edit). The grid is one tab stop: the arrows move the selection and stop at the edges, enter plays the selected pad.
- A pad is green at 24 % while it sounds, fading with the sound: the view takes the peaks of every pad once per poll and draws again only when a level moves by a 24th.
- A file from the Finder let go of on a pad, or picked with `Choose file…` in the macOS file panel, is copied into `assets/audio/` on a background thread (`sound_media::import`) and makes that pad play it, as one undo step "Load sample": at its own pitch, with a decay one and a half times its length (`Pad::load_sample`). While files are over a pad, it has the lavender ring. A file that does not play is the notice of the window.
- A sample pad has the waveform glyph, and a pad whose file is missing the peach warning glyph, read from what is known of the file in memory only (`sound_media::cached`).
- Every knob goes through `ControlEdit`: a drag is one gesture and one undo step ("Change volume", "Change pitch", "Change decay", "Change pan"), a key step or a reset one commit. "Change sound", "Add to choke group" and "Take out of choke group" are one commit each. Which pad is selected and whether the card is expanded are interface state and not saved.

## Ports

| Name | Kind |
| --- | --- |
| `notes` | event input carrying `sound_notes::NoteEvent` |
| `audio` | audio output, stereo |

## Checks

```sh
cargo nextest run -p drum-pad
cargo nextest run -p runtime --test projects drums       # a real project: close and reopen, an outside edit
cargo nextest run -p runtime --test window drum_pad       # the card, with a simulated mouse and keys
RTSAN_ENABLE=1 cargo nextest run -p drum-pad              # with the realtime sanitizer
cargo nextest run -p drum-pad --run-ignored only listen --no-capture   # WAVs of the kit and a beat
```
