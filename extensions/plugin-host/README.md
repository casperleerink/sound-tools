# plugin-host

Third-party audio plugins as tools of a project. CLAP instruments since step 4a of the second
milestone, VST 3 instruments since step 5a, their windows since 5b, and effects since step 6.
The decisions are in
[ARCHITECTURE.md](../../ARCHITECTURE.md), "Hosting plugins". `agent-doc.md` is what an agent
reads; this file is for whoever works on the host.

VST is a registered trademark of Steinberg Media Technologies GmbH.

## The tool

One tool, `plugin`. Its record names the format, the plugin's own id and the file that holds
the plugin's own settings:

```json
{
  "tool": "plugin",
  "state": {"format": "clap", "plugin_id": "com.example.piano", "state_asset": "piano"}
}
```

`format` is `clap` or `vst3`. It declares three ports of the note contract, `notes` in, `audio`
in and `audio` out, so one record fits the `instrument` child of a track like the built-in
synth and an effect slot after it. Nothing in the arrangement knows about plugins and nothing
here knows about tracks or slots: which ports a track wires is the track's business.

So nothing here asks what a plugin says it is before it loads one. What a plugin declares is
what a picker offers it for (`Plugins::instruments`, `Plugins::effects`) and nothing more:
Spectral Freeze on the machine this was written on declares itself an instrument and is an
effect, and a record written by hand may name any plugin in either place. A plugin with no
audio input in an effect slot is given its input and drops it, so what reached it is replaced
by what it plays.

An instrument's audio input is connected to nothing and is silent, which is what every audio
input of a plugin got before effects existed. A slot whose plugin is missing, or whose plugin
failed, copies its input to its output: that is what keeps one missing effect from silencing a
track, and for an instrument it is the silence it always was. The block a plugin fails on is
already such a slot: a backend writes nothing into its output when it fails, so a host that
passed through only from the next block would leave one block of silence in the chain.

`state_asset` is a name, not a path: the file is `assets/plugin-state/<name>.bin`, through the
core's `AssetName`, so a record can never point outside the project folder.

## The parts

| File | What is in it |
| --- | --- |
| `lib.rs` | The record, the format, the tool, the behaviour and `new_state_asset`. |
| `host.rs` | `Plugins`: the plugins this project has loaded, the saving rule, the problems and the windows. It knows no format. |
| `backend.rs` | What the host needs of a plugin, whatever its format: `LoadedPlugin`, `PluginGui`, `Requests`. |
| `processor.rs` | The engine processor around a plugin's audio side: the note contract and one stereo port in, one stereo port out. `Started` is the audio side of a backend. |
| `scan.rs` | What this machine has, found in a child process per bundle, with the cache of this machine. |
| `window.rs` | The plugin's own window: one window of the application per open plugin. |
| `view.rs` | The card of a plugin in a rack, 200 pt: `Open window` at the top of its body and `CLAP · <maker>` on the value line of its second row. No expand, because nothing is hidden, and no power, because there is no bypass yet. And what a rack calls one. |
| `clap.rs` | The CLAP backend: the host callbacks, loading, the window and playing. |
| `vst3/` | The VST 3 backend. `module.rs` loads a bundle, `plugin.rs` is the control side, `process.rs` the audio side, `context.rs` what the host is from the plugin's side, including the edits its controller makes, `stream.rs` an `IBStream` over bytes, `view.rs` the plugin's window (`IPlugView`, `IPlugFrame`). |
| `src/bin/plugin-scan.rs` | The child process, for the tests of this crate. The runtime is its own child. |

## What each format decides, and what they share

Both formats split a plugin in two and put the two halves on different threads, so the host is
built once:

- The plugin's own handle belongs to the application's main thread. Only its audio side may go
  to the audio thread: CLAP's audio processor, VST 3's `IAudioProcessor`.
- Starting and stopping the processing belongs to the audio thread. Activating and deactivating
  belong to the main thread while nothing is processing.

