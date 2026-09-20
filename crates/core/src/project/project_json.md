# project.json: tempo, time signature and extra connections

One file at the top of the project folder. Most work needs no edit here: a track plays through its own instrument and reaches the output by itself.

```json project.json
{
  "format": 1,
  "extensions": [{{extensions}}],
  "tempo_map": {"time_signature": "{{time_signature}}", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []
}
```

- `tempo_map.tempo_changes`: the tempo from a tick on, as steps. The first is at tick 0, ticks go up, `bpm` is 10 to 1000. Edit it to change the tempo. It applies live.
- `tempo_map.time_signature`: one for the whole project, such as `"3/4"` or `"6/8"`. The bar math of the map follows it when the map is written again.
- `connections`: only routing that tools do not make themselves, for example `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`. The ends are instance ids and port names. `device_output` is the first device channel of the connection: audio is stereo everywhere, so the left channel goes to that number and the right one to the channel after it. A connection whose instance is missing stays in the file, unused, and is listed in `problems.txt`.
- Leave `format` and `extensions` alone. A change to `extensions` is refused while the project runs.

The runtime writes this file whole. While it holds a change that does not load, the runtime does not write it, and `problems.txt` says that tempo and connection edits are not saved until the file is fixed.
