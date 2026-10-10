# Performance results

Baseline vs final: main at 1a1c1e2 against the graph runtime at 871839f, same machine, same sound
(bit for bit).

| measure | main 1a1c1e2 | 871839f |
| --- | --- | --- |
| render mixed, % of a core | 46.9 | 15.5 (3.03x less) |
| ts-synth / built-in synth, arp and chords | 9.2x, 8.1x | 1.8x, 1.5x |
| live mixed and mixed+edit: CPU, late callbacks, xruns | 49%, 1, 1 per session | 28%, 0, 0 |
| live mixed-x2: late callbacks per session | about 1500 (40% of callbacks lost) | 0 while playing; 1 at startup in 1 of 6 sessions |

Newest first. Apple M1 Max, macOS. How to run: [README.md](README.md).

## 2026-10-10: built-ins baseline, main at f613f8c

The built-in devices as they are, before any work on them. The built-ins are the same at
1a1c1e2 (no change under `extensions/` or `crates/core`). The machine was busy with other work:
spread was 5 to 30%, but medians of two passes (7 and 5 runs) agree within 2%.

Render: `bench.py render f613f8c --projects builtins --runs 5`, 61 s. Each `bi-` project is 4
tracks of the device. Own cost per instance: an effect `(bi-<effect> - bi-synth) / 4`, an
instrument `(bi-<instrument> - empty) / 4`. The profile shares of each `bi-` project agree with
these within 0.01.

| device | project, % of a core | own, per instance | where the time goes (profile of the `bi-` project) |
| --- | ---: | ---: | --- |
| synth | 0.85 | 0.18 | `Synth::render` 76%: one voice bank, 4 voices |
| wavetable | 4.32 | **1.05** | `Voice::render` 71%, `FilterVoice::render` 21% (default patch, no unison) |
| sampler | 4.00 | **0.97** | `Varispeed::render` 80%: a 32-tap windowed sinc per voice and frame |
| drum-pad | 0.29 | 0.04 | plays buffers it made beforehand; mostly the engine and the master |
| reverb | 2.76 | **0.48** | `Reverb::process` 60%, `memcmp` 7.4% from it |
| delay | 1.04 | 0.05 | `Delay::process` 17% |
| eq | 1.35 | 0.13 | `Eq::process` 33%, 4 bands on |
| compressor | 1.27 | 0.11 | `Compressor::process` 25%, `expf` and `log10f` 6% |
| limiter | 1.05 | 0.05 | `PeakLimiter::next` (shared with the master) |
| saturator | 3.12 | **0.57** | `Oversampler::up` 32%, `down` 26%, `Saturator::process` 16% |
| filter | 1.35 | 0.13 | `Filter::process` 41%, LFO on the cutoff |
| modulation | 1.20 | 0.09 | `Modulation::process` 29% (chorus) |
| utility | 1.00 | 0.04 | `Utility::process` 16% |
| empty project | 0.14 | | the master limiter, which runs on silence too |

`bi-heavy` (24 tracks, synth or wavetable, through reverb, delay, EQ and compressor) costs
28.5% of a core. Its profile: reverb 39% (35% plus 4% `memcmp`), wavetable 27%, EQ 10%,
compressor 9%, synth 4%, delay 4%, engine 2.4%.

The master chain is the arrangement's `Master` with its fixed `PeakLimiter`, about 0.05% of a
core while sound plays. It does not return early: in the empty project it is most of the 0.14%.

Idle: does a device return before it touches its output once nothing sounds?

| device | after its sound died | own cost over the tail project of the synth, per instance |
| --- | --- | ---: |
| reverb | returns early, once nothing in its lines is over -180 dB: about 6 s after the last note at a 2 s decay, 3 min at 60 s | 0.06 (7 s of full cost) |
| delay | returns early once its lines are empty | 0.02 |
| eq, filter, modulation, utility | return early | 0.01 |
| compressor, limiter, saturator | return early, but their silence check runs `held()` on every sample, and the compressor on 4 channels (its key is its input when nothing is keyed) | 0.02 to 0.03 |
| synth, wavetable, sampler, drum-pad | return early with no notes and no voice | 0.01 to 0.02 |
| master limiter | never returns early | about 0.07 total |

The profiles of the `-tail` projects, sampled after the tail, confirm it: each device is 9 to 36%
of a project that costs about 0.2% of a core, which is the silence check, not frames.

