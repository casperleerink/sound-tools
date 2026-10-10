# Performance results

Newest first. Apple M1 Max, macOS. How to run: [README.md](README.md).

## 2026-10-10: final, graph runtime at efde207 (t3code/native-typed-graph-runtime)

What changed since 7b4949e: values that stay the same over a span are worked out once, glides
stop once steady, steady settings of phasor, filter and envelope are worked out once per span,
and an instrument with nothing to play returns silence at once.

Null test against main: every `one-` and ts-synth project is still bit for bit equal (-inf dB).

Render: `bench.py render origin/main 7b4949e efde207`, 61 s, median of 3, interleaved (mixed and
mixed-x2 each in their own run). Spread was 0 to 4%.

| project | main | 7b4949e | efde207 | main / 7b4949e | main / efde207 | 7b4949e / efde207 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| builtin12 | 9.72 | 9.76 | 9.73 | 1.0x | 1.0x | 1.00x |
| one-acid-bass | 2.56 | 0.94 | 0.87 | 2.7x | 2.9x | 1.07x |
| one-dorian-drift | 4.13 | 1.36 | 1.25 | 3.0x | 3.3x | 1.09x |
| one-gravity-harp | 5.32 | 2.14 | 1.90 | 2.5x | 2.8x | 1.12x |
| one-storm | 5.12 | 2.06 | 1.90 | 2.5x | 2.7x | 1.08x |
| one-life | 10.42 | 2.02 | 1.77 | 5.2x | 5.9x | 1.14x |
| one-grain-cloud | 15.52 | 5.90 | 5.79 | 2.6x | 2.7x | 1.02x |
| mixed-base | 4.64 | 4.63 | 4.63 | 1.0x | 1.0x | 1.00x |
| **mixed** | 46.94 | 18.36 | **17.40** | 2.6x | **2.7x** | 1.06x |
| mixed-x2 | 93.94 | 37.06 | 35.24 | 2.5x | 2.7x | 1.05x |
| instrument.synth-arp | 0.21 | 0.22 | 0.22 | 1.0x | 1.0x | 1.00x |
| instrument.synth-chords | 0.30 | 0.30 | 0.30 | 1.0x | 1.0x | 1.00x |
| ts-synth-arp | 1.99 | 0.50 | 0.40 | 4.0x | 5.0x | 1.26x |
| ts-synth-chords | 2.43 | 0.57 | 0.45 | 4.3x | 5.4x | 1.27x |

Targets:

| target | result | met |
| --- | --- | --- |
| ts-synth at most 2x the built-in synth (1.5x preferred) | arp 1.8x, chords 1.5x (main 9.3x, 8.2x) | yes; 1.5x on the chords only |
| mixed at most main / 3 (15.6%) | 17.4%, main / 2.7. The tools (mixed minus mixed-base) cost 12.8%, 3.3x less than main; they must reach 11.0% | no |
| live mixed, mixed+edit: 0 late callbacks, 0 xruns | 0 and 0 in every run | yes |
| live mixed-x2: 0 late callbacks, 0 xruns | 1 and 1 per session, all at startup; none while playing | during play only |

Live: `bench.py live efde207 --window 60`, 3 runs of 60 s, master at -60 dB. Main's mixed-x2 is
in the baseline entry (56.6%, about 1400 late callbacks per run).

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 28.6 | 0.2 | 134 | 60 | 0, 0, 0 | 0, 0, 0 | 6.9, 7.3, 7.2 ms |
| mixed+edit | 28.8 | 0.3 | 173 | 60 | 0, 0, 0 | 0, 0, 0 | 6.0, 6.5, 6.4 ms |
| mixed-x2 | 36.4 | 0.1 | 177 | 60 | 1, 1, 1 | 1, 1, 1 | 11.1, 11.2, 11.2 ms |

- mixed ran at the high live level (about 30%, see 7b4949e). Interleaved with 7b4949e, 2 runs:
  29.7% there, 28.6% now.
- Where mixed-x2's late callback falls: `status` does not print callback counts, so sessions that
  wait 10 s and quit without playing were compared with sessions that wait 10 s, then play 60 s.
  All 3 idle sessions had 1 late callback and 1 xrun (slowest 11.2 ms); 4 of 5 play sessions had
  the same 1 and 1, one had 0 and 0. Play adds none: the late callback is at startup.

Profile: `bench.py profile efde207 --projects mixed,one-grain-cloud`, busy samples.

- mixed: `Span::run_loop` 27%, the reverbs 25%, `Machine::render` 23%, wavetable voice 3.5%,
  `_platform_memcmp` 3.0%.
