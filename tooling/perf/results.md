# Performance results

Newest first. Apple M1 Max, macOS. How to run: [README.md](README.md).

## 2026-10-09: baseline, main at 1a1c1e2 (Hum interpreted per sample)

Render: `bench.py render origin/main --runs 5`, 61 s, median of 5. The machine was not idle;
spread is how far apart the 5 runs were.

| project | dsp % of a core | spread % | startup cpu s | peak MB |
| --- | ---: | ---: | ---: | ---: |
| builtin12 | 9.92 | 12 | 0.02 | 50 |
| one-acid-bass | 2.59 | 4 | 0.05 | 46 |
| one-dorian-drift | 4.22 | 5 | 0.05 | 50 |
| one-gravity-harp | 5.48 | 4 | 0.05 | 50 |
| one-storm | 5.19 | 8 | 0.05 | 50 |
| one-life | 10.69 | 5 | 0.05 | 47 |
| one-grain-cloud | 15.83 | 16 | 0.08 | 51 |
| mixed-base | 4.76 | 29 | 0.01 | 33 |
| **mixed** | **48.37** | 10 | 0.10 | 64 |
| mixed-x2 | 96.43 | 7 | 0.11 | 106 |
| instrument.synth-arp | 0.23 | 18 | 0.01 | 13 |
| instrument.synth-chords | 0.31 | 9 | 0.01 | 13 |
| ts-synth-arp | 2.03 | 27 | 0.05 | 46 |
| ts-synth-chords | 2.49 | 20 | 0.05 | 46 |

The ts-synth rebuild costs **9.0x** the built-in synth on the arp, **8.1x** on the chords.
Each `one-` project has a reverb and a delay on its track.

Live: `bench.py live origin/main --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| builtin12 | 12.1 | 0.0 | 59 | 0 | 0, 0, 0 | 0, 0, 0 | 3.3, 3.1, 4.0 ms |
| mixed | 49.1 | 0.1 | 129 | 56 | 1, 1, 1 | 1, 1, 1 | 11.5, 11.8, 11.2 ms |
| mixed+edit | 48.2 | 0.2 | 179 | 56 | 1, 1, 1 | 1, 1, 1 | 11.2, 11.3, 11.2 ms |
| mixed-x2 | 56.6 | 0.1 | 171 | 56 | 1616, 1434, 1367 | 2302, 2206, 2342 | 15.5, 15.1, 15.5 ms |

- mixed: the one late callback and xrun come at startup; a session that never plays has them too.
- mixed-x2 needs a whole core, so the audio thread cannot keep up: about 40% of callbacks are lost,
  which is also why its live CPU (57%) is below its render CPU.
- mixed+edit made 11 sound rebuilds per run (13 engine edits, against 2 without).

Profile: `bench.py profile origin/main --projects one-grain-cloud,mixed`, busy samples.

- one-grain-cloud: 89% in `sound_hum::Machine::run`, then `sinf`, `powf`, `fmodf`, `cosf` (6%) and the reverb (2.5%).
- mixed: 76% in `Machine::run`, 10% in the reverbs (12 of them), 4% `sinf`; the built-in synth
  and wavetable together under 4%.
- `Machine::run` is one inlined loop, so the profile does not split it into operations.

Null test: two renders of one commit are bit for bit equal (noise is seeded), so any difference
between commits is real. gravity-harp renders silence: it only plucks from its page.
