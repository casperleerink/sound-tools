# sound-ui

The Sound Tools UI SDK: design tokens, shared components and the bridge from a live project to GPUI views. The design rules are in [DESIGN.md](../../DESIGN.md). The gallery in `crates/gallery` shows every component. How a tool saves and edits state is in the [core README](../core/README.md), "Read and edit from an interface". This file is the guide for writing a view of a tool.

Read the GPUI skills in `.agents/skills/` before you write GPUI code. The pinned GPUI differs from what blog posts show.

## The bridge

`Session` is one GPUI entity that owns the `Project` on the main thread. There is one per window, made by the runtime. It polls the engine and the file watcher every 16 ms (`POLL_INTERVAL`) from a timer, never from `render`. A poll that finds nothing new notifies nobody, so a stopped project draws no frames.

A view gets two things from it:

- `session.read(cx).project()`: the project, to read state while rendering.
- `session.read(cx).playhead()`: an `Entity<Playhead>` with `playing` and `tick`. It is an entity of its own, because it changes on every frame during playback. Observe it only in the small view that shows the position.

What the session tells its observers:

- It emits every `ProjectEvent` (`Created`, `Changed`, `Deleted`, `ProjectFileChanged`, `ProblemsChanged`). Interface edits, file edits by an agent, undo and redo all arrive this way. Subscribe and refresh only for the ids you show.
- It notifies once per group of events, after every `edit` and when the notice changes. Observe it when you refresh on anything, as a menu with undo labels does.

## Write a view

A view holds the session and a typed `Instance<S>`. It reads the current state in `render` and keeps no copy of saved state. Its own fields are interface state only: scroll, zoom, selection, an open gesture.

```rust
pub struct ToneView {
    session: Entity<Session>,
    tone: Instance<ToneState>,
    /// Open while a slider is dragged.
    drag: Option<ProjectEdit>,
}

impl ToneView {
    pub fn new(session: Entity<Session>, tone: Instance<ToneState>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&session, |view, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Changed(id) if id == view.tone.id()) {
                cx.notify();
            }
        })
        .detach();
        Self { session, tone, drag: None }
    }
}

impl Render for ToneView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the instance is deleted. Show nothing then.
        let Some(state) = self.session.read(cx).project().state(&self.tone) else {
            return div();
        };
        div().child(format!("{} Hz", state.frequency_hz))
    }
}
```

A view of an owner that shows its children refreshes for `id.is_inside(owner.id())` too. `extensions/arrangement/src/view.rs` does this.

Register the view next to the tool, in a function the runtime calls:

```rust
pub fn register(views: &mut Views) {
    views.register(ToneView::new);     // for the tool of `ToneState`
}
```

The runtime collects the views of every bundled extension and gives them to its window: `Shell::new(session, views, ..)` takes a `Views` and installs it (`views.install(cx)`), so the runtime cannot forget it. It is a GPUI global from then on.

A tool that edits what it owns shows that inside its own view. The window has one main area with one root view. Decided for the first milestone: the note editor is a panel inside the arrangement view and belongs to the arrangement extension, which opens it for the selected clip. The window does not know it.

`Views::view_of(&session, &id, window, cx)` makes the view of an instance from the installed registry. The window shows the view of `Views::main_instance(&session, cx)`: the first instance at the top of the project whose tool has a view. This is provisional. Composing a workspace from many views is later work.

### Host the view of another instance

Any view may call `Views::view_of`, because the registry is a global and not a field of the window. So a view can host the view of an instance whose tool it does not know, and an extension can show what another extension owns without depending on it. The track panel of the arrangement does this for the instrument of a track (`extensions/arrangement/src/view/track_panel.rs`):

```rust
// `None`: the instance is gone, its tool has no view, or no registry is installed.
let view: Option<AnyView> = Views::view_of(&session, &slot, window, cx);
// In `render`:
card.child(view.clone())
```

