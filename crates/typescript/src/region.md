# Region

Plays a stretch of an audio file at its place in the song, out of its output `audio`: an audio clip or a take. It plays at the speed of its file, so a tempo change moves only where it starts, unless warp points stretch it.

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
| `warp` | points `{"ticks": 3840, "seconds": 2.0}`: that second of the file plays that many ticks after `start` | none |

With warp points it plays longer or shorter than its file at the pitch of its file, and follows the tempo. `start` plays `file_start_seconds`, each point its second at its tick, with straight lines between, and it stops at the last point, so leave out `file_end_seconds`. Both ticks and seconds go forward from point to point. To play two seconds of a file over two bars of 4/4: `"warp": [{"ticks": 7680, "seconds": 2.0}]`. A sound lands within a period of its tick: a few milliseconds in a note, up to 10 ms in drums, noise or a mix. Moving it changes only `start`.

Its parent connects it like any sound: `{ from: { child: "verse" }, to: { port: "audio" } }`. Regions that play into one place play as one audio track: where one ends and the next starts they crossfade, and where they overlap the last by name is heard. A file that is not there is a problem until it arrives. Its parent's view reads how long it plays, in seconds of its file, as `now.value("verse.seconds")`, or with warp points in the ticks of the last, and draws the file's waveform with `now.file("voice-take-1.wav")` in a `Wave`.
