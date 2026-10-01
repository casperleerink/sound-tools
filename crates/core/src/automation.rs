//! Automation: an owner moves the numbers of a device over time, block by block.
//!
//! The owner, such as the arrangement, plays its lanes on the audio thread and sends each
//! device the values of this block as [`Automation`] events, to the [`AutomationInput`] the
//! device names with [`BehaviourContext::automation`](crate::BehaviourContext::automation).
//! The device keeps an [`Automated`] in place of its record, and aims at the [`Targets`] it
//! gives. Every decision about how fast a number moves is made here, once.

use crate::parameter::Parameter;
use crate::processor::{EventInput, ProcessContext, Timed};

/// The value of one number of a device for this block, in the units of its record (Hz, dB, 0
/// to 1). `parameter` is the place of the number in the list of the device's
/// [`AutomationInput`].
///
/// A lane sends its value every block, at offset 0, also while the project does not play. So
/// a number that hears nothing in a block is no longer automated, and goes back to its record.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Automation {
    pub parameter: u16,
    pub value: f32,
}

/// The most numbers one device takes automation for. A lane player sends one event per number
/// per block, so this keeps a block far under the capacity of an event port.
pub const MAX_AUTOMATED: usize = 64;

/// The automation input of a device: its event port, and the numbers it takes, in the order of
/// the index of an [`Automation`] event. One constant names both, and the behaviour and the
/// processor both read it, so the index a lane is sent with is the one the device reads.
///
/// List the numbers that move smoothly. Leave out a whole number, such as a count of voices or
/// an octave: a lane is a straight line, and a lane of a number left out is reported.
pub struct AutomationInput<S: 'static, const N: usize> {
    port: EventInput<Automation>,
    parameters: [&'static Parameter<S>; N],
}

impl<S, const N: usize> Clone for AutomationInput<S, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, const N: usize> Copy for AutomationInput<S, N> {}

impl<S, const N: usize> AutomationInput<S, N> {
    /// The event input at `index` among the event inputs of the processor.
    pub const fn new(index: usize, parameters: [&'static Parameter<S>; N]) -> Self {
        assert!(
            N <= MAX_AUTOMATED,
            "a device takes at most 64 automated numbers"
        );
        Self {
            port: EventInput::new(index),
            parameters,
        }
    }

    /// The port, for [`Ports::event_input`](crate::Ports::event_input).
    pub const fn port(&self) -> EventInput<Automation> {
        self.port
    }

    pub const fn parameters(&self) -> &[&'static Parameter<S>; N] {
        &self.parameters
    }
}

/// How many frames each number takes to get to its new target, which the [`Automated`] of the
/// device holds. A ramp of 0 takes the value at once:
/// [`Smoothed::set_target`](crate::Smoothed::set_target) does that.
pub struct Targets<S: 'static, const N: usize> {
    parameters: [&'static Parameter<S>; N],
    ramps: [f32; N],
    edit: f32,
    snaps: bool,
}

impl<S, const N: usize> Targets<S, N> {
    /// The frames `parameter` takes. A number that did not move, or that the device does not
    /// take automation for, takes the glide of an edit: its target is the one it had, so it
    /// goes on gliding as it did. Smooth each number on its own; a target worked out from
    /// several numbers would take one ramp for all of them.
    pub fn ramp(&self, parameter: &Parameter<S>) -> f32 {
        let mut numbers = self.parameters.iter().zip(self.ramps);
        let ramp = numbers.find(|(number, _)| number.field == parameter.field);
        ramp.map_or(self.edit, |(_, ramp)| ramp)
    }

    /// The glide of an edit, for what is not one of the numbers, such as a choice.
    pub fn edit(&self) -> f32 {
        self.edit
    }

    /// Whether some number takes its value at once: the first block of a device. A device that
    /// keeps something worked out from its numbers, and works it out again only while a value
    /// moves, works it out now.
    pub fn snaps(&self) -> bool {
        self.snaps
    }
}

/// What a device plays while lanes move some of its numbers: its record, with the value of
/// each lane over the record's. It reads as that record, and changes it in place: so a record
/// that is not `Copy`, such as one that names a file, is never copied or dropped on the audio
/// thread.
///
/// - A lane moves its number over one block, so the value is on time and a sweep has no steps.
/// - A lane that takes a number over or lets it go glides as an edit does, because the two
///   values may be far apart, and its later moves end with that glide, not before.
/// - So does every lane after a seek or a stop, which moves the lanes anywhere.
/// - In the first block of the device, the lanes take their values at once: a render or a new
///   device starts where its lanes are, with no glide from the record.
///
/// Each number has its own ramp, so a lane that takes one number over does not slow another
/// down. Realtime safe.
pub struct Automated<S: 'static, const N: usize> {
    input: AutomationInput<S, N>,
    /// The record, with the value of each lane over it.
    state: S,
    /// The value of each number in the record, which it goes back to when its lane lets go.
    record: [f32; N],
    /// The value of each number that a lane holds.
    lanes: [Option<f32>; N],
    /// The frames left of the edit glide of each number.
    gliding: [f32; N],
    /// Whether a block of events came yet.
    followed: bool,
}

