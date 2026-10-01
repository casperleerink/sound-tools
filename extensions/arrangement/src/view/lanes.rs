//! The expression lanes of the note editor: the bend and mod wheels and the key pressure of a
//! clip, one at a time, in the lane under the pitch rows where the velocities are. Pure math,
//! no GPUI and no project, like [`super::roll`].
//!
//! A lane shows as the line through its points, as it plays ([`sound_notes::value_at`]). A
//! drag draws a new line over the ticks it passes, alt and a drag erase the points between the
//! press and the pointer, and a double click clears the lane. Each works from the lane as it
//! was at mouse down, so a drag there and back ends where it began.

use std::collections::BTreeMap;
use std::iter::once;
use std::ops::{Range, RangeInclusive};

use sound_core::Ticks;
use sound_notes::{Clip, ExpressionValue, Point, cut, thinned};

use super::layout::Viewport;
use super::roll::{VELOCITY_BOTTOM, VELOCITY_HEIGHT, VELOCITY_TOP};
use super::snap::Grid;

/// How far apart a drag draws points when it does not snap, in pixels.
const DRAW_SPACING: f32 = 4.0;

/// What the lane under the pitch rows shows. Interface state: not saved, no undo step.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Shown {
    #[default]
    Velocity,
    Lane(Lane),
}

/// An expression lane of a clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lane {
    Bend,
    ModWheel,
    Pressure,
}

impl Shown {
    /// In the order of the select, each with the value and the label of its item.
    pub const ALL: [(Self, &'static str, &'static str); 4] = [
        (Self::Velocity, "velocity", "Velocity"),
        (Self::Lane(Lane::Bend), "bend", "Bend"),
        (Self::Lane(Lane::ModWheel), "mod-wheel", "Mod wheel"),
        (Self::Lane(Lane::Pressure), "pressure", "Pressure"),
    ];

    /// The value of its item in the select.
    pub fn value(self) -> &'static str {
        let found = Self::ALL.into_iter().find(|(shown, ..)| *shown == self);
        found.map_or("velocity", |(_, value, _)| value)
    }

    pub fn from_value(value: &str) -> Option<Self> {
        let found = Self::ALL.into_iter().find(|(_, of, _)| *of == value);
        found.map(|(shown, ..)| shown)
    }
}

/// What a gesture in a lane does to its points, in ticks of the clip.
pub enum LaneEdit<'a> {
    /// The points from the first to the last tick drawn give way to the line drawn, thinned.
    /// The line is the height of the pointer in the lane at each tick it drew.
    Draw(&'a BTreeMap<Ticks, f32>),
    /// The points in these ticks go.
    Erase(RangeInclusive<Ticks>),
    /// Every point goes: the lane is at rest.
    Clear,
}

impl LaneEdit<'_> {
    fn applied<V: ExpressionValue>(&self, points: &[Point<V>]) -> Vec<Point<V>> {
        match self {
            Self::Draw(drawn) => {
                let (Some((&first, _)), Some((&last, _))) =
                    (drawn.first_key_value(), drawn.last_key_value())
                else {
                    return points.to_vec();
                };
                let line: Vec<Point<V>> = drawn
                    .iter()
                    .map(|(tick, y)| Point {
                        tick: *tick,
                        value: value_at_y(*y),
                    })
                    .collect();
                let before = points.iter().filter(|point| point.tick < first);
                let after = points.iter().filter(|point| point.tick > last);
                before
                    .copied()
                    .chain(thinned(&line))
                    .chain(after.copied())
                    .collect()
            }
            Self::Erase(ticks) => {
                let kept = points.iter().filter(|point| !ticks.contains(&point.tick));
                kept.copied().collect()
            }
            Self::Clear => Vec::new(),
        }
    }
}

impl Lane {
    /// `clip` with this lane of `origin` changed by `edit`. The rest of `clip` stays as it is,
    /// also what changed in it since `origin`, and the lane fits it when it got shorter.
    pub fn edit(self, clip: &mut Clip, origin: &Clip, edit: &LaneEdit) {
        let inside = Ticks(0)..clip.length.ticks();
        match self {
            Self::Bend => clip.bend = cut(&edit.applied(&origin.bend), inside),
            Self::ModWheel => clip.mod_wheel = cut(&edit.applied(&origin.mod_wheel), inside),
            Self::Pressure => clip.pressure = cut(&edit.applied(&origin.pressure), inside),
        }
    }