- Keep the `AnyView` in a field and make it when the instance or its tool changes, in a subscription with a window (`cx.subscribe_in`), never in `render`. Remember the tool name you made it for (`project.tool_of(&id)`): a file from outside can put another tool at the same id.
- Show something quiet when there is no view. A tool without a view is normal.
- The hosted view owns its edits and its gestures. The host gives it a surface, such as a card, and nothing else. When the host drops the view during a drag, the view must finish its gesture when it is released (`cx.on_release`), as `SynthView` does.
- A test with the window of the runtime has the registry through `Shell::new`. A test of a view alone installs one itself: `views.install(cx)`.

## Edit from a view

Every change goes through `Session::edit`. It runs one project operation, tells the observers what changed, and puts an error into the notice, which the window shows as a quiet line. It gives `None` on an error, so a view cannot drop one.

```rust
// One step.
session.update(cx, |session, cx| {
    session.edit(cx, |project| {
        let mut changes = Changes::new();
        changes.delete(clip.id());
        project.commit("Delete clip", changes)
    })
});

// A drag is a gesture of the session. Sound and every other view follow each move. The file
// is written once, at the end, and the whole drag is one undo step.
// Mouse down:
self.session.update(cx, |session, cx| session.begin_gesture("Change frequency", cx));
// Each mouse move:
let tone = self.tone.clone();
self.session.update(cx, |session, cx| {
    session.gesture(cx, |project, edit| project.update(edit, &tone, |state| state.frequency_hz = hz))
});
// Mouse up. On escape it is `cancel_gesture`, which applies the state from before.
self.session.update(cx, |session, cx| session.finish_gesture(cx));
```

The session keeps the open edit of a gesture, not the view. So a view cannot leave one open by losing it, a new gesture finishes one that was left open, and the session knows that a drag is going on: `Session::undo` and `Session::redo`, which the window's cmd-z, shift-cmd-z and menu call, do nothing until the gesture ends. An undo in the middle of a drag would be overwritten by the next mouse move. A view keeps only what the gesture needs of its own, such as the clip as it was at mouse down. Call `session.undo(cx)` for an undo button of your own, never `edit(cx, Project::undo)`.

What the clip and note drags of the arrangement added to this pattern, in `extensions/arrangement/src/view.rs`:

- Begin the gesture with the first mouse move that changes something, not at mouse down. A plain click is then no undo step, and an empty step never reaches the history.
- Work out each move from the value at mouse down and the distance the pointer went, not from the live value. A drag there and back then ends where it began.
- Skip the publish when the value did not change. Most mouse moves are inside one snap step.
- A mouse move listener of a drag is not hit tested: the drag goes on wherever the pointer is. A move without the button means that the mouse up went somewhere else: finish.
- The target may go away under the drag, by an outside delete. Read it on every move and finish the gesture when it is gone. Subscribe to `Deleted` too, so the drag ends when it happens and not at the next move.
- Something that should sound now and is not an edit, such as a preview note, goes through `Project::send` inside `session.edit`. See the core README, "Updates".

Transport goes through `session.engine()`: `play`, `pause`, `stop`, `seek`. `Session::toggle_playback` is what space does. The result shows in the `Playhead` after the next poll.

### A knob on saved state

`Knob` is controlled, so a view of saved state keeps no copy of it: give the value on every render and handle the `KnobChange`. `extensions/instrument/src/view.rs` is the example, and the mixer section of `extensions/arrangement/src/view/track_panel.rs` is a shorter one.

```rust
Knob::new("cutoff_hz")
    .range(KnobRange::logarithmic(20., 20_000.))   // or `KnobRange::linear`
    .value(state.cutoff_hz)
    .default_value(2_000.)                          // what a double click sets
    .label("Cutoff")
    .readout("2 kHz")                               // the caller formats: it knows the unit
    .on_change(callback)
```

