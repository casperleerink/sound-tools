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
| `lib.rs` | The record, the tool, the behaviour and `free_state_asset`. |
| `host.rs` | `Plugins`: the plugins this project has loaded, and the host callbacks CLAP plugins call. |
| `processor.rs` | The engine processor around a plugin's audio processor: the note contract in, one stereo port out. |
| `scan.rs` | What this machine has, found in a child process. |
| `window.rs` | The plugin's own window: one window of the application per open plugin. |
| `view.rs` | The card of a plugin in a rack, and what a rack calls one. |
| `src/bin/clap-scan.rs` | That child process, for the tests of this crate. The runtime is its own child. |

## Threads

CLAP splits a plugin in two. The plugin's own handle belongs to the application's main thread;
only its audio processor may go to the audio thread. So:

- `Plugins` lives on the thread the project lives on, which is the main thread. It holds every
  `PluginInstance`, loads them, reads and writes their state and answers their callbacks.
- `HostedPlugin`, the engine processor, holds the started audio processor and nothing else of
  the plugin. It is sent a plugin through a `Processor::Update` and gets `None` when the plugin
  goes, so the old one rides back to the control thread and is dropped there.
- CLAP puts `start_processing` and `stop_processing` on the audio thread and wants `deactivate`
  on the main thread while nothing is processing. So a plugin is stopped before it leaves the
  audio thread: in `Processor::update` when it is swapped, and in `Processor::leaving`, the
  core's last call to a processor, when the engine takes it out of its slot. `Drop for Loaded`
  is the last resort for the engine being torn down, when there is no audio thread left.
  `tests/plugin_host/lifecycle.rs` drives the engine from a thread of its own and reads what
  the test plugin wrote down, so what a strict plugin would assert is asserted.

This is why a behaviour is no longer `Send`: it keeps an `Rc` of the host. The project has
always lived on one thread.

Nothing in `process` allocates, locks or makes a system call, including the translation of
notes and the sustain pedal. The plugin's own calls are wrapped in an `rtsan` `ScopedDisabler`,
one call at a time and nothing of ours inside it: what a plugin does inside itself is not ours
to check. The repository's own test plugin does not need it; a real one may.

That exemption would also hide a buffer of ours growing while a plugin pushed into it, so the
plugin is given `OutputEvents::void()`: it takes every event and keeps none. Nothing reads what
a plugin sends out, because MIDI from a plugin is not built. A test counts every allocation of
the process while a plugin sends fifty thousand events a block, and the count is zero.

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

Every child has `SCAN_TIMEOUT`, ten seconds. Licensed plugins hang while they are listed when
they cannot reach their server, and the scan is what a project waits for while it opens, so a
child that does not finish is killed, waited for, and reported like one that crashed. Ten
seconds is a thousand times what a real bundle costs and is paid once, by that one bundle.

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

A plugin's state is opaque. Two moments write it:

- While the project is open, when the plugin says its state changed (`clap_host_state.mark_dirty`),
  at the next `Plugins::poll`, which is every 16 ms in the window and every 5 ms headless, and
  then at most once a second while it keeps saying so. A plugin marks itself dirty on every
  step of a knob drag, and serializing a sampler's state is not cheap. The flag is not cleared
  until the state is written, so a change that waits for the second is written by a later poll.
- When the project closes, for every loaded plugin, whether it said so or not. A plugin that
  changes its state without telling the host, which CLAP asks it not to do, keeps its work.
- When a plugin goes, because its record was deleted or now names another plugin. So undo of a
  delete brings the plugin back as it sounded.

Bytes that are already in the project are not written again, so a session that changed nothing
leaves no diff. A plugin's state is not project state: it is never an undo step, and undo and
redo never touch it.

What a crash can lose: up to a second of a plugin's own changes, and anything a plugin changed
without saying so since the project opened. CLAP asks a plugin to
mark its state dirty whenever it changes, including on a parameter change, so a plugin that
follows the specification loses at most one poll.

## The table and the engine

The behaviour hands the engine a plugin every time it runs. This host never asks what the
engine already has, because the answer would be a guess: the project applies an edit group
whole or not at all, and a group it rejects never reaches the engine although this host has
already loaded for it.

- `open` lets go of whatever the instance held, saved on the way out, and then loads. A load
  that fails leaves no entry, so the record and the engine agree: silence and a problem.
- `poll` lets go of every entry whose record no longer says what the entry holds: deleted, no
  longer a plugin, or changed by an edit the project rolled back.
- An entry that has been let go of is still polled and saved until the engine gives its audio
  processor back, so a plugin that is still playing misses no callback and loses no change.

