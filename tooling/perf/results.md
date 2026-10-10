# Performance results

Newest first. Apple M1 Max, macOS. How to run: [README.md](README.md).

## 2026-10-10: graph runtime, 18e97e0 (t3code/native-typed-graph-runtime)

What changed since 2bb342a: mono code runs on the right channel only the operations that differ
from the left, and stateful operations keep their memory in locals over a span.

Null test against main: every `one-` and ts-synth project is still bit for bit equal (-inf dB).

Render: `bench.py render origin/main 2bb342a 18e97e0`, 61 s, median of 3, interleaved (mixed and
mixed-x2 each in their own run of the command). Spread was 0 to 3% (8% on the tiny synth-chords).

| project | main | 2bb342a | now | main / 2bb342a | main / now |
| --- | ---: | ---: | ---: | ---: | ---: |
| builtin12 | 9.86 | 9.78 | 9.80 | 1.0x | 1.0x |
| one-acid-bass | 2.59 | 1.30 | 0.94 | 2.0x | 2.8x |
| one-dorian-drift | 4.17 | 2.28 | 1.37 | 1.8x | 3.0x |
| one-gravity-harp | 5.41 | 3.56 | 2.86 | 1.5x | 1.9x |
| one-storm | 5.11 | 2.96 | 1.99 | 1.7x | 2.6x |
| one-life | 10.48 | 3.96 | 1.96 | 2.6x | 5.3x |
| one-grain-cloud | 15.54 | 7.56 | 7.24 | 2.1x | 2.1x |
| mixed-base | 4.66 | 4.67 | 4.70 | 1.0x | 1.0x |
| **mixed** | 47.02 | 25.53 | **20.30** | 1.8x | **2.3x** |
| mixed-x2 | 94.04 | 50.97 | 40.62 | 1.8x | 2.3x |
| instrument.synth-arp | 0.22 | 0.22 | 0.22 | 1.0x | 1.0x |
| instrument.synth-chords | 0.30 | 0.30 | 0.30 | 1.0x | 1.0x |
| ts-synth-arp | 2.01 | 0.99 | 0.49 | 2.0x | 4.1x |
| ts-synth-chords | 2.45 | 1.16 | 0.55 | 2.1x | 4.5x |

- ts-synth / built-in synth: **2.2x** on the arp, **1.8x** on the chords (main 9.2x, 8.3x;
  2bb342a 4.5x, 3.9x; target 2x). The chords meet it, the arp is just over.
- mixed: **2.3x** less CPU than main, 20.3% of a core (target 3x, about 16%). Without the
  built-ins (mixed minus mixed-base) the tools cost 2.7x less than on main.
- one-grain-cloud barely moved since 2bb342a (7.56 to 7.24).

Live: `bench.py live 18e97e0 --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 23.3 | 0.1 | 135 | 60 | 0, 0, 0 | 0, 0, 0 | 7.1, 7.2, 6.5 ms |
| mixed+edit | 23.4 | 0.2 | 186 | 60 | 0, 0, 0 | 0, 0, 0 | 6.4, 7.4, 8.6 ms |
| mixed-x2 | 41.8 | 0.1 | 177 | 60 | 1, 1, 1 | 1, 1, 1 | 11.9, 11.9, 12.0 ms |

- The first live runs of mixed and mixed+edit gave 30.3% and 30.4%, with tight runs, while
  mixed-x2 was in line with its render. Rerun: mixed against 2bb342a interleaved gave 28.4% for
  2bb342a and 23.3% now, and mixed+edit gave 23.4%. The table has the reruns.
- mixed: 49.1% on main, now 23.3% (2.1x less). mixed-x2's one late callback and xrun are at
  startup, as before.

Profile: `bench.py profile 18e97e0 --projects mixed,one-grain-cloud`, busy samples.

- mixed: `Machine::render` 38%, the reverbs 22%, `apply_binary` 16%, wavetable voice 3.1%, `powf` 3.0%;
  then `_platform_memcmp` 2.9% (1.9% at 2bb342a).
- one-grain-cloud: `Machine::render` 54%, `apply_binary` 28%, the reverb 5.8%, `powf` 3.2%, `fmodf` 1.6%.
- The 12 reverbs are about 4.5% of a core in mixed and do not change with this work. To reach 16%,
  the tools must go from 15.6% to about 11.3%.

## 2026-10-09: graph runtime, 2bb342a (t3code/native-typed-graph-runtime)

What changed: Hum is replaced by a JSON graph, run a block at a time; only feedback and buffer
operations run frame by frame.

Null test against main: every `one-` and ts-synth project is bit for bit equal (-inf dB).
ts-synth loads unchanged on the new SDK.

Render: `bench.py render origin/main 2bb342a`, 61 s, median of 3, interleaved (mixed and
mixed-x2 in a second run of the command).

| project | main dsp % | now dsp % | main / now |
| --- | ---: | ---: | ---: |
| builtin12 | 9.87 | 9.94 | 1.0x |
| one-acid-bass | 2.59 | 1.30 | 2.0x |
| one-dorian-drift | 4.20 | 2.30 | 1.8x |
| one-gravity-harp | 5.41 | 3.61 | 1.5x |
| one-storm | 5.16 | 3.00 | 1.7x |
| one-life | 10.57 | 4.00 | 2.6x |
| one-grain-cloud | 15.77 | 7.64 | 2.1x |
| mixed-base | 4.70 | 4.72 | 1.0x |
| **mixed** | **48.16** | **26.34** | **1.8x** |
| mixed-x2 | 98.45 | 53.97 | 1.8x |
| instrument.synth-arp | 0.22 | 0.22 | 1.0x |
| instrument.synth-chords | 0.30 | 0.30 | 1.0x |
| ts-synth-arp | 2.02 | 1.00 | 2.0x |
| ts-synth-chords | 2.47 | 1.17 | 2.1x |

- ts-synth / built-in synth: **4.6x** on the arp, **3.9x** on the chords (main 9.1x, 8.2x;
  target 2x).
- mixed: **1.8x** less CPU than main (target 3x). Without the built-ins (mixed minus
  mixed-base) the tools cost 2.0x less. Startup and peak MB are unchanged.

Live: `bench.py live 2bb342a --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 28.4 | 0.1 | 134 | 60 | 0, 0, 0 | 0, 0, 0 | 7.3, 6.8, 6.7 ms |
| mixed+edit | 30.4 | 0.2 | 191 | 60 | 0, 0, 0 | 0, 0, 0 | 7.2, 7.6, 7.9 ms |
| mixed-x2 | 51.6 | 0.1 | 177 | 60 | 1, 1, 1 | 1, 1, 1 | 12.0, 12.0, 12.4 ms |

- mixed: 49.1% on main, now 28.4% (1.7x less), and its startup xrun is gone.
- mixed-x2 now keeps up (main lost about 40% of callbacks). Its one late callback and xrun come at
  startup: a session that never plays has them too.

Profile: `bench.py profile 2bb342a --projects mixed,one-grain-cloud`, busy samples.

- mixed: `Machine::render` 47%, the reverbs 18%, `apply_binary` 14%, `powf` 3.5%, `sinf` 2.9%.
- one-grain-cloud: `Machine::render` 58%, `apply_binary` 25%, the reverb 5%, `powf` 3%, `fmodf` 1.5%.
- The reverbs are now the second cost of mixed. `_platform_memcmp` shows at 1.9% in mixed.

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