So `Plugins` lives on the thread the project lives on and hands the audio sides to the engine,
and `HostedPlugin`, the engine processor, holds the audio side and nothing else. It is sent a
plugin through a `Processor::Update` and gets `None` when the plugin goes, so the old one rides
back to the control thread and is dropped there. A plugin is stopped before it leaves the audio
thread: in `Processor::update` when it is swapped, and in `Processor::leaving`, the core's last
call to a processor. `Drop` on the audio side is the last resort for the engine being torn down,
when there is no audio thread left. `tests/plugin_host/lifecycle.rs` drives the engine from a
thread of its own and reads what the test plugin wrote down, for both formats, so what a strict
plugin would assert is asserted.

The one difference the host had to grow for VST 3 is that a plugin's two halves only meet
through the host. CLAP has `clap_host_state.mark_dirty` and one object. VST 3 has neither:

- What the plugin changed by itself comes back in the block's output parameter changes. The
  host reads those on the audio thread into a lock-free ring, and the next poll gives them to
  the plugin's controller with `setParamNormalized` and marks the state to be saved.
- What the composer changed in the plugin's own window arrives at
  `IComponentHandler::performEdit`, and has to go the other way, to the processor.
  `ivsteditcontroller.h`: "Allow transfer of parameter editing to component (processor) via
  host and support automation." The handler keeps the newest value of every parameter, by
  parameter; the poll moves them into a second ring; `begin_block` empties that ring into the
  block's `inputParameterChanges`. By parameter, because a knob drag is hundreds of edits of
  one parameter and only the last is the sound, so what the ring has no room for goes back and
  waits for the next poll instead of being dropped. A render polls for every block, so a render
  gets the edits too.

The once-a-second rule, the saving moments and the asset are the same for both.

This is why a behaviour is no longer `Send`: it keeps an `Rc` of the host. The project has
always lived on one thread.

Nothing in `process` allocates, locks or makes a system call, including the translation of
notes and the sustain pedal and everything VST 3 needs per block: the event list, the parameter
changes and the process data are all made when the plugin loads. The plugin's own calls are
wrapped in an `rtsan` `ScopedDisabler`, one call at a time and nothing of ours inside it: what a
plugin does inside itself is not ours to check.

That exemption would also hide a buffer of ours growing while a plugin pushed into it, so a
CLAP plugin is given `OutputEvents::void()` and a VST 3 plugin is given an event list that
refuses `addEvent` and parameter queues with a fixed number of points. A test counts every
allocation of the process while a plugin sends fifty thousand things a block, for both formats,
and the count is zero.

## Notes, the pedal and stopping

The translation of the note contract is in `processor.rs` and is the same for both formats: the
list of keys that are down, the expansion of `AllOff` into the keys that are really down, and
the bound of 512 events a block. A backend only says how one event is written down.

- CLAP: a note on and a note off go as CLAP note events when the plugin's note port takes that
  dialect, else as raw MIDI. The dialect is read once, when the plugin loads. The sustain pedal
  always goes as raw MIDI controller 64 with its value, 0 to 127: CLAP note events have no
  sustain.
- VST 3: notes go as `kNoteOnEvent` and `kNoteOffEvent` with `noteId` -1, so a note off matches
  by pitch. VST 3 has no MIDI controller event at all. The format's own answer is
  `IMidiMapping`: the plugin's controller says which parameter MIDI controller 64 is mapped to,
  and the host sends that parameter as a value from 0 to 1 in the block's parameter changes, at
  the frame the pedal moved.
- A plugin that takes notes and offers neither gets the notes and not the pedal, and the
  record is listed in `problems.txt` saying so. A plugin with no note port at all, which is
  what an ordinary effect is, has no pedal to miss and is not listed: this host cannot ask what
  a record is for, so it goes by what the plugin has.
- `NoteEvent::AllOff` becomes a note off for every key this wrapper started, plus the pedal up.
  Both formats have a note off that matches every key, and not every plugin handles one, so the
  exact keys go out. The wrapper keeps that list as 128 bits.
- At most 512 events reach the plugin in one block. Anything above that is counted in
  `EngineStatus::event_overflows`, never allocated.

## Scanning

Loading a plugin runs its code, so the scan never happens in the application's process. One
child process per bundle: a plugin that crashes while it is looked at costs one bundle and is
reported, and the application lives. The runtime is its own child, through
`runtime --scan-plugin <format> <bundle>`; the tests of this crate use `plugin-scan` in this
crate. The format of a bundle is its file extension, `.clap` or `.vst3`, so one list of search
folders covers both.

