# midi

MIDI input: a keyboard plays the instrument of the selected track, and what is played is recorded into a clip.

This crate registers no tool and no record. It is a processor in the engine, a device layer over `midir`, and a recorder, like the metronome is a processor and a switch. A recording is the only thing here that edits the project, and the window makes that edit.

## What it does

- Every MIDI input port of the machine is open, all channels merged. There is no device picker, no input routing per track and no channel filter. A keyboard plugged in later works without a restart.
- Note on, note off and the sustain pedal (controller 64) are used. Pitch bend, the mod wheel, aftertouch, program change, MIDI clock and every other message are left alone.
- A message sounds at the start of the next audio block, whether the project plays or not.
- While recording, every message the engine sounded is kept with the tick it sounded on and the time it arrived. When the take ends it becomes a [`Clip`](../../crates/notes/README.md) and a raw take file.

## The path from a key to sound

Three threads, none waiting for another:

| Thread | Does |
| --- | --- |
| A device thread, one per port | Reads a message and puts it in the input ring. It is the one place with a lock, and it is never the audio thread. |
| The audio thread | Takes what is in the ring at the start of a block and sends it to the instrument at offset 0 of that block. |
| The control thread | Reads the report ring for the take and the latency. Nothing it does can delay sound. |

So a message waits for the next block and nothing else: no interface, no 16 ms poll, and no allocation, lock or system call in `process`. A message that does not fit in the event buffer of the block stays in the ring and goes out in the next one, so a key press is never lost. Both rings count what they had to drop (`Keyboard::lost`), and both are far larger than any burst a keyboard makes.

## One release path

The processor is the only place that knows what the live input holds, so it is the only place that can let go of it. `Input::release_held` asks it to, and it sends the pedal up and an off for every held pitch at the start of the next block, the pedal first so the offs are not latched under it. Three things ask:

- the port it plays into is about to change (`Keyboard::play_into`), so the offs reach the instrument that holds the notes and not the one that comes next;
- a port went away while it held keys, which `Ports::refresh` notices: an unplugged keyboard sends no note off, ever;
- a message did not fit in the input ring, and it may have been an off.

It releases what every keyboard holds, not only the one that went. That is the rule the note contract already has for `AllOff`, and it is why there is one path and not one per reason. `Keyboard::poll` keeps the old connection until the processor says the release is out.

## Using it

```rust
use midi::{Keyboard, Ports};

let mut keyboard = Keyboard::attach(engine)?;          // &mut EngineControl
keyboard.play_into(engine, Some(notes))?;              // the `notes` port of an instrument
let mut ports = Ports::new(keyboard.input());          // the device layer
ports.refresh()?;                                      // about once a second

keyboard.poll(Some(&timing));                          // regularly: the take and the latency
keyboard.start_recording(playhead);
let take = keyboard.finish_recording(playhead);
```

- `play_into` is where the live input goes. Read the port again after every change of the project and pass it in: an endpoint moves when the instrument behind it is built again. The same one twice costs nothing and no compile.
- `poll` drains what the engine reported. Call it whether the project plays or not.
- `Keyboard::latency` is how long a message took from arriving to the start of its sound at the device, measured against `sound_core::StreamTiming`. It covers the wait for the next block and the output latency the device reports, not the keyboard's own scan and its cable.
- `Keyboard` is not project state. Attaching it writes nothing and adds no undo step. A finished take is an edit, and the caller makes it.

## The raw take

`Take::clip()` is the music: a note at the tick the engine sounded it, so playing the clip back renders what was heard. A note still held when recording ends ends there. A note off with no note on before it, from a key that was already down, is left out.

`Take::write(root)` writes the performance as it arrived, once, under a name of its own: `assets/takes/take-1.json`, then `take-2` and so on. It gives that name back, and the clip keeps it in its `take` field. The name never comes from a clip id, because a clip id changes when it moves to another track or an agent renames its file, and it is never used twice, also not after an undo took the clip of an earlier take. The file is created with `create_new` and never opened again, so no performance can be written over.

The caller writes the take before it makes the clip, and whatever happens to the clip: a track deleted during the recording leaves the performance in the project all the same. Nothing removes a take, not even undo of the recording. It holds both velocities of every note, the pedal, where the pedal stood when it began, and the times in microseconds from the start of the recording, which is real time and holds whatever the tempo map does. `agent-doc.md` in this crate is what an agent reads about it; the runtime writes it into every project as `agent-docs/takes.md`.

## Not built

Overdub, merging takes, a count-in, punch in and out, loop recording, quantize, a MIDI monitor, a device picker, latency compensation, other controllers than the sustain pedal, MIDI output, MIDI files and MIDI clock.

## Checks

```sh
cargo nextest run -p midi
RTSAN_ENABLE=1 cargo nextest run -p midi          # the realtime sanitizer over `process`
cargo nextest run -p runtime --test projects      # recording whole projects
```

CI has no MIDI device, so everything but the device layer is tested with messages the test sends itself, through the same ring a keyboard writes into.
