//! What the automation lanes of a track play into the numbers of one device, for its view.
//!
//! The knob of an automated number shows the value that plays, with a mark, and does not drag
//! (DESIGN.md). The view keeps a [`Lanes`] of its instance. It works the values out at the
//! playhead from the lanes the owner of the instance shows ([`Project::lanes`]), with the math
//! of the player, and tells the view only when one of them changed: while the project plays,
//! when the playhead moves while it stands, and when the lanes change. So a device with no
//! lanes costs one look per frame while the project plays, and nothing while it stands.
//!
//! [`Project::lanes`]: sound_core::Project::lanes

use gpui::{App, Context, Entity, prelude::*};
use sound_core::{InstanceId, Parameter};

use crate::session::Session;

pub struct Lanes {
    session: Entity<Session>,
    instance: InstanceId,
    /// The value of each lane at the playhead, by the field of its number.
    values: Vec<(&'static str, f32)>,
}

impl Lanes {
    /// The lanes of `instance`, for the view of `cx`, which draws again when a value changes.
    pub fn follow<V: 'static>(
        session: &Entity<Session>,
        instance: &InstanceId,
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
                values: Vec::new(),
            };
            lanes.values = lanes.played(cx);
            lanes
        });
        cx.observe(&lanes, |_, _, cx| cx.notify()).detach();
        lanes
    }

    /// The value the lane of the number `field` plays now, `None` when no lane moves it.
    pub fn value(&self, field: &str) -> Option<f32> {
        let mut values = self.values.iter();
        let found = values.find(|(automated, _)| *automated == field);
        found.map(|(_, value)| *value)
    }

    pub fn is_automated(&self, field: &str) -> bool {
        self.value(field).is_some()
    }

    /// Lays the value of each lane over a copy of the record, so what the view draws from it,
    /// a knob or a display, shows what plays. `parameters` are the numbers the device takes
    /// automation for, as its `AutomationInput` names them.
    pub fn apply<S>(&self, parameters: &[&Parameter<S>], state: &mut S) {
        for parameter in parameters {
            if let Some(value) = self.value(parameter.field) {
                (parameter.set)(state, value);
            }
        }
    }

    fn played(&self, cx: &App) -> Vec<(&'static str, f32)> {
        let session = self.session.read(cx);
        let tick = session.playhead().read(cx).tick;
        let lanes = session.project().lanes(&self.instance);
        lanes.map_or_else(Vec::new, |lanes| lanes.values_at(tick))
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let values = self.played(cx);
        if values != self.values {
            self.values = values;
            cx.notify();
        }
    }
}
