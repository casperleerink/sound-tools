# Plan: plugin parameters

## Summary for Casper

- **Pinned parameters.** A plugin record can list a few of the plugin's parameters by id. Those are shown on the card, an agent can read and write them, and automation lanes can move them. The rest stays in the plugin's own state file.
- **Two ways.** Writing a pinned value in the record moves the plugin. Turning a pinned knob in the plugin's own window writes the record, one undo step per gesture. A value that automation moves is never written back.
- **Controls by kind.** On the card, a parameter with two steps is a toggle, a few named steps are a dropdown, and anything else is a knob. The readout is the plugin's own text (`1.2 kHz`).
- **Agents.** `sound-tools --plugin-params <format> <id>` lists every parameter of a plugin: id, name, range, default and its options. An agent pins one by writing it in the record.
- **Safe.** The host reads pinned values by asking the plugin on the main thread, in the poll that already runs every 16 ms. Nothing new is read on the audio thread. Sending a value into a CLAP plugin uses a fixed-size ring, the way VST 3 edits already reach the processor.
- **Decided.** The card gets a parameter view, so "no generic parameter view" goes from DESIGN.md and `view.rs`.

---

## Goal

A composer sees and turns the parameters of a plugin that matter to them on its card, with controls that fit each parameter. An agent can list a plugin's parameters, set them and automate them, the same as the numbers of a built-in device.

## Decided constraints

- A pin lives in the plugin record, in `parameters`, keyed by the plugin's parameter id, with its name and value: `"parameters": {"12": {"name": "Cutoff", "value": 0.42}}`. The name is for reading; the id is what counts. Unpinned parameters stay in the state asset only.
- A value is always a number, in the format's own units: the plain value for CLAP, 0 to 1 for VST 3. Step names are for showing and for the list, never stored: they can repeat or change.
- The record wins over the state asset: after the state is loaded, every pinned value is sent to the plugin. A change of the pins sends what changed and never reopens the plugin; today every run of the behaviour opens it again (`host.rs`), so pins need a path that keeps the instance.
- The host reads pinned values on the main thread in the existing poll (CLAP `params.get_value`, VST 3 `getParamNormalized`), and writes the record only when the plugin's value differs from the one the host last sent or read for that pin. So a value still on its way to the processor is not overwritten, and a value the plugin rounds when it takes it is not written back. A gesture is one undo step: VST 3 says where it begins and ends (`beginEdit`/`endEdit`); for CLAP the host waits until the values are quiet for a moment. A pin a lane moves is not written.
- The audio thread reads nothing new from a plugin. Values go into a CLAP plugin as param events from a fixed-size ring. A value that does not fit waits on the main thread for the next poll, so the last value always arrives.
- At most 64 pins per plugin (`MAX_AUTOMATED`). The VST 3 block holds 64 parameter queues today, shared with edits from the plugin's window and the MIDI controls: raise it so all of them fit at once.
- A pin whose id the plugin does not have, or a value outside its range, is a problem in `problems.txt`. The rest of the record plays.
- The parameter list and step names come from the plugin (CLAP `get_info`, `IS_STEPPED`/`IS_ENUM`, `value_to_text`; VST 3 `getParameterInfo`, `stepCount`, `kIsList`, `getParamStringByValue`). Read-only parameters are left out. Step names are listed only up to a cap.
- `--inspect` still loads no plugin. `--plugin-params` loads the one plugin it is asked about, in its own process.
- When the plugin changes its parameters or their names (CLAP `rescan`, VST 3 `kParamIDMappingChanged` and `kParamTitlesChanged`), the list is read again.
- The card: a toggle for two steps, a dropdown for named steps, a knob otherwise, each with the plugin's text as readout. Pins are added from a search of the plugin's parameters on the card and removed there. An automated pin follows DESIGN.md like any automated knob.
- An automation lane names a pin by its path in the record, `parameters.<id>.value`. Lanes move on the parameter's own range, linear. Only a parameter the plugin says is automatable and not stepped takes a lane, as a whole number of a built-in device takes none. When the lane goes, the record value is sent again.
- The agent doc of the plugin host explains pins, the CLI and a lane on a pin, with examples the doc test loads.

## Order

Each step is one PR, merged before the next starts.

1. **Read.** The parameter list for both formats, `--plugin-params`, the test plugins given parameters with steps and text, the agent doc.
2. **Pins.** The record field, sending values into the plugin (CLAP ring), reading them back in the poll, undo per gesture, problems.
3. **Card.** The controls on the plugin card, the search to pin and unpin, DESIGN.md and `view.rs` updated.
4. **Automation.** Lanes on pins; core takes numbers whose names are known only at runtime. This PR deletes this plan.

## How to verify

- The CLAP and VST 3 test plugins each have a continuous parameter and one with named steps. `--plugin-params` lists both with ranges and names (runtime test).
- A pinned value in the record reaches the plugin and sounds (render test). A value the test plugin changes itself lands in the record as one undo step. A record edit is not written back, and does not reopen the plugin.
- An unknown id and a value out of range are reported and the rest plays.
- The realtime sanitizer job and the counting allocator tests of the plugin host pass with pins and lanes playing.
- A window test pins a parameter from the card, turns it, and checks the record and the undo history. The window snapshot shows a toggle, a dropdown and a knob on a plugin card.
- A render with a lane on a pin moves the parameter as the lane says, and renders the same bytes twice.
