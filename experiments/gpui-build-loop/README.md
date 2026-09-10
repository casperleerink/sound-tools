# GPUI build-loop experiment

Run on Casper's Mac on September 9, 2026. This is an isolated experiment, not the Sound Tools SDK or application.

The small all-Rust extension loop is workable on this machine. Seven one-line extension edits took a median 2.24 seconds from writing source to the replacement runtime's first GPUI frame callback. This supports continuing with Rust and GPUI provisionally. It does not establish build times for a large runtime or optimized audio code.

| Measurement | Median | Range |
| --- | ---: | ---: |
| Incremental build including relink | 1.317 s | 1.248–1.529 s |
| Linker driver, included in build above | 0.275 s | 0.260–0.340 s |
| Stop old process through new first-frame callback | 0.895 s | 0.855–1.056 s |
| Source edit through new first-frame callback | 2.243 s | 2.148–2.422 s |

A clean build into an empty target directory took **55.590 seconds**, with dependency downloads already cached. This excludes registry download time. See [clean-build evidence](results/clean-build.json).

A no-change build took 0.363 seconds and rebuilt nothing. A later restoration build outside the seven-run series took 4.079 seconds. These are observations on a working Mac, not a guaranteed latency budget.

## Setup and measurement

- Apple M1 Max, 10 CPU cores, 32 GB RAM; macOS 26.6.2, build 25G83.
- Rust and Cargo 1.95.0, native aarch64-apple-darwin target; GPUI pinned to 0.2.2 with Cargo.lock retained.
- One runtime crate directly depends on GPUI and a separate extension crate. The extension also depends on GPUI and contains the custom view.
- Unoptimized Cargo dev profile with incremental compilation and `debug = 0`. Full debug symbols and release builds were not measured.
- GPUI `runtime_shaders` enabled. Shader setup is included in process startup. The machine lacked Xcode's Metal compiler component; no Xcode installation was changed.
- Every measured edit changes one extension source line, a revision string displayed in the window. The old process stays alive during the build. After success, the runner terminates it, launches the new executable, and checks its revision in `FIRST_FRAME`.
- Readiness uses GPUI's `Window::on_next_frame` callback, registered on the first render. This measures a GPUI frame callback, not physical display presentation or audio readiness.
- `linker.py` wraps `/usr/bin/clang` and measures its elapsed time. Its duration is part of the total Cargo build time. The remainder also includes Cargo, Rust compilation, process startup and wrapper overhead.
- All seven runs rebuilt only `timing_extension` and `sound-tools-timing`; GPUI and other dependencies were fresh. See [dependency reuse](results/dependency-reuse.json) and [raw measurements](results/measurements.json).

## Editor authoring and verification

A separate agent read the official [GPUI overview](https://docs.rs/gpui/0.2.2/gpui/), [Hello World example](https://gpui.rs/), and versioned Context and StatefulInteractiveElement documentation. It produced two independently editable pulse cards with step toggles and level controls in about 1.5 minutes of reading and authoring.

The custom view compiled on its first source attempt, without API corrections. That integrated build took 2.151 seconds, including a 0.370-second link. The separately authored host needed one correction for the WindowOptions API and missing AppContext trait import. Dependency and environment failures are recorded separately in [the authoring log](AUTHORING.md).

Native mouse checks passed. Editing A left B unchanged, then editing B preserved A's changes. [The screenshot](results/independent-editors.png) shows A at 60%, B at 40%, and different enabled steps. The accessibility tree contained the window controls but none of these custom controls. Accessible controls remain unproven and would need explicit work.

An intentional `compile_error!` failed in 0.486 seconds. The existing process stayed alive, the executable's SHA-256 stayed identical, and that executable launched again to its first frame. See [failure evidence](results/failure-check.json). This verifies the simple local build failure case, not recovery from a failed linker or a runtime crash.

## Setup failures and limits

The sandbox initially blocked registry DNS access and native window startup. Retrying with approved network/desktop access allowed both. The first dependency build then failed after 84.321 seconds because the Metal Toolchain was missing. Enabling the crate's existing `runtime_shaders` feature resolved that failure. The next build reached the host API errors after 11.675 seconds; the corrected host and first custom view then compiled successfully. These failed setup runs are not the clean-build result.

There is no audio engine, project restoration, outer agent process, shutdown flush protocol or SDK in this experiment. Restart uses process termination and resets editor state. It verifies the requested compile/relink/window-restart loop, not the complete future reload contract. Rich editor gestures, large extension sets, release optimization, Windows and Linux remain untested.

## Reproduce

Copy this folder to a throwaway directory. Rust, macOS desktop access and Cargo dependencies are required. The lockfile pins this run's dependency selection. The scripts modify only this experiment's extension and terminate only the processes they launch.

```sh
cargo fetch --locked
python3 -c 'from measure import build; print(build("bootstrap"))'
python3 measure.py --runs 7
python3 verify_failure.py
```

The bootstrap build must succeed before running measurements. `measure.py` records each run immediately and restores source on exit. Its last compiled executable still contains the last measured revision; a later build restores the original revision. Existing result files with matching names are overwritten, so keep the original results if comparing runs.

For a clean build with cached downloads, use an empty target directory:

```sh
cargo build --offline --locked --target-dir target-clean --timings
```

The first clean build needs no separate Metal compiler because `runtime_shaders` is enabled. Build caches are excluded from this folder's retained evidence.
