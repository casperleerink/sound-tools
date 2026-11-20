//! What views read of the engine as the piece plays: the numbers lanes and cables play.

use std::sync::Arc;

use sound_core::{InstanceId, Project, Ticks};

/// The name an owner keeps the watch of what a cable plays into the number of a child under,
/// `child.number`.
pub(crate) fn played_name(child_number: &str) -> String {
    format!("played:{child_number}")
}

/// Where the piece plays, and whether a hand on a number a lane moves writes into the lane now.
#[derive(Clone, Copy)]
pub(crate) struct Moment {
    tick: Ticks,
    writes: bool,
}

impl Moment {
    /// At `tick`, where a hand writes into no lane.
    pub(crate) fn at(tick: Ticks) -> Self {
        Self {
            tick,
            writes: false,
        }
    }
}

/// What a lane or a cable plays into the number `number` of `instance` now, and whether it
/// holds its control: the lane decides the value, not the composer, unless a hand writes into
/// the lane. `None` while nothing moves the number.
pub(crate) fn lane_value(
    project: &Project,
    instance: &InstanceId,
    number: &str,
    at: Moment,
) -> Option<(f32, bool)> {
    let lanes = project.lanes(instance)?;
    let mut values: Vec<(Arc<str>, f32)> = Vec::new();
    lanes.values_at(at.tick, &mut values);
    let (_, value) = values.into_iter().find(|(each, _)| **each == *number)?;
    Some((value, !(at.writes && lanes.source(number).is_some())))
}
