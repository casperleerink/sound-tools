# Spike: pictures next to sound

## Goal

Tools of a project make pictures as well as sound, so Sound Tools builds interactive sound and visual pieces, as Max with Jitter does.

## Decided constraints

- **One language.** A picture is built from the same `Signal` functions as a sound. An agent who writes sounds writes pictures.
- **Motion never crosses to Bun.** A picture runs in the window on every step, as live values do; its code runs once to build it.
- **Sound and picture talk through what exists:** `output` and `now.value` one way, number inputs the other.
- **The core knows no pictures.** Nothing in `crates/core`.

## How to verify

Each demo under `docs/prototypes/` opens with `cargo run -p runtime -- docs/prototypes/<name>`: `glow`, `spectra`, `stage`, `video`, `eyes`, `smoke`, `ripples`, `liquid`. `--screenshot <png> --seconds 3` and `--render-video <mp4> --seconds 6` show them without a window.

The agent evals `visuals-builder` and `video-builder` passed 4 runs of 4 (3 to 8 minutes, about $1 each): from the docs alone an agent built a drone with flowing colours on the `Stage` and a preview on the page, and a video kaleidoscope whose light and movement play a pad (`tooling/agent-eval/examples/`).

## What it is

Branches `t3/visual-*`, stacked; PRs #392 to #410, `t3/visual-gpu-all` on top.

| Part | What a tool writes |
| --- | --- |
| `Picture` | `<Picture pixels={(pixel) => ({ red, green, blue, alpha })}>`: a graph per pixel, from `pixel.x`, `y`, `aspect`, `time`, `noise(x, y, z)` |
| Reads | `pixel.live(value)` any live value; `pixel.wave(list, at)` a list such as `now.spectrum(meter)` or `now.scope(meter)`; `pixel.video(file, { at })`; `pixel.photo(file)`; `pixel.picture(key)` another picture; `pixel.last(x, y)` the step before, for feedback |
| Plays the sound | `pixel.measure(input, value)`: the mean over the picture moves a number input, as a knob does |
| Stage | `<Stage>` shows part of a page in its own window: full screen on a second screen, else floating in a corner |
| Time | `now.seconds` follows the piece; the DAW song's `video` field shows a film at the playhead |
| Pointer | `now.space(key).pointerX`, `pointerY`, `pointerOver` |
| Output | `--render-video` films the window with its sound, frame by frame |

## Decisions

- **A graph per pixel on the GPU on macOS, else the CPU.** `crates/pictures/src/gpu.rs` writes the graph as a Metal compute shader, line for line the CPU's maths, and a test holds both pictures and measures equal. It writes to memory the CPU reads as it is, and the picture paints as a GPUI image, so screenshots and films work alike. 1920 by 1080 pixels of 60 sines: 1.5 ms on the GPU, 35 ms on all CPU cores; a picture may be 1920 across there, 480 on the CPU. The video demo went from 88% of a core to 40%. Rejected: `paint_surface`, which GPUI takes only as video (YCbCr), without alpha.
- **Its own small evaluator**, not the signals machine: a pixel has no time between its neighbours, so filters, delays and phasors make no sense in it. The SDK refuses them with a sentence that says so.
- **A picture steps 60 times a second of its time**, whatever the screen draws, so feedback looks alike on any screen, in a screenshot and in a film.
- **Painted as a GPUI image**, freed by the window it was painted in. Under lines and text drawn after it, as GPUI orders images.
- **Video by the system:** AVFoundation reads frames in order on a thread per video and starts again only on a jump. A 1080p film costs about a tenth of a core. Rejected: seeking for each frame (`AVAssetImageGenerator`). Linux and Windows show no video.
- **A film is the offscreen window stepped by its clock:** each frame is one thirtieth of a second of offline sound, and videos wait for their frame, so picture and sound keep time and repeat.

## Open

- The GPU: the shader could write the bytes GPUI paints, not the CPU; Linux and Windows could use wgpu.
- Camera input: as a video, from `AVCaptureSession`; it needs the camera permission, so it was not tried.
- A picture another reads must show to step; a hidden one could step too.
- A stage closed by hand comes back only when the view drops and adds it again; no keys on the stage. A film shows the main window, not the stage.
- The DAW has no way in the window to pick its film; the agent sets `video`.
- `measure` of a live value is a way round to play an input from the pointer; a space could say which live values play which inputs.

## Recommendation

Keep `Picture` with `live`, `wave`, `last`, `noise`, `alpha`, `measure`, the GPU path with the CPU behind it, and `--screenshot --seconds`: in the grain of the SDK, and agents used them well from the docs alone. Then `Stage` and video. Treat `--render-video` and the DAW film as product decisions.
