# plugin-host

Third-party audio plugins as tools of a project. CLAP instruments since step 4a of the second
milestone; VST3 and effects come later. The decisions are in [ARCHITECTURE.md](../../ARCHITECTURE.md),
"Hosting plugins". `agent-doc.md` is what an agent reads; this file is for whoever works on the
host.

## The tool

One tool, `plugin`. Its record names the format, the plugin's own id and the file that holds
the plugin's own settings:

```json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "piano"}
}
```

It declares the ports of the note contract, `notes` in and `audio` out, so it fits the
`instrument` child of a track like the built-in synth. Nothing in the arrangement knows about
plugins and nothing here knows about tracks.

`state_asset` is a name, not a path: the file is `assets/plugin-state/<name>.bin`, through the
core's `AssetName`, so a record can never point outside the project folder.

## The parts

| File | What is in it |
| --- | --- |
| `lib.rs` | The record, the tool and the behaviour. |
| `host.rs` | `Plugins`: the plugins this project has loaded, and the host callbacks CLAP plugins call. |
| `processor.rs` | The engine processor around a plugin's audio processor: the note contract in, one stereo port out. |
| `scan.rs` | What this machine has, found in a child process. |
| `src/bin/clap-scan.rs` | That child process, for the tests of this crate. The runtime is its own child. |

## Threads

CLAP splits a plugin in two. The plugin's own handle belongs to the application's main thread;
only its audio processor may go to the audio thread. So:

- `Plugins` lives on the thread the project lives on, which is the main thread. It holds every
  `PluginInstance`, loads them, reads and writes their state and answers their callbacks.
- `HostedPlugin`, the engine processor, holds the started audio processor and nothing else of
  the plugin. It is sent a plugin through a `Processor::Update` and gets `None` when the plugin
  goes, so the old one rides back to the control thread and is dropped there.

This is why a behaviour is no longer `Send`: it keeps an `Rc` of the host. The project has
always lived on one thread.

Nothing in `process` allocates, locks or makes a system call, including the translation of
notes and the sustain pedal. The one exception is the plugin's own `process` call, which is
wrapped in an `rtsan` `ScopedDisabler`: what a plugin does inside itself is not ours to check.
The repository's own test plugin does not need it; a real one may.

## Notes, the pedal and stopping

- A note on and a note off go as CLAP note events when the plugin's note port takes that
  dialect, else as raw MIDI. The dialect is read once, when the plugin loads.
- The sustain pedal always goes as raw MIDI controller 64 with its value, 0 to 127: CLAP note
  events have no sustain. A plugin whose note port takes no MIDI gets the notes and not the
  pedal, and the record is listed in `problems.txt` saying so.
- `NoteEvent::AllOff` becomes a note off for every key this wrapper started, plus the pedal up.
  CLAP has a note off that matches every key, but not every plugin handles it, so the exact
  keys go out. The wrapper keeps that list as 128 bits.
- At most 512 events reach the plugin in one block. Anything above that is counted in
  `EngineStatus::event_overflows`, never allocated.

## Scanning

Loading a plugin runs its code, so the scan never happens in the application's process. One
child process per bundle: a plugin that crashes while it is looked at costs one bundle and is
reported, and the application lives. The runtime is its own child, through
`runtime <bundle> --scan-clap`; the tests of this crate use `clap-scan` in this crate.

The child prints one marked line per plugin. Anything else on its output is the plugin's own
logging, which real plugins do while they load, and it is ignored.

The scan runs once per session, the first time a record needs a plugin. A project with no
plugin record never scans. There is no cache: measured September 20, 2026 on an Apple Silicon
laptop with two real bundles holding three plugins, a whole scan takes 20 to 26 ms, about 10 ms
per bundle. A machine with fifty plugins would pay half a second, once, so a cache would buy
nothing yet. A plugin installed while the app runs is not found until the next start.

`runtime --plugins` prints every plugin of this machine with its id and how long the scan took.
It is how a composer or an agent finds the id a record needs.

Search folders are `~/Library/Audio/Plug-Ins/CLAP`, `/Library/Audio/Plug-Ins/CLAP` and
`CLAP_PATH`. Tests point the host at a folder of their own, with the repository's own test
plugin in it, so no test needs a plugin of the machine.

## When plugin state is saved

A plugin's state is opaque. It is written to its asset when the plugin says it changed
(`clap_host_state.mark_dirty`), at the next `Plugins::poll`, which is every 16 ms in the window
and every 5 ms headless. The window polls once more on its way out. A plugin's state is not
project state: it is never an undo step, and undo and redo never touch it.

What a crash can lose: whatever a plugin changed in the last poll, and anything a plugin
changed without saying so. CLAP asks a plugin to mark its state dirty whenever it changes,
including on a parameter change, so a plugin that follows the specification loses at most one
poll.

## What is not built

Effects, VST3, AU, a plugin sandbox, latency compensation, parameter automation, a parameter
view, presets, MIDI out of a plugin, more than one audio output bus, and the plugin's own
window with the picker that opens it, which is step 4b.

A plugin is an instrument when it says so in its CLAP features. Nothing checks whether that is
true: a plugin with the `instrument` feature that is really an effect loads, gets notes and is
silent. It is reported when it has audio inputs, which is what such a plugin usually has.

## Checks

```sh
cargo nextest run -p plugin-host
RTSAN_ENABLE=1 cargo nextest run -p plugin-host
```

The tests build `tooling/test-clap-plugin` themselves and copy it into a folder of their own.
