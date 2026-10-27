use gpui::{App, Context, Entity};
use sound_core::{AutomationInput, Instance, ProjectEvent, State};

use crate::components::gesture::ValueChange;
use crate::components::knob::{Knob, ParameterKnob};
use crate::control_edit::{ControlEdit, weak_callback};
use crate::lanes::Lanes;
use crate::session::Session;

/// What the view of a device keeps to show and edit its record: the session, the record, the
/// lanes that move its numbers and the drag of a control. It draws the view again when the
/// record changes, and finishes an open drag when the record is deleted or the view goes away.
pub struct DeviceEdit<S: 'static> {
    pub session: Entity<Session>,
    pub instance: Instance<S>,
    pub lanes: Entity<Lanes<S>>,
    edit: ControlEdit,
}

/// A view that shows one record through a [`DeviceEdit`].
pub trait DeviceView: 'static {
    type State: State;

    fn device(&mut self) -> &mut DeviceEdit<Self::State>;
}

impl<S: State> DeviceEdit<S> {
    /// For a device that takes automation for the numbers of `automation`.
    pub fn new<V: DeviceView<State = S>, const N: usize>(
        session: Entity<Session>,
        instance: Instance<S>,
        automation: AutomationInput<S, N>,
        cx: &mut Context<V>,
    ) -> Self {
        let lanes = Lanes::follow(&session, instance.id(), automation, cx);
        Self::with_lanes(session, instance, lanes, cx)
    }

    /// For a device that names the numbers it takes automation for as it runs, such as a plugin.
    pub fn named<V: DeviceView<State = S>>(
        session: Entity<Session>,
        instance: Instance<S>,
        cx: &mut Context<V>,
    ) -> Self {
        let lanes = Lanes::follow_named(&session, instance.id(), cx);
        Self::with_lanes(session, instance, lanes, cx)
    }

    fn with_lanes<V: DeviceView<State = S>>(
        session: Entity<Session>,
        instance: Instance<S>,
        lanes: Entity<Lanes<S>>,
        cx: &mut Context<V>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| {
            let device = view.device();
            match event {
                ProjectEvent::Changed(id) if id == device.instance.id() => cx.notify(),
                ProjectEvent::Deleted(id) if id == device.instance.id() => {
                    device.finish(cx);
                    cx.notify();
                }
                _ => {}
            }
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.device().finish(cx)).detach();
        Self {
            session,
            instance,
            lanes,
            edit: ControlEdit::default(),
        }
    }

    /// The record as the view shows it: what plays, with the value of each lane over it. `None`
    /// once the record is deleted.
    pub fn state(&self, cx: &App) -> Option<S> {
        self.lanes.read(cx).state(cx)
    }

    pub fn is_automated(&self, field: &str, cx: &App) -> bool {
        self.lanes.read(cx).is_automated(field)
    }

    /// See [`ControlEdit::apply`].
    pub fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut S, V),
        cx: &mut App,
    ) {
        self.edit
            .apply(&self.session, &self.instance, label, change, set, cx);
    }

    /// See [`ControlEdit::finish`].
    pub fn finish(&mut self, cx: &mut App) {
        self.edit.finish(&self.session, cx);
    }

    /// The knob of `control` at its value in `state`, which a lane may move, and whose changes
    /// edit the record.
    pub fn knob<V: DeviceView<State = S>>(
        &self,
        control: ParameterKnob<S>,
        state: &S,
        cx: &mut Context<V>,
    ) -> Knob {
        let automated = self.is_automated(control.parameter.field, cx);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view: &mut V, change, cx| {
                let (label, set) = (control.undo_label, control.parameter.set);
                view.device().change(label, change, set, cx);
            }))
    }
}
