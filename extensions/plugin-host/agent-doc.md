# Third-party plugins

A plugin is the tool `plugin`, a child like a built-in device: an instrument its parent plays
`notes` into, or an effect its parent wires sound through. CLAP and VST 3 work; AU does not.
Here the instrument of `rhodes`:

```json state/song/rhodes/synth.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "rhodes"}
}
```

| Field | Meaning |
| --- | --- |
| `format` | `clap` or `vst3`. |
| `plugin_id` | For `clap`, the maker's id, such as `com.u-he.diva`. For `vst3`, the class id as thirty-two hex digits. Not the file name. |
| `state_asset` | A name you choose for `assets/plugin-state/<name>.bin`, the plugin's own settings. Give every plugin its own name: two records that name one file share it. |

- Nothing checks whether a plugin is an instrument or an effect. A plugin that takes no audio
  in replaces the sound that reached it.
- The composer can change the record in the app, pins too. Read it before you write it.

## Never edit the state asset

Do not open, edit or copy `assets/plugin-state/<name>.bin`. The app writes it, and undo never
changes it. Deleting a record does not delete its file. To make a plugin start fresh, delete the
file while no app has the project open.

## Which plugins this machine has

Every `plugin_id` already under `state/` is one this machine had. For the rest:

```sh
sound-tools --plugins
```

    clap  com.example.piano  Example Audio Piano (instrument, instrument synthesizer)
    vst3  A1B2C3D4E5F60718293A4B5C6D7E8F90  Example Audio Strings (instrument, Instrument Synth)

The first word is the `format`, the second the `plugin_id`. When `sound-tools` is not on your
`PATH`, ask the composer to pick **Install command line tool** in the project menu, or ask for
the id.

## Parameters

```sh
sound-tools --plugin-params clap com.example.synth
```

    0  Cutoff  20 to 20000  default 1000
    1  Wave    0 to 2       default 0     3 steps: 0 = Sine, 1 = Saw, 2 = Square  not automatable

Values are in the format's own units: a `clap` plugin's own value, always `0` to `1` for `vst3`.
The list can be thousands of lines, so search it, for example with `grep -i cutoff`.

`parameters` pins them by id. A pin wins over the state file and moves the plugin live:

```json state/song/rhodes/synth.json
{
  "tool": "plugin",
  "state": {
    "format": "clap",
    "plugin_id": "com.example.piano",
    "state_asset": "rhodes",
    "parameters": {"0": {"name": "Cutoff", "value": 1200.0}}
  }
}
```

- A stepped parameter takes the step's number, never its name.
- `name` is only for the reader; the id counts.
- At most 64 pins.

## Automating a pin

A `lane` (`agent-docs/lane.md`) moves a pin: the parent connects the lane's `value` to
`parameters.<id>.value` of the plugin, in the pin's units. Only a pin in range, without steps
and not `not automatable`, takes a lane.

## When it does not play

`problems.txt` says what is wrong. Some lines need more:

- `the plugins of this machine are still being looked at`: read `problems.txt` again in a few
  seconds.
- `the state of the plugin ... could not be read`: usually two records name one `state_asset`
  for different plugins. Give each its own name.
- `... offers the host no way to send the sustain pedal`, or `... asked to be started again ...
  and did not start`: nothing to fix in the file.

## VST

VST is a registered trademark of Steinberg Media Technologies GmbH.
