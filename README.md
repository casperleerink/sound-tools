# Sound Tools

A small DAW that an AI agent can work in. A project is a folder of small JSON files, and the running app applies every change to them live, so an agent adds a part by writing a file and you hear it without a build. The agent is an external coding agent, run in the project folder.

Two milestones are built. The first: tracks, clips, notes, one synth and a window to edit them. The second: stereo tracks with gain, pan and mute, a metronome, MIDI recording with the sustain pedal, CLAP and VST 3 plugins as instruments and as effects with their own windows, and **fit tempo** — play freely with no click, and one action moves the grid onto your playing, so bars and beats land where you hear them and everything added afterwards follows. Both were checked on the real application; the second on September 21, 2026, see [ARCHITECTURE.md](ARCHITECTURE.md), "Verified September 21, 2026".

## Requirements

- macOS. Other platforms are not tried yet.
- Rust through `rustup`. `rust-toolchain.toml` pins the version and `rustup` installs it on the first build.
- The first build takes about a minute, plus the download of the dependencies. Later builds take seconds.
- `.cargo/config.toml` puts the build output in `/private/tmp/sound-tools-timing/target`, shared by every checkout. `/private/tmp` is emptied on a restart of the Mac, so the first build after that is slow again.

## Run it

```sh
cargo run -p runtime -- ~/Music/my-piece
```

The folder is the project. When it is empty or missing, the app makes the default project in it, which is 120 bpm, 4/4 and one track with a synth. There is no save. The app writes every finished edit to the folder.

## Two-minute tour

1. Click the project name top-left and pick **Add track**.
2. Double click on empty space in a track row. That adds a clip of one bar.
3. Double click the clip. The note editor opens below.
4. Drag on empty space inside the clip to draw a note. It sounds. Drag a note to move it, drag its end to change its length, press delete to remove it.
5. Press space to play and again to pause. Click the ruler to move the playhead. The view follows it and pages forward while it plays.
6. In the transport pill at the bottom: drag the tempo number up or down to change the tempo, and click the metronome to turn the click on or off. The click is not part of the piece and is never in a render.
7. Plug in a MIDI keyboard and play. It sounds through the instrument of the selected track, with or without playback. Press the red record button, or `r`, to record what you play onto that track from the playhead, and press it again to end the take. The take becomes a clip, with the sustain pedal, as one undo step, and the performance as you played it is kept under `assets/takes/`, which nothing ever changes.
8. Click the name of a track on the left. Its panel opens below with the synth, and at the right end the mixer of the track: gain, pan and mute. Drag a knob up or down while it plays, and double click a knob to reset it.
9. Click `Synth` at the top of that card to pick another instrument: the built-in synth, or any CLAP or VST 3 instrument this Mac has. The card gets `Open window`, which opens the plugin's own window beside this one. Change a sound there and it is saved with the piece. Picking an instrument is one undo step. The app looks for the plugins of this Mac on a thread of its own, so a project always opens at once; the picker says so while it is still looking, and a track whose plugin has not turned up yet is quiet for a moment and then plays.
10. Click **Add effect** at the end of that rack and pick `Filter`, the built-in filter, or an effect plugin. It lands after the instrument, and the track plays through it. Add another and it lands after the first. The small `x` on a card takes that effect off. Both are one undo step. To change the order, edit `effects` in the track's `instance.json`, which an agent can do for you; the rack follows at once.
11. Record a take with the click off, click the clip, then click the project name and pick **Fit tempo to take**. The tempo map now follows what you played: the bar lines land on your beats and the take sounds exactly as it did. A `steady` number appears in the transport next to the tempo; drag it up to pull the tempo towards one steady one, and back to 0 for the playing as it was. If the grid runs at twice or half the speed of the music, or the bar lines are in the wrong place, ask an agent: "the grid runs twice as fast as the music, fix the fit". It is one field in `state/fit-tempo.json`.
12. Press cmd-z to undo and shift-cmd-z to redo. Every drag and every key is one step.
13. Press cmd-q to quit. Run the same command again and the piece is back.

Every mouse action and key is in [DESIGN.md](DESIGN.md), "Using the app".

## A project made before this milestone

It opens and plays as it did, and nothing is rewritten. Some things need one edit of `project.json` before the new parts of the app are within reach. Add them to `extensions` and open the project again:

```json
"extensions": ["arrangement", "compressor", "filter", "fit-tempo", "instrument", "plugin-host", "tone"]
```

- `plugin-host` for CLAP and VST 3 plugins. Without it the picker shows every plugin greyed out with that line under it.
- `fit-tempo` for **Fit tempo to take**. Without it the menu item is greyed out with that line under it.
- `filter` for the built-in Filter effect. Without it `Add effect` shows it greyed out.

One thing is reported: a project of the first milestone connects a track once per device channel, and audio is stereo now, so one connection carries both channels. `problems.txt` names the second line and says to remove it. The project sounds as it did in the meantime.

## What to check by ear

Nobody who built this can hear. Three things need the owner, and each takes a few minutes:

1. **Latency.** Plug in your keyboard, click a track name, play. Does it feel like an instrument, or is there a wait? The numbers say 20 ms from key to sound on the built-in speakers; a wired interface should be faster.
2. **Your pianos.** Put each piano plugin you use on a track (click `Synth` at the top of its card), open its window, load a sound, play. Does it sound the way it does in your other DAW, and does its window work?
3. **A free take fitted.** Turn the click off, press `r`, play something with rubato, press `r` again. Click the clip, then the project name, then **Fit tempo to take**. Are the bar lines where you hear the beats? Drag `steady` up and back to 0. Does the take still sound as you played it?

