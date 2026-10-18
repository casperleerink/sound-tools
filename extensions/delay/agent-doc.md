# Delay: the built-in delay effect

`delay` is an effect: it repeats the sound of a track. Synced, the repeats fall on a note of the tempo, such as every eighth note, and follow the tempo when it changes. Free, they come after a time in ms. Each repeat is quieter than the one before by the feedback, and thinner and darker by the cuts. Ping-pong sends them from left to right and back.

Here the lead plays through a delay named `echo`, on the dotted eighth:

```json state/arrangement/lead/echo.json
{
  "tool": "delay",
  "state": {"division": "1/8", "feel": "dotted", "feedback": 0.35, "mix": 0.25}
}
```

{{fields delay}}

The time is at most 4 s: a synced time longer than that, such as `"1/1"` under 60 bpm, plays at 4 s.

Starting points: a slapback on a voice or a guitar is `sync` `false`, `time_ms` 100, `feedback` 0, `mix` 0.3. Echoes on the beat are `"1/4"` or `"1/8"` with `feedback` 0.4. The classic dotted eighth is the example above. A wide, busy repeat is `ping_pong` `true`. For dark, dub-like echoes use `high_cut_hz` 2500, `low_cut_hz` 300 and `feedback` 0.7. Keep `mix` low, 0.2 to 0.35, on a lead, so the repeats sit behind it.

Every number here can move over time with an automation lane of the track; `time_ms` follows its lane in 20 ms fades from one time to the next, without a pitch slide. See `agent-docs/arrangement.md`.

An edit applies while the track plays and glides over 20 ms, so it does not click. A new time, from the record or from a tempo change, fades from the old repeats to the new ones over those 20 ms, without a pitch slide.
