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

A tool that edits what it owns shows that inside its own view. The window has one main area with one root view. Decided for the first milestone: the note editor is a panel inside the arrangement view and belongs to the arrangement extension, which opens it for the selected clip. The window does not know it.

`Views::view_of(&session, &id, window, cx)` makes the view of an instance. The window shows the view of `Views::main_instance`: the first instance at the top of the project whose tool has a view. This is provisional. Composing a workspace from many views is later work.

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

Transport goes through `session.engine()`: `play`, `pause`, `stop`, `seek`. `Session::toggle_playback` is what space does. The result shows in the `Playhead` after the next poll.

## Rules

- No `cx.notify()` and no entity updates inside `render` or inside a paint callback. Mouse listeners that a canvas registers while painting may update: they run later, on an event.
- No blocking I/O on the main thread beyond what `Project` does per edit.
- Draw only what is visible. A view of many records paints on a `canvas`, like the arrangement, and does not make an element per record.
- Keep what walks many records between the project events that can change it, and read it again in `render`, once per group of events, not per paint and not per event. The arrangement keeps its track order and its end this way. This is the one kind of copy a view holds.
- Keys: the window binds space, cmd-z and shift-cmd-z in the context `Shell && !TextInput`, so a focused `TextInput` gets them first. Give a view of your own a `key_context` and bind its keys there.
- Keep what repaints with the playhead apart from the rest. A view that GPUI is to keep while the playhead moves must not have the playhead view inside it: a notified view also renders every view above it. Make them siblings and put `.cached(..)` on the heavy one. See `ArrangementView`.
- Put coordinate math in pure functions with tests (`extensions/arrangement/src/view/layout.rs`).
- Use the components of this crate and the theme tokens (`cx.theme()`). A new general component goes here with a gallery entry. What only one tool needs stays in its extension.

## Test a view

`crates/ui/tests/bridge.rs` and `crates/runtime/tests/window.rs` show the pattern: a project on a temporary folder with an offline engine, a `Session`, `#[gpui::test]`, and `cx.executor().advance_clock(POLL_INTERVAL)` to let the poll timer fire. Call `engine.process_block(..)` yourself, so that transport commands apply. Wait with `cx.background_executor().timer(..)`, never with `smol::Timer`.

`cargo test -p runtime --test snapshots` renders the whole window to PNGs with no visible window, and `cargo test -p gallery --test snapshots` renders the components.
