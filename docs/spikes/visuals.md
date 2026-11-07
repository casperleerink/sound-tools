# Visuals next to sound

## Goal

Tools of a project make pictures as well as sound, so agents build interactive sound and visual pieces, as Max with Jitter does. The app looks the same until a tool uses them.

## Decided constraints

- **One language.** A canvas is built from the same `Signal` functions as a sound. An agent who writes sounds writes canvases.
- **Motion never crosses to Bun.** A canvas runs in the window on every step, as live values do; its code runs once to build it.
- **Sound and canvas talk through what exists:** `output` and `now.value` one way, number inputs the other. Neither needs the other.
- **The core knows no canvases.** Nothing in `crates/core`.

## How to verify

The agent evals `visuals-builder` and `video-builder` (`tooling/agent-eval/`) build from the docs alone; they passed 4 runs of 4. `--screenshot <png> --seconds 3` shows what moves.

## The primitives

| Part | What a tool writes |
| --- | --- |
| `Canvas` | `<Canvas pixels={(pixel) => ({ red, green, blue, alpha })}>`: a graph per pixel, from `pixel.x`, `y`, `aspect`, `time`, `noise(x, y, z)` |
| Reads | `pixel.live(value)` any live value; `pixel.wave(list, at)` such as `now.spectrum(meter)`; `pixel.video(file, { at })`; `pixel.photo(file)`; `pixel.canvas(key)` another canvas; `pixel.last(x, y)` the step before, for feedback |
| Plays the sound | `pixel.measure(input, value)`: the mean over the canvas moves a number input, as a knob does |
| Stage | `<Stage>`: part of a page in a window the composer moves, sizes and puts full screen on any screen; closed, it shows in the page |
| Live values | `now.seconds`, `now.spectrum(meter)`, `now.space(key).pointerX`, `pointerY`, `pointerOver` |
| Drops | Films go to `assets/video/`, photos to `assets/images/`, named in `videos` and `photos` |

## Decisions

- **A graph per pixel on the GPU on macOS, else the CPU.** `crates/canvas/src/gpu.rs` writes the graph as a Metal compute shader with the CPU's maths, and a test holds both equal. The canvas paints as a GPUI image, so screenshots work alike. 1920 by 1080 pixels of 60 sines: 1.5 ms on the GPU, 35 ms on all CPU cores; a canvas may be 1920 across there, 480 on the CPU. Rejected: `paint_surface`, which GPUI takes only as video (YCbCr), without alpha.
- **Its own small evaluator**, not the signals machine: a pixel has no time between its neighbours, so filters, delays and phasors make no sense in it. The SDK refuses them with a sentence that says so.
- **Named `Canvas`**: an area a tool fills every frame, whatever fills it. Rejected: `Picture`, which sounds still.
- **A canvas steps 60 times a second of its time**, whatever the screen draws, so feedback looks alike on any screen and in a screenshot.
- **Video by the system:** AVFoundation reads frames in order on a thread per video and starts again only on a jump. A 1080p film costs about a tenth of a core. Rejected: seeking for each frame (`AVAssetImageGenerator`). Linux and Windows show no video.
- **The stage is the composer's window**, kept where they left it per project on this machine (`.sound-tools/stages.json`): screen, size, full screen, closed. Its bar is drawn over it: GPUI keeps a window's frame but opens one by its content. Rejected: opening full screen on a second screen by itself.
- **Left out:** filming the window with its sound (`--render-video`) and a film on the DAW's timeline. Both worked as prototypes; they are product decisions, not primitives.

## Open

- The GPU: the shader could write the bytes GPUI paints, not the CPU; Linux and Windows could use wgpu.
- Camera input: as a video, from `AVCaptureSession`; it needs the camera permission, so it was not tried.
- A canvas another reads must show to step; a hidden one could step too.
- No keys on the stage, and screenshots show the main window only.
