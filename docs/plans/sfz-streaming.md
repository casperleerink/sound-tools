# Plan: stream large samples from disk

## Summary for Casper

- **Problem.** Every sample is held whole in memory. A concert piano is 875 MB to 2 GB, so the library leaves pianos and drum kits out.
- **Approach.** Keep the start of each sample in memory, and map the rest of the file instead of reading it. The operating system then reads the rest from disk as it plays and drops it under memory pressure. A helper thread reads ahead of every note that starts, so the audio thread finds the data already loaded.
- **FLAC** cannot be mapped as plain samples, so it is decoded once into a cache file of plain samples next to the library, and that file is mapped.
- **What changes for users.** Nothing visible: big instruments load in seconds and take a fraction of the memory. Then the library gets a concert piano and full drum kits.

---

## Goal

A library or project instrument of several gigabytes loads quickly and plays without dropouts, while the app's memory stays small.

## Decided constraints

- The whole sample stays behind `Audio::read`, so the Sampler, the resampler and the filters do not change.
- Small files stay as they are: read whole. Only samples of SFZ instruments over a size use the start in memory plus a mapped rest.
- The start in memory covers longer than the read-ahead takes, so a note's first frames never wait for the disk.
- The audio thread never asks for anything: it only publishes which sample and frame a note started at, without a lock. The helper thread reads ahead from there.
- A render reads the same bytes, so it stays deterministic. It may wait on the disk; it is offline.
- The FLAC cache is machine data (the cache folder), rebuilt when the source file changes, never in the project.
- Licences and sizes of new library instruments are checked as before.

## How to verify

- Tests: a mapped file reads the same frames as the same file read whole, across the edge of the part in memory.
- A render of a piano piece gives the same bytes with streaming as with whole files.
- Memory: the Headroom piano opens with a resident size far below its 875 MB, and loads faster than the 2 s now (measured).
- The realtime sanitizer job passes with a streamed instrument playing.
- In the app: play fast passages on the piano, no dropouts (xruns counter stays 0).
