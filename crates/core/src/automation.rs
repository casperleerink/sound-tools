//! Automation: an owner moves the numbers of a device over time, block by block.
//!
//! The owner, such as the arrangement, plays its lanes on the audio thread and sends each
//! device the values of this block as [`Automation`] events, to the input the device names
//! with [`BehaviourContext::automation`](crate::BehaviourContext::automation). The device keeps
//! an [`Automated`] next to its record and plays what it gives.

use crate::parameter::Parameter;
use crate::processor::Timed;

/// The value of one number of a device for this block, in the units of its record (Hz, dB, 0
/// to 1). `parameter` is the place of the number in the list the device named its automation
/// input with, its `PARAMETERS`.
///
/// A lane sends its value every block, at offset 0, also while the project does not play. So
/// a number that hears nothing in a block is no longer automated, and goes back to its record.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Automation {
    pub parameter: u16,
    pub value: f32,
}

/// How fast a device moves to what [`Automated::follow`] gives.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutomationRamp {
    /// A lane moved: over this block, so the value is on time and a sweep has no steps.
    Block,
    /// A lane took a number over or let it go back to its record: as an edit glides, because
    /// the two values may be far apart.
    Edit,
}

/// What a device plays while lanes move some of its numbers: its record, with the value of
/// each lane over the record's.
///
/// The device keeps one in place of its record: [`set_record`](Self::set_record) in `update`,
/// [`follow`](Self::follow) with the events of each block in `process`, and it aims at
/// [`state`](Self::state) whenever either changed something. Both are realtime safe.
pub struct Automated<S: 'static, const N: usize> {
    parameters: [&'static Parameter<S>; N],
    record: S,
    /// The value of each number that a lane holds.
    lanes: [Option<f32>; N],
}

impl<S: Copy, const N: usize> Automated<S, N> {
    /// No number automated yet.
    pub const fn new(parameters: [&'static Parameter<S>; N], record: S) -> Self {
        Self {
            parameters,
            record,
            lanes: [None; N],
        }
    }

    /// A new record from an update. The numbers that a lane holds keep the lane's value.
    pub fn set_record(&mut self, record: S) {
        self.record = record;
    }

    /// What plays: the record, with the value of each lane.
    pub fn state(&self) -> S {
        let mut state = self.record;
        for (parameter, lane) in self.parameters.iter().zip(self.lanes) {
            if let Some(value) = lane {
                (parameter.set)(&mut state, value);
            }
        }
        state
    }

    /// Takes the lane values of one block. `None` when nothing that plays changed, else how
    /// fast to move to the new [`state`](Self::state). A value outside the range is held to
    /// it, and an index that names no number is left out.
    pub fn follow(&mut self, events: &[Timed<Automation>]) -> Option<AutomationRamp> {
        let mut heard = [false; N];
        let mut ramp = None;
        for Timed { event, .. } in events {
            let index = usize::from(event.parameter);
            let (Some(parameter), Some(lane), Some(heard)) = (
                self.parameters.get(index),
                self.lanes.get_mut(index),
                heard.get_mut(index),
            ) else {
                continue;
            };
            // Not `clamp`: it panics on a NaN, and nothing may panic on the audio thread.
            let value = event.value.max(parameter.min).min(parameter.max);
            ramp = match *lane {
                None => ramp.max(Some(AutomationRamp::Edit)),
                Some(before) if before != value => ramp.max(Some(AutomationRamp::Block)),
                Some(_) => ramp,
            };
            *lane = Some(value);
            *heard = true;
        }
        for (lane, heard) in self.lanes.iter_mut().zip(heard) {
            if !heard && lane.take().is_some() {
                ramp = Some(AutomationRamp::Edit);
            }
        }
        ramp
    }
}

#[cfg(test)]
mod tests {
    use super::{Automated, Automation, AutomationRamp};
    use crate::parameter::{Parameter, Scale};
    use crate::processor::Timed;

    #[derive(Copy, Clone, Debug, PartialEq)]
    struct State {
        cutoff: f32,
        mix: f32,
    }

    const CUTOFF: Parameter<State> = Parameter {
        field: "cutoff",
        min: 20.,
        max: 20_000.,
        default: 1_000.,
        scale: Scale::Logarithmic,
        get: |state| state.cutoff,
        set: |state, value| state.cutoff = value,
    };
    const MIX: Parameter<State> = Parameter {
        field: "mix",
        min: 0.,
        max: 1.,
        default: 1.,
        scale: Scale::Linear,
        get: |state| state.mix,
        set: |state, value| state.mix = value,
    };
    const RECORD: State = State {
        cutoff: 1_000.,
        mix: 1.,
    };

    fn block(values: &[(u16, f32)]) -> Vec<Timed<Automation>> {
        let events = values
            .iter()
            .map(|&(parameter, value)| Automation { parameter, value });
        events.map(|event| Timed { offset: 0, event }).collect()
    }

    #[test]
    fn a_lane_takes_over_moves_and_lets_go_back_to_the_record() {
        let mut automated = Automated::new([&CUTOFF, &MIX], RECORD);
        assert_eq!(automated.follow(&[]), None);
        assert_eq!(
            automated.follow(&block(&[(0, 500.)])),
            Some(AutomationRamp::Edit)
        );
        assert_eq!(automated.state().cutoff, 500.);
        // The same value again moves nothing.
        assert_eq!(automated.follow(&block(&[(0, 500.)])), None);
        assert_eq!(
            automated.follow(&block(&[(0, 600.)])),
            Some(AutomationRamp::Block)
        );
        // An edit of the record keeps the lane's value, and the rest of the record plays.
        automated.set_record(State {
            cutoff: 2_000.,
            mix: 0.5,
        });
        let expected = State {
            cutoff: 600.,
            mix: 0.5,
        };
        assert_eq!(automated.state(), expected);
        // A block with no value for it lets the number go.
        assert_eq!(automated.follow(&[]), Some(AutomationRamp::Edit));
        assert_eq!(automated.state().cutoff, 2_000.);
    }

    #[test]
    fn a_value_is_held_to_the_range_and_an_unknown_index_is_left_out() {
        let mut automated = Automated::new([&CUTOFF, &MIX], RECORD);
        automated.follow(&block(&[(0, 1e9), (1, f32::NAN), (7, 0.5)]));
        let expected = State {
            cutoff: 20_000.,
            mix: 0.,
        };
        assert_eq!(automated.state(), expected);
    }
}
