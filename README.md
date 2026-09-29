# Sound Tools

A small DAW that an AI agent can work in. A project is a folder of small JSON files, and the running app applies every change to them live, so an agent adds a part by writing a file and you hear it without a build. The agent is an external coding agent, run in the project folder.

What it has: instrument and audio tracks, clips with notes or audio, a synth, a Sampler, a Drum pad, CLAP and VST 3 plugins, nine effects (Filter, Compressor, Limiter, EQ, Delay, Reverb, Saturator, Utility, and Modulation: a chorus, flanger and phaser), a mixer with a limited master, MIDI and audio recording, and **fit tempo**: play freely with no click, and one action moves the grid onto your playing.

## Requirements

- macOS 11 or later, the main platform. Linux builds from source too, see "Linux" below. Windows is not tried yet.
- To build it: Rust through `rustup`. `rust-toolchain.toml` pins the version and `rustup` installs it on the first build.
- The first build takes about a minute, plus the download of the dependencies. Later builds take seconds.
- `.cargo/config.toml` puts the build output in `/private/tmp/sound-tools-timing/target`, shared by every checkout. `/private/tmp` is emptied on a restart of the Mac, so the first build after that is slow again.

### Linux

Linux x86_64 and arm64 build from source, and CI runs the tests on Ubuntu. On Ubuntu or Debian, install:

```sh
sudo apt-get install build-essential pkg-config libasound2-dev libfontconfig-dev \
  libfreetype-dev libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libvulkan1 \
  mesa-vulkan-drivers
```

`/private/tmp` is a macOS folder, so set another build folder first, for example `export CARGO_TARGET_DIR=target`. Then build and run as on macOS. The window needs Vulkan: a graphics driver, or `mesa-vulkan-drivers` for a software one.

CI builds and tests on Ubuntu. Playback, MIDI, recording and plugins go through ALSA and should work, but nobody has tried them on a real Linux desktop yet. Plugins are looked for in `~/.clap` and `/usr/lib/clap`, and in `~/.vst3`, `/usr/lib/vst3` and `/usr/local/lib/vst3`. The plugin cache is in `~/.cache/sound-tools/`.

A release has a tarball for x86_64 and one for aarch64, see "Releases". `tooling/bundle-linux.sh` makes it: the program, a menu entry, the icon and `install.sh`, which puts them in `~/.local`. It needs glibc 2.39 or later (Ubuntu 24.04 or newer) and the runtime libraries the page of the release names, not the `-dev` packages above.

With no folder, `runtime` opens the last project, which it keeps in `~/.config/sound-tools/`, or else a folder panel. The panel needs the XDG desktop portal, which GNOME and KDE have. **Install command line tool** links `sound-tools` into `~/.local/bin`.

Not on Linux:

- Plugin windows. The card of a plugin has `Open window` greyed out and says why. The plugin plays, and its state is saved.
- The snapshot tests. They render with Metal, so on Linux they only say so. The rest of the tests run.
- **Open terminal in project folder** starts `$TERMINAL`, or else `x-terminal-emulator`, so it needs one of the two.

## Run it

```sh
cargo run -p runtime -- ~/Music/my-piece
```

The folder is the project. When it is empty or missing, the app makes the default project in it, which is 120 bpm, 4/4 and one track with a synth. There is no save. The app writes every finished edit to the folder.

### Install the app

```sh
tooling/bundle-macos.sh
cp -R "dist/Sound Tools.app" /Applications/
```

The script makes a release build and puts `Sound Tools.app` in `dist/`. Double click it in the Finder. It opens the last project you had open, or asks for a folder the first time: pick a project, or click **New Folder** for a new project. Cancel quits. **Open project…** in the project menu switches to another one.

The app is signed on this Mac only ("ad hoc"), not by a known developer. If macOS says it cannot check the app, right-click it in the Finder, pick **Open**, and confirm once. macOS asks for the microphone the first time you record audio; a new build of the app asks again.

The app and `cargo run -p runtime` are the same program. Opening a folder on the command line also makes it the last project of the app. `tooling/bundle-macos.sh --zip` also writes a zip of the app, for a release.

## Releases