impl<S, const N: usize> std::ops::Deref for Automated<S, N> {
    type Target = S;

    /// The record, with the value of each lane over it: what the device plays.
    fn deref(&self) -> &S {
        &self.state
    }
}

impl<S, const N: usize> Automated<S, N> {
    /// No number automated yet.
    pub fn new(input: AutomationInput<S, N>, record: S) -> Self {
        Self {
            input,
            record: input.parameters.map(|parameter| (parameter.get)(&record)),
            state: record,
            lanes: [None; N],
            gliding: [0.0; N],
            followed: false,
        }
    }

    /// Every number moving in `ramp` frames: for a device that is made, which snaps after.
    pub fn targets(&self, ramp: f32) -> Targets<S, N> {
        self.targets_with([ramp; N], ramp, false)
    }

    /// Takes a new record from an update, and gives what to aim at: the numbers that a lane
    /// holds keep the lane's value, and the rest glide in `edit` frames. The record it replaces
    /// rides back in `record`, so nothing is dropped here.
    pub fn set_record(&mut self, record: &mut S, edit: f32) -> Targets<S, N> {
        std::mem::swap(&mut self.state, record);
        let numbers = self.input.parameters.iter().zip(&mut self.record);
        for ((parameter, value), lane) in numbers.zip(self.lanes) {
            *value = (parameter.get)(&self.state);
            if let Some(lane) = lane {
                (parameter.set)(&mut self.state, lane);
            }
        }
        self.targets(edit)
    }

    /// Takes the lane values of this block from the device's automation input. `None` when no
    /// value changed, else what to aim at. `edit` is the frames of the device's edit glide.
    pub fn follow(&mut self, context: &ProcessContext<'_>, edit: f32) -> Option<Targets<S, N>> {
        let events = context.event_inputs.get(self.input.port);
        let block = context.frames as f32;
        self.take(events, block, context.transport.jumped, edit)
    }

    /// [`follow`](Self::follow) without the context: the events of a block of `block` frames.
    fn take(
        &mut self,
        events: &[Timed<Automation>],
        block: f32,
        jumped: bool,
        edit: f32,
    ) -> Option<Targets<S, N>> {
        let snaps = !std::mem::replace(&mut self.followed, true);
        let mut heard = [None; N];
        for timed in events {
            let index = usize::from(timed.event.parameter);
            let (Some(parameter), Some(heard)) =
                (self.input.parameters.get(index), heard.get_mut(index))
            else {
                continue;
            };
            // Not `clamp`: it panics on a NaN, and nothing may panic on the audio thread.
            *heard = Some(timed.event.value.max(parameter.min).min(parameter.max));
        }
        // A number that does not move keeps its target, and so its glide.
        let mut ramps = [edit; N];
        let mut changed = false;
        let numbers = self.input.parameters.iter().zip(self.record);
        let numbers = numbers.zip(self.lanes.iter_mut().zip(&mut self.gliding));
        for (((parameter, record), (lane, gliding)), (ramp, heard)) in
            numbers.zip(ramps.iter_mut().zip(heard))
        {
            let moved = match (*lane, heard) {
                (None, None) => None,
                (Some(before), Some(now)) if before == now => None,
                (_, Some(_)) if snaps => {
                    *gliding = 0.0;
                    Some(0.0)
                }
                (Some(_), Some(_)) if *gliding > 0.0 && !jumped => Some(*gliding),
                (Some(_), Some(_)) if !jumped => Some(block),
                // Takes over, lets go back to the record, or jumps.
                _ => {
                    *gliding = edit;
                    Some(edit)
                }
            };
            if let Some(moved) = moved {
                changed = true;
                *ramp = moved;
                (parameter.set)(&mut self.state, heard.unwrap_or(record));
            }
            *gliding = (*gliding - block).max(0.0);
            *lane = heard;
        }
        changed.then(|| self.targets_with(ramps, edit, snaps))
    }

