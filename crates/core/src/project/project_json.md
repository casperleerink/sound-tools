# project.json: tempo, time signatures and extra connections

One file at the top of the project folder. Most work needs no edit here: a track plays through its own instrument and reaches the output by itself.

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
  "connections": []
}
```

- `tempo_map.tempo_changes`: the tempo from a tick on, as steps. The first is at tick 0, ticks go up, `bpm` is 10 to 1000. Edit it to change the tempo. It applies live.
- `tempo_map.time_signatures`: the time signature of every bar, as runs of bars, in order. `signature` is written like `"4/4"`, `"7/8"` or `"3/16"`: an upper number from 1 to 32 and a lower one of 1, 2, 4, 8, 16 or 32. `bars` is how many bars the run has, 1 or more. The last run goes on to the end of the piece, so the example is 8 bars of 4/4, 2 of 7/8, and 3/4 from bar 11 on. A change always falls on a bar line. For a new time signature every bar, as in the Danse sacrale, write one run per bar.
- Changing the time signatures moves bar lines, never notes: every clip and note keeps its ticks. To keep a part on its bars after you change a run, move its clips yourself.
- A project from before time signatures could change has `"time_signature": "4/4"` in place of the list. It reads the same as one run, and the runtime writes it as the list the next time it saves this file.
- `connections`: only routing that tools do not make themselves, for example `{"from": {"instance": "drone", "port": "audio"}, "to": {"device_output": 0}}`. The ends are instance ids and port names. `device_output` is the first device channel of the connection: audio is stereo everywhere, so the left channel goes to that number and the right one to the channel after it. One connection is the whole path, so a second line from the same port to the channel after it is not used, and `problems.txt` says so: that is what a project written before audio was stereo looks like, and the extra line can go. A connection whose instance is missing stays in the file, unused, and is listed in `problems.txt`.
- `{"from": {"device_input": 0}, "to": {"input": {"instance": "mic", "port": "audio"}}}` plays the audio input of the system (a microphone or an interface) live into an audio input port. `device_input` counts like `device_output`: input channel 0 to the left side and 1 to the right, or 0 to both on a one-channel input. It is heard only in the window, and only when input and output run at one rate, else `problems.txt` says so. A render hears silence.
- `extensions`: the extensions whose records this project loads. The example lists every extension of this runtime, which is what a new project lists. A project made before an extension existed may not list it: then a record of its tools does not load, `problems.txt` says the tool is not registered, and the app shows what that extension offers as out of reach. To fix it, add the missing names to the list, as in the example, and tell the composer to open the project again. A change to `extensions` is refused while the project runs, so it applies only after that.
- Leave `format` alone.

The runtime writes this file whole. While it holds a change that does not load, the runtime does not write it, and `problems.txt` says that tempo and connection edits are not saved until the file is fixed.