The child prints one marked line per plugin. Anything else on its output is the plugin's own
logging, which real plugins do while they load, and it is ignored.

Every child has `SCAN_TIMEOUT`, ten seconds, and the deadline covers the whole bundle. Licensed
plugins hang while they are listed when they cannot reach their server, so a child that does
not finish is killed, waited for, and reported like one that crashed.

The deadline covers the threads that read the child's output as well. Both pipes are read while
the child runs, so a plugin that prints more than a pipe holds is not blocked on its own write;
and a pipe ends only when its last writer lets go of it, which a licensing helper that the
plugin left behind and that inherited the pipe does not do. Such a reader is given a quarter of
a second after the child has ended and is then left to itself: everything the child printed is
already read, and the scan goes on.

### Off the thread that draws, and the cache

Measured September 21, 2026 on an Apple Silicon laptop with the 28 bundles this machine really
has, 26 VST 3 and 2 CLAP:

| | |
| --- | --- |
| A whole scan, nothing remembered | 7.9 s |
| A VST 3 bundle, mean | 216 ms |
| A VST 3 bundle, slowest | 333 ms (Auto-Tune Vocal EQ) |
| A VST 3 bundle, fastest that answered | 9 ms |
| A CLAP bundle | 9 to 12 ms |
| Bundles that hung | none |
| Bundles that crashed | none |
| Bundles that showed a dialog | none |
| Bundles that could not be loaded | 1 (Vital, an x86_64 binary an arm64 host cannot load) |

A VST 3 bundle costs twenty times a CLAP one because it is a real bundle: `CFBundleCreate`,
`CFBundleLoadExecutable` and the plugin's static initializers, which for a licensed plugin
include its licence check.

Eight seconds is far too much to pay on the thread that draws, and it is paid on every start.
So:

- The window starts the scan on a thread of its own (`Plugins::start_scanning`) before it opens
  the project, and never waits for it. `--render`, `--inspect`, `--headless` and `--plugins`
  have no window to keep answering and wait for it the first time a record needs a plugin.
- There is a cache, and it belongs to the machine and not to any project:
  `~/Library/Caches/sound-tools/plugins.json`, or `SOUND_TOOLS_PLUGIN_CACHE`. A bundle is
  remembered by the modified time and size of the binary inside it, so a plugin that was
  installed or updated is looked at again and nothing else is. A bundle that crashed or hung is
  remembered as such and is not tried again on every start. `runtime --plugins` looks at
  everything again and writes the result, which is how one that was fixed comes back.
- A record whose plugin the scan has not found yet is reported as such, the track is silent, and
  the host asks for that record's behaviour to be run again when the scan finds it
  (`Plugins::take_retries` and `Project::rebind`). Nothing of the composer's is needed. Measured
  on this machine with nothing remembered: the project opens at once, `problems.txt` says three
  plugins are still being looked at, and twelve seconds later it says "No problems".
- With the cache, opening this project of three sampled VST 3 instruments costs 0.59 s and the
  scan nothing; without it, a blocking open costs 6.0 s.
- The picker shows what is known and says "Still looking for the plugins of this Mac…" while a
  scan runs.

A plugin installed while the app runs is not found until the next start.

`runtime --plugins` prints every plugin of this machine with its format, its id and how long
the scan took. It is how a composer or an agent finds the id a record needs.

Search folders are `~/Library/Audio/Plug-Ins/CLAP`, `/Library/Audio/Plug-Ins/CLAP` and
`CLAP_PATH`, plus `~/Library/Audio/Plug-Ins/VST3`, `/Library/Audio/Plug-Ins/VST3`,
`/Network/Library/Audio/Plug-Ins/VST3` and `VST3_PATH`. Tests point the host at a folder of
their own, with the repository's own test plugins in it, so no test needs a plugin of the
machine, and they never read or write the cache.

## Rendering and playing

A render is not a device run, and a plugin is told which it is in the way its own format has:

- VST 3: `ProcessSetup::processMode` is `kOffline` instead of `kRealtime`, and every
  `ProcessData` of that setup carries the same mode, which is what the format asks.
- CLAP: the `clap.render` extension, set on the main thread before the plugin is activated.

