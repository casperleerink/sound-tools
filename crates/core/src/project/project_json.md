# project.json: tempo, time signatures, sections and extra connections

One file at the top of the project folder. Most work needs no edit here: the connections a project starts with already reach the speakers.

```json project.json
{
  "format": 1,
  "extensions": {{extensions}},
  "tempo_map": {
    "time_signatures": [
      {"signature": "4/4", "bars": 8},
      {"signature": "7/8", "bars": 2},
      {"signature": "3/4", "bars": 1}
    ],
    "tempo_changes": [{"tick": 0, "bpm": 120.0}]
  },
  "sections": [{"bar": 1, "name": "Intro"}, {"bar": 5, "name": "Verse"}, {"bar": 13, "name": "Chorus"}],
  "connections": []
}
```

- `tempo_map.tempo_changes`: the tempo from a tick on, as steps. The first is at tick 0, ticks go up, `bpm` is 10 to 1000.
- `tempo_map.time_signatures`: runs of bars, in order. An upper number from 1 to 32 and a lower one of 1, 2, 4, 8, 16 or 32; `bars` is 1 or more. The last run goes on to the end, so the example is 8 bars of 4/4, 2 of 7/8, and 3/4 from bar 11 on. An older project has `"time_signature": "4/4"` in place of the list: one run.
- Changing the time signatures moves bar lines, never notes: every clip and note keeps its ticks. To keep a part on its bars, move its clips yourself.
- `sections`: the named parts of the piece. Each runs from the start of its `bar`, counted from 1, up to the next; sorted by bar, each bar once, names not empty. In the example the verse is bars 5 to 12. When you move, repeat or remove the music of a section, move the later sections by as many bars.
- `connections`: only routing that tools do not make themselves, such as `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`. The ends are instance ids and port names. Audio is stereo: `device_output` is the left channel, the right goes to the next one. A second line from the same port to the next channel is not used.
- `{"from": {"device_input": 0}, "to": {"input": {"instance": "mic", "port": "audio"}}}` plays the audio input of the system live into an audio input port, channel 0 to the left and 1 to the right, or 0 to both on a one-channel input.
- `{"from": {"app": "Music"}, "to": {"input": {"instance": "fx", "port": "audio"}}}` plays another app live into an audio input port: its name as the Dock shows it, a program such as `"afplay"`, or `"all"` for every app but Sound Tools. macOS 14.2 or later. To send the project's sound to another app, the composer picks an output device such as BlackHole in the project menu.
- A device input or an app is heard only in the window; a render hears silence.
- `extensions`: the extensions whose records this project loads. A project made before an extension existed may not list it, and `problems.txt` then says its tool is not registered. To fix it, add the missing names to the list, as in the example, and tell the composer to open the project again: a change to `extensions` applies only then.
- `main`: the instance at the top whose view fills the window, such as `"main": "song"`. Left out, it is the first one with a view, by name.
- Leave `format` alone.

The runtime writes this file whole. While it holds a change that does not load, the runtime does not write it.