    pub fn is_empty(self, clip: &Clip) -> bool {
        match self {
            Self::Bend => clip.bend.is_empty(),
            Self::ModWheel => clip.mod_wheel.is_empty(),
            Self::Pressure => clip.pressure.is_empty(),
        }
    }

    /// The line of this lane of `clip` in the lane, as the places where it turns: from the
    /// clip start to its end, level before the first point and after the last. Only what
    /// `ticks` shows, with one point on each side. Empty for a lane with no points.
    pub fn line(self, viewport: &Viewport, clip: &Clip, ticks: Range<Ticks>) -> Vec<(f32, f32)> {
        match self {
            Self::Bend => line(viewport, clip, &clip.bend, ticks),
            Self::ModWheel => line(viewport, clip, &clip.mod_wheel, ticks),
            Self::Pressure => line(viewport, clip, &clip.pressure, ticks),
        }
    }

    /// The height of the rest of the lane, where it plays with no points, when it is not the
    /// bottom: the middle of the bend.
    pub fn middle(self) -> Option<f32> {
        (self == Self::Bend).then(|| lane_y(sound_notes::Bend::MIDDLE))
    }

    /// The undo label of an edit of this lane.
    pub fn label(self, edit: &LaneEdit) -> &'static str {
        match (self, edit) {
            (Self::Bend, LaneEdit::Draw(_)) => "Draw bend",
            (Self::Bend, LaneEdit::Erase(_)) => "Erase bend",
            (Self::Bend, LaneEdit::Clear) => "Clear bend",
            (Self::ModWheel, LaneEdit::Draw(_)) => "Draw mod wheel",
            (Self::ModWheel, LaneEdit::Erase(_)) => "Erase mod wheel",
            (Self::ModWheel, LaneEdit::Clear) => "Clear mod wheel",
            (Self::Pressure, LaneEdit::Draw(_)) => "Draw pressure",
            (Self::Pressure, LaneEdit::Erase(_)) => "Erase pressure",
            (Self::Pressure, LaneEdit::Clear) => "Clear pressure",
        }
    }
}

fn line<V: ExpressionValue>(
    viewport: &Viewport,
    clip: &Clip,
    points: &[Point<V>],
    ticks: Range<Ticks>,
) -> Vec<(f32, f32)> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    let placed = |tick: Ticks, value: V| (viewport.x_of(clip.start + tick), lane_y(value));
    // A recorded lane has many points and the editor shows a few bars of it.
    let from = points.partition_point(|point| clip.start + point.tick < ticks.start);
    let to = points.partition_point(|point| clip.start + point.tick < ticks.end);
    let shown = points
        .get(from.saturating_sub(1)..(to + 1).min(points.len()))
        .unwrap_or_default();
    once(placed(Ticks(0), first.value))
        .chain(shown.iter().map(|point| placed(point.tick, point.value)))
        .chain(once(placed(clip.length.ticks(), last.value)))
        .collect()
}

/// The height in the lane of a value: the lowest at the bottom, the highest at the top, with
/// the air of a velocity bar.
pub fn lane_y<V: ExpressionValue>(value: V) -> f32 {
    let bottom = VELOCITY_HEIGHT - VELOCITY_BOTTOM;
    let share = (value.number() - V::LOWEST) as f32 / (V::HIGHEST - V::LOWEST) as f32;
    bottom - (bottom - VELOCITY_TOP) * share
}

/// The value at height `y` in the lane. Above the lane it is the highest, below the lowest.
pub fn value_at_y<V: ExpressionValue>(y: f32) -> V {
    let bottom = VELOCITY_HEIGHT - VELOCITY_BOTTOM;
    let share = ((bottom - y) / (bottom - VELOCITY_TOP)).clamp(0.0, 1.0);
    let span = (V::HIGHEST - V::LOWEST) as f32;
    V::nearest(i64::from(V::LOWEST) + (share * span).round() as i64)
}