Each [release](https://github.com/casperleerink/sound-tools/releases) has the app for macOS (Apple silicon) and Linux (x86_64 and aarch64). The page of a release says how to install it:

- macOS: unzip and drag `Sound Tools.app` to Applications. It is not signed by a known developer, so macOS blocks the first open. Run `xattr -dr com.apple.quarantine "/Applications/Sound Tools.app"` once, or click **Open Anyway** in System Settings, Privacy & Security.
- Linux: extract the tarball and run `./install.sh`. Run it again to update.

To make a release, run `/release` in Claude Code, with `patch` (the default), `minor`, `major` or a version. It bumps the version, merges that through a pull request, tags main with `v<version>` and watches `.github/workflows/release.yml` build the files and make the release. The version is `version` in `[workspace.package]` of `Cargo.toml`, the only place it is written. `gh workflow run release.yml --ref <branch>` is a dry run: it builds the files and makes no release. The steps are in `.agents/skills/release/SKILL.md`.

## Two-minute tour

1. Click **Add track** under the track headers.
2. Double click empty space in the track row to add a clip, then double click the clip to open the note editor.
3. Double click in the clip to add a note. Drag to move it, drag its end to change its length.
4. Press space to play. Click the ruler to move the playhead.
5. Click a track name for its panel: the instrument, the effects rack and the mixer. Click `Synth` to pick another instrument or a plugin. **Add effect** adds an effect.
6. Drag a WAV or AIFF file from the Finder onto the arrangement for an audio clip.
7. Plug in a MIDI keyboard and play. Press `r` to record a take.
8. cmd-z undoes, shift-cmd-z redoes. cmd-q quits. There is no save; every edit is written to the folder.

Every mouse action and key is in [DESIGN.md](DESIGN.md), "Using the app".

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
   - `Add a drum track with a four-bar beat that ends in a fill`
   - `Put the file in assets/audio on a new audio track, so it starts at bar 3`
   - `The grid runs twice as fast as the music. Fix the fit`
4. The part shows up and plays while the agent still writes. One cmd-z in the app takes the whole request back.

What the agent uses:

- `AGENTS.md`. The app writes it into the project, with a `CLAUDE.md` that imports it. It is a short map: the folder layout, the bar math of this project, how to check the work, and a list of docs in `agent-docs/` with one line each saying when to open it. The record formats live in those docs, one per extension. Agents read the map by themselves and open only the doc their task needs.
- `problems.txt`. The app keeps it current while it runs. It lists every file that did not load, and why. `No problems. Every file is live.` means all of it plays. The agent reads it to check its work.
- `sound-tools . --inspect`. It prints the tempo, every track and every clip with its bar range, and the problems. It works next to the running app and loads no plugin. Install `sound-tools` once with **Install command line tool** in the project menu. From a checkout, `cargo run -p runtime -- . --inspect` is the same.

## Without the window

```sh
cargo run -p runtime -- ~/Music/my-piece --inspect
cargo run -p runtime -- ~/Music/my-piece --render /tmp/my-piece.wav
cargo run -p runtime -- ~/Music/my-piece --headless
cargo run -p runtime -- --plugins
cargo run -p runtime -- --version
```

With the command line tool installed, `sound-tools` takes the place of `cargo run -p runtime --` in all of these.

- `--inspect` prints a summary and changes nothing. It loads no plugin.
- `--render` writes a WAV offline at 48 kHz, stereo, 32-bit float. By default it plays to the end of the last clip, then keeps going until reverbs and releases are silent for half a second, at most 10 s more. `--from <ticks> --to <ticks>` renders a range the same way. `--seconds <n>` renders exactly the first n seconds. **Export audio…** and **Export selection…** in the project menu run the same render. Export selection covers the time from the first selected clip to the last one, with all tracks playing.
- `--headless` plays the project live without a window and reads commands from stdin: `play`, `pause`, `stop`, `seek <ticks>`, `undo`, `redo`, `status`, `quit`. It prints every change that arrives from the folder. Only one app can have a project open live. `--inspect` and `--render` work next to it.
- `--plugins` prints the installed CLAP and VST 3 plugins with their ids and kind (instrument, effect or both). It scans every plugin again, so it also retries one that failed before.
- `--version` prints the version.

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

CI runs the same on macOS, plus the realtime sanitizer, see [ENGINEERING.md](ENGINEERING.md). The tools come from `cargo install cargo-nextest cargo-shear cargo-deny typos-cli`. The build comes first because the tests load the repository's own CLAP and VST 3 test plugins, which `cargo test` does not build.

The two snapshot tests render the UI components and the window to PNGs without opening a window. They print the folder they write to. `cargo run -p gallery` opens the component gallery in a window.

## Docs

- [CONCEPT.md](CONCEPT.md): what the product is for.
- [ARCHITECTURE.md](ARCHITECTURE.md): the technical decisions and why.
- [ENGINEERING.md](ENGINEERING.md): how to work in this code: audio thread rules, testing, rules for agents.
- [DESIGN.md](DESIGN.md): the look, and every mouse action and key of the app.
- [crates/core/README.md](crates/core/README.md) and [crates/ui/README.md](crates/ui/README.md): how to write an extension and its view.
- `extensions/*/agent-doc.md`: the docs the app writes into each project for the agent.

## License

[MIT](LICENSE).

VST is a registered trademark of Steinberg Media Technologies GmbH.
