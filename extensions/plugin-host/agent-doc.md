# Third-party plugins

A plugin is the tool `plugin`, a child like a built-in device: an instrument its parent plays
`notes` into, or an effect its parent wires sound through. CLAP and VST 3 work; AU does not.
One record serves both places. Here the instrument of `rhodes`:

```json state/song/rhodes/synth.json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "rhodes"}
}
```

A VST 3 plugin, with its own state file:

```json state/song/strings/synth.json
{
  "tool": "plugin",
  "state": {"format": "vst3", "plugin_id": "A1B2C3D4E5F60718293A4B5C6D7E8F90", "state_asset": "strings"}
}
```

| Field | Meaning |
| --- | --- |
| `format` | `clap` or `vst3`. |
| `plugin_id` | For `clap`, the maker's id, such as `com.u-he.diva`. For `vst3`, the class id as thirty-two hex digits. Not the plugin's file name. |
| `state_asset` | A name you choose for `assets/plugin-state/<name>.bin`, the plugin's own settings. Give every plugin its own name: two records that name one file share it. |

- Writing a plugin record over a synth record replaces the synth, and the other way round.
- Nothing checks whether a plugin is an instrument or an effect: a record may name any plugin in
  either place. A plugin that takes no audio in replaces the sound that reached it.
- The composer can change the record in the app, pins too. Read it before you write it.

## Never edit the state asset

Do not open, edit or copy `assets/plugin-state/<name>.bin`. The app writes it, and undo never
changes it. Deleting a record does not delete its file. To make a plugin start fresh, delete the
file while no app has the project open.

## Which plugins this machine has

Every `plugin_id` already under `state/` is a plugin this machine had. For the rest, when you
can run commands:

```sh
sound-tools --plugins
```

    clap  com.example.piano                 Example Audio Piano (instrument, instrument synthesizer)
    vst3  A1B2C3D4E5F60718293A4B5C6D7E8F90  Example Audio Strings (instrument, Instrument Synth)

The first word is the `format`, the second the `plugin_id`. When `sound-tools` is not on your
`PATH`, ask the composer to pick **Install command line tool** in the project menu, or ask for
the id.

## The parameters of a plugin

```sh
sound-tools --plugin-params clap com.example.synth
```

    0  Cutoff  20 to 20000  default 1000
    1  Wave    0 to 2       default 0     3 steps: 0 = Sine, 1 = Saw, 2 = Square  not automatable

One line per parameter: id, name, range, default, and steps if any. Values are in the format's
own units: a `clap` plugin's own value, always `0` to `1` for `vst3`. The list can be thousands
of lines, so search it, for example with `grep -i cutoff`.

## Pinned parameters

`parameters` holds pins, by the id `--plugin-params` prints. A pin's value wins over the state
file and moves the plugin live. This pins the `Cutoff` of `rhodes`:

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

- `value` is in the units `--plugin-params` prints. A stepped parameter takes the step's number,
  never its name.
- `name` is only for the reader; the id counts.
- At most 64 pins. Leave `parameters` out for none.

## Automating a pin

A `lane` (`agent-docs/lane.md`) moves a pin: the parent connects the lane's `value` to
`parameters.<id>.value` of the plugin. Lane values are in the pin's units. This one opens the
`Cutoff` above over bar 1 and closes it over bar 2:

```json state/song/rhodes/lane-cutoff.json
{
  "tool": "lane",
  "state": {
    "points": [{"tick": 0, "value": 400.0}, {"tick": 3840, "value": 4000.0}, {"tick": 7680, "value": 400.0}]
  }
}
```

Only a pinned parameter takes a lane, and not one `--plugin-params` prints with steps or
`not automatable`. Pin it first.

## When it does not play

`problems.txt` says what is wrong. Some lines need more:

- `the plugins of this machine are still being looked at`: nothing is wrong. Read
  `problems.txt` again in a few seconds.
- `the state of the plugin ... could not be read`: usually two records name one `state_asset`
  for different plugins. Give each its own name.
- `... offers the host no way to send the sustain pedal`, or `... asked to be started again ...
  and did not start`: nothing to fix in the file.
- On the parent, a connection to `parameters.<id>.value` that the plugin takes no number of:
  the pin is missing, has steps, may not be automated, or is out of range.

## VST

VST is a registered trademark of Steinberg Media Technologies GmbH.