Loading every time costs nothing: a behaviour runs when its own record changed, on opening the
project and on a retry, and every change a plugin record can have needs another plugin or
another state file. `tests/plugin_host/consistency.rs` walks the sequences.

## The plugin's own window

CLAP offers two ways to show a plugin: a floating window the plugin makes and owns, or a
window the host makes with the plugin's view embedded in it. The specification calls the
floating one a fallback every plugin should support. Neither real CLAP instrument on the
machine this was written on does: both answer `is_api_supported` with `false` for a floating
window and `true` for an embedded one. So this host makes the window.

- One GPUI window per open plugin, beside the main one, with a root view that draws nothing.
  The plugin gets that window's `NSView` through `set_parent` and fills it.
- The window is as big as `get_size` says and is not resizable by dragging. A plugin that asks
  for another size with `request_resize` gets it at the next poll, which is how Six Sines
  sizes itself as it opens.
- Opening a window that is open brings it forward. Closing one frees the plugin's view
  (`destroy`) and takes the window down, and touches nothing of the plugin's sound or state.
- A window goes whenever its plugin does: another plugin in the record, the record deleted
  from a file or by an undo, the track deleted, the project closing. Opening one is not an
  edit, so undo never brings one back.
- `Plugins::open_window` and `close_window` need the application. `Plugins::poll` and the drop
  of the host do not have it, so they free the plugin's view and leave the window to
  `Plugins::settle_windows`, which whoever polls calls with the application in hand. That call
  also gives a window the size its plugin asked for. `Plugins::close_all_windows` is what the
  application calls as it quits, before anything of it is torn down.
- The plugin lets go of the view it is in before that view is released, whatever takes the
  window down. Opening a window registers a GPUI `on_window_closed` observer: GPUI removes a
  window from the application, tells those observers, and only then drops the `Window` it is
  still holding, which is what releases the `NSWindow` and its view. So the order holds by
  construction, also when the window's own close control is what took it down.
- Nothing of GPUI runs while the table of plugins is borrowed: a card that is drawn asks this
  host what its plugin has, and that would be a second borrow. `open_window` is in three
  steps for that reason.
- `clap_host_gui.closed` is the one window callback a plugin may make from another thread. Like
  the other cross-thread callbacks it only sets a flag that the next poll reads.

`tests/plugin_host/window.rs` drives all of it on GPUI's platform for tests, whose windows are
not real ones, so no display is needed. The plugin then gets no view to draw in, which is the
one thing those tests cannot cover; it is checked by hand with a real plugin.

## What a composer picks

`Plugins::instruments` is every CLAP instrument of this machine, from the scan, for a picker.
`new_state_asset(assets, wanted)` gives a `state_asset` whose file no plugin has ever written
into: it is `Assets::create` and its numbering, the rule a raw take follows, so the file is
made and never opened. A plugin that is picked can therefore never come up holding the sound an
older one left behind, and undo brings the older one back as it sounded. The file is empty
until the plugin saves into it, and the host reads an empty one as nothing saved yet. The runtime turns both into
`sound_ui::DeviceOffer`s for the track panel; nothing here knows about tracks or panels.

## What is not built

Effects, VST3, AU, a plugin sandbox, latency compensation, parameter automation, a parameter
view, presets, MIDI out of a plugin, more than one audio output bus, a plugin window that
follows a drag of its edge, remembering where a window sat or whether it was open, a floating
window for a plugin that only floats, and keeping a plugin's window above the main one
(`set_transient`: it needs a handle to our view that outlives the plugin's window, and on macOS
the main window is dropped before the project is).

Two records may name one `state_asset` and then share it. Nothing refuses either: the project
runs only the behaviour of the record that was edited, so a complaint about another record
could never be taken back when that other record went. Two records for *different* plugins
report themselves anyway, because the second cannot read the first one's state.

A plugin that asks to be started again (`request_restart`), which it may do after changing its
own port layout, is told to the composer instead of being restarted. A plugin that asks for
audio processing to begin (`request_process`) needs nothing: the host calls every plugin every
block while its track exists.

A plugin is an instrument when it says so in its CLAP features. Nothing checks whether that is
true: a plugin with the `instrument` feature that is really an effect loads, gets notes and is
silent. Audio input ports are no sign of one: Six Sines is an instrument with a stereo input
for audio-rate modulation. The host gives every audio input of a plugin silence.

## Checks

```sh
cargo nextest run -p plugin-host
RTSAN_ENABLE=1 cargo nextest run -p plugin-host
```

The tests build `tooling/test-clap-plugin` themselves and copy it into a folder of their own.
