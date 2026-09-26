# Third-party plugins on a track

A track owns one child named `instrument`, and after it the effects its record lists. Either can
be a third-party plugin. This doc is about the plugin. `agent-docs/arrangement.md` says how a
track names its effects and in what order they play.

CLAP and VST 3 work, as instruments and as effects. AU does not.

## The record

A plugin is the tool `plugin`. One record serves both places: put it where the instrument of
the track belongs, so for a track `rhodes`:

```json state/arrangement/rhodes/instrument.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "rhodes"}
}
```

A VST 3 plugin is the same record with another `format` and the class id as its `plugin_id`.
This is a second track, `strings`, with its own track record and its own state file:

```json state/arrangement/strings/instance.json
{
  "tool": "arrangement.track",
  "state": {"name": "strings", "colour": "sky", "order": 2, "gain_db": 0.0, "pan": 0.0, "mute": false}
}
```

```json state/arrangement/strings/instrument.json
{
  "tool": "plugin",
  "state": {"format": "vst3", "plugin_id": "A1B2C3D4E5F60718293A4B5C6D7E8F90", "state_asset": "strings"}
}
```

| Field | Meaning |
| --- | --- |
| `format` | `clap` or `vst3`. |
| `plugin_id` | For `clap`, the id the plugin's maker gave it, such as `com.u-he.diva`. For `vst3`, the plugin's class id as thirty-two hex digits, such as `A1B2C3D4E5F60718293A4B5C6D7E8F90`. Neither is the file name of the plugin, and neither is in any file of the project. Get them from `runtime --plugins`, or ask the composer. |
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

The same record under any other name in the track folder is an effect, once the track record
names it in `effects`. Nothing of the record changes: only where the track wires it. A plugin
that says it is an instrument can be an effect and the other way round; nothing checks, and a
plugin that takes no audio in replaces the sound that reached it instead of changing it.

The composer can do the same in the app, by picking an instrument on the card of the track
panel, so the record may change under you. Read it before you write it.

## The extension has to be enabled

`project.json` lists the extensions this project loads. A project made before the plugin host
existed does not list it, and then a plugin record does not load: `problems.txt` says the tool
is not registered. One edit fixes it: add `"plugin-host"` to the
`extensions` list in `project.json`, so that it reads

    "extensions": ["arrangement", "instrument", "plugin-host", "tone"]

and then tell the composer to open the project again. Enabling an extension while a project
runs is refused, so nothing of it works until they do. A project this runtime made lists it
already, and the app shows every plugin in its picker as out of reach until then.

## Never edit the state asset

`assets/plugin-state/<name>.bin` holds the plugin's own settings, in a format only that plugin
understands. A VST 3 plugin keeps two states, so its file holds both. Do not open it, do not edit it, do not copy it between plugins. The app writes it
when the plugin says its settings changed, at most once a second, and when the project closes.
It is not part of the undo history: undo and redo never change a plugin's settings.

Deleting a plugin's record does not delete its state file. To make the plugin start fresh,
delete `assets/plugin-state/<name>.bin` while no app has the project open.

A file that is already there is never taken up by a plugin the composer picks in the app: that
gets a name of its own, numbered like a take (`six-sines-1`, `six-sines-2`). Reusing a name is
for you, when you mean two records to share one sound.

`workspace.json` at the root of the project is where the app keeps the plugin windows: where
each was on the screen and whether it was open. It is not part of the piece and not part of the
undo history. Leave it alone; the app writes it while the composer moves windows.

## Which plugins this machine has

The ids this project already uses are in its own records: every `plugin_id` under `state/` is a
plugin this machine had when it was written. Read them before you look further; a plugin that
plays here is one you can name again.

For the rest, not from any file. When you can run commands:

```sh
runtime --plugins
```

It prints every plugin with its format, its id and what it says it is, and it looks at every
plugin again, so a plugin that failed once is tried again. `runtime` is the program that has
this project open. When it is not on your `PATH`, ask the composer for the id.

The first word of a line is the format, which is what `format` in the record takes:

    clap  com.example.piano                 Example Audio Piano (instrument, instrument synthesizer)
    vst3  A1B2C3D4E5F60718293A4B5C6D7E8F90  Example Audio Strings (instrument, Instrument Synth)
    clap  com.example.warmth                Example Audio Warmth (effect, audio-effect stereo)

What a plugin says it is is what the composer's pickers offer it for, and nothing more: a
record may name any plugin in either place.

## When it does not play

`problems.txt` names the record and says what is wrong. The usual lines:

- `this machine has no CLAP plugin with the id ...`, or `no VST 3 plugin`: the id is wrong, or
  the plugin is not installed here. The record stays as it is, while everything else plays.
  A missing instrument leaves its track silent; a missing effect lets the sound through
  unchanged, so the rest of the chain still plays. Correct `plugin_id` and it plays at once,
  with no restart. While the app's window is open, a plugin installed meanwhile is found
  within seconds and the record plays then.
- `a vst3 plugin_id is the class id as thirty-two hex digits`: the record itself is refused.
  You wrote something else, perhaps a CLAP-style id or the plugin's name.
- `the plugins of this machine are still being looked at`: nothing is wrong. The app looks for
  plugins on a thread of its own while it opens, so a project never waits for it. The track
  plays as soon as the scan reaches its plugin, which needs nothing of you. Read
  `problems.txt` again in a few seconds.
- `the state of the plugin ... could not be read`: usually two records that name one
  `state_asset` for different plugins. Give each its own name.
- `... offers the host no way to send the sustain pedal`: the notes play, the pedal does not.
  There is nothing to fix in the file. A CLAP plugin whose note port takes no MIDI, or a VST 3
  plugin that maps no parameter to MIDI controller 64, says this. An effect with no note port
  at all never says it: it has no pedal to miss.
- `... asked to be started again, because its latency or its buses changed, and did not
  start`: the app restarts such a plugin, and this one failed to start. Nothing in the file is
  wrong. The slot is silent, or lets the sound through for an effect, until the record changes.

## VST

VST is a registered trademark of Steinberg Media Technologies GmbH.