/// What a drag draws between two places in the lane, `(x, y)` from the last move to this one:
/// the ticks of the clip it passes, each with the height of the pointer there. On the grid
/// lines when the grid snaps, the one nearest the pointer included, else every few pixels.
/// Inside the clip only.
pub fn drawn_between(
    viewport: &Viewport,
    clip: &Clip,
    grid: &Grid,
    from: (f32, f32),
    to: (f32, f32),
) -> Vec<(Ticks, f32)> {
    let (left, right) = if from.0 <= to.0 {
        (from, to)
    } else {
        (to, from)
    };
    let height_at = |x: f32| match right.0 - left.0 {
        across if across < f32::EPSILON => to.1,
        across => left.1 + (right.1 - left.1) * ((x - left.0) / across).clamp(0.0, 1.0),
    };
    let ticks: Vec<Ticks> = if grid.snaps() {
        let last = viewport.tick_at(right.0);
        let first = viewport.tick_at(left.0);
        let mut line = grid.floor(first);
        if line < first {
            line = grid.next_line(line);
        }
        let mut lines = vec![grid.snap(viewport.tick_at(to.0))];
        while line <= last {
            lines.push(line);
            line = grid.next_line(line);
        }
        lines
    } else {
        let steps = ((right.0 - left.0) / DRAW_SPACING) as usize;
        let passed = (0..=steps).map(|step| left.0 + step as f32 * DRAW_SPACING);
        passed
            .chain(once(right.0))
            .map(|x| viewport.tick_at(x))
            .collect()
    };
    let inside = clip.start..clip.end();
    ticks
        .into_iter()
        .filter(|tick| inside.contains(tick))
        .map(|tick| {
            (
                tick.saturating_sub(clip.start),
                height_at(viewport.x_of(tick)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sound_core::TimeSignatures;
    use sound_notes::{Amount, Bend, Length};

    use super::super::snap::Snap;
    use super::*;

    fn clip() -> Clip {
        Clip::new(Ticks(3840), Length::new(Ticks(3840)).unwrap(), Vec::new())
    }

    /// A quarter is 96 pixels, and tick 3840 is at the left edge of the lane.
    fn viewport() -> Viewport {
        let mut viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        viewport.scroll_x = f64::from(viewport.x_of(Ticks(3840)) - viewport.x_of(Ticks(0)));
        viewport
    }

    fn bend(tick: u64, value: i16) -> Point<Bend> {
        Point {
            tick: Ticks(tick),
            value: Bend::new(value).unwrap(),
        }
    }

    #[test]
    fn a_value_has_one_height_and_the_middle_of_the_bend_is_in_the_middle() {
        let top = VELOCITY_TOP;
        let bottom = VELOCITY_HEIGHT - VELOCITY_BOTTOM;
        assert_eq!(lane_y(Amount::NONE), bottom);
        assert_eq!(lane_y(Amount::new(127).unwrap()), top);
        assert_eq!(lane_y(Bend::new(-8192).unwrap()), bottom);
        let middle = Lane::Bend.middle().unwrap();
        assert!((middle - (top + bottom) / 2.0).abs() < 0.01, "{middle}");
        for value in [-8192, -100, 0, 4096, 8191] {
            let bend = Bend::new(value).unwrap();
            let back: Bend = value_at_y(lane_y(bend));
            assert!((back.value() - value).abs() <= 1, "{value}");
        }
        assert_eq!(value_at_y::<Amount>(-50.0), Amount::new(127).unwrap());
        assert_eq!(value_at_y::<Amount>(500.0), Amount::NONE);
    }

    /// With snap on, a drag draws one point per grid line it passes, and one on the line
    /// nearest the pointer, at the height of the pointer there.
    #[test]
    fn a_snapped_drag_draws_on_the_grid_lines_it_passes() {
        let grid = Snap::Sixteenth.grid(&TimeSignatures::default());
        let viewport = viewport();
        let clip = clip();
        let x = |tick: u64| viewport.x_of(Ticks(3840 + tick));
        let drawn = drawn_between(&viewport, &clip, &grid, (x(0), 10.0), (x(700), 30.0));
        let ticks: Vec<u64> = drawn.iter().map(|(tick, _)| tick.0).collect();
        assert_eq!(ticks, [720, 0, 240, 480]);
        // A small move inside a cell draws only the line nearest the pointer, not the one it
        // did not cross.
        let small = drawn_between(&viewport, &clip, &grid, (x(230), 10.0), (x(235), 10.0));
        let ticks: Vec<u64> = small.iter().map(|(tick, _)| tick.0).collect();
        assert_eq!(ticks, [240]);
        let (_, height) = drawn[2];
        assert!((height - (10.0 + 20.0 * 240.0 / 700.0)).abs() < 0.01);
        // Left of the clip draws nothing there.
        let before = drawn_between(&viewport, &clip, &grid, (x(0) - 50.0, 10.0), (x(0), 10.0));
        let ticks: Vec<u64> = before.iter().map(|(tick, _)| tick.0).collect();
        assert_eq!(ticks, [0, 0]);
    }

    /// Without snap, as with cmd held, a drag draws every few pixels.
    #[test]
    fn a_free_drag_draws_every_few_pixels() {
        let grid = Snap::Sixteenth.grid(&TimeSignatures::default()).free();
        let viewport = viewport();
        let from = viewport.x_of(Ticks(3840 + 100));
        let drawn = drawn_between(&viewport, &clip(), &grid, (from, 10.0), (from + 20.0, 10.0));
        // 96 pixels a quarter is 10 ticks a pixel.
        let ticks: Vec<u64> = drawn.iter().map(|(tick, _)| tick.0).collect();
        assert_eq!(ticks, [100, 140, 180, 220, 260, 300, 300]);
    }

    #[test]
    fn a_draw_replaces_the_points_it_passes_and_keeps_the_rest() {
        let mut origin = clip();
        origin.bend = vec![bend(0, 100), bend(480, 200), bend(960, 300), bend(2000, 0)];
        let y = lane_y(Bend::new(4000).unwrap());
        let drawn: BTreeMap<Ticks, f32> = [(Ticks(400), y), (Ticks(600), y), (Ticks(1000), y)]
            .into_iter()
            .collect();
        let mut clip = origin.clone();
        Lane::Bend.edit(&mut clip, &origin, &LaneEdit::Draw(&drawn));
        let values: Vec<(u64, i16)> = (clip.bend.iter())
            .map(|point| (point.tick.0, point.value.value()))
            .collect();
        let drawn_value = value_at_y::<Bend>(y).value();
        // The level line drawn is thinned to its ends.
        assert_eq!(
            values,
            [(0, 100), (400, drawn_value), (1000, drawn_value), (2000, 0)]
        );
        assert_eq!(clip.notes, origin.notes);

        Lane::Bend.edit(
            &mut clip,
            &origin,
            &LaneEdit::Erase(Ticks(400)..=Ticks(960)),
        );
        assert_eq!(clip.bend, [bend(0, 100), bend(2000, 0)]);
        Lane::Bend.edit(&mut clip, &origin, &LaneEdit::Clear);
        assert!(Lane::Bend.is_empty(&clip));
    }

    /// The line runs from the clip start to its end, level outside its points, and holds only
    /// what shows with one point on each side.
    #[test]
    fn the_line_of_a_lane_spans_the_clip() {
        let viewport = viewport();
        let mut clip = clip();
        assert!(
            Lane::Bend
                .line(&viewport, &clip, Ticks(0)..Ticks(100_000))
                .is_empty()
        );
        clip.bend = vec![bend(480, 0), bend(960, 8191), bend(1440, 0), bend(3000, 0)];
        let x = |tick: u64| viewport.x_of(Ticks(3840 + tick));
        let line = Lane::Bend.line(&viewport, &clip, Ticks(0)..Ticks(100_000));
        let xs: Vec<f32> = line.iter().map(|(x, _)| *x).collect();
        assert_eq!(xs, [x(0), x(480), x(960), x(1440), x(3000), x(3840)]);
        assert_eq!(line[2].1, VELOCITY_TOP);
        // Only ticks 1000 to 1200 show: the points on each side of them draw the line there.
        let visible = Ticks(3840 + 1000)..Ticks(3840 + 1200);
        let line = Lane::Bend.line(&viewport, &clip, visible);
        let xs: Vec<f32> = line.iter().map(|(x, _)| *x).collect();
        assert_eq!(xs, [x(0), x(960), x(1440), x(3840)]);
    }

    #[test]
    fn the_select_names_each_lane_once() {
        for (shown, value, _) in Shown::ALL {
            assert_eq!(shown.value(), value);
            assert_eq!(Shown::from_value(value), Some(shown));
        }
        assert_eq!(Shown::from_value("tempo"), None);
    }
}
