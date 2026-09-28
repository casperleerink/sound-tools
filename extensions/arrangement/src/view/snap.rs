//! The grid that drags, keys and new shapes land on: the snap setting of the window and the
//! math of it. Pure, no GPUI.
//!
//! The setting is interface state, like zoom and selection: it is not saved and every session
//! starts on a sixteenth. The timeline and the note editor share one, so a change in the corner
//! of the arrangement is the grid of both.

use std::cell::Cell;
use std::rc::Rc;

use sound_core::{Bar, TICKS_PER_QUARTER, Ticks, TimeSignatures};

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

    /// The grid of this setting over the time signatures of the piece.
    pub fn grid(self, time_signatures: &TimeSignatures) -> Grid {
        Grid {
            snap: self,
            time_signatures: time_signatures.clone(),
            free: false,
        }
    }

    /// The grid a drag moves by and a new shape starts on, in `bar`. One tick when snap is off.
    /// A bar and a beat follow the time signature of the bar: a beat of 6/8 is an eighth.
    fn step_in(self, bar: Bar) -> Ticks {
        match self {
            Self::Off => Ticks(1),
            _ => self.unit_in(bar),
        }
    }

    /// What an arrow key moves by, the length of a new note, and the shortest a clip or a note
    /// gets from a drag: the step, and a sixteenth when snap is off, the grid a session starts
    /// on. A tick is too small to see or to press a key for, and a thirty-second made a note
    /// added with snap off too short to hit.
    fn unit_in(self, bar: Bar) -> Ticks {
        match self {
            Self::Bar => bar.length(),
            Self::Beat => Ticks(bar.signature.ticks_per_beat()),
            Self::Eighth => Ticks(TICKS_PER_QUARTER / 2),
            Self::Sixteenth | Self::Off => Ticks(TICKS_PER_QUARTER / 4),
            Self::ThirtySecond => Ticks(TICKS_PER_QUARTER / 8),
        }
    }
}

/// Where drags, keys and new shapes land: a setting over the time signatures of the piece.
///
/// Every bar line is on the grid, and the grid inside a bar counts from its bar line. So an
/// eighth grid after a bar of 3/16 still meets the next bar line, and a bar grid is the bar
/// lines however long each bar is.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    snap: Snap,
    time_signatures: TimeSignatures,
    /// Snap bypassed, as cmd does during a drag.
    free: bool,
}

impl Grid {
    /// The grid with snap bypassed, as cmd does during a drag: every tick is on it, and the
    /// shortest shape stays what the setting says.
    pub fn free(self) -> Self {
        Self { free: true, ..self }
    }

    /// The bar that `tick` is in.
    pub fn bar_at(&self, tick: Ticks) -> Bar {
        self.time_signatures.bar_at(tick)
    }

    /// See [`Snap::unit_in`], in the bar that `tick` is in.
    pub fn unit_at(&self, tick: Ticks) -> Ticks {
        self.snap.unit_in(self.time_signatures.bar_at(tick))
    }

    /// The grid line at or before `tick`: the grid cell that a pointer is in.
    pub fn floor(&self, tick: Ticks) -> Ticks {
        self.lines_around(tick).0
    }

    /// The nearest grid line.
    pub fn snap(&self, tick: Ticks) -> Ticks {
        let (before, after) = self.lines_around(tick);
        match (tick.0 - before.0) * 2 >= after.0 - before.0 {
            true => after,
            false => before,
        }
    }

    /// How far a drag moves a shape, from the tick under the pointer at mouse down to the tick
    /// under it now. `anchor` is the start or the edge of the shape that is dragged, as it was
    /// at mouse down.
    ///
    /// The grid line nearest the anchor moves to the grid line nearest to where the pointer
    /// takes it, and the shape moves by as much. So a shape on the grid lands on the grid
    /// across bars of any length, and one that an agent wrote off the grid keeps its offset.
    pub fn delta(&self, anchor: Ticks, grab: Ticks, pointer: Ticks) -> i64 {
        let distance = pointer.0 as i64 - grab.0 as i64;
        if self.free {
            return distance;
        }
        let line = self.snap(anchor).0 as i64;
        let target = self.snap(Ticks((line + distance).max(0) as u64));
        target.0 as i64 - line
    }