| project | render, % of a core | live, % of a core (3 runs of 30 s) |
| --- | ---: | ---: |
| empty | 0.14 | 1.2 |
| bi-heavy-idle (24 tracks, 120 devices, no clips) | 1.95 | 7.6 |
| bi-heavy | 28.5 | 31.0 |

Idle costs 0.075% of a core per track in a render and 0.27% live: the same functions, slower
with cold caches between 10 ms callbacks. Where idle goes (live profile): engine
`process_block` 25%, compressor 20%, mixer 12%, reverb 9%, EQ 8%, delay 8%, wavetable 7%.

What to work on, by gain:

1. Reverb, 0.48 per instance and the largest cost in `mixed` and `bi-heavy`. The `memcmp` is
   `Taps<16>::is_fading` (`crates/core/src/dsp.rs`), which compares two `[usize; 16]` arrays,
   called by `weight()` and `advance()` twice per frame from `Reverb::frame`. A flag or
   `fade < 1.0` drops 7.4% of `bi-reverb`, about 11% of the reverb, bit for bit. The rest is
   16 delay line reads and writes per frame over 16 lines of 8192 floats (512 KB, past L1), a
   Hadamard of 16 and 16 damping filters. A new algorithm (fewer lines, or a cheaper diffusion)
   is allowed and could halve it. Stopping at -120 dB instead of -180 dB cuts the work after the
   last note by a third.
2. Wavetable, 1.05 per instance, 6 times the synth. Per frame it reads two table frames and
   morphs them even when the position does not move, then a filter per voice. Reading one
   frame when the morph is 0 is exact (`one + (two - one) * 0`). Perhaps 20 to 30% of the voice.
3. Idle: 7.6% of a core live for an idle 24-track project. A silence check of `x == 0.0` on 2
   channels for the compressor, limiter and saturator (NaN counts as silent today, so keep
   that), and a master that returns on silence, would take off most of the compressor's 20%
   share. The rest is the engine and the mixer running every processor of 24 tracks every 64
   frames.

Not first: the sampler's sinc (0.97) must stay bit for bit, so its order of sums is fixed; the
saturator's oversampler is already a polyphase half band with 8-lane dot products.

Reverb quality of main: `reverb.py f613f8c f613f8c`, all differences 0 (null -inf dB), so
the check reports nothing on equal commits. Wet only, click at -6 dBFS.

| setting | T30 125 Hz to 8 kHz, s | EDT 1 kHz | energy 125 Hz to 8 kHz, dB | echo density 0.95 at | correlation | tail peak | peak, RMS of 1 s |
| --- | --- | ---: | --- | ---: | ---: | --- | --- |
| default (2 s, size 0.5) | 2.05 1.97 1.99 1.93 1.75 1.36 0.83 | 2.09 | -23.2 -18.6 -15.1 -11.7 -10.0 -9.1 -10.7 | 40 ms | -0.04 | 8.9 dB at 973 Hz | -32.7, -52.8 dBFS |
| long (6 s, size 1, damping 0.3) | 6.02 5.97 5.97 5.91 5.63 4.90 3.53 | 6.31 | -24.5 -19.3 -16.2 -13.0 -10.2 -8.7 -9.5 | 122 ms | -0.05 | 4.4 dB at 1453 Hz | -34.9, -53.0 dBFS |
| short (0.6 s, size 0.15, damping 0.6) | 0.58 0.60 0.57 0.57 0.50 0.37 0.20 | 0.77 | -25.6 -18.7 -14.0 -13.3 -10.0 -9.4 -10.9 | 32 ms | -0.06 | 12.0 dB at 4160 Hz | -27.4, -52.9 dBFS |

T30 holds the decay setting within 5% up to 1 kHz. The tail peaks are real resonances, not
chance: the same measure on noise with the same envelope gives 1 to 3 dB. So main's short room
rings at 4.2 kHz; a new reverb should not ring more (limit: A + 3 dB). Music: on synth chords
peak -23.3 dBFS, RMS -36.2; on drums peak -14.0, RMS -26.1.

## 2026-10-10: final, graph runtime at 871839f (t3code/native-typed-graph-runtime)

What changed since f548b98: a hold whose trigger does not fire in a span gives one value over the
span, so the math on it runs once per span (871839f). 4539bfa adds a test and a debug check only.

