# Performance harness

Measures release builds of git commits against each other. Results go in the pull request.
Python 3, no packages (numpy for `reverb.py`).

```sh
python3 tooling/perf/bench.py render main HEAD            # CPU of a 61 s offline render, median of 3
python3 tooling/perf/bench.py live main --window 60       # headless playback at -60 dB: CPU, memory, late callbacks
python3 tooling/perf/bench.py window main HEAD            # the window, stopped or `+play`: CPU, memory (macOS, takes the keys)
python3 tooling/perf/bench.py null main HEAD              # same sound? peak difference of 20 s renders
python3 tooling/perf/bench.py profile main --projects mixed   # macOS `sample` of a render, hottest functions
python3 tooling/perf/bench.py render main HEAD --projects builtins --runs 5   # every built-in device
python3 tooling/perf/reverb.py main HEAD                  # the reverb of B against A
```

`--projects a,b` picks projects (`projects.names()`); `--runs` and `--seconds` change the
counts. Builds, projects and logs go to `/tmp/st-perf` (`PERF_DIR` moves it). Remove it and run
`git worktree prune` to clean up.

Built-ins must stay bit for bit the same (`null`). Only the reverb may change its sound. B
passes `reverb.py` when, against A, for every setting:

- T30 per band within ±10%
- energy per band within ±1 dB
- echo density at 0.95 no later than A + 20 ms
- correlation per band within ±0.1
- the highest narrow tail peak at most A + 3 dB (ringing)

## The window

```sh
python3 tooling/perf/ui.py                        # the song of the DAW template, 8 tracks of 32 clips
python3 tooling/perf/ui.py path/to/project        # any project; the song's own scenarios are skipped
python3 tooling/perf/ui.py --against before.json  # and compares with a saved run
```

It runs each scenario of `crates/runtime/tests/window/tools/scenarios.rs` in a new test window
(offline engine, simulated polls, the system's text, no GPU) and writes JSON: per update, time,
draw time and allocations of the window's thread; renders and own render time per part and
instance; gpui's self time per view type (phases of 0.5 ms or more); nodes per card. Renders
and allocations repeat exactly; times move about 15% between runs.

It builds the `perf` profile with the runtime's `perf` feature, which rebuilds gpui, so
`cargo verify` never sets it. Rejected: `#[gpui::bench]`, which times one view under Criterion
in real time; the scenarios need Bun, the offline engine and polls stepped by hand.
