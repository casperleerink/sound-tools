# Delay

`delay` is an effect that repeats the sound. Synced, the repeats fall on a note of the tempo and follow it; free, they come after `time_ms`.

Here the lead plays through a delay named `echo`, on the dotted eighth:

```json state/song/lead/chain/echo.json
{
  "tool": "delay",
  "state": {"division": "1/8", "feel": "dotted", "feedback": 0.35, "mix": 0.25}
}
```

{{fields delay}}

A synced time over 4 s, such as `"1/1"` under 60 bpm, plays at 4 s.

Starting points: a slapback is `sync` `false`, `time_ms` 100, `feedback` 0, `mix` 0.3. Dub echoes: `high_cut_hz` 2500, `low_cut_hz` 300, `feedback` 0.7. On a lead keep `mix` at 0.2 to 0.35.
