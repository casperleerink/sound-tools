# Delay: the built-in delay effect

`delay` is an effect: it repeats the sound of a track. Synced, the repeats fall on a note of the tempo, such as every eighth note, and follow the tempo when it changes. Free, they come after a time in ms. Each repeat is quieter than the one before by the feedback, and thinner and darker by the cuts. Ping-pong sends them from left to right and back.

Like every effect it is two lines: the record in the track folder, and its file name in `effects` of the track record. `agent-docs/arrangement.md` has the rules for the list. Here the keys play through a delay named `echo`:

```json state/arrangement/keys/instance.json
{
  "tool": "arrangement.track",
  "state": {
    "name": "Keys",
    "colour": "lavender",
    "order": 1,
    "gain_db": 0.0,
    "pan": 0.0,
    "mute": false,
    "effects": ["echo"]
  }
}
```

```json state/arrangement/keys/echo.json
{
  "tool": "delay",
  "state": {
    "sync": true,
    "division": "1/8",
    "feel": "straight",
    "time_ms": 250.0,
    "feedback": 0.4,
    "ping_pong": false,
    "low_cut_hz": 100.0,
    "high_cut_hz": 8000.0,
    "mix": 0.3
  }
}
```

These are the defaults. A field you leave out takes its default, so `"state": {}` is the default delay.

| Field | Meaning | Values | Default |
| --- | --- | --- | --- |
| `sync` | `true`: the time is `division` and `feel` at the tempo of the project, and follows it. `false`: the time is `time_ms`. | `true` or `false` | `true` |
| `division` | The note of a synced time. `"1/4"` is one beat of the tempo. | `"1/32"`, `"1/16"`, `"1/8"`, `"1/4"`, `"1/2"`, `"1/1"` | `"1/8"` |
| `feel` | A synced time is the note, one and a half of it (`"dotted"`), or two thirds of it (`"triplet"`). | `"straight"`, `"dotted"`, `"triplet"` | `"straight"` |
| `time_ms` | The time from the sound to its first repeat, when `sync` is `false`. | 1 to 4000 | 250 |
| `feedback` | How loud each repeat is against the one before. 0 is one repeat, 0.5 halves each, 0.95 goes on for a long time and always dies away. | 0 to 0.95 | 0.4 |
| `ping_pong` | `true` sends the repeats left, right, left, from the sound of both sides as one. `false` repeats each side on its own side. | `true` or `false` | `false` |
| `low_cut_hz` | Each repeat is cut below this once more, so later repeats are thinner. | 20 to 20000 | 100 |
| `high_cut_hz` | Each repeat is cut above this once more, so later repeats are darker. | 20 to 20000 | 8000 |
| `mix` | 0 is only the sound as it came in, 1 is only the repeats. | 0 to 1 | 0.3 |

The time is at most 4 s: a synced time longer than that, such as `"1/1"` under 60 bpm, plays at 4 s.

Starting points: a slapback on a voice or a guitar is `sync` `false`, `time_ms` 100, `feedback` 0, `mix` 0.3. Echoes on the beat are `"1/4"` or `"1/8"` with `feedback` 0.4. The classic dotted eighth is `"1/8"`, `"dotted"`, `feedback` 0.35, `mix` 0.25. A wide, busy repeat is `ping_pong` `true`. For dark, dub-like echoes use `high_cut_hz` 2500, `low_cut_hz` 300 and `feedback` 0.7. Keep `mix` low, 0.2 to 0.35, on a lead, so the repeats sit behind it.

An edit applies while the track plays and glides over 20 ms, so it does not click. A new time, from the record or from a tempo change, fades from the old repeats to the new ones over those 20 ms, without a pitch slide.
