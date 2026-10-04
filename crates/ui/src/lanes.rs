//! What the automation lanes of a track play into the numbers of one device, for its view.
//!
//! The knob of an automated number shows the value that plays, with a mark, and does not drag
//! (DESIGN.md). The view keeps a [`Lanes`] of its instance, with the numbers the device takes
//! automation for. It works the values out at the playhead from the lanes the owner of the
//! instance shows ([`Project::lanes`]), with the math of the player, and tells the view only
//! when one of them changed: while the project plays, when the playhead moves while it stands,
//! and when the lanes change. So a device with no lanes costs one look per frame while the
//! project plays, and nothing while it stands, and a look allocates nothing.
//!
//! [`Project::lanes`]: sound_core::Project::lanes

use std::sync::Arc;

use gpui::{App, Context, Entity, prelude::*};
use sound_core::{AutomationInput, InstanceId, Parameter, State};

use crate::session::Session;

pub struct Lanes<S: 'static> {
    session: Entity<Session>,
    instance: InstanceId,
    /// The numbers the device takes automation for, in the order of its `AutomationInput`.
    /// None for a device that names its numbers as its behaviour runs.
    parameters: Vec<&'static Parameter<S>>,
    /// The value of each lane at the playhead, by the name of its number.
    values: Vec<(Arc<str>, f32)>,
    /// Where the next look puts the values, so that it allocates nothing.
    next: Vec<(Arc<str>, f32)>,
}

impl<S: 'static> Lanes<S> {
    /// The lanes of `instance`, whose device takes automation for the numbers of `input`, for
    /// the view of `cx`, which draws again when a value changes.
    pub fn follow<V: 'static, const N: usize>(
        session: &Entity<Session>,
        instance: &InstanceId,
        input: AutomationInput<S, N>,
        cx: &mut Context<V>,
    ) -> Entity<Self> {
        Self::follow_parameters(session, instance, input.parameters().to_vec(), cx)
    }

    /// The lanes of `instance`, whose device names the numbers it takes automation for as its
    /// behaviour runs, such as the pins of a plugin. [`Self::value`] gives what each plays;
    /// [`Lanes::state`] has no number to lay over the record.
    pub fn follow_named<V: 'static>(
        session: &Entity<Session>,
        instance: &InstanceId,
        cx: &mut Context<V>,
    ) -> Entity<Self> {
        Self::follow_parameters(session, instance, Vec::new(), cx)
    }

    fn follow_parameters<V: 'static>(
        session: &Entity<Session>,
        instance: &InstanceId,
        parameters: Vec<&'static Parameter<S>>,
        cx: &mut Context<V>,
    ) -> Entity<Self> {
        let lanes = cx.new(|cx: &mut Context<Self>| {
            let playhead = session.read(cx).playhead().clone();
            cx.observe(&playhead, |lanes, _, cx| lanes.refresh(cx))
                .detach();
            // The session notifies once per group of edits, which may change the lanes.
            cx.observe(session, |lanes, _, cx| lanes.refresh(cx))
                .detach();
            let mut lanes = Self {
                session: session.clone(),
                instance: instance.clone(),
                parameters,
                values: Vec::new(),
                next: Vec::new(),
            };
            lanes.refresh(cx);
            lanes
        });
        cx.observe(&lanes, |_, _, cx| cx.notify()).detach();
        lanes
    }

    /// Follows the lanes of another instance, for a view that shows another one now, such as
    /// the panel of another track.
    pub fn set_instance(&mut self, instance: &InstanceId, cx: &mut Context<Self>) {
        if self.instance != *instance {
            self.instance = instance.clone();
            self.refresh(cx);
        }
    }

    /// The value the lane of the number `field` plays now, `None` when no lane moves it.
    pub fn value(&self, field: &str) -> Option<f32> {
        let mut values = self.values.iter();
        let found = values.find(|(automated, _)| **automated == *field);
        found.map(|(_, value)| *value)
    }

    pub fn is_automated(&self, field: &str) -> bool {
        self.value(field).is_some()
    }

    /// Whether a lane moves the number `field` of an object of a nested record, which a lane
    /// names by the path of the object, a dot and the field: `bands[0].gain_db`, `pads.36.pan`,
    /// `osc_1.position`. An empty `object` is the record itself. See [`object_of`].
    pub fn is_automated_in(&self, object: &str, field: &str) -> bool {
        let mut names = self.values.iter();
        names.any(|(lane, _)| is_number_of(lane, object, field))
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.next.clear();
        let session = self.session.read(cx);
        let tick = session.playhead().read(cx).tick;
        if let Some(lanes) = session.project().lanes(&self.instance) {
            lanes.values_at(tick, &mut self.next);
        }
        if self.next != self.values {
            std::mem::swap(&mut self.next, &mut self.values);
            cx.notify();
        }
    }
}

impl<S: State> Lanes<S> {
    /// The record of the instance as the view shows it: what plays, with the value of each
    /// lane over it, so a knob or a display drawn from it shows the lane. `None` once the record
    /// is gone, or when it is not the record the numbers read.
    pub fn state(&self, cx: &App) -> Option<S> {
        let project = self.session.read(cx).project();
        let mut state = project
            .state(&project.resolve::<S>(&self.instance)?)?
            .clone();
        for parameter in &self.parameters {
            if let Some(value) = self.value(parameter.field) {
                (parameter.set)(&mut state, value);
            }
        }
        Some(state)
    }
}

/// Whether `lane` names the number `field` of `object`, as [`Lanes::is_automated_in`] reads
/// it.
pub fn is_number_of(lane: &str, object: &str, field: &str) -> bool {
    match object {
        "" => lane == field,
        object => {
            lane.strip_prefix(object)
                .and_then(|rest| rest.strip_prefix('.'))
                == Some(field)
        }
    }
}

/// The path of the object a lane of a nested record moves a number of: `bands[0]` for the lane
/// `bands[0].gain_db`, empty for a number of the record itself. For
/// [`Lanes::is_automated_in`], from the lanes a device names.
pub fn object_of(lane: &'static str) -> &'static str {
    lane.rsplit_once('.').map_or("", |(object, _)| object)
}