    fn targets_with(&self, ramps: [f32; N], edit: f32, snaps: bool) -> Targets<S, N> {
        Targets {
            parameters: self.input.parameters,
            ramps,
            edit,
            snaps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Automated, Automation, AutomationInput, Targets};
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
    const INPUT: AutomationInput<State, 2> = AutomationInput::new(0, [&CUTOFF, &MIX]);
    const RECORD: State = State {
        cutoff: 1_000.,
        mix: 1.,
    };
    const BLOCK: f32 = 64.;
    const EDIT: f32 = 960.;

    fn block(values: &[(u16, f32)]) -> Vec<Timed<Automation>> {
        let events = values
            .iter()
            .map(|&(parameter, value)| Automation { parameter, value });
        events.map(|event| Timed { offset: 0, event }).collect()
    }

    /// A block of `values`, not after a jump.
    fn take(
        automated: &mut Automated<State, 2>,
        values: &[(u16, f32)],
    ) -> Option<Targets<State, 2>> {
        automated.take(&block(values), BLOCK, false, EDIT)
    }

    fn ramps(targets: &Targets<State, 2>) -> (f32, f32) {
        (targets.ramp(&CUTOFF), targets.ramp(&MIX))
    }

    #[test]
    fn the_first_block_takes_the_lanes_at_once() {
        let mut automated = Automated::new(INPUT, RECORD);
        let targets = take(&mut automated, &[(0, 500.)]).unwrap();
        assert!(targets.snaps());
        assert_eq!(automated.cutoff, 500.);
        assert_eq!(ramps(&targets), (0., EDIT));
        // The same value again moves nothing, and a move takes one block.
        assert!(take(&mut automated, &[(0, 500.)]).is_none());
        let targets = take(&mut automated, &[(0, 600.)]).unwrap();
        assert!(!targets.snaps());
        assert_eq!(ramps(&targets), (BLOCK, EDIT));
    }

    #[test]
    fn a_lane_that_takes_over_glides_as_an_edit_and_its_moves_end_with_that_glide() {
        let mut automated = Automated::new(INPUT, RECORD);
        assert!(take(&mut automated, &[]).is_none());
        let targets = take(&mut automated, &[(0, 500.)]).unwrap();
        assert_eq!(ramps(&targets), (EDIT, EDIT));
        // The next move still ends where the glide ends, one block later than it is now.
        let targets = take(&mut automated, &[(0, 600.)]).unwrap();
        assert_eq!(targets.ramp(&CUTOFF), EDIT - BLOCK);
        // After the glide, a move takes one block.
        for _ in 0..20 {
            take(&mut automated, &[(0, 600.)]);
        }
        let targets = take(&mut automated, &[(0, 700.)]).unwrap();
        assert_eq!(targets.ramp(&CUTOFF), BLOCK);
    }

    /// A lane that takes the mix over while the cutoff sweeps does not slow the sweep down.
    #[test]
    fn each_number_has_its_own_ramp() {
        let mut automated = Automated::new(INPUT, RECORD);
        take(&mut automated, &[(0, 500.)]);
        let targets = take(&mut automated, &[(0, 600.), (1, 0.5)]).unwrap();
        assert_eq!(ramps(&targets), (BLOCK, EDIT));
        // And a lane that lets go glides back alone.
        let targets = take(&mut automated, &[(0, 700.)]).unwrap();
        assert_eq!(ramps(&targets), (BLOCK, EDIT));
        assert_eq!(automated.mix, RECORD.mix);
    }

    #[test]
    fn after_a_jump_the_lanes_glide_as_an_edit() {
        let mut automated = Automated::new(INPUT, RECORD);
        take(&mut automated, &[(0, 500.)]);
        let targets = automated
            .take(&block(&[(0, 5_000.)]), BLOCK, true, EDIT)
            .unwrap();
        assert_eq!(targets.ramp(&CUTOFF), EDIT);
    }

    #[test]
    fn an_edit_of_the_record_keeps_the_lane_and_a_block_without_it_lets_go() {
        let mut automated = Automated::new(INPUT, RECORD);
        take(&mut automated, &[(0, 600.)]);
        let edited = State {
            cutoff: 2_000.,
            mix: 0.5,
        };
        let mut update = edited;
        automated.set_record(&mut update, EDIT);
        let expected = State {
            cutoff: 600.,
            mix: 0.5,
        };
        assert_eq!(*automated, expected);
        // What it played before rides back in the update.
        let before = State {
            cutoff: 600.,
            mix: 1.,
        };
        assert_eq!(update, before);
        let targets = take(&mut automated, &[]).unwrap();
        assert_eq!(*automated, edited);
        assert_eq!(targets.ramp(&CUTOFF), EDIT);
    }

    #[test]
    fn a_value_is_held_to_the_range_and_an_unknown_index_is_left_out() {
        let mut automated = Automated::new(INPUT, RECORD);
        let targets = take(&mut automated, &[(0, 1e9), (1, f32::NAN), (7, 0.5)]).unwrap();
        let expected = State {
            cutoff: 20_000.,
            mix: 0.,
        };
        assert!(targets.snaps());
        assert_eq!(*automated, expected);
    }
}