    /// How far an arrow key moves a shape at `anchor`: from the grid line at or before it to
    /// the next or the previous one. So a shape on the grid stays on it, bar lines included, and
    /// one off the grid keeps its offset. With snap off it moves by a sixteenth.
    pub fn nudge(&self, anchor: Ticks, forward: bool) -> i64 {
        if self.snap == Snap::Off {
            let unit = self.unit_at(anchor).0 as i64;
            return if forward { unit } else { -unit };
        }
        let line = self.floor(anchor);
        let to = match forward {
            true => self.lines_around(line).1,
            false => self.floor(line.saturating_sub(Ticks(1))),
        };
        to.0 as i64 - line.0 as i64
    }

    /// The grid lines at or before `tick` and after it. The one after is the next bar line
    /// when the step does not fit into the rest of the bar.
    fn lines_around(&self, tick: Ticks) -> (Ticks, Ticks) {
        let bar = self.time_signatures.bar_at(tick);
        let step = match self.free {
            true => 1,
            false => self.snap.step_in(bar).0.max(1),
        };
        let before = bar.start.0 + (tick.0 - bar.start.0) / step * step;
        (
            Ticks(before),
            Ticks(before.saturating_add(step)).min(bar.end()),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use sound_core::{SignatureRun, TimeSignature};

    use super::*;

    fn time_signatures(runs: &[(&str, u32)]) -> TimeSignatures {
        let runs = runs
            .iter()
            .map(|(signature, bars)| SignatureRun {
                signature: signature.parse().unwrap(),
                bars: NonZeroU32::new(*bars).unwrap(),
            })
            .collect();
        TimeSignatures::new(runs).unwrap()
    }

    fn four_four(snap: Snap) -> Grid {
        snap.grid(&TimeSignatures::default())
    }

    #[test]
    fn snap_goes_to_the_nearest_line() {
        let grid = four_four(Snap::Sixteenth);
        assert_eq!(grid.snap(Ticks(0)), Ticks(0));
        assert_eq!(grid.snap(Ticks(119)), Ticks(0));
        assert_eq!(grid.snap(Ticks(120)), Ticks(240));
        assert_eq!(grid.snap(Ticks(359)), Ticks(240));
        assert_eq!(grid.snap(Ticks(3840 + 130)), Ticks(3840 + 240));
        assert_eq!(four_four(Snap::Bar).snap(Ticks(2000)), Ticks(3840));
        assert_eq!(four_four(Snap::Off).snap(Ticks(1234)), Ticks(1234));
        assert_eq!(grid.clone().free().snap(Ticks(1234)), Ticks(1234));

        assert_eq!(grid.floor(Ticks(239)), Ticks(0));
        assert_eq!(grid.floor(Ticks(240)), Ticks(240));
        assert_eq!(grid.floor(Ticks(3840 + 479)), Ticks(3840 + 240));
        assert_eq!(grid.free().floor(Ticks(3840 + 479)), Ticks(3840 + 479));
    }

    #[test]
    fn a_drag_moves_by_whole_steps() {
        let grid = four_four(Snap::Sixteenth);
        // A shape on the grid, grabbed in its middle.
        let delta = |pointer| grid.delta(Ticks(960), Ticks(1000), Ticks(pointer));
        assert_eq!(delta(1000), 0);
        assert_eq!(delta(1119), 0);
        assert_eq!(delta(1120), 240);
        assert_eq!(delta(881), 0);
        assert_eq!(delta(879), -240);
        assert_eq!(delta(40), -960);
        assert_eq!(delta(0), -960);
        assert_eq!(grid.delta(Ticks(0), Ticks(0), Ticks(3840)), 3840);
        // Off the grid, the shape keeps its offset from the grid.
        assert_eq!(grid.delta(Ticks(1000), Ticks(1000), Ticks(1200)), 240);
        // Off, or cmd held: the pointer's own distance.
        assert_eq!(
            four_four(Snap::Off).delta(Ticks(0), Ticks(1000), Ticks(1013)),
            13
        );
        assert_eq!(grid.free().delta(Ticks(0), Ticks(1000), Ticks(1013)), 13);
        let bar = four_four(Snap::Bar);
        assert_eq!(bar.delta(Ticks(0), Ticks(1000), Ticks(3000)), 3840);
    }

    #[test]
    fn a_bar_and_a_beat_follow_the_time_signature() {
        let six_eight = TimeSignatures::constant(TimeSignature::new(6, 8).unwrap());
        let unit = |snap: Snap, time_signatures: &TimeSignatures| {
            snap.grid(time_signatures).unit_at(Ticks(0))
        };
        let four_four = TimeSignatures::default();
        assert_eq!(unit(Snap::Bar, &four_four), Ticks(3840));
        assert_eq!(unit(Snap::Beat, &four_four), Ticks(960));
        assert_eq!(unit(Snap::Bar, &six_eight), Ticks(2880));
        assert_eq!(unit(Snap::Beat, &six_eight), Ticks(480));
        assert_eq!(unit(Snap::Eighth, &four_four), Ticks(480));
        assert_eq!(unit(Snap::Sixteenth, &four_four), Ticks(240));
        assert_eq!(unit(Snap::ThirtySecond, &four_four), Ticks(120));
        // A key moves by the step, and by a sixteenth when snap is off.
        assert_eq!(unit(Snap::Off, &four_four), Ticks(240));
        assert_eq!(Snap::default(), Snap::Sixteenth);
        for snap in Snap::ALL {
            assert_eq!(Snap::from_label(snap.label()), Some(snap));
        }
    }

    /// Bars of 3/16, 2/8 and 4/4: the grid starts again at every bar line.
    #[test]
    fn the_grid_counts_from_every_bar_line() {
        let changing = time_signatures(&[("3/16", 1), ("2/8", 1), ("4/4", 1)]);
        // Bar 2 starts at 720, bar 3 at 1680.
        let bar = Snap::Bar.grid(&changing);
        assert_eq!(bar.snap(Ticks(300)), Ticks(0));
        assert_eq!(bar.snap(Ticks(400)), Ticks(720));
        assert_eq!(bar.floor(Ticks(1679)), Ticks(720));
        assert_eq!(bar.snap(Ticks(1300)), Ticks(1680));
        assert_eq!(bar.unit_at(Ticks(800)), Ticks(960));

        // An eighth grid in 3/16 has a line at 480 and then the bar line at 720, and in 2/8 it
        // counts from 720, not from 0.
        let eighth = Snap::Eighth.grid(&changing);
        assert_eq!(eighth.floor(Ticks(700)), Ticks(480));
        assert_eq!(eighth.snap(Ticks(650)), Ticks(720));
        assert_eq!(eighth.snap(Ticks(590)), Ticks(480));
        assert_eq!(eighth.floor(Ticks(1300)), Ticks(1200));
        assert_eq!(eighth.snap(Ticks(900)), Ticks(720));

        let beat = Snap::Beat.grid(&changing);
        assert_eq!(beat.unit_at(Ticks(0)), Ticks(240));
        assert_eq!(beat.unit_at(Ticks(720)), Ticks(480));
        assert_eq!(beat.unit_at(Ticks(1680)), Ticks(960));

        // A clip on bar 1 dragged by a bar grid lands on the bar lines, however long the bars
        // it passes are.
        assert_eq!(bar.delta(Ticks(0), Ticks(100), Ticks(900)), 720);
        assert_eq!(bar.delta(Ticks(0), Ticks(100), Ticks(1900)), 1680);
        assert_eq!(bar.delta(Ticks(1680), Ticks(1700), Ticks(1000)), -960);

        // Keys step from grid line to grid line both ways, bar lines included.
        assert_eq!(bar.nudge(Ticks(720), true), 960);
        assert_eq!(bar.nudge(Ticks(720), false), -720);
        assert_eq!(bar.nudge(Ticks(1680), false), -960);
        assert_eq!(eighth.nudge(Ticks(480), true), 240);
        assert_eq!(eighth.nudge(Ticks(720), true), 480);
        assert_eq!(eighth.nudge(Ticks(720), false), -240);
        // Off the grid: the offset from the line before it stays.
        assert_eq!(eighth.nudge(Ticks(500), true), 240);
        assert_eq!(four_four(Snap::Sixteenth).nudge(Ticks(250), false), -240);
        assert_eq!(four_four(Snap::Off).nudge(Ticks(250), false), -240);
        assert_eq!(four_four(Snap::Off).nudge(Ticks(250), true), 240);
    }
}
