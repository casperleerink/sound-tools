# sound-media

The audio files of a project, and how to read them. No extension: the audio clips of the arrangement, and later the Sampler and the Drum pad, read files through this crate, and none of them depends on another. The core knows no audio files; to it a file under `assets/audio/` is an asset like any other.

## Naming a file

A record names a file by its file name under `assets/audio/`: `"voice-take-1.wav"`. The type is `AudioAsset`, which saves as that string and loads only a name of lowercase letters, digits, `-` and `_`, a dot and an extension of the same letters. So a record can never point outside the project folder, and a project folder copied to another place plays the same. `asset.project_path()` is `assets/audio/voice-take-1.wav`, for messages.

## Bringing a file in

`import(assets, source)` reads a file from anywhere, checks that it plays, and copies it into `assets/audio/` under a name made from its own: `My Take (2).WAV` becomes `my-take-2.wav`, and when that is taken `my-take-2-2.wav`, then `-3`. It writes a temporary file, takes the name with an empty file (`Assets::reserve`, which never opens a file that is there) and renames the temporary file over it. So a name that is taken is never written over, a half-written file never has a name, it works on FAT and network shares, and a failure leaves nothing behind. It gives back `Imported`: the name and the file in memory. The cache holds that weakly, so while the caller holds it the first `load` reads nothing, and a copy that never becomes a clip does not stay in memory. Nothing here ever changes or deletes a file of `assets/audio/` that it did not just make.

## Reading a file

`load(assets, asset)` gives the file in memory as an `Arc<Audio>`. It is read the first time something names it and shared by everything that names it after that; the cache holds no samples alive, so a file that nothing plays leaves memory. `info(assets, asset)` gives what the file is, `Info` (frames, rate, channels), for how long a clip is, from the header of the file and never its samples. `probe(path)` does the same for a file anywhere, such as one dragged over the window. `cached(assets, asset)` says what is known of a file from memory only, with no look at the disk at all: for the thread that draws. The lock of this cache is never held while a file is read. What a file is, and a file that does not play, are kept by path, so after the first time either costs one look at the size and time of the file. A file whose size or modification time changed is read again. `MediaError::Missing` says the file is not there, with its path in the project.

`Audio` is a WAV or AIFF file kept as its own bytes: it costs its size on disk and is never converted as a whole. It knows its `sample_rate()`, `channels()`, `frames()`, `seconds()` and `memory()`. `audio.read(start, &mut out)` turns frames `start..` into left and right `f32` on the audio thread, with no allocation, lock or system call: a mono file plays on both channels, a file of more than two plays its first two, and a frame outside the file is silence.

Read: WAV with 8, 16, 24 or 32-bit integers or 32 or 64-bit floats, plain or `WAVE_FORMAT_EXTENSIBLE`; AIFF with 8 to 32-bit integers; AIFF-C with `NONE`, `twos`, `sowt`, `in24`, `in32`, `fl32` and `fl64`. From 8 kHz to 384 kHz. A WAV `data` chunk of length 0 is a file that streamed its samples after it, unless another chunk follows: then it has none. Not read: compressed formats (MP3, AAC, FLAC, µ-law, ADPCM), RF64 and big-endian WAV. The parser is our own, about 300 lines: these containers are a few chunks around plain samples, and a decoding library would only add a copy of what the file already holds.

## Playing at another rate

`Resampler::new(file_rate, engine_rate)` is a windowed sinc filter (Kaiser window, 100 dB), designed for the pair of rates: flat up to 20 kHz, or to 95 % of the lower Nyquist frequency when that is lower, and stopping from the lower Nyquist frequency, so nothing folds back. From 44.1 to 48 kHz that is 184 taps, worked out once into a table of 1024 phases. `resampler(file_rate, engine_rate)` gives a shared one. `render(audio, origin, first, out, scratch)` gives engine frames `first..` of a stream that starts at file frame `origin`: frame `n` reads the file at `origin + n * file_rate / engine_rate`, worked out exactly in integers. It keeps no state, so any block can be rendered on its own from any place in the file and a render gives the same bytes every time. That is why this is not a block resampler such as `rubato`: those keep a filter state that must run in from the start, and a clip is heard from wherever the playhead is. At the same rate it is a plain read, sample for sample.