- one-grain-cloud: `Span::run_loop` 52%, `Machine::render` 25%, the reverb 7.2%, `powf` 4.9%, `fmodf` 2.2%.

## 2026-10-10: graph runtime, 7b4949e (t3code/native-typed-graph-runtime)

What changed since 18e97e0: loops of a feedback or a buffer run one value at a time from
registers, and loops that do not hear each other run side by side in one pass over the frames.

Null test against main: every `one-` and ts-synth project is still bit for bit equal (-inf dB).

Render: `bench.py render origin/main 18e97e0 7b4949e`, 61 s, median of 3, interleaved (mixed and
mixed-x2 each in their own run). Spread was 1 to 5%. A compile at nice 19 ran during the first
run; a rerun of the projects that got slower (18e97e0 against 7b4949e, 5 runs) gave the same.

| project | main | 18e97e0 | now | main / 18e97e0 | main / now | 18e97e0 / now |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| builtin12 | 9.84 | 9.88 | 9.86 | 1.0x | 1.0x | 1.00x |
| one-acid-bass | 2.59 | 0.94 | 0.94 | 2.8x | 2.7x | 0.99x |
| one-dorian-drift | 4.20 | 1.38 | 1.39 | 3.1x | 3.0x | 0.99x |
| one-gravity-harp | 5.43 | 2.90 | 2.17 | 1.9x | 2.5x | 1.34x |
| one-storm | 5.18 | 2.01 | 2.07 | 2.6x | 2.5x | 0.97x |
| one-life | 10.57 | 1.99 | 2.03 | 5.3x | 5.2x | 0.98x |
| one-grain-cloud | 15.72 | 7.29 | 6.11 | 2.2x | 2.6x | 1.19x |
| mixed-base | 4.82 | 4.73 | 4.77 | 1.0x | 1.0x | 0.99x |
| **mixed** | 48.14 | 20.71 | **18.85** | 2.3x | **2.6x** | 1.10x |
| mixed-x2 | 97.43 | 42.85 | 38.00 | 2.3x | 2.6x | 1.13x |
| instrument.synth-arp | 0.22 | 0.22 | 0.22 | 1.0x | 1.0x | 1.00x |
| instrument.synth-chords | 0.31 | 0.31 | 0.31 | 1.0x | 1.0x | 1.00x |
| ts-synth-arp | 2.08 | 0.50 | 0.52 | 4.1x | 4.0x | 0.96x |
| ts-synth-chords | 2.53 | 0.55 | 0.59 | 4.6x | 4.3x | 0.94x |

- ts-synth / built-in synth: **2.4x** on the arp, **1.9x** on the chords (18e97e0 2.3x, 1.8x;
  target 2x). ts-synth, one-life and one-storm are 2 to 6% slower than at 18e97e0, the same in
  both runs.
- mixed: **2.6x** less CPU than main, 18.9% of a core (target 3x, 16.0%). The tools (mixed minus
  mixed-base) cost 14.1%, 3.1x less than main; they must reach about 11.3%.
- The gain is in gravity-harp (1.34x) and grain-cloud (1.19x).

Live: `bench.py live 7b4949e --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 20.0 | 0.0 | 134 | 60 | 0, 0, 0 | 0, 0, 0 | 6.5, 6.3, 7.4 ms |
| mixed+edit | 20.9 | 0.2 | 172 | 60 | 0, 0, 0 | 0, 0, 0 | 6.4, 5.3, 6.8 ms |
| mixed-x2 | 38.0 | 0.1 | 177 | 60 | 1, 1, 1 | 1, 1, 1 | 11.1, 11.0, 11.0 ms |

- Live CPU of mixed has two levels, about 20% and about 30%, whatever the commit. A rerun of
  mixed and mixed+edit against 18e97e0, interleaved, gave 22.0, 30.5, 30.2 for 18e97e0 and 19.3,
  29.7, 29.6 now. The low runs came while other apps were busy, so it is likely the clock speed
  of the cores. Compare live CPU only within one interleaved run: there, 7b4949e equals 18e97e0.
- No late callbacks or xruns in mixed. mixed-x2's one is at startup, as before.

Profile: `bench.py profile 7b4949e --projects mixed,one-grain-cloud`, busy samples.

- mixed: `Machine::render` 26%, the reverbs 25%, `Span::run` 16%, `apply_binary` 3.6%, `powf` 3.5%;
  `_platform_memcmp` 3.1%. The graph (render, `Span::run`, `apply_binary`) is 46%, from 54% at 18e97e0.
- one-grain-cloud: `Machine::render` 54%, `Span::run` 14%, `apply_binary` 6.8%, the reverb 6.2%, `powf` 4.1%.

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
