//! The grid that drags, keys and new shapes land on: the snap setting of the window and the
//! math of it. Pure, no GPUI.
//!
//! The setting is interface state, like zoom and selection: it is not saved and every session
//! starts on a sixteenth. The timeline and the note editor share one, so a change in the corner
//! of the arrangement is the grid of both.

use std::cell::Cell;
use std::rc::Rc;

use sound_core::{TICKS_PER_QUARTER, Ticks, TimeSignature};

/// The snap of the window. `Off` places at the tick under the pointer.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Snap {
    Off,
    Bar,
    Beat,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
}

/// One setting for the timeline and the note editor. Interface state, never saved.
pub type SharedSnap = Rc<Cell<Snap>>;

impl Snap {
    /// In the order of the menu.
    pub const ALL: [Snap; 6] = [
        Self::Off,
        Self::Bar,
        Self::Beat,
        Self::Eighth,
        Self::Sixteenth,
        Self::ThirtySecond,
    ];

    /// What the menu says, and the value of its item.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bar => "Bar",
            Self::Beat => "Beat",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
        }
    }

    /// The setting a menu item names.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|snap| snap.label() == label)
    }

    /// The grid a drag moves by and a new shape starts on. One tick when snap is off. A bar and a
    /// beat follow the time signature: a beat of 6/8 is an eighth.
    pub fn step(self, time_signature: TimeSignature) -> Ticks {
        match self {
            Self::Off => Ticks(1),
            _ => self.unit(time_signature),
        }
    }

    pub fn grid(self, time_signature: TimeSignature) -> Grid {
        Grid {
            step: self.step(time_signature),
            unit: self.unit(time_signature),
        }
    }

    /// What an arrow key moves by, and the shortest a clip or a note gets from a drag: the step,
    /// and a thirty-second when snap is off, because a tick is too small to see or to press
    /// a key for.
    pub fn unit(self, time_signature: TimeSignature) -> Ticks {
        match self {
            Self::Bar => Ticks(time_signature.ticks_per_bar()),
            Self::Beat => Ticks(time_signature.ticks_per_beat()),
            Self::Eighth => Ticks(TICKS_PER_QUARTER / 2),
            Self::Sixteenth => Ticks(TICKS_PER_QUARTER / 4),
            Self::ThirtySecond | Self::Off => Ticks(TICKS_PER_QUARTER / 8),
        }
    }
}

/// The two sizes a setting gives in one time signature, see [`Snap::step`] and [`Snap::unit`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub step: Ticks,
    pub unit: Ticks,
}

impl Grid {
    /// The grid with snap bypassed, as cmd does during a drag: the step is a tick and the
    /// shortest shape stays what the setting says.
    pub fn free(self) -> Self {
        Self {
            step: Ticks(1),
            ..self
        }
    }
}

/// The nearest multiple of `step`.
pub fn snap(tick: Ticks, step: Ticks) -> Ticks {
    let step = step.0.max(1);
    Ticks((tick.0 + step / 2) / step * step)
}

/// The multiple of `step` at or before a tick: the grid cell that a pointer is in.
pub fn snap_floor(tick: Ticks, step: Ticks) -> Ticks {
    let step = step.0.max(1);
    Ticks(tick.0 / step * step)
}

/// How far a drag went, from the tick under the pointer at mouse down to the tick under it now,
/// as the nearest whole number of steps. A drag moves by this and does not snap the result, so
/// what an agent wrote off the grid keeps its offset.
pub fn snapped_delta(from: Ticks, to: Ticks, step: Ticks) -> i64 {
    let step = step.0.max(1) as i64;
    let delta = to.0 as i64 - from.0 as i64;
    (delta + delta.signum() * step / 2) / step * step
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIXTEENTH: Ticks = Ticks(240);

    #[test]
    fn snap_goes_to_the_nearest_step() {
        assert_eq!(snap(Ticks(0), SIXTEENTH), Ticks(0));
        assert_eq!(snap(Ticks(119), SIXTEENTH), Ticks(0));
        assert_eq!(snap(Ticks(120), SIXTEENTH), Ticks(240));
        assert_eq!(snap(Ticks(359), SIXTEENTH), Ticks(240));
        assert_eq!(snap(Ticks(3840 + 130), SIXTEENTH), Ticks(3840 + 240));
        assert_eq!(snap(Ticks(2000), Ticks(3840)), Ticks(3840));
        assert_eq!(snap(Ticks(1234), Ticks(1)), Ticks(1234));
    }

    #[test]
    fn a_drag_moves_by_whole_steps() {
        assert_eq!(snap_floor(Ticks(239), SIXTEENTH), Ticks(0));
        assert_eq!(snap_floor(Ticks(240), SIXTEENTH), Ticks(240));
        assert_eq!(snap_floor(Ticks(3840 + 479), SIXTEENTH), Ticks(3840 + 240));
        assert_eq!(snap_floor(Ticks(3840 + 479), Ticks(1)), Ticks(3840 + 479));

        assert_eq!(snapped_delta(Ticks(1000), Ticks(1000), SIXTEENTH), 0);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(1119), SIXTEENTH), 0);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(1120), SIXTEENTH), 240);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(881), SIXTEENTH), 0);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(880), SIXTEENTH), -240);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(0), SIXTEENTH), -960);
        assert_eq!(snapped_delta(Ticks(0), Ticks(3840), SIXTEENTH), 3840);
        // Off, or cmd held: the pointer's own distance.
        assert_eq!(snapped_delta(Ticks(1000), Ticks(1013), Ticks(1)), 13);
        assert_eq!(snapped_delta(Ticks(1000), Ticks(3000), Ticks(3840)), 3840);
    }

    #[test]
    fn a_bar_and_a_beat_follow_the_time_signature() {
        let four_four = TimeSignature::new(4, 4).unwrap();
        let six_eight = TimeSignature::new(6, 8).unwrap();
        assert_eq!(Snap::Bar.step(four_four), Ticks(3840));
        assert_eq!(Snap::Beat.step(four_four), Ticks(960));
        assert_eq!(Snap::Bar.step(six_eight), Ticks(2880));
        assert_eq!(Snap::Beat.step(six_eight), Ticks(480));
        assert_eq!(Snap::Eighth.step(four_four), Ticks(480));
        assert_eq!(Snap::Sixteenth.step(four_four), Ticks(240));
        assert_eq!(Snap::ThirtySecond.step(four_four), Ticks(120));
        assert_eq!(Snap::Off.step(four_four), Ticks(1));
        // A key moves by the step, and by a thirty-second when snap is off.
        assert_eq!(Snap::Off.unit(four_four), Ticks(120));
        assert_eq!(Snap::Bar.unit(four_four), Ticks(3840));
        assert_eq!(Snap::default(), Snap::Sixteenth);
        for snap in Snap::ALL {
            assert_eq!(Snap::from_label(snap.label()), Some(snap));
        }
    }
}
