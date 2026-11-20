# Performance harness

Measures release builds of git commits against each other. Results go in the pull request.
Python 3, no packages (numpy for `reverb.py`).

```sh
python3 tooling/perf/bench.py render main HEAD            # CPU of a 61 s offline render, median of 3
python3 tooling/perf/bench.py live main --window 60       # headless playback at -60 dB: CPU, memory, late callbacks
python3 tooling/perf/bench.py null main HEAD              # same sound? peak difference of 20 s renders
python3 tooling/perf/bench.py profile main --projects mixed   # macOS `sample` of a render, hottest functions
python3 tooling/perf/bench.py render main HEAD --projects builtins --runs 5   # every built-in device
python3 tooling/perf/reverb.py main HEAD                  # the reverb of B against A
```

The window, headless, on a release build of this checkout:

```sh
python3 tooling/perf/window.py            # compares with window.json, the last saved run
python3 tooling/perf/window.py --save     # and keeps this run as window.json
```

Each scenario (idle, play, scroll, zoom, drag a clip, the note editor, the mixer playing, a
fader's drag) is a Rune `--script` of 3 s on the DAW template and on a song of 64 tracks and 500
clips, and a project of 64 lanes of 200 points gets idle, scroll, zoom and an opened lane. The table
has Rune's frame times, the first frame, the process's CPU and memory, and memory with Bun; `--play
60` is the app's own playback. Last, the big song plays, scrolls and zooms for 10 s and for 120 s:
memory that grew between them is a leak. Frame times are wall time: on a busy machine they double,
so compare runs of a similar load average. Headless, every frame builds the whole window, which a
window at rest does not. Rejected: `--native --trace`, which opens a window.

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