It is the one thing a plugin can be told that makes a streaming sampler wait for its samples
instead of playing the silence of what is not loaded yet. What the engine hands a processor is
`PrepareConfig::offline`, which is the whole of what the core knows about this; `runtime::OFFLINE`
is the engine `--render` and `--inspect` open a project with.

A render also does the main-thread work of the host for every buffer, exactly as a live session
does (`runtime::render_block`). A plugin may be silent until its host answers it: CLAP has
`request_callback`, and a VST 3 plugin's controller has to be given back what the plugin changed
by itself. A render that only ran the engine would write that silence into the file.

Measured with the three sampled VST 3 instruments of this machine, whose samples stream from
disk: renders of a project whose samples are loaded are byte for byte the same, one after
another and across a close and a reopen.

## What a plugin id is

- CLAP: the id its maker chose, such as `com.u-he.diva`.
- VST 3: the class id, sixteen bytes, as thirty-two uppercase hex digits. That is what
  Steinberg's own `FUID::toString` gives on macOS and Linux and what a `.vstpreset` file holds,
  so the id in a record is the one a plugin's maker publishes. A class id never changes, which
  is what makes it the name of a plugin for good. A `plugin_id` of a `vst3` record that is not
  thirty-two hex digits is refused by the record itself, so an agent is told where the mistake
  is and not that the plugin is missing.

## When plugin state is saved

A plugin's state is opaque. Two moments write it:

- While the project is open, when the plugin says its state changed, at the next
  `Plugins::poll`, which is every 16 ms in the window and every 5 ms headless, and then at most
  once a second while it keeps saying so. A plugin marks itself dirty on every step of a knob
  drag, and serializing a sampler's state is not cheap. What the host was told is kept until
  the bytes are written, so a change that waits for the second is written by a later poll.
- When the project closes, for every loaded plugin, whether it said so or not.
- When a plugin goes, because its record was deleted or now names another plugin. So undo of a
  delete brings the plugin back as it sounded.

Bytes that are already in the project are not written again, so a session that changed nothing
leaves no diff. A plugin's state is not project state: it is never an undo step, and undo and
redo never touch it.

A VST 3 plugin keeps two states, the component's and the controller's, as a preset file does.
The asset holds both: `SVT3`, the component's state with its length, then the controller's. The
controller's state is asked of the edit controller interface whatever object it is, because one
object that is both halves does not promise that its two states are the same bytes.

The state in the project is the composer's sound, so nothing replaces it with a guess:

- A plugin half that answers `kNotImplemented` or `kResultFalse` keeps no such state, which is
  what a one-object plugin usually says of its controller half. Any other failure code is a
  failure: the asset is left exactly as it is and the composer is told
  (`problems.txt`). A plugin that could not give its state at all is not written at all.
- A state above `MAX_STATE`, half a gigabyte, is refused before the asset is touched: the two
  lengths in the file are four bytes each, so a longer one would not read back. The same number
  bounds what a file that is not ours can make this process allocate.
- The stream a plugin is given is Steinberg's `MemoryStream` call for call, including a seek
  past the end: a plugin that leaves room for a header, writes its payload and seeks back to
  fill the header in is doing something the format allows, and a stream that clamped that seek
  would write the payload at byte zero for the header to overwrite.
- A state that could not be written is still to be written: the host tries again at a later
  poll and when the plugin goes.

What a crash can lose: up to a second of a plugin's own changes, and anything a plugin changed
without saying so since the project opened.

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
  side back, so a plugin that is still playing misses no callback and loses no change.

Loading every time costs nothing: a behaviour runs when its own record changed, on opening the
project and on a retry, and every change a plugin record can have needs another plugin or
another state file. `tests/plugin_host/consistency.rs` walks the sequences, for both formats.

## The plugin's own window

Both formats put the plugin's own view in a window of ours. The window machinery in `host.rs`
and `window.rs` knows no format: all it needs of a plugin is `backend::PluginGui`, and the two
backends fill that in. The VST 3 one is `vst3/view.rs` and it changed nothing of the design.

CLAP offers two ways to show a plugin: a floating window the plugin makes and owns, or a
window the host makes with the plugin's view embedded in it. The specification calls the
floating one a fallback every plugin should support. Neither real CLAP instrument on the
machine this was written on does: both answer `is_api_supported` with `false` for a floating
window and `true` for an embedded one. So this host makes the window.

