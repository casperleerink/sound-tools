# Performance harness

Measures release builds of git commits against each other. Python 3, no packages. Each commit is
built once in its own worktree under `/tmp/st-perf/<sha>/` (`PERF_DIR` moves it), and its
projects are made by its own runtime from its own tool files. Runs use a temporary `HOME`.

```sh
python3 tooling/perf/bench.py render main HEAD            # CPU of a 61 s offline render, median of 3
python3 tooling/perf/bench.py live main --window 60       # headless playback at -60 dB: CPU, memory, late callbacks
python3 tooling/perf/bench.py null main HEAD              # same sound? peak difference of 20 s renders
python3 tooling/perf/bench.py profile main --projects mixed   # macOS `sample` of a render, hottest functions
```

`--projects a,b` picks projects (see `projects.names()`; live also takes `<project>+edit`, which
changes a choice of the gravity harp every 5 s so its sound is built again while it plays).
`--runs` and `--seconds` change the counts. Raw numbers go to `/tmp/st-perf/results/`, runtime
output to `/tmp/st-perf/logs/`. Results worth keeping go in [results.md](results.md).

`tools/` holds TypeScript rebuilds of built-in instruments, played on the same notes as the
built-in to compare their cost. Remove `/tmp/st-perf` and run `git worktree prune` to clean up.
