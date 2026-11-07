# Spike: pictures next to sound

## Goal

Tools of a project make pictures as well as sound, so Sound Tools builds interactive sound and visual pieces, as Max with Jitter does. Prototypes, one branch each, stacked.

## Decided constraints

- **One language.** A picture is built from the same `Signal` functions as a sound. An agent who can write a sound can write a picture.
- **Motion never crosses to Bun.** A picture runs in the window on every frame, as live values do; code runs once to build it.
- **Sound drives pictures through what exists:** `output` in the sound, `now.value` in the view.
- **The core knows no pictures.** Nothing in `crates/core`.

## How to verify

Open a demo project under `docs/prototypes/` with `cargo run -p runtime -- docs/prototypes/<name>`. It plays a sound and shows a picture that follows it.

## Prototypes

### 1. `Picture`, a graph per pixel (`t3/visual-picture`)

A shape whose colour per pixel is a signal graph: `pixel.x`, `pixel.y`, `pixel.time`, `pixel.live(value)` and `pixel.last(x, y)`, the frame before, for feedback. Run on the CPU, a row at a time, in `crates/pictures` (optimized in dev). Demo: `docs/prototypes/glow`.

- Cost: a 240 pixel wide picture at 120 frames a second is about 14% of one core on an M-series Mac.
- Rejected for now: a GPU shader. GPUI draws no shader of ours; a Metal texture would need `paint_surface` and macOS-only code. The graph is the same either way, so a GPU backend can come later without changing the SDK.
- Open: the picture runs on the main thread. Past 480 across it should move to a thread.

### 2. Pictures read lists: spectrum and scope (`t3/visual-picture-waves`)

`pixel.wave(list, at)` reads any live list in a picture. New list `now.spectrum(meter)`: 20 Hz to 20 kHz, 0 to 1, made from the meter's scope once a frame; a `Wave` draws it too. Demo: `docs/prototypes/spectra`, a scrolling spectrogram made with `last`, with the scope as a `Wave` over it.

- A picture paints after lines and text in its layer, as GPUI orders images, so shapes after it show over it.

### 3. Screenshots after a while (`t3/visual-screenshot-seconds`)

`--screenshot <png> --seconds <n>` runs the window n seconds, drawing every poll, before the picture. An agent sees what a picture, a scope or a meter does over time, as vork's loop of render and look.

### 4. Stage, a window for the audience (`t3/visual-stage`)

`<Stage>` is a div of a page shown in a window of its own: fullscreen on a second screen, else floating in a corner as a preview. The rest of the view stays in the app. Drawn from the page's nodes, as the title bar is. Demo: `docs/prototypes/stage`.

- Open: a stage closed by hand comes back only when the view drops and adds it again. No keys on the stage.

### 5. Video in pictures (`t3/visual-video`)

`pixel.video(asset, { at, x, y })` reads a video under `assets/video/`, decoded by macOS (AVFoundation) on a thread per video; the picture shows the newest frame decoded. `now.seconds` is the playhead in seconds, to keep a video with the piece. Demo: `docs/prototypes/video`, a video folded in a mirror whose colours split on each kick.

- Cost: decoding by seeking to each frame (`AVAssetImageGenerator`) is about a third of a core at 30 frames a second. Open: read frames in order (`AVAssetReader`) and seek only on a jump.
- Open: Linux and Windows decode no video.
