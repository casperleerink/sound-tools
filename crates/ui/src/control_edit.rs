//! The session side of a control on saved state: what every view that puts a knob, a volume or
//! a handle on a record does with its [`ValueChange`]s.

use gpui::{App, Context, Entity, Window};
use sound_core::{Instance, InstanceId, Project, ProjectEdit, ProjectError, State};

use crate::components::gesture::ValueChange;
use crate::session::Session;

/// The gesture of the session for one drag, from mouse down to mouse up. It opens with the first
/// [`Self::publish`] and not at the press, so a press that changes nothing is no undo step and
/// writes nothing. It is the only place a view keeps whether its drag opened the gesture, so no
/// view opens its gesture twice or forgets to close it.
///
/// Finish it at mouse up, when what it drags goes away, and when the view is released. The
/// session finishes a gesture that is left open when the next one begins, but until then undo
/// and redo wait for it.
#[derive(Debug, Default)]
pub struct DragEdit {
    open: bool,
}

impl DragEdit {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// One move of the drag, as [`Session::gesture`]. The first one opens the gesture, as the
    /// undo step `label`.
    pub fn publish<R>(
        &mut self,
        session: &Entity<Session>,
        label: &str,
        cx: &mut App,
        publish: impl FnOnce(&mut Project, &mut ProjectEdit) -> Result<R, ProjectError>,
    ) -> Option<R> {
        let open = std::mem::replace(&mut self.open, true);
        session.update(cx, |session, cx| {
            if !open {
                session.begin_gesture(label, cx);
            }
            session.gesture(cx, publish)
        })
    }

    /// Ends the drag as one undo step. Whether it had opened the gesture.
    pub fn finish(&mut self, session: &Entity<Session>, cx: &mut App) -> bool {
        let open = std::mem::take(&mut self.open);
        if open {
            session.update(cx, |session, cx| session.finish_gesture(cx));
        }
        open
    }

    /// Ends the drag and goes back to the state before it. Whether it had opened the gesture.
    pub fn cancel(&mut self, session: &Entity<Session>, cx: &mut App) -> bool {
        let open = std::mem::take(&mut self.open);
        if open {
            session.update(cx, |session, cx| session.cancel_gesture(cx));
        }
        open
    }
}

/// What a view keeps for the controls it puts on saved state: the drag of one of them. One per
/// view, because a view has one drag at a time.
///
/// Finish it when the view is released and when the record goes under a drag (see
/// [`Self::finish`]).
#[derive(Debug, Default)]
pub struct ControlEdit {
    drag: DragEdit,
}

impl ControlEdit {
    /// One change of a control on a field of a record, named `label` in the undo history.
    ///
    /// A drag is one gesture and one undo step: it begins with its first `Drag`, so a click is
    /// no step, publishes every move so the sound follows, and ends at `DragEnd`, or goes back
    /// to the state before it at `DragCancel`. A `Set`, a key step or a reset, is one commit.
    pub fn apply<S: State, V>(
        &mut self,
        session: &Entity<Session>,
        instance: &Instance<S>,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut S, V),
        cx: &mut App,
    ) {
        let update = |project: &mut Project, edit: &mut ProjectEdit, value| {
            project.update(edit, instance, |state| set(state, value))
        };
        self.apply_with(session, instance.id(), label, change, update, cx);
    }

    /// [`Self::apply`] for a view that knows the record only as JSON, such as one written in
    /// TypeScript. `set` gets the `state` of the record.
    pub fn apply_json<V>(
        &mut self,
        session: &Entity<Session>,
        id: &InstanceId,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut serde_json::Value, V),
        cx: &mut App,
    ) {
        let update = |project: &mut Project, edit: &mut ProjectEdit, value| {
            project.update_json(edit, id, |state| set(state, value))
        };
        self.apply_with(session, id, label, change, update, cx);
    }

    fn apply_with<V>(
        &mut self,
        session: &Entity<Session>,
        id: &InstanceId,
        label: &str,
        change: ValueChange<V>,
        update: impl FnOnce(&mut Project, &mut ProjectEdit, V) -> Result<(), ProjectError>,
        cx: &mut App,
    ) {
        match change {
            ValueChange::Drag(value) => {
                // A move may still arrive in the frame that lost the record.
                if session.read(cx).project().tool_of(id).is_none() {
                    return;
                }
                // The gesture is gone: a control that went away during its drag sent no end,
                // and another view or the session may have closed it since. It opens again.
                if !session.read(cx).gesture_open() {
                    self.drag = DragEdit::default();
                }
                self.drag.publish(session, label, cx, |project, edit| {
                    update(project, edit, value)
                });
            }
            ValueChange::DragEnd => self.finish(session, cx),
            ValueChange::DragCancel => {
                self.drag.cancel(session, cx);
            }
            ValueChange::Set(value) => session.update(cx, |session, cx| {
                if session.project().tool_of(id).is_none() {
                    return;
                }
                session.edit(cx, |project| {
                    let mut edit = project.begin(label);
                    update(project, &mut edit, value)?;
                    project.finish(edit)
                });
            }),
        }
    }

    /// Ends a drag that is open as one undo step. For a view that goes away, or whose record was
    /// deleted from outside during the drag: the delete was the last write, so the gesture
    /// finishes and does not cancel, which would bring the record back.
    pub fn finish(&mut self, session: &Entity<Session>, cx: &mut App) {
        self.drag.finish(session, cx);
    }
}

/// A callback for a control that a view puts on screen, such as `Knob::on_change`. It holds the
/// view weakly, as `cx.listener` does: with `cx.processor` the listeners of the last frame would
/// keep a view that was just closed alive for one more frame, and an open drag with it. It takes
/// its argument by value, which `cx.listener` does not.
pub fn weak_callback<V: 'static, E>(
    cx: &Context<V>,
    f: impl Fn(&mut V, E, &mut Context<V>) + 'static,
) -> impl Fn(E, &mut Window, &mut App) + 'static {
    let view = cx.weak_entity();
    move |event, _, cx| {
        // Released: there is nothing left to tell.
        view.update(cx, |view, cx| f(view, event, cx)).ok();
    }
}

/// The same for a control that reports a click with nothing to say, such as the clip light of
/// a meter.
pub fn weak_action<V: 'static>(
    cx: &Context<V>,
    f: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = cx.weak_entity();
    move |_, cx| {
        view.update(cx, |view, cx| f(view, cx)).ok();
    }
}