- One GPUI window per open plugin, beside the main one, with a root view that draws nothing.
  The plugin gets that window's `NSView` and fills it.
- The window is as big as the plugin says and is not resizable by dragging. A plugin that asks
  for another size gets it at the next poll, which is how Six Sines sizes itself as it opens.
- Opening a window that is open brings it forward. Closing one frees the plugin's view and
  takes the window down, and touches nothing of the plugin's sound or state.
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

### What VST 3 asks that CLAP does not

`vst3/view.rs`, read from `pluginterfaces/gui/iplugview.h` and not from memory:

- The view comes from the plugin's edit controller, `createView(ViewType::kEditor)`, and it is
  made when the composer opens the window and never before. A card offers the window of any
  plugin with an edit controller; one whose `createView` gives nothing says so once and is not
  offered a window again this session. Asking at load would mean building the plugin's whole
  interface, which is 0.97 s for Crow Hill Origins, in every mode, see ARCHITECTURE.md.
- The order is create, `setFrame`, `getSize`, `attached`, and there is no separate show: a view
  is on screen as soon as it is attached. `removed` is called for an `attached` that was
  answered and for nothing else, then a null frame, then the release. The frame goes in before
  `attached` because the header says a plugin may ask to be resized from inside that call.
- A plugin that wants another size calls `IPlugFrame::resizeView`, and then, in the words of
  the header, "in the same callstack, the host has to call IPlugView::onSize". So the frame
  answers `onSize` inside the request and notes the size for the next poll, which is the one
  place that has the application and can resize the window. CLAP's `request_resize` only notes.
  The guards are Steinberg's own, from `editorhost.cpp`: a request that names a view this frame
  was not given is refused, a request made from inside the answer to another one is refused, a
  view that already has the size asked for is told nothing, and the view's own size is read
  afterwards, because that is what the window ends on. Without the second one a plugin that
  answers `onSize` with the same request runs the host out of stack.
- A view is taken apart in the order `closePlugView` takes it: the frame first, so the plugin
  cannot reach an object of ours in the middle of its own removal, then `removed` for an
  `attached` that was answered and for nothing else, then the release.
- On macOS a `ViewRect` is in logical units, so nothing sets a scale, which is what the CLAP
  side does too.
- VST 3 has no way for a plugin to close the window it is in, because the host owns that
  window. `Requests::window_closed` is therefore always false for a VST 3 plugin.
- Every call of a view belongs to the thread the user interface lives on. The one call a plugin
  makes of its own accord is `resizeView`; it is kept in an atomic all the same, so a plugin
  that calls it from elsewhere cannot make this host unsound.

`tests/plugin_host/window.rs` drives all of it on GPUI's platform for tests, whose windows are
not real ones, so no display is needed, and every check that is about the host and not about
one format runs for both. A window of that platform has no `NSView` to give a plugin, so
`attached` and `removed` never run there; the unit tests in `src/vst3/view.rs` drive the
backend itself with a parent the test plugin never touches, and cover attaching, a plugin that
refuses its parent, and a resize asked for from inside `attached`. What is left for a real
plugin is a view that really draws, which is checked by hand.

`tests/plugin_host/editing.rs` is the other way round: what the composer changes in the
plugin's window reaching the processor, the last value of a burst winning, and the state saved
afterwards holding it.

## Unsafe code

Every call into a plugin is unsafe, and every one of them is behind a safe layer here. CLAP's
is `clack-host`, so the only `unsafe` on that side is loading a bundle and giving a plugin a
view. VST 3 has no safe layer on crates.io: `vst3` 0.3 is the raw COM interfaces generated from
Steinberg's headers, so the safe layer is `src/vst3/` and nothing outside that folder calls a
VST 3 interface.

A VST 3 bundle is loaded once per process and never unloaded. Unloading runs the plugin's
static destructors and unregisters its Objective-C classes while views, timers and threads of
that plugin may still exist; every host this was written against keeps them loaded.

## What a composer picks

