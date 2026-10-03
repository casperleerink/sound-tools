# Plan: SFZ instruments and a free sound library

## Summary for Casper

- **The Sampler plays SFZ.** A record names an `.sfz` file instead of one `sample`. The Sampler then plays many samples: per key range, velocity layer, round robin, keyswitch and release sample, with loops.
- **Own packs live in the project.** `assets/instruments/<pack>/<file>.sfz` plus its samples. An agent can make one from a few recorded notes; it is a text file.
- **Free library, downloaded per instrument.** A small catalog of CC0 and CC-BY instruments is compiled in (VSCO 2 orchestra, Karoryfer guitars and basses, a piano, drums). Only the instruments a project uses are downloaded, into the support folder, shared by every project. The record names it by library and file.
- **Download on request.** Nothing downloads by itself. Picking an instrument on the card, or Download there, starts it; an agent that names one is told in `problems.txt` to ask the composer.
- **Loads in the background** in the window: the card says Loading, the old sound plays until the new one is there.
- **Ignored, not reported:** SFZ opcodes we do not play. Reported: a file that does not read, missing samples, a failed download.

---

## Goal

A composer or the agent can say "add a cello section" or "a nylon guitar" and get a real sampled instrument, with no plugin and no manual download. A composer can also turn their own samples into an instrument by asking the agent to write an SFZ file.

## Decided constraints

- One tool: `sampler`. A record has either `sample` (one file, as now) or `sfz`. Inside, one sample is a one-region instrument, so the processor has one path.
- `sfz` alone is a project pack under `assets/instruments/`. `sfz` with `library` is a file of a catalog library. The SFZ path and the sample paths it names never leave their pack folder or library.
- With `sfz`, the envelope and velocity come from the SFZ file; the record's envelope and `velocity_to_volume` apply only to `sample`. `gain_db` applies to both.
- Old records load and sound unchanged.
- SFZ subset: `<control> <global> <master> <group> <region>`, `#define`, `#include`, `default_path`, `sample`, `key lokey hikey pitch_keycenter` (numbers and note names), `lovel hivel`, `lorand hirand`, `seq_length seq_position`, `sw_lokey sw_hikey sw_last sw_default`, `trigger=release`, `offset end`, `loop_mode loop_start loop_end` (and loops in the WAV), `volume pan amplitude tune transpose pitch_keytrack amp_veltrack`, `ampeg_attack hold decay sustain release`, `group off_by off_mode off_time`, `locc hicc` against `set_cc` defaults. Everything else is ignored.
- WAV, AIFF and FLAC samples. Samples are held in memory, as now; the catalog only offers instruments of a sane size.
- Downloads use `/usr/bin/curl` like the app update, from GitHub at a pinned commit, into `library/<id>/` in the support folder. `--inspect` and `--render` never download.
- The catalog lists only CC0 and CC-BY libraries; a CC-BY instrument shows its attribution.
- Nothing about this machine goes into the project or the agent docs.

## How to verify

- Parser tests on real files from the catalog libraries: defines, includes, keyswitches, release triggers, note names.
- Render tests: a project pack plays the right sample per key, velocity and round robin; a keyswitch changes the articulation; a loop sustains past the end of its file.
- The realtime sanitizer job passes with an SFZ instrument playing.
- In the app: picking the library cello downloads it, the card shows progress, then it plays (screenshot).

## Phases

1. SFZ playback for project packs, FLAC, agent doc for making packs.
2. The library catalog, per-instrument download, the instrument picker on the card.
3. Stream large samples from disk, see `docs/plans/sfz-streaming.md`.
