# Region

Plays a stretch of an audio file at its place in the song, out of its output `audio`: an audio clip or a take, a child of a parent of this project (`agent-docs/extensions.md`). It plays at the speed of its file, so a tempo change moves only where it starts. Every edge and every jump gets a short ramp, so nothing clicks.

```json state/song/voice/verse.json
{"tool": "region", "state": {"asset": "voice-take-1.wav", "start": 3840, "file_start_seconds": 0.5, "fade_in_ms": 10}}
```

| Field | Values | Default |
| --- | --- | --- |
| `asset` | the file, under `assets/audio/` | |
| `start` | where it starts in the song, in ticks: 3840 is a bar of 4/4 | `0` |
| `file_start_seconds` | where in the file it starts playing | `0` |
| `file_end_seconds` | where in the file it stops | the end of the file |
| `gain_db` | how much louder or quieter than its file, -96 (silence) to 24 | `0` |
| `fade_in_ms`, `fade_out_ms` | a straight line from silence at its start, and to silence at its end | `0` |

Its parent connects it like any sound: `{ from: { child: "verse" }, to: { port: "audio" } }`. Regions that overlap are heard together: to hear one over another, shorten the one below. A file that is not there is a problem; the region keeps its place and plays once the file arrives. The song lasts at least to the end of each region. Its parent's view reads how long it plays, in seconds of its file, as `now.value("verse.seconds")`, and draws the file's waveform with `now.file("voice-take-1.wav")` in a `Wave`.
