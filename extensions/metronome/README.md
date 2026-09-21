# metronome

The click. One processor on the beats of the project's tempo map, and a switch for it.

The click is not music, so this crate registers no tool, no record and no agent doc. It is a bundled extension because it makes sound and is built on the SDK like every other processor, not because a composer creates instances of it.

## What it does

- It clicks every beat of the tempo map. A beat is the note value of the time signature's lower number, so 6/8 clicks six times a bar and 3/4 three times.
- The first beat of a bar sounds at 1600 Hz, every other beat at 1000 Hz. That is the only difference between them: there is nothing to choose and nothing to save.
- One click is a sine burst of 30 ms that starts at its peak and decays to silence. It is synthesized: no sample file and no asset.
- A stop, a seek or switching the click off fades the sounding click out in 2 ms, so nothing hangs.

The processor holds no tempo map. Every block it asks `ProcessContext::transport` which ticks the block covers and the clock on which frame each of them lands, so a tempo change, a tempo map written from outside and another time signature need no update at all.

## Using it

```rust
use metronome::Click;

let mut click = Click::attach(engine)?;   // &mut EngineControl, off and silent
click.set_on(engine, true)?;              // an update: no compile, no click in the signal
click.is_on();
```

`Click` is not project state. Nothing is saved, nothing is written to the project folder and there is no undo step, so `cmd-z` can never toggle the click. Only the application window attaches one, in `crates/runtime/src/window/transport.rs`. `--render`, `--inspect` and `--headless` never do, so an offline render is the same whatever the window does.

The click goes to device channels 0 and 1, next to whatever the project connects there.

## Checks

```sh
cargo nextest run -p metronome
RTSAN_ENABLE=1 cargo nextest run -p metronome    # the realtime sanitizer over `process`
```
