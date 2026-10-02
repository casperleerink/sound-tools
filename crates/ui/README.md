# sound-ui

Design tokens, shared components, and the bridge from a live project to GPUI views. This guide
gives the mental model for writing the view of a tool. The design rules are in `DESIGN.md`,
and `crates/gallery` shows every component. Read the GPUI skills in `.agents/skills/` before
you write GPUI code: the pinned GPUI differs from what most examples online show.

## The mental model

- **`Session`** is one GPUI entity per window. It owns the `Project` on the main thread. It
  polls the engine and the file watcher every 16 ms from a timer, never from `render`. A poll
  that finds nothing new tells nobody, so a still project draws no frames.
- **A view** holds the session and a typed `Instance<S>`. It reads the current state in
  `render` and **keeps no copy of saved state**. Its own fields are interface state only:
  scroll, zoom, selection, whether a card is expanded. Interface state is never saved and is
  never an undo step.
- **The session emits every `ProjectEvent`.** Edits from a view, file edits by an agent, undo
  and redo all look the same. A view subscribes and notifies only for the ids it shows. Events
  carry no state and do not say where a change came from.

So an agent that edits a file while a view is open is shown at once, also in the middle of a
drag. Nothing needs to sync.

## How a view edits

- **One step:** `session.edit(cx, |project| project.commit(label, changes))`. An error goes to
  the notice line of the window, so a view cannot lose one.
- **A drag is a gesture of the session:** `begin_gesture`, `gesture` per move, then
  `finish_gesture`, or `cancel_gesture` on escape. Sound and every view follow each move. The
  file is written once, and the whole drag is one undo step. The session holds the open edit,
  so undo and redo wait until the drag ends.
- **A control on saved state is controlled.** Give it the value on every render and hand its
  `ValueChange` to `ControlEdit::apply`. That one call does the gesture, a key step and a reset.
- **A knob on a number of the record is a `ParameterKnob`.** Its name, range and default come
  from the `Parameter`; the card gives the label, the name of the undo step and a readout such
  as `hertz_readout`.
- **A file from the composer goes through `import`:** `choose_file` opens the file panel, and
  `import_file` copies the file into the assets on a background thread.
- **A number an automation lane moves does not drag.** A device view keeps a
  `Lanes::follow(session, instance, Device::AUTOMATION)`, draws from `Lanes::state`, the record
  with the lanes over it, and makes each knob or handle of an automated number `automated`. A
  number of a nested object is asked with `is_automated_in`. `Lanes` draws the view again only
  when a value it shows changes.
- **Something that should sound now but is not an edit**, such as a preview note, goes through
  `Project::send`. Nothing is saved.

## How views find each other

Extensions never depend on each other, and neither do their views. The runtime fills two
registries (`crates/runtime/src/lib.rs`, `views`) and installs them as GPUI globals:

- `Views`: `register` gives a tool its main view, and `register_card` gives it a card for a
  slot in a rack. A host calls `Views::card_of` or `Views::view_of` to show the view of an
  instance whose tool it does not know.
- `Devices`: what a rack says about a slot (`describe`) and what a composer can pick for it
  (`instruments`, `effects`). A picker reads the offers when it is made, not per frame, because
  a source may have to look at the machine. It fills itself again when
  `Devices::offers_generation` changes. Every offer names its `OfferGroup`, and the picker
  lists the groups in that order. A built-in device names its icon, `device-<name>` in
  `assets/icons`; a plugin shows the plug.

## Rules that are easy to miss

- No `cx.notify()` and no entity updates inside `render` or a paint callback.
- Begin a gesture at the first mouse move that changes something, not at mouse down. A plain
  click is then no undo step.
- Work out each move from the value at the press and the distance moved, not from the live
  value. A drag there and back then ends where it began.
- The target of a drag can be deleted under it by an agent. Check on every move and on
  `Deleted`, and finish the gesture when it is gone. Finish an open gesture when the view is
  released too (`cx.on_release`).
- A callback that a control keeps holds the view weakly: `cx.listener`, `weak_callback` or
  `weak_action`, not `cx.processor`. A strong one keeps a closed view and its drag alive.
- Keep what repaints with the playhead apart from heavy views, and put `.cached(..)` on the
  heavy one. A notified view also renders every view above it.
- A meter reads `sound_core::Peaks` once per poll with `Metering` and `every_poll`, and
  notifies only when what it shows changed.
- Draw only what is visible. A view of many records paints on one `canvas`.
- Show a focus ring only when the focus came from the keyboard (`KeyboardFocus`).
- Put coordinate math in pure functions with tests.
- Use the components and theme tokens of this crate. A general component goes here, with a
  gallery entry. What only one tool needs stays in its extension.

## Copy an example

| You want | Start from |
| --- | --- |
| A device card with knobs and a display | `extensions/filter/src/view.rs` |
| A card with handles on a display | `extensions/instrument/src/view.rs` |
| A card with sections behind expand, selects and a list of rows | `extensions/wavetable/src/view/` |
| A rack that hosts cards of other extensions | `extensions/arrangement/src/view/track_panel.rs` |
| A meter and a mixer strip | `extensions/arrangement/src/view/track_panel.rs` |
| A large view on a canvas, with drags and keys | `extensions/arrangement/src/view.rs` |
| A test of the bridge | `crates/ui/tests/bridge.rs` |
| A test of a view with a simulated mouse and keys | `crates/runtime/tests/window/` |