## Work with an agent

The agent is any coding agent that can edit files. The app must be running on the project, else the edits are saved but nobody checks or plays them.

1. Run the app on a project and press space.
2. Click the project name top-left and pick **Open terminal in project folder**, then start Claude Code or Codex there:

   ```sh
   claude        # or: codex
   ```

3. Ask in plain language:
   - `Add a bass line in bars 5 to 8 that follows the chords on the piano track`
   - `Add a new track with a simple melody over bars 1 to 4`
   - `Make the bass sound darker`
   - `Turn the piano down a few dB and put the bass a little to the left`
   - `Put the reverb before the delay on the piano track`
   - `The grid runs twice as fast as the music. Fix the fit`
4. The part shows up and plays while the agent still writes. One cmd-z in the app takes the whole request back.

What the agent uses:

- `AGENTS.md`. The app writes it into the project, with a `CLAUDE.md` that imports it. It is a short map: the folder layout, the bar math of this project, how to check the work, and a list of docs in `agent-docs/` with one line each saying when to open it. The record formats live in those docs, one per extension. Agents read the map by themselves and open only the doc their task needs.
- `problems.txt`. The app keeps it current while it runs. It lists every file that did not load, and why. `No problems. Every file is live.` means all of it plays. The agent reads it to check its work.
- `runtime <folder> --inspect`. It prints the tempo, every track and every clip with its bar range, and the problems. It works next to the running app. It loads no plugin: it says which plugin a track names and whether this Mac has it, and runs none of them, so no plugin can end it.

## Without the window

```sh
cargo run -p runtime -- ~/Music/my-piece --inspect
cargo run -p runtime -- ~/Music/my-piece --render /tmp/my-piece.wav --seconds 16
cargo run -p runtime -- ~/Music/my-piece --headless
cargo run -p runtime -- --plugins
```

- `--inspect` prints a summary and changes nothing. It loads no plugin, so it costs nothing extra and no plugin of this Mac runs in it.
- `--render` writes a WAV offline at 48 kHz, stereo, 32-bit float.
- `--headless` plays the project live without a window and reads commands from stdin: `play`, `pause`, `stop`, `seek <ticks>`, `undo`, `redo`, `status`, `quit`. It prints every change that arrives from the folder. Only one app can have a project open live. `--inspect` and `--render` work next to it.
- `--plugins` prints the CLAP and VST 3 plugins of this Mac with their ids and whether each says it is an instrument, an effect or both, which is what a track record needs when an agent writes one. In the app you pick one by name instead. Each is looked at in a child process, so one that crashes costs that one and is reported. It looks at every plugin again, whatever the app remembered, so it is also how a plugin that failed once is tried again.

A release build is `cargo build --release -p runtime`. The binary is `/private/tmp/sound-tools-timing/target/release/runtime`.

## Checks

```sh
cargo fmt --all --check
typos
cargo clippy --workspace --all-targets --locked --config .cargo/ci-config.toml
cargo build --workspace --locked
cargo nextest run --workspace --locked
cargo test -p gallery --test snapshots --locked
cargo test -p runtime --test snapshots --locked
cargo shear
cargo deny check
```

CI runs the same on macOS, plus the realtime sanitizer from [ENGINEERING.md](ENGINEERING.md) section 3. The tools come from `cargo install cargo-nextest cargo-shear cargo-deny typos-cli`. The build
comes first because the tests load the repository's own CLAP and VST 3 plugins, which are
dynamic libraries that `cargo test` does not build.

The two snapshot tests render the UI components and the window to PNGs without opening a window. They print the folder they write to. `cargo run -p gallery` opens the component gallery in a window.

## Docs

- [CONCEPT.md](CONCEPT.md): what the product is for.
- [ARCHITECTURE.md](ARCHITECTURE.md): the decisions, both milestones with their checks, and the known gaps after each.
- [ENGINEERING.md](ENGINEERING.md): how to build: dependencies, the audio engine, testing, rules for agents.
- [docs/milestone-2.md](docs/milestone-2.md): the plan of the second milestone, done September 21, 2026. [docs/agent-brief.md](docs/agent-brief.md) is the shared brief for the agents that built it.
- [docs/milestone-3.md](docs/milestone-3.md): the plan of the third milestone: mixer, built-in effects, reliable plugins and a better window. Not started.
- [DESIGN.md](DESIGN.md): the look, and every mouse action and key of the app.
- [SDK_SKETCH.md](SDK_SKETCH.md): an early sketch of the extension SDK.
- Guides per crate: [core](crates/core/README.md) for extension authors, [ui](crates/ui/README.md) for view authors, [notes](crates/notes/README.md) for the note contract, [arrangement](extensions/arrangement/README.md), [instrument](extensions/instrument/README.md), [filter](extensions/filter/README.md), [metronome](extensions/metronome/README.md), [midi](extensions/midi/README.md), [plugin-host](extensions/plugin-host/README.md), [fit-tempo](extensions/fit-tempo/README.md).

## License

[MIT](LICENSE).

VST is a registered trademark of Steinberg Media Technologies GmbH.
