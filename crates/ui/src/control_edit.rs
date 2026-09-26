//! The session side of a control on saved state: what every view that puts a knob, a volume or
//! a handle on a record does with its [`ValueChange`]s.

use gpui::{App, Context, Entity, Window};
use sound_core::{Changes, Instance, State};

use crate::components::gesture::ValueChange;
use crate::session::Session;

/// What a view keeps for the controls it puts on saved state: whether a drag of one of them has
/// the gesture of the session open. One per view, because a view has one drag at a time.
///
/// Finish it when the view is released and when the record goes under a drag (see
/// [`Self::finish`]). The session finishes a gesture that is left open when the next one
/// begins, but until then undo and redo wait for it.
#[derive(Debug, Default)]
pub struct ControlEdit {
    dragging: bool,
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
        match change {
            ValueChange::Drag(value) => {
                // A move may still arrive in the frame that lost the record.
                if session.read(cx).project().state(instance).is_none() {
                    return;
                }
                let begun = std::mem::replace(&mut self.dragging, true);
                session.update(cx, |session, cx| {
                    // Also when the gesture is gone: a control that went away during its drag
                    // sent no end, and another view or the session may have closed it since.
                    if !begun || !session.gesture_open() {
                        session.begin_gesture(label, cx);
                    }
                    session.gesture(cx, |project, edit| {
                        project.update(edit, instance, |state| set(state, value))
                    });
                });
            }
            ValueChange::DragEnd => self.finish(session, cx),
            ValueChange::DragCancel => {
                if std::mem::take(&mut self.dragging) {
                    session.update(cx, |session, cx| session.cancel_gesture(cx));
                }
            }
            ValueChange::Set(value) => session.update(cx, |session, cx| {
                let Some(mut state) = session.project().state(instance).cloned() else {
                    return;
                };
                set(&mut state, value);
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    changes.set(instance, state);
                    project.commit(label, changes)
                });
            }),
        }
    }

    /// Ends a drag that is open as one undo step. For a view that goes away, or whose record was
    /// deleted from outside during the drag: the delete was the last write, so the gesture
    /// finishes and does not cancel, which would bring the record back.
    pub fn finish(&mut self, session: &Entity<Session>, cx: &mut App) {
        if std::mem::take(&mut self.dragging) {
            session.update(cx, |session, cx| session.finish_gesture(cx));
        }
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
