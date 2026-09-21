# Third-party plugins on a track

A track owns one child named `instrument`. It can be the built-in synth or a third-party
plugin. This doc is about the plugin.

Only CLAP instruments work today. VST3 and effect plugins are not built yet.

## The record

A plugin is the tool `plugin`. Put it where the instrument of the track belongs, so for a
track `rhodes`:

```json state/arrangement/rhodes/instrument.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "rhodes"}
}
```

| Field | Meaning |
| --- | --- |
| `format` | `clap`. The only one this build hosts. |
| `plugin_id` | The id the plugin's maker gave it, such as `com.u-he.diva`. Ask the composer for it. It is not the file name of the plugin. |
| `state_asset` | A name you choose for the file that holds the plugin's own settings: `assets/plugin-state/<name>.bin`. Lowercase letters, digits, `-` and `_`. Give every plugin its own name: two records that name one file share it, and two different plugins that name one file cannot read each other's settings. |

The track record itself says nothing about the plugin:

```json state/arrangement/rhodes/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "rhodes", "colour": "peach", "order": 1, "gain_db": -3.0, "pan": 0.2, "mute": false}
}
```

Writing the plugin record over `instrument.json` replaces the synth with the plugin, live, as
one undo step. Writing a synth record back replaces the plugin again. Clips, notes and the
gain, pan and mute of the track are the same whichever instrument the track has.

## Never edit the state asset

`assets/plugin-state/<name>.bin` holds the plugin's own settings, in a format only that plugin
understands. Do not open it, do not edit it, do not copy it between plugins. The app writes it
when the plugin says its settings changed, at most once a second, and when the project closes.
It is not part of the undo history: undo and redo never change a plugin's settings.

Deleting a plugin's record does not delete its state file. To make the plugin start fresh,
delete `assets/plugin-state/<name>.bin` while no app has the project open.

## Which plugins this machine has

Not from any file. When you can run commands:

```sh
runtime --plugins
```

It prints every plugin with its id and whether it is an instrument. `runtime` is the program
that has this project open. When it is not on your `PATH`, ask the composer for the id.

## When it does not play

`problems.txt` names the record and says what is wrong. The usual lines:

- `this machine has no CLAP plugin with the id ...`: the id is wrong, or the plugin is not
  installed here. The record stays as it is and the track is silent, while everything else
  plays. Correct `plugin_id` and it plays at once, with no restart.
- `... is not an instrument`: the plugin is an effect. It cannot be the `instrument` of a
  track. Effect plugins are not built yet.
- `the state of the plugin ... could not be read`: usually two records that name one
  `state_asset` for different plugins. Give each its own name.
- `... takes no MIDI, so the sustain pedal does not reach it`: the notes play, the pedal does
  not. There is nothing to fix in the file.
- `... asked to be started again`: the plugin wants the app to reload it, which this build does
  not do. Nothing in the file is wrong. Tell the composer to take the plugin off the track and
  put it back if it stopped sounding.