`Plugins::instruments` is every instrument of this machine and `Plugins::effects` every effect,
of every format, from the scan, for the two pickers of a rack. `Plugins::scan_generation` goes
up whenever the scan learns something and once more when it ends: a picker filled while a scan
ran holds a part of the list and the line that says so, and this is what tells it to fill
again. The window's poll asks for a frame when it changes, and `Devices::offers_generation`
carries it to whoever draws a menu. A plugin decides which list it
is in: CLAP's `instrument` and `audio-effect` features, VST 3's `Instrument` and `Fx`
subcategories. A plugin that says both is in both. `new_state_asset(assets, wanted)` gives a `state_asset` whose file no plugin has ever
written into: it is `Assets::create` and its numbering, the rule a raw take follows, so the file
is made and never opened. A plugin that is picked can therefore never come up holding the sound
an older one left behind, and undo brings the older one back as it sounded. The file is empty
until the plugin saves into it, and the host reads an empty one as nothing saved yet. The
runtime turns both into `sound_ui::DeviceOffer`s for the track panel; nothing here knows about
tracks or panels.

### Writing "VST"

Steinberg's VST usage guidelines ask for the VST Compatible Logo next to the term, and for the
attribution notice where the logo does not fit. A row of a menu is such a place, so the picker
writes the format as `VST 3` in plain text and carries `plugin_host::VST_TRADEMARK`, the
notice, as a quiet line under its offers whenever it offers a VST 3 plugin. `runtime --plugins`
prints the same line, and the docs carry it. "VST" is not in the product name, not in the
company name, and never stylized. The VST 3 SDK is MIT licensed since 3.8 and needs no signed
agreement to host or to write plugins.

## What is not built

AU, a plugin sandbox, parameter automation, a parameter view,
presets and program lists, MIDI out of a plugin, more than the first event input and the first
stereo output, the transport a plugin can read (`ProcessContext` is null, so a plugin that syncs
to the tempo runs free), answering `kParamValuesChanged` by reading every parameter of the
controller back into the processor, a plugin window that follows a drag of its edge (VST 3 says how, with
`canResize` and `checkSizeConstraint`, and CLAP does too; neither is wired to a GPUI resize),
key events passed to a view (`IPlugView::onKeyDown`; a plugin's own `NSView` is in the responder
chain of our window, so typing in it works through AppKit), remembering where a window sat or
whether it was open, a floating window for a plugin that only floats, keeping a plugin's window
above the main one, and finding a plugin installed while the app runs.

Two records may name one `state_asset` and then share it. Nothing refuses either: the project
runs only the behaviour of the record that was edited, so a complaint about another record
could never be taken back when that other record went. Two records for *different* plugins
report themselves anyway, because the second cannot read the first one's state.

A plugin whose latency changes is started again: CLAP's `request_restart` and VST 3's
`kLatencyChanged`. The host asks the engine for the plugin's audio side back, deactivates and
activates the same plugin on the main thread, reads its latency (`clap_plugin_latency.get`,
`getLatencySamples`) and hands the audio side back. The plugin keeps its state and its window;
in between its slot is empty, so an instrument is silent and an effect lets the sound through,
for about two buffers. The engine compensates the latency the plugin reports, see
ARCHITECTURE.md, "Latency compensation". The other VST 3 flags that ask for a restart,
`kReloadComponent`, `kIoChanged` and `kPrefetchableSupportChanged`, are told to the composer
and not done. ARCHITECTURE.md has the table of every flag.

A plugin is an instrument when it says so: the CLAP feature `instrument`, or the VST 3
subcategory `Instrument`, and an effect by `audio-effect` or `Fx`. Nothing checks whether
either is true, and nothing can. Audio input ports are no sign of an effect: Six Sines is an
instrument with a stereo input for audio-rate modulation, and it is offered as an instrument
because that is what it says.

## Checks

```sh
cargo nextest run -p plugin-host
RTSAN_ENABLE=1 cargo nextest run -p plugin-host
```

The tests build `tooling/test-clap-plugin` and `tooling/test-vst3-plugin` themselves and copy
them into a folder of their own. The two are the same plugin in the two formats, from
`tooling/test-plugin-support`, so a test reads either render the same way.

That one plugin is an instrument and an effect, and says both, because a second plugin in each
bundle would be a second descriptor, a second class id and a second copy of the window and
state code for nothing a test needs. In an effect slot it adds what it is played times a half
plus the offset of its saved state, so two of them chained say by their samples which came
first; and it learns that offset from an input of 0.9 or more, which is how a test makes an
effect change its own state, as the pedal does for the instrument half.
