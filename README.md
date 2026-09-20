# Sound Tools

A small DAW that an AI agent can work in. A project is a folder of small JSON files, and the running app applies every change to them live, so an agent adds a part by writing a file and you hear it without a build. The first milestone is built. It has tracks, clips, notes, one synth and a window to edit them, and the agent is an external coding agent for now.

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
5. Press space to play and again to pause. Click the ruler to move the playhead.
6. Press cmd-z to undo and shift-cmd-z to redo. Every drag and every key is one step.
7. Press cmd-q to quit. Run the same command again and the piece is back.

Every mouse action and key is in [DESIGN.md](DESIGN.md), "Using the app".

## Work with an agent

The agent is any coding agent that can edit files. The app must be running on the project, else the edits are saved but nobody checks or plays them.

1. Run the app on a project and press space.
2. Open a terminal in the project folder and start Claude Code or Codex there:

   ```sh
   cd ~/Music/my-piece
   claude        # or: codex
   ```

3. Ask in plain language:
   - `Add a bass line in bars 5 to 8 that follows the chords on the piano track`
   - `Add a new track with a simple melody over bars 1 to 4`
   - `Make the bass sound darker`
4. The part shows up and plays while the agent still writes. One cmd-z in the app takes the whole request back.

What the agent uses:

- `AGENTS.md`. The app writes it into the project, with a `CLAUDE.md` that imports it. It explains the folder layout, the record formats and the bar math of this project. Agents read it by themselves.
- `problems.txt`. The app keeps it current while it runs. It lists every file that did not load, and why. `No problems. Every file is live.` means all of it plays. The agent reads it to check its work.
- `runtime <folder> --inspect`. It prints the tempo, every track and every clip with its bar range. It works next to the running app.

## Without the window

```sh
cargo run -p runtime -- ~/Music/my-piece --inspect
cargo run -p runtime -- ~/Music/my-piece --render /tmp/my-piece.wav --seconds 16
cargo run -p runtime -- ~/Music/my-piece --headless
```

- `--inspect` prints a summary and changes nothing.
- `--render` writes a WAV offline at 48 kHz, stereo, 32-bit float.
- `--headless` plays the project live without a window and reads commands from stdin: `play`, `pause`, `stop`, `seek <ticks>`, `undo`, `redo`, `status`, `quit`. It prints every change that arrives from the folder. Only one app can have a project open live. `--inspect` and `--render` work next to it.

A release build is `cargo build --release -p runtime`. The binary is `/private/tmp/sound-tools-timing/target/release/runtime`.

## Checks

```sh
cargo fmt --all --check
typos
cargo clippy --workspace --all-targets --locked --config .cargo/ci-config.toml
cargo nextest run --workspace --locked
cargo test -p gallery --test snapshots --locked
cargo test -p runtime --test snapshots --locked
cargo build --workspace --locked
cargo shear
cargo deny check
```

CI runs the same on macOS, plus the realtime sanitizer from [ENGINEERING.md](ENGINEERING.md) section 3. The tools come from `cargo install cargo-nextest cargo-shear cargo-deny typos-cli`.

The two snapshot tests render the UI components and the window to PNGs without opening a window. They print the folder they write to. `cargo run -p gallery` opens the component gallery in a window.

## Docs

- [CONCEPT.md](CONCEPT.md): what the product is for.
- [ARCHITECTURE.md](ARCHITECTURE.md): the decisions, the first milestone with its check, and the known gaps.
- [ENGINEERING.md](ENGINEERING.md): how to build: dependencies, the audio engine, testing, rules for agents.
- [DESIGN.md](DESIGN.md): the look, and every mouse action and key of the app.
- [SDK_SKETCH.md](SDK_SKETCH.md): an early sketch of the extension SDK.
- Guides per crate: [core](crates/core/README.md) for extension authors, [ui](crates/ui/README.md) for view authors, [notes](crates/notes/README.md) for the note contract, [arrangement](extensions/arrangement/README.md), [instrument](extensions/instrument/README.md).