Null test against main: every `one-` and ts-synth project is still bit for bit equal (-inf dB).

Render: `bench.py render origin/main f548b98 871839f`, 61 s, interleaved. Median of 5 for mixed
and mixed-x2 (each in its own run) and for the synth rows (main's ts-synth-arp showed 8% spread
in 3 runs); median of 3 for the rest. Spread was 0 to 3% on every graph runtime row.

| project | main | f548b98 | 871839f | main / 871839f | f548b98 / 871839f |
| --- | ---: | ---: | ---: | ---: | ---: |
| builtin12 | 9.84 | 9.75 | 9.74 | 1.0x | 1.00x |
| one-acid-bass | 2.56 | 0.87 | 0.87 | 2.9x | 1.00x |
| one-dorian-drift | 4.13 | 1.25 | 1.25 | 3.3x | 1.00x |
| one-gravity-harp | 5.33 | 1.59 | 1.58 | 3.4x | 1.01x |
| one-storm | 5.08 | 1.87 | 1.85 | 2.7x | 1.01x |
| one-life | 10.40 | 1.77 | 1.76 | 5.9x | 1.01x |
| one-grain-cloud | 15.52 | 4.47 | 4.21 | 3.7x | 1.06x |
| mixed-base | 4.63 | 4.64 | 4.62 | 1.0x | 1.00x |
| **mixed** | 46.90 | 15.76 | **15.50** | **3.03x** | 1.02x |
| mixed-x2 | 94.45 | 31.84 | 31.19 | 3.03x | 1.02x |
| instrument.synth-arp | 0.22 | 0.22 | 0.22 | 1.0x | 1.00x |
| instrument.synth-chords | 0.30 | 0.30 | 0.30 | 1.0x | 1.00x |
| ts-synth-arp | 1.98 | 0.41 | 0.40 | 5.0x | 1.02x |
| ts-synth-chords | 2.43 | 0.45 | 0.45 | 5.4x | 1.00x |

The commit's own claims hold: grain cloud 6% less CPU, mixed 2%; storm 1% (claimed 2%).

Targets:

| target | result | met |
| --- | --- | --- |
| ts-synth at most 2x the built-in synth (1.5x preferred) | arp 1.84x, chords 1.52x (main 9.2x, 8.1x) | yes; 1.5x on the chords only |
| mixed at most main / 3 (15.63%) | 15.50%, main / 3.03. The tools (mixed minus mixed-base) cost 10.9%, 3.9x less than main | yes |
| live mixed, mixed+edit, mixed-x2: 0 late callbacks, 0 xruns | 0 and 0 in all 9 sessions that play | yes |