- `KnobChange::Drag(value)`: begin the gesture when it is the first of this drag, then publish. The knob works the value out from the value at the press, and sends it only when it is not the value it sent last. It does not compare with the value of the last render, because several mouse moves arrive between two frames. Back at the height of the press the value is exactly that of the press, so a press with a sideways move never rounds a value that was written by hand.
- `KnobChange::DragEnd`: `finish_gesture`. `KnobChange::DragCancel` (escape): `cancel_gesture`. Both come only after a `Drag`, so a plain click is no undo step.
- `KnobChange::Set(value)`: a key step or a reset. One `commit`.
- The knob has its own tab stop and focus ring, and stops at the ends of its range. `KnobRange::value` gives three significant digits, and `knob::short` writes a number the same way for a readout: `2`, `15.5`, `632`.
- Every knob hears every mouse up and every press of the window, because a drag goes on outside it. It tells nobody unless a drag was open, so a click somewhere else renders nothing. A press while a drag is still open ends that drag: its mouse up was lost.

## Rules

- No `cx.notify()` and no entity updates inside `render` or inside a paint callback. Mouse listeners that a canvas registers while painting may update: they run later, on an event.
- No blocking I/O on the main thread beyond what `Project` does per edit.
- Draw only what is visible. A view of many records paints on a `canvas`, like the arrangement, and does not make an element per record.
- Keep what walks many records between the project events that can change it, and read it again in `render`, once per group of events, not per paint and not per event. The arrangement keeps its track order and its end this way. This is the one kind of copy a view holds.
- Keys: the window binds space, cmd-z and shift-cmd-z in the context `Shell && !TextInput`, so a focused `TextInput` gets them first, and tab and shift-tab in `Shell`. Bindings run before key listeners. For keys of your own view, the simplest is `track_focus` with a tab stop and `on_key_down` on the root of the view, as the arrangement does: they reach the view only while it has the focus, and it calls `cx.stop_propagation()` for a key it used. Focus the view on mouse down.
- Show a focus ring only when the focus came from the keyboard. `.focus_visible(..)` also shows it when a key follows a click, and space follows a click all the time. `sound_ui::KeyboardFocus` works it out while rendering or painting: keep one next to the focus handle, ask `shows_ring(&handle, window)` and call `pressed(cx)` on a mouse press. The arrangement, the knob and the segmented control use it.
- A callback that a control keeps, such as `Knob::on_change`, should hold the view weakly. `cx.listener` does. `cx.processor` holds it strongly, and then the mouse listeners of the last frame keep a view that was just closed alive for one more frame, with its open drag. `SynthView::callback` in `extensions/instrument/src/view.rs` is the weak form for a callback that takes its argument by value.
- Keep what repaints with the playhead apart from the rest. A view that GPUI is to keep while the playhead moves must not have the playhead view inside it: a notified view also renders every view above it. Make them siblings and put `.cached(..)` on the heavy one. See `ArrangementView`.
- Put coordinate math in pure functions with tests (`extensions/arrangement/src/view/layout.rs`).
- Use the components of this crate and the theme tokens (`cx.theme()`). A new general component goes here with a gallery entry. What only one tool needs stays in its extension.

## Test a view

`crates/ui/tests/bridge.rs` and `crates/runtime/tests/window/` show the pattern: a project on a temporary folder with an offline engine, a `Session`, `#[gpui::test]`, and `cx.executor().advance_clock(POLL_INTERVAL)` to let the poll timer fire. Call `engine.process_block(..)` yourself, so that transport commands apply. Wait with `cx.background_executor().timer(..)`, never with `smol::Timer`. `tests/window/support.rs` has the hands of a composer: press, drag and release at the place of a tick, a track or a pitch, worked out with the layout functions of the view. A control made of elements has no layout function. The knob and the segments of a segmented control name themselves for tests with GPUI's `debug_selector` (`knob-<id>`, `segment-<value>`), which does nothing in a normal build, and `Opened::control("knob-cutoff_hz")` gives the middle of one. It asks for a whole frame first, because a cached view that was not painted again has no bounds in the last frame. Two keys in one `simulate_keystrokes` call have no frame between them. Send them one by one when the second needs what the first painted, such as the tab order.

`cargo test -p runtime --test snapshots` renders the whole window to PNGs with no visible window, and `cargo test -p gallery --test snapshots` renders the components.
