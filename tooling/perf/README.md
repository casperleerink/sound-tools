# Performance harness

Measures release builds of git commits against each other. Python 3, no packages (numpy for
`reverb.py`). Each commit is
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

## Built-in devices

`--projects builtins` is every `bi-` project (see `projects.BI_TRACKS`): 4 tracks of one device in
a typical use. An effect sits on the synth playing chords, so its own cost per instance is
`(bi-<effect> - bi-synth) / 4`, and an instrument's `(bi-<instrument> - empty) / 4`. A `-tail`
project plays the first bar only, so what it costs over `bi-synth-tail` is the device's tail and
then its idle check. `bi-heavy` is 24 synth and wavetable tracks through reverb, delay, EQ and
compressor; `bi-heavy-idle` is the same with no clips. Built-ins must stay bit for bit the same
(`null`); only the reverb may change its sound, within the limits below.

```sh
python3 tooling/perf/bench.py render main HEAD --projects builtins --runs 5
python3 tooling/perf/bench.py profile main --projects bi-reverb,bi-heavy
python3 tooling/perf/reverb.py main HEAD     # the reverb of B against A
```

`reverb.py` renders a click through a wet reverb (`projects.REVERBS`: default, a long hall, a
short room) and the default reverb on synth chords and on drums, on both commits, into
`/tmp/st-perf/reverb/<a>-<b>/` to listen to. Per octave band from 125 Hz to 8 kHz it reports T30
and early decay time from the Schroeder curve, energy and left/right correlation; and the time
the echo density (Abel and Huang) reaches 0.95, peak and RMS level, the highest narrow peak of
the tail's spectrum over the third of an octave around it, and the null difference. B passes
when, against A, for every setting:

- T30 per band within ±10%
- energy per band within ±1 dB
- echo density at 0.95 no later than A + 20 ms
- correlation per band within ±0.1
- the highest narrow tail peak at most A + 3 dB (ringing)

`tools/` holds TypeScript rebuilds of built-in instruments, played on the same notes as the
built-in to compare their cost. Remove `/tmp/st-perf` and run `git worktree prune` to clean up.
