# GPUI side: facts

App: `gpui-daw/src/main.rs` (one file, ~560 lines). gpui at the root's rev, Cargo's default
release profile (not the root's thin LTO). Target dir `/tmp/spike-target-gpui`.

```sh
B=/tmp/spike-target-gpui/release/gpui-daw
$B --scenario play --tracks 8 --clips 32 --seconds 10     # window; prints one JSON line on quit
$B --screenshot out.png --scenario play --at 3.3            # offscreen PNG at 2x, no window
```

## Numbers (M-series Mac; the Rune build ran at the same time, so times are noisy)

| | |
| --- | --- |
| Cold release build, sccache off, empty target | 110 s |
| Same with the user's sccache on (first run) | 198 s |
| Binary | 10.9 MB, 8.4 MB stripped |
| One-line colour edit, release rebuild (3 runs) | 4.84 / 4.64 / 4.63 s, median 4.64 s |
| Edit to PNG on disk (rebuild + `--screenshot`) | 5.65 / 5.40 / 5.36 s, median 5.40 s |
| `--screenshot` alone | 0.14 to 0.64 s |

3 s real-window runs (8 tracks, 32 clips): idle 2 frames / 2 timeline renders; play 176 / 2;
scroll 175 / 175; zoom 176 / 176; edit 176 / 31. Play at 32 tracks, 512 clips: 176 / 2.
So the light layer holds ~59 fps and the heavy view renders only on scroll, zoom and edits.

## Agent experience

- Built on the 2nd compile. One borrow error (a header iterator borrowed `song`, which the
  canvas closure then moved) and one deprecation (`on_scroll_wheel` is now `on_scroll`).
- Most time went to looking up APIs in the Zed checkout: `quad`, `shape_line` + `paint`,
  `HeadlessAppContext::with_platform`, `cached`. The repo's gpui skills covered the model and
  the screenshot path; online gpui examples would have been wrong (renamed APIs).
- Without a window I could check: the picture at any time of a scenario (PNG). I could not
  check: frame pacing, the 16 ms poll, wheel scrolling, clicks. GPUI has no element tree dump
  and no scripted input in a release binary; that needs a `#[gpui::test]` with
  `VisualTestContext` (`simulate_click`, `debug_bounds`), written per check.

## What could make the comparison unfair

- The poll is a 16 ms executor timer, not vsync: ~58.7 frames a second, and it still wakes
  60 times a second when idle (as the real app's `Session` does), drawing nothing.
- `--screenshot` jumps the scenario straight to `--at`; nothing plays in between.
- Clip names and ruler numbers are shaped in the canvas paint each frame (gpui caches the
  shaped lines); headers and mixer strips are divs.
- At 32 tracks the mixer strips past the window are clipped, not scrollable; rows scroll only
  with the wheel.