Live: `bench.py live 871839f --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 27.7 | 0.2 | 135 | 61 | 0, 0, 0 | 0, 0, 0 | 6.5, 5.9, 4.8 ms |
| mixed+edit | 27.8 | 0.3 | 182 | 60 | 0, 0, 0 | 0, 0, 0 | 5.7, 5.9, 6.5 ms |
| mixed-x2 | 36.4 | 0.1 | 178 | 60 | 0, 0, 0 | 0, 0, 0 | 10.0, 10.6, 10.0 ms |
| mixed-x2, 10 s, no play | | | | | 0, 0, 1 | 0, 0, 1 | 10.6, 9.8, 10.8 ms |

- The late callback at startup of mixed-x2 still comes now and then: 1 of these 6 sessions, in
  one that never played. None came while playing.

Profile: `bench.py profile 871839f --projects mixed`, busy samples. The reverb 29% (plus 3%
`memcmp` called from it), `Machine::render` 24%, `Span::run_loop` 20%, wavetable voice 3.7%,
`sinf` 3.1%. Much as at f548b98.

What's left: the signal machine (`Machine::render` and `Span::run_loop`) is still 44% of mixed,
and the reverbs, unchanged from main, are now the largest single cost.

## 2026-10-10: final, graph runtime at f548b98 (t3code/native-typed-graph-runtime)

What changed since efde207: loops of the same formulas take turns per operation, so a frame
matches each formula once per run, not once per operation. Also a docs commit and a small
processor cleanup.

Null test against main: every `one-` and ts-synth project is still bit for bit equal (-inf dB).

Render: `bench.py render origin/main efde207 f548b98`, 61 s, median of 3, interleaved (mixed and
mixed-x2 each in their own run). one-storm, ts-synth-arp and instrument.synth-arp were rerun with
5 runs after one-storm showed a 13% spread; mixed was confirmed with 5 runs (main 46.95,
f548b98 15.82). Spread was 0 to 2% after that.

| project | main | efde207 | f548b98 | main / f548b98 | efde207 / f548b98 |
| --- | ---: | ---: | ---: | ---: | ---: |
| builtin12 | 9.77 | 9.74 | 9.72 | 1.0x | 1.00x |
| one-acid-bass | 2.56 | 0.87 | 0.87 | 2.9x | 1.00x |
| one-dorian-drift | 4.15 | 1.25 | 1.25 | 3.3x | 1.00x |
| one-gravity-harp | 5.32 | 1.90 | 1.59 | 3.3x | 1.19x |
| one-storm | 5.08 | 1.87 | 1.87 | 2.7x | 1.00x |
| one-life | 10.42 | 1.77 | 1.77 | 5.9x | 1.00x |
| one-grain-cloud | 15.51 | 5.80 | 4.48 | 3.5x | 1.29x |
| mixed-base | 4.62 | 4.63 | 4.63 | 1.0x | 1.00x |
| **mixed** | 46.88 | 17.41 | **15.77** | **2.97x** | 1.10x |
| mixed-x2 | 94.00 | 35.00 | 31.71 | 2.96x | 1.10x |
| instrument.synth-arp | 0.22 | 0.22 | 0.22 | 1.0x | 1.00x |
| instrument.synth-chords | 0.30 | 0.30 | 0.30 | 1.0x | 1.00x |
| ts-synth-arp | 1.99 | 0.40 | 0.40 | 5.0x | 1.00x |
| ts-synth-chords | 2.48 | 0.45 | 0.46 | 5.4x | 0.98x |

The commit's own claims hold: grain cloud 23% less CPU, gravity harp 16%, mixed 9%; the rest
the same.

Targets:

| target | result | met |
| --- | --- | --- |
| ts-synth at most 2x the built-in synth (1.5x preferred) | arp 1.8x, chords 1.5x (main 9.2x, 8.3x) | yes; 1.5x on the chords only |
| mixed at most main / 3 (15.6%) | 15.8%, main / 2.97. The tools (mixed minus mixed-base) cost 11.1%, 3.8x less than main; they must reach 11.0% | no, by 1% |
| live mixed, mixed+edit: 0 late callbacks, 0 xruns | 0 and 0 in every run | yes |
| live mixed-x2: 0 late callbacks, 0 xruns | at most 1 and 1 per session, at startup; none while playing | during play only |

Live: `bench.py live f548b98 --window 60`, 3 runs of 60 s, master at -60 dB.

| project | cpu % | bun cpu % | MB | bun MB | late callbacks | xruns | slowest callback |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| mixed | 28.0 | 0.1 | 134 | 60 | 0, 0, 0 | 0, 0, 0 | 5.7, 5.4, 6.6 ms |
| mixed+edit | 28.2 | 0.3 | 161 | 60 | 0, 0, 0 | 0, 0, 0 | 6.0, 6.1, 5.0 ms |
| mixed-x2 | 35.5 | 0.1 | 178 | 60 | 1, 0, 0 | 1, 0, 0 | 10.9, 10.1, 10.6 ms |
| mixed-x2, 10 s, no play | | | | | 1, 0, 1 | 0, 0, 1 | 10.7, 9.4, 10.9 ms |

- The last row is 3 sessions that start, wait 10 s and quit without playing. They have as many
  late callbacks as the sessions that play for 60 s, so the late callback is at startup, not
  during play. It now comes in 3 of 6 sessions; at efde207 it came in nearly every one.

Profile: `bench.py profile f548b98 --projects mixed,one-grain-cloud`, busy samples.

- mixed: the reverb 27%, `Machine::render` 25%, `Span::run_loop` 20%, wavetable voice 3.8%,
  `sinf` 3.4%. `Span::run_loop` went from 27% to 20%; the reverbs now cost the most.
- one-grain-cloud: `Span::run_loop` 39%, `Machine::render` 33%, the reverb 8.7%, `powf` 5.6%,
  `fmodf` 3.2%. `Span::run_loop` went from 52% to 39%.

## 2026-10-10: graph runtime at efde207 (t3code/native-typed-graph-runtime)

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
