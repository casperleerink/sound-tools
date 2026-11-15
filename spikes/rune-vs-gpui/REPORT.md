# Spike: Rune instead of GPUI

The DAW song page (SPEC.md) built twice, the way each framework says is fastest, on the same
data (`model/`): `gpui-daw/` (gpui at the root's rev) and `rune-daw/` (Rune `7a92d24`). M-series
MacBook Pro, 120 Hz display, release builds, both apps updating at 60 Hz.

## Performance (`measure.py`, real windows, process CPU, median of 3)

DAW template size: 8 tracks, 32 clips (~2,900 notes).

| scenario | GPUI CPU | Rune CPU | GPUI fps | Rune fps | GPUI MB | Rune MB |
| --- | --- | --- | --- | --- | --- | --- |
| idle | 1.4% | 0.0% | 0 | 0 | 79 | 97 |
| play (playhead, meters, time) | 23% | 5% | 60 | 57 | 80 | 97 |
| scroll | 10% | 5% | 60 | 56 | 79 | 98 |
| zoom (view rebuilt every frame) | 24–27% | 9–19% | 60 | 55 | 80 | 99 |
| edit (a clip moves every 100 ms) | 16–27% | 7–12% | 60 | 56 | 80 | 97 |

Stress: 32 tracks, 512 clips (~46,000 notes).

| scenario | GPUI CPU | Rune CPU | GPUI fps | Rune fps |
| --- | --- | --- | --- | --- |
| play | 23% | 10% | 60 | 56 |
| scroll | 27% | 15% | 60 | 55 |
| zoom | 31–35% | 25–32% | 60 | **47–48** |
| edit | 24% | 11% | 60 | 56 |

- Rune uses 2–5x less CPU wherever only some things move. Its `Live` values move the
  playhead, meters and scroll without running the view. GPUI redraws the whole window every
  frame, even with the heavy views `.cached`.
- Rune's weak spot: the whole view rebuilt every frame on a big song. Zoom drops to ~47 fps.
  Frame CPU is low (p50 2.5 ms, from `--native --trace`), but ~30% of frames miss the refresh,
  so the limit is getting a fully new scene to the screen. One canvas per lane instead of per
  clip lowered CPU, not the dropped frames.
- Rune's 55–57 fps comes from my 60 Hz limit landing on a 120 Hz display (play is 5% CPU),
  not from load. Only big-song zoom is a real drop.
- Rune uses ~20 MB more memory (35–45 MB more on the big song).
- Runs differ by up to ±50% (GPUI edit: 16% then 27%). Only the 2x gaps are solid. GPU and
  WindowServer time are not counted.

## Working with an agent

| | GPUI | Rune |
| --- | --- | --- |
| First build of the app | 2nd attempt (borrow error, renamed API) | 1st attempt |
| See the screen without a window | `--screenshot`, written by us | `--snap` built in |
| Element tree (positions, text, colours) | none; a `#[gpui::test]` per check | `--tree` built in |
| Scripted clicks, keys, scroll | none in the binary | `--script` built in, by element key |
| Click test in `cargo test` | `#[gpui::test]` + `VisualTestContext` | `Runtime::new`, `find`, `pointer_down`; 0.4 s |
| One-line edit to PNG, debug | 4.4 s | 5.7 s |
| One-line edit to PNG, release | 4.5 s | 11.5 s |
| Cold release build | 110 s | ~4 min (incl. git fetch) |
| Binary | 10.9 MB | 28.9 MB (with agent tooling) |

Traps hit in Rune: colours animate by default, so a test reads a fill only after the fade
settles; setting a `Live` value wakes the next display frame (120 Hz) unless the app limits it.
Both apps were written by agents from docs: Rune's README, and for GPUI our skills plus reading
Zed's source.

## Moving the app (read from code, not built)

- Size: 201 files, ~85,000 lines touch gpui: runtime 31k, ui 16k, typescript (the tree that
  draws TSX tools) 14k, extensions 12k, agent 9k.
- Different model: one app state and `fn view(&State) -> El`, not entities that each notify.
  View state such as selection and zoom moves into app state. Rune keeps scroll itself.
- Plugin windows: Rune gives no public `NSView` of its windows. Plugin windows need our own
  AppKit/Win32 window code, or a Rune issue. Rune takes no pull requests.
- Pre-release: pin a `dev` rev and repeat Rune's five crate patches. Toolchain 1.99.1 (ours
  is 1.98.1 for gpui).

## Verdict

- Performance on the DAW template is not a reason to stay with GPUI. Rune was cheaper in every
  scenario at template size and held 55–57 fps where GPUI held 60.
- Agent checks are much better in Rune: tree, scripts and a cheap test runtime come built in.
  With GPUI we write and maintain these ourselves.
- Risks: views that rebuild every frame on big songs (fix: keep zoom and scroll out of the
  view, as with `Live`); a port of ~85k lines; plugin windows; a pre-release dependency.
- Next step if we go on: port one real tool view and the TSX tree renderer (`crates/typescript`
  card) onto Rune behind the same `Session`, and measure the real DAW template with
  `tooling/perf`.
