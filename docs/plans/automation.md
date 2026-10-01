# Plan: automation

## Summary for Casper

- **Model.** Automation lives on the track: one line per parameter, in project ticks. No clip automation, no toggle.
- **Moving a clip moves the automation under it.** It replaces what was at the new spot. Alt-drag moves the clip alone. A ghost of the line follows the drag, so you see the result before you drop.
- **Audio path.** The arrangement reads the lines every block and sends each device the new values as events. Renders stay deterministic.
- **v1 scope.** Track volume and pan, plus every `Parameter` of the built-in instruments and effects. Plugins come later.
- **Knob.** An automated knob shows the playing value with an automation mark and does not drag.
- **Curves later.** v1 lines are straight. A curve can be added later as an optional field per point, so no file changes.

---

## Goal

A composer or the agent can make any built-in knob change over time: "filter opens over bars 9–16" is two points. Moving, copying and duplicating clips keeps the automation that belongs to them, and it is always visible what a move will do.

## Decided constraints

- One place: lanes are in the track record, in ticks, as `{tick, value}` points. Values are in the parameter's own units (Hz, dB, 0 to 1), the same as the record.
- Between two points the line is straight in the knob's travel, so a cutoff sweep sounds even. Before the first point the lane holds the first value, after the last it holds the last. A parameter with no lane plays its record value.
- Moving a clip moves the points within its length, `[start, end)`, and they replace the points at the new spot. The values just outside both spots stay what they were (edge points, as `lane::cut` does), so nothing jumps there. Alt-drag moves the clip only. Copy, cut, paste and cmd-d take the automation along the same way. Each is one undo step.
- While dragging: the moved part of the line follows as a ghost, the part it replaces fades, alt flips it live, and a hint says "Automation moves · alt to leave it" when there is automation under the clip. With lanes collapsed, the clip shows an automation mark during the drag.
- The knob of an automated parameter shows the playing value with an automation mark and does not drag. Removing the lane gives the knob back its record value.
- Lanes are drawn and erased like the bend, mod and pressure lanes (DESIGN.md), one lane per automated parameter under the track.
- The audio thread evaluates lanes from ticks per block, like the expression lanes. Nothing comes from the control thread while playing, so a render is the same bytes every time.
- An automated value ramps over the block, not the 20 ms edit glide, so it is on time and does not click.
- Extensions do not depend on each other: the automation event type lives in `sound-core`, next to `Parameter`.
- The agent doc of the arrangement explains lanes with an example that the doc test loads.

## How to verify

- A render with a filter sweep in the track record gives the same bytes twice, and the cutoff at bar 9 and bar 16 matches the points (a runtime project test).
- Moving a clip in a test moves its points, replaces those at the destination, and leaves values outside both spots unchanged. Alt-move leaves the lane untouched. Undo restores both clip and lane in one step.
- Copy, paste and cmd-d of a clip copy its automation.
- The realtime sanitizer job passes with automation playing.
- In the app: draw a volume fade, drag a clip with and without alt, and check the ghost and the hint (screenshot).

## Later

- Curved lines: one optional bend amount per point for the segment after it, not bezier handles. Missing means straight, so old files read the same.
- Plugins: CLAP and VST 3 both take parameter changes in a block, so the plugin host can take the same events.

## Notes for the implementer

- Most processors take their whole state as `Update` (`FilterState`, `DelayState`, …). Add one event input per automatable processor, `Automation { parameter: u16, value: f32 }`, where `parameter` is the index in its `PARAMETERS` list. The processor applies it with the `Parameter::set` of that index to its own copy of the state and runs the same code its `update` runs, with a one-block ramp. A small helper in `sound-core` keeps this to a few lines per processor.
- The arrangement already builds each track's chain, so it connects the track sequencer's automation output to each device on the track. Lanes reach the sequencer in its snapshot, resolved from field names to indexes on the control side.
- Nested records (Wavetable) need a parameter id per object. Name them by path, for example `filter_1.cutoff`, and keep the index resolution in one place.
- `lane.rs` points are integer `LaneValue`s. Automation needs `f32` values and the knob travel per `Parameter` (a curve, so the line is straight in travel). Reuse `value_at` and `cut` by making them generic over the value, not by copying them.
- Track volume and pan are arrangement fields, so they skip the event and are read by the mixer directly.