## Playing at the pitch of a key

`varispeed()` is the one `Varispeed`, which reads a file at any place: `render(audio, position, step, out, scratch)` gives the file at `position`, `position + step`, `position + 2 * step` and so on, in frames of the file, in `f64`. An instrument that plays a sample at the pitch of a key gives `step = 2^(semitones / 12) * file_rate / engine_rate` and moves its place on by `step` per frame; it keeps no state of its own. The Sampler uses it, and the Drum pad can for its pitch. Make it on the control side (call `varispeed()` when the processor is made): the first call makes its table.

It is an interpolating windowed sinc of 32 taps (Kaiser, beta 8) from a table of 256 phases with a straight line between two, whose cutoff is the Nyquist frequency of the file. So a whole place gives its sample exactly, and a file played at its own rate and speed comes out sample for sample. Measured in `tests/media.rs`, the pitch of a sine of 440 Hz from files at 44.1, 48 and 96 kHz into 48 kHz, four octaves down to four up: within 0.0041 cents and 0.002 dB.

Above a step of 1 the file goes by faster than the engine plays it, so the kernel is stretched by `step / 0.85`, read from a fine table of it, with `2 * ceil(16 * step / 0.85)` taps and the weights of each frame summed to 1. Its cutoff then follows the output: flat to 16 kHz within 0.05 dB, -4 dB at 20 kHz, and what would fold back is down 80.6 dB or more. Measured (`sound-media media::varispeed_folds_nothing_back_above_a_step_of_one`), what lands above the output's Nyquist frequency at 48 kHz: at step 1.06, 24.5 kHz -80.6 dB and 25 kHz -95.4 dB; at step 2, 25, 30 and 40 kHz -103.7, -91.3 and -104.9 dB; at step 4, 25 to 80 kHz -99.2 dB or less; a 96 kHz file at its root (step 2), 25, 30 and 40 kHz in the file -103.7, -91.3 and -104.9 dB. The stretch stops at a step of 8 (`MAX_STEP`, 302 taps); above it what a file holds above `8 / step` of the output's Nyquist frequency folds back. A voice costs, in the dev profile (the Sampler's `performance::`, run by hand, on a busy laptop): 0.14 % of a core at step 1, 0.35 % at 1.06, 0.60 % at 2, 1.21 % at 4, 2.28 % at 8. The `Resampler` above is exact for one pair of rates, which a key is not.

`engine_frames(file_frames, file_rate, engine_rate)` is how many engine frames a stretch of a file plays at its own speed.

Measured in `tests/media.rs`, from 44.1 to 48, 48 to 44.1 and 96 to 48 kHz, sines from 100 Hz to 20 kHz: the level within 0.0001 dB, the worst difference from the ideal sine 102.5 dB down or more. Tones of 22.1 to 23.5 kHz in a 48 kHz file played at 44.1 kHz leave 102.6 dB down or more.

## Waveforms

`Overview::of(&audio)` is what a waveform draws: the loudest sample of every 64 frames, left and right together, and coarser levels of 8 times as many frames each. `overview.peak(from, to)` gives the loudest sample of any stretch in 8 to 64 lookups, and `columns(from_seconds, to_seconds, count)` the peaks of a waveform of `count` columns. It reads the whole file once, 35 ms for 10 minutes of stereo 24-bit in the dev profile, so it is made away from the thread that draws; `sound_ui::Waveforms` does that and keeps them. It is never written into the project folder.

## Checks

```sh
cargo nextest run -p sound-media --no-capture
RTSAN_ENABLE=1 cargo nextest run -p sound-media     # reading, resampling and the varispeed inside process_block
```
