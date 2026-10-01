//! What a clip takes along of the automation of its track when it moves, is cut or is copied:
//! the line of each lane under it, from its start to its end. Pure, apart from the two last
//! functions: the timeline writes what these give, and a drag can draw it before the drop.
//!
//! A clip takes a lane only when the lane has a point inside the clip. Over a stretch with no
//! points the lane is not the clip's, so a move never puts a held value into a line elsewhere.
//!
//! The line under a clip replaces the line where it lands. The values just outside the place it
//! leaves and the place it lands stay what they were, by a point on each edge, so nothing jumps
//! there, and the place it leaves becomes a straight line between its edges. An edge that is on
//! the straight line through its neighbours is left out, so moves do not pile up points.
//!
//! A line is cut on the travel of its knob, where it is straight, so a point on an edge sits on
//! the line that plays. A point that is cut out and put back keeps its value as it was saved.
//!
//! The volume and the pan go along to any track. A lane of a device goes only to the track it
//! came from: the device belongs to the track, and another track may have no such device, or
//! one of another tool under the same name.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use sound_core::{AutomatedNumber, Changes, InstanceId, Parameter, Project, Ticks, ValueRange};
use sound_notes::{LaneValue, Point, cut, value_at};

use super::{AutomationLane, AutomationValue};
use crate::TrackState;
use crate::mixer::{Mix, Mixer};

/// A number of a device, by the id of the device and the field of the number: what the project
/// knows now, see [`travel_in`]. `None` for a number it does not know.
pub type Travel<'a> = dyn Fn(&InstanceId, &str) -> Option<AutomatedNumber> + 'a;

/// How far from the straight line through its neighbours, on the travel, a point may be and
/// still be left out: far under what a knob shows. Also how a drawn line is thinned.
pub(crate) const ON_THE_LINE: f32 = 1e-4;

/// The automation one clip takes along: the line of each lane of its track under it, with
/// ticks from the start of the clip. Each lane has a point at the start and at the last tick of
/// the clip, so the line is whole wherever it lands.
#[derive(Clone, Debug, PartialEq)]
pub struct Carried {
    /// The track the clip was on, and where.
    track: InstanceId,
    range: Range<Ticks>,
    lanes: Vec<AutomationLane>,
}

impl Carried {
    /// The line of each lane of `state`, the record of `track`, inside `range`, of the lanes
    /// with a point there that move. A lane whose points all hold one value holds its knob
    /// still, and is no line a clip could take along.
    pub fn under(
        track: &InstanceId,
        state: &TrackState,
        range: Range<Ticks>,
        travel: &Travel<'_>,
    ) -> Self {
        let mix = Mix::of(state);
        let lanes = state.automation.iter().filter(|lane| {
            let mut points = lane.points.iter();
            let inside = points.clone().any(|point| range.contains(&point.tick));
            let first = lane.points.first().map(|point| point.value);
            inside && points.any(|point| Some(point.value) != first)
        });
        let lanes = lanes.map(|lane| {
            let points = on_travel(&lane.points, range_of(track, &mix, lane, travel));
            AutomationLane {
                device: lane.device.clone(),
                parameter: lane.parameter.clone(),
                points: values(taken(&points, range.clone())),
            }
        });
        Self {
            track: track.clone(),
            range: range.clone(),
            lanes: lanes.collect(),
        }
    }

    /// The lanes, with ticks from the start of the clip.
    pub fn lanes(&self) -> &[AutomationLane] {
        &self.lanes
    }

    /// The track the clip was on.
    pub fn track(&self) -> &InstanceId {
        &self.track
    }

    /// How long the clip was on the timeline.
    pub fn length(&self) -> Ticks {
        self.range.end.saturating_sub(self.range.start)
    }

    /// Whether the clip takes the lane of `number` along to the track `to`: when it carries
    /// it, and it is the volume or the pan, or `to` is the track it came from.
    fn takes(&self, number: &AutomationLane, to: &InstanceId) -> bool {
        let carries = self.lanes.iter().any(|lane| lane.same_number(number));
        carries && (number.device.is_none() || self.track == *to)
    }

    /// `state`, the record of `to`, with the line put down from `start`, over what each lane
    /// had there. A number the track has no lane for yet gets one that holds its record value
    /// around what lands: the volume and the pan always, a device of the track while it has
    /// the number.
    pub fn place(
        &self,
        to: &InstanceId,
        state: &mut TrackState,
        start: Ticks,
        travel: &Travel<'_>,
    ) {
        let range = start..start + self.length();
        let mix = Mix::of(state);
        for carried in self.lanes.iter().filter(|lane| self.takes(lane, to)) {
            let number = number(to, &mix, carried, travel);
            let travel = number.map(|number| number.range);
            let inside = on_travel(&carried.points, travel).into_iter();
            let inside = inside.map(|point| Point {
                tick: start + point.tick,
                value: point.value,
            });
            let inside: Vec<_> = inside.collect();
            let mut lanes = state.automation.iter_mut();
            match lanes.find(|lane| carried.same_number(lane)) {
                Some(lane) => {
                    let points = on_travel(&lane.points, travel);
                    lane.points = values(spliced(&points, range.clone(), inside));
                }
                None => {
                    let Some(record) = number.and_then(|number| number.record) else {
                        continue;
                    };
                    let around = [Point {
                        tick: range.end,
                        value: OnTravel::new(record, travel),
                    }];
                    state.automation.push(AutomationLane {
                        points: values(spliced(&around, range.clone(), inside)),
                        ..carried.clone()
                    });
                }
            }
        }
    }
}

/// `state`, the record of `track`, without the lines clips took from it: `taken` is what each
/// clip took and the track it goes to. Each lane a clip takes along becomes a straight line
/// between the edges of where it was. Clips that touch or overlap are one place, so no edge
/// sits inside another clip that left. What a move and a cut leave behind.
pub fn clear(
    track: &InstanceId,
    state: &mut TrackState,
    taken: &[(&Carried, &InstanceId)],
    travel: &Travel<'_>,
) {
    let mix = Mix::of(state);
    for lane in &mut state.automation {
        let left = taken
            .iter()
            .filter(|(carried, to)| carried.track == *track && carried.takes(lane, to));
        let left: Vec<Range<Ticks>> = left.map(|(carried, _)| carried.range.clone()).collect();
        if left.is_empty() {
            continue;
        }
        let travel = range_of(track, &mix, lane, travel);
        let mut points = on_travel(&lane.points, travel);
        for range in merged(left) {
            points = spliced(&points, range, Vec::new());
        }
        lane.points = values(points);
    }
}

/// The ranges, with those that touch or overlap made one, in tick order.
fn merged(mut ranges: Vec<Range<Ticks>>) -> Vec<Range<Ticks>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<Ticks>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// One clip of a move, for the automation it takes along: the track it was on and where, and
/// the track it goes to and where it starts there.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneMove {
    pub from: InstanceId,
    pub range: Range<Ticks>,
    pub to: InstanceId,
    pub start: Ticks,
}

/// What [`moved`] gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moved {
    /// The lanes of every track a moving clip leaves or lands on, changed or not.
    pub lanes: BTreeMap<InstanceId, Vec<AutomationLane>>,
    /// For each move, in order, the lanes it takes to where it lands, with ticks from its
    /// start: what a drag shows while it goes on. Empty for one that takes none.
    pub carried: Vec<Vec<AutomationLane>>,
}

/// The lanes after clips move, from `tracks` as they were before the move: each clip takes the
/// line under it, the place it leaves becomes straight between its edges, and its line
/// replaces what was where it lands. Every clip takes its line from the tracks as they were,
/// so clips that move together keep their own lines, also where one lands on where another
/// was. A clip that stays where it was changes nothing.
///
/// A drag calls it with the tracks of mouse down at every move, and draws what it carried.
pub fn moved(
    tracks: &BTreeMap<InstanceId, TrackState>,
    moves: &[LaneMove],
    travel: &Travel<'_>,
) -> Moved {
    let under = |step: &LaneMove| {
        let moves = step.from != step.to || step.range.start != step.start;
        let state = tracks.get(&step.from).filter(|_| moves)?;
        Some(Carried::under(
            &step.from,
            state,
            step.range.clone(),
            travel,
        ))
    };
    let each: Vec<Option<Carried>> = moves.iter().map(under).collect();
    let carried_lanes = each.iter().zip(moves).map(|(carried, step)| {
        let lanes = carried.iter().flat_map(|carried| {
            let taken = carried.lanes.iter();
            taken.filter(|lane| carried.takes(lane, &step.to)).cloned()
        });
        lanes.collect()
    });
    let carried_lanes = carried_lanes.collect();
    let carried: Vec<(&Carried, &LaneMove)> = each
        .iter()
        .zip(moves)
        .filter_map(|(carried, step)| Some((carried.as_ref()?, step)))
        .collect();
    let taken: Vec<(&Carried, &InstanceId)> = carried
        .iter()
        .map(|(carried, step)| (*carried, &step.to))
        .collect();
    let mut after = BTreeMap::new();
    let left: BTreeSet<&InstanceId> = carried.iter().map(|(_, step)| &step.from).collect();
    for track in left {
        if let Some(state) = working(&mut after, tracks, track) {
            clear(track, state, &taken, travel);
        }
    }
    for (carried, step) in &carried {
        if let Some(state) = working(&mut after, tracks, &step.to) {
            carried.place(&step.to, state, step.start, travel);
        }
    }
    let lanes = after.into_iter();
    let lanes = lanes.map(|(track, state)| (track, state.automation));
    Moved {
        lanes: lanes.collect(),
        carried: carried_lanes,
    }
}

/// The record of `track` as the move has made it so far, from `tracks` the first time.
fn working<'a>(
    after: &'a mut BTreeMap<InstanceId, TrackState>,
    tracks: &BTreeMap<InstanceId, TrackState>,
    track: &InstanceId,
) -> Option<&'a mut TrackState> {
    if !after.contains_key(track) {
        after.insert(track.clone(), tracks.get(track)?.clone());
    }
    after.get_mut(track)
}

/// The numbers of the devices of `project`, as their behaviours named them. `Copy`, so it
/// holds nothing to drop and a borrow of the project ends where it is last used.
pub fn travel_in(
    project: &Project,
) -> impl Fn(&InstanceId, &str) -> Option<AutomatedNumber> + Copy + '_ {
    |device, field| project.automation(device, field)
}

/// Writes the lanes of each track that differ from what its record has now, to a group of
/// changes. The rest of the record stays as it is now.
pub(crate) fn write(
    project: &Project,
    changes: &mut Changes,
    lanes: impl IntoIterator<Item = (InstanceId, Vec<AutomationLane>)>,
) {
    for (track, automation) in lanes {
        let Some(track) = project.resolve::<TrackState>(&track) else {
            continue;
        };
        let Some(state) = project.state(&track) else {
            continue;
        };
        if state.automation != automation {
            let state = TrackState {
                automation,
                ..state.clone()
            };
            changes.set(&track, state);
        }
    }
}

impl AutomationLane {
    /// The number this lane of `track`, whose record is `state`, moves. `None` for one the
    /// project does not know.
    pub fn number(
        &self,
        track: &InstanceId,
        state: &TrackState,
        travel: &Travel<'_>,
    ) -> Option<AutomatedNumber> {
        number(track, &Mix::of(state), self, travel)
    }

    /// Whether this lane and `other` move the same number.
    pub fn same_number(&self, other: &Self) -> bool {
        self.device == other.device && self.parameter == other.parameter
    }
}

/// Points of a lane as places on the travel of `range`, where its line is straight: what
/// plays, and what the timeline draws.
pub fn positions(points: &[Point<AutomationValue>], range: ValueRange) -> Vec<Point<f32>> {
    let points = points.iter().map(|point| Point {
        tick: point.tick,
        value: range.position(point.value.0),
    });
    points.collect()
}

/// The volume or the pan of the track that a lane moves. `None` for a lane of a device.
fn track_parameter(lane: &AutomationLane) -> Option<&'static Parameter<Mix>> {
    let parameters = Mixer::AUTOMATION.parameters().iter().copied();
    parameters
        .filter(|_| lane.device.is_none())
        .find(|parameter| parameter.field == lane.parameter)
}

/// The number a lane of `track` moves, whose mix is `mix`. `None` for one the project does
/// not know.
fn number(
    track: &InstanceId,
    mix: &Mix,
    lane: &AutomationLane,
    travel: &Travel<'_>,
) -> Option<AutomatedNumber> {
    match &lane.device {
        None => track_parameter(lane).map(|parameter| AutomatedNumber {
            range: ValueRange::of(parameter),
            record: Some((parameter.get)(mix)),
        }),
        Some(device) => travel(&track.child(device).ok()?, &lane.parameter),
    }
}

fn range_of(
    track: &InstanceId,
    mix: &Mix,
    lane: &AutomationLane,
    travel: &Travel<'_>,
) -> Option<ValueRange> {
    number(track, mix, lane, travel).map(|number| number.range)
}

/// A value of a lane with its place on the travel of its knob, so the line between two of them
/// is straight on the travel, as it plays. A number the project does not know has no range,
/// and its line is straight in its own units, with `-inf` as the lowest number.
#[derive(Copy, Clone, Debug, PartialEq)]
struct OnTravel {
    value: f32,
    place: f32,
    range: Option<ValueRange>,
}

impl OnTravel {
    fn new(value: f32, range: Option<ValueRange>) -> Self {
        let place = match range {
            Some(range) => range.position(value),
            None => value.max(f32::MIN),
        };
        Self {
            value,
            place,
            range,
        }
    }
}

impl LaneValue for OnTravel {
    /// Between two equal values the value itself, so an edge on a flat line is that value
    /// exactly and not one rounded on the travel and back.
    fn between(from: Self, to: Self, done: f64) -> Self {
        if from.value == to.value {
            return from;
        }
        let place = f32::between(from.place, to.place, done);
        let value = from.range.map_or(place, |range| range.exact(place));
        Self {
            value,
            place,
            range: from.range,
        }
    }
}

/// The points of a lane on the travel of its number, `range`.
fn on_travel(points: &[Point<AutomationValue>], range: Option<ValueRange>) -> Vec<Point<OnTravel>> {
    let points = points.iter().map(|point| Point {
        tick: point.tick,
        value: OnTravel::new(point.value.0, range),
    });
    points.collect()
}

fn values(points: impl IntoIterator<Item = Point<OnTravel>>) -> Vec<Point<AutomationValue>> {
    let points = points.into_iter().map(|point| Point {
        tick: point.tick,
        value: AutomationValue(point.value.value),
    });
    points.collect()
}

/// The line of a lane inside `range`, with ticks from its start, as [`cut`] gives it, and a
/// point on the first and on the last tick of the range, so it is whole on its own.
fn taken(points: &[Point<OnTravel>], range: Range<Ticks>) -> Vec<Point<OnTravel>> {
    let Some(last) = range.end.0.checked_sub(1).map(Ticks) else {
        return Vec::new();
    };
    let mut taken = cut(points, range.clone());
    let edge = |tick: Ticks| {
        value_at(points, tick).map(|value| Point {
            tick: tick.saturating_sub(range.start),
            value,
        })
    };
    if taken.first().map(|point| point.tick) != Some(Ticks(0))
        && let Some(edge) = edge(range.start)
    {
        taken.insert(0, edge);
    }
    if taken.last().map(|point| point.tick) != Some(last.saturating_sub(range.start))
        && let Some(edge) = edge(last)
    {
        taken.push(edge);
    }
    taken
}

/// The lane with `inside`, in project ticks inside `range`, in place of what it had there. The
/// points before and after stay, and the values just outside stay what they were, by a point
/// on each edge that has none. So with nothing inside, the place becomes a straight line
/// between its edges.
///
/// Only a point on an edge, just outside the place, is left out when it is on the line: an
/// edge this made, or one an earlier move made, so nudges do not pile them up. What lands
/// inside always stays, so a clip that lands where its points are on a flat line still has
/// them, and takes them along on its next move.
fn spliced(
    points: &[Point<OnTravel>],
    range: Range<Ticks>,
    inside: Vec<Point<OnTravel>>,
) -> Vec<Point<OnTravel>> {
    let edge = |tick: Ticks| {
        let taken = points.iter().any(|point| point.tick == tick);
        let value = value_at(points, tick).filter(|_| !taken);
        value.map(|value| Point { tick, value })
    };
    let before = range.start.0.checked_sub(1).map(Ticks);
    let mut spliced: Vec<_> = points
        .iter()
        .filter(|point| point.tick < range.start)
        .copied()
        .collect();
    spliced.extend(before.and_then(edge));
    spliced.extend(inside);
    spliced.extend(edge(range.end));
    spliced.extend(points.iter().filter(|point| point.tick >= range.end));
    for tick in [before, Some(range.end)] {
        drop_on_line(&mut spliced, tick);
    }
    spliced
}

/// Leaves out the point at `tick` when the line is the same without it: it is on the straight
/// line through its neighbours, within [`ON_THE_LINE`], or it is first or last and its one
/// neighbour has its value, so what is held past it stays exactly. A lane keeps one point.
fn drop_on_line(points: &mut Vec<Point<OnTravel>>, tick: Option<Ticks>) {
    let Some(index) = points.iter().position(|point| Some(point.tick) == tick) else {
        return;
    };
    let before = index.checked_sub(1).and_then(|before| points.get(before));
    let point = points[index];
    let same = match (before, points.get(index + 1)) {
        (Some(before), Some(after)) => {
            let line = before.towards(*after, point.tick).place;
            (line - point.value.place).abs() <= ON_THE_LINE
        }
        (Some(only), None) | (None, Some(only)) => only.value.value == point.value.value,
        (None, None) => false,
    };
    if same {
        points.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use sound_core::Scale;

    use super::*;

    const BAR: u64 = 3840;

    fn id(id: &str) -> InstanceId {
        InstanceId::new(id).unwrap()
    }

    fn lane(device: Option<&str>, parameter: &str, points: &[(u64, f32)]) -> AutomationLane {
        AutomationLane {
            device: device.map(str::to_string),
            parameter: parameter.to_string(),
            points: points
                .iter()
                .map(|&(tick, value)| Point {
                    tick: Ticks(tick),
                    value: AutomationValue(value),
                })
                .collect(),
        }
    }

    fn points(lane: &AutomationLane) -> Vec<(u64, f32)> {
        let points = lane.points.iter();
        points.map(|point| (point.tick.0, point.value.0)).collect()
    }

    fn ticks(lane: &AutomationLane) -> Vec<u64> {
        lane.points.iter().map(|point| point.tick.0).collect()
    }

    /// Where a lane of the first track stands at `tick`, on the line that plays.
    fn line(lane: &AutomationLane, tick: u64) -> f32 {
        let mix = Mix::of(&track(Vec::new()));
        let range = range_of(&id("a/one"), &mix, lane, &travel);
        let points = on_travel(&lane.points, range);
        value_at(&points, Ticks(tick)).unwrap().value
    }

    fn track(lanes: Vec<AutomationLane>) -> TrackState {
        let mut track = TrackState::new("Track", crate::Colour::Blue, 0);
        track.automation = lanes;
        track
    }

    /// A cutoff of 20 Hz to 20 kHz on a logarithmic knob, as the filter has, at 1 kHz in the
    /// record of `a/one/dark`. The other track has no such device.
    fn travel(device: &InstanceId, field: &str) -> Option<AutomatedNumber> {
        let known = *device == id("a/one/dark") && field == "cutoff_hz";
        known.then_some(AutomatedNumber {
            range: ValueRange::logarithmic(20., 20000.),
            record: Some(1000.),
        })
    }

    fn step(range: Range<u64>, to: &str, start: u64) -> LaneMove {
        LaneMove {
            from: id("a/one"),
            range: Ticks(range.start)..Ticks(range.end),
            to: id(to),
            start: Ticks(start),
        }
    }

    /// The lanes of the first track after clips on it move along it.
    fn moved_on(lanes: Vec<AutomationLane>, moves: &[LaneMove]) -> Vec<AutomationLane> {
        let tracks = BTreeMap::from([(id("a/one"), track(lanes.clone()))]);
        // A clip that stays where it was leaves its track out.
        let mut moved = moved(&tracks, moves, &travel).lanes;
        moved.remove(&id("a/one")).unwrap_or(lanes)
    }

    fn moved_once(
        lanes: Vec<AutomationLane>,
        range: Range<u64>,
        start: u64,
    ) -> Vec<AutomationLane> {
        moved_on(lanes, &[step(range, "a/one", start)])
    }

    /// The points inside go with the clip and replace those where it lands. The place it left
    /// is a straight line between its edges, and just outside both places nothing changes.
    #[test]
    fn a_clip_takes_the_points_under_it_and_they_replace_those_where_it_lands() {
        let pan = lane(
            None,
            "pan",
            &[
                (0, 0.),
                (BAR + 960, -1.),
                (2 * BAR - 1, -1.),
                (5 * BAR, 0.5),
            ],
        );
        // Bar 2 to bar 5, up to the point at bar 6.
        let moved = moved_once(vec![pan.clone()], BAR..2 * BAR, 4 * BAR);
        let [moved] = &moved[..] else {
            panic!("one lane");
        };
        // Inside the place it landed: what was under the clip.
        for tick in [0, 960, 2000, BAR - 1] {
            assert_eq!(line(moved, 4 * BAR + tick), line(&pan, BAR + tick));
        }
        // Just outside both places, as it was.
        for tick in [BAR - 1, 2 * BAR, 4 * BAR - 1, 5 * BAR, 6 * BAR] {
            let (now, before) = (line(moved, tick), line(&pan, tick));
            assert!((now - before).abs() < 1e-6, "at {tick}: {now} for {before}");
        }
        // Where it was: a straight line from its left edge to its right edge.
        let (left, right) = (line(&pan, BAR - 1), line(&pan, 2 * BAR));
        let middle = line(moved, BAR + BAR / 2);
        assert!((middle - (left + right) / 2.).abs() < 1e-3);
        AutomationLane::check_all(std::slice::from_ref(moved)).unwrap();
    }

    /// An edge in the middle of a cutoff sweep sits on the line that plays, which is straight
    /// in octaves, not in hertz: halfway from 100 Hz to 400 Hz is 200 Hz.
    #[test]
    fn an_edge_is_on_the_line_of_the_travel_and_kept_points_keep_their_values() {
        let sweep = lane(Some("dark"), "cutoff_hz", &[(0, 100.), (2 * BAR, 400.)]);
        let tracks = track(vec![sweep]);
        let carried = Carried::under(&id("a/one"), &tracks, Ticks(BAR)..Ticks(3 * BAR), &travel);
        let [carried] = carried.lanes() else {
            panic!("one lane");
        };
        let (start, point) = (carried.points[0], carried.points[1]);
        assert_eq!(start.tick, Ticks(0));
        assert!((start.value.0 - 200.).abs() < 0.01, "{}", start.value.0);
        // The point inside, as it was saved, not a round trip over the travel.
        assert_eq!((point.tick.0, point.value.0), (BAR, 400.));
        assert_eq!(carried.points.len(), 3);
    }

    #[test]
    fn a_move_onto_where_the_clip_was_keeps_its_own_line() {
        let gain = lane(None, "gain_db", &[(0, -12.), (BAR, 0.), (2 * BAR, -6.)]);
        // Half a bar on: the clip lands over most of where it was.
        let moved = moved_once(vec![gain.clone()], 0..2 * BAR, BAR / 2);
        let [moved] = &moved[..] else {
            panic!("one lane");
        };
        for tick in [0, 960, BAR, 2 * BAR - 1] {
            assert_eq!(line(moved, BAR / 2 + tick), line(&gain, tick), "at {tick}");
        }
        // Before where it lands, the left part of where it was: held at the edge after it.
        assert_eq!(line(moved, 0), line(&gain, 2 * BAR));
        assert_eq!(moved.points.last().unwrap().value.0, -6.);
    }

    /// A clip over a stretch with no points takes nothing: the sweep where it lands stays,
    /// and a move that ends where it began changes nothing at all.
    #[test]
    fn a_clip_over_no_points_takes_nothing() {
        let held = lane(None, "gain_db", &[(0, -6.), (BAR, -3.)]);
        let sweep = lane(None, "pan", &[(0, -1.), (4 * BAR, 1.), (16 * BAR, -1.)]);
        let lanes = vec![held, sweep];
        let moved = moved_once(lanes.clone(), BAR + 1..4 * BAR, 8 * BAR);
        assert_eq!(moved, lanes);
        assert_eq!(moved_once(lanes.clone(), 0..BAR, 0), lanes);
    }

    #[test]
    fn a_track_with_no_lanes_has_nothing_to_move() {
        assert_eq!(moved_once(Vec::new(), 0..BAR, 2 * BAR), []);
    }

    /// Silence is the bottom of the fader: a fade from it is cut on the fader, and the points
    /// at silence stay silence.
    #[test]
    fn a_fade_from_silence_moves_with_its_minus_infinity() {
        let fade = lane(
            None,
            "gain_db",
            &[
                (0, f32::NEG_INFINITY),
                (BAR, f32::NEG_INFINITY),
                (2 * BAR, 0.),
            ],
        );
        let moved = moved_once(vec![fade], BAR..3 * BAR, 4 * BAR);
        let [moved] = &moved[..] else {
            panic!("one lane");
        };
        let at = |tick: u64| line(moved, tick);
        assert_eq!(at(4 * BAR), f32::NEG_INFINITY);
        assert_eq!(at(5 * BAR), 0.);
        // Halfway up the fade, as the fader goes there, not a number that is not one.
        let fader = ValueRange {
            min: f32::NEG_INFINITY,
            max: 6.,
            scale: Scale::Fader,
        };
        let halfway = at(4 * BAR + BAR / 2);
        assert!((halfway - fader.exact(fader.position(0.) / 2.)).abs() < 0.01);
        assert!(moved.points.iter().all(|point| !point.value.0.is_nan()));
        // Where the fade was, silence holds until the edge, then the line goes on to the end.
        assert_eq!(at(BAR - 1), f32::NEG_INFINITY);
    }

    /// Several clips move together, each with its own line, also where one lands on where the
    /// other was.
    #[test]
    fn clips_that_move_together_each_take_their_own_line() {
        let steps = lane(None, "pan", &[(0, -1.), (BAR - 1, -1.), (BAR, 1.)]);
        // Bar 1 to bar 2 and bar 2 to bar 3.
        let moves = [
            step(0..BAR, "a/one", BAR),
            step(BAR..2 * BAR, "a/one", 2 * BAR),
        ];
        let moved = moved_on(vec![steps], &moves);
        let at = |tick: u64| line(&moved[0], tick);
        assert_eq!((at(BAR), at(2 * BAR - 1)), (-1., -1.));
        assert_eq!((at(2 * BAR), at(3 * BAR - 1)), (1., 1.));
    }

    /// Clips next to each other, or over each other, leave one place behind: one straight line
    /// from its left edge to its right edge, with no value from inside either clip.
    #[test]
    fn clips_that_touch_or_overlap_leave_one_straight_line() {
        let pan = lane(
            None,
            "pan",
            &[
                (0, 0.),
                (BAR + 960, 1.),
                (2 * BAR + 960, -1.),
                (4 * BAR, 0.),
            ],
        );
        let (left, right) = (line(&pan, BAR - 1), line(&pan, 3 * BAR));
        let next_to = [
            step(BAR..2 * BAR, "a/one", 5 * BAR),
            step(2 * BAR..3 * BAR, "a/one", 6 * BAR),
        ];
        let over = [
            step(BAR..2 * BAR + BAR / 2, "a/one", 5 * BAR),
            step(2 * BAR..3 * BAR, "a/one", 6 * BAR),
        ];
        for moves in [&next_to[..], &over[..]] {
            let moved = moved_on(vec![pan.clone()], moves);
            let left_behind = ticks(&moved[0]);
            let left_behind = left_behind
                .iter()
                .filter(|tick| (BAR..3 * BAR).contains(tick));
            assert_eq!(left_behind.count(), 0, "{:?}", points(&moved[0]));
            let middle = line(&moved[0], 2 * BAR);
            assert!((middle - (left + right) / 2.).abs() < 1e-3, "{middle}");
        }

        // A cut of both does the same.
        let mut state = track(vec![pan]);
        let one = id("a/one");
        let a = Carried::under(&one, &state, Ticks(BAR)..Ticks(2 * BAR), &travel);
        let b = Carried::under(&one, &state, Ticks(2 * BAR)..Ticks(3 * BAR), &travel);
        clear(&one, &mut state, &[(&a, &one), (&b, &one)], &travel);
        assert_eq!(ticks(&state.automation[0]), [0, BAR - 1, 3 * BAR, 4 * BAR]);
    }

    /// Nudges of a clip on a ramp, one step at a time, leave no points behind: every edge a
    /// nudge leaves is on the line.
    #[test]
    fn nudges_on_a_ramp_do_not_pile_up_points() {
        let ramp = lane(None, "pan", &[(0, -1.), (BAR + 1920, -0.25), (4 * BAR, 1.)]);
        let mut lanes = vec![ramp];
        let mut counts = Vec::new();
        for nudge in 0..10 {
            let start = BAR + 240 * nudge;
            lanes = moved_once(lanes, start..start + BAR, start + 240);
            counts.push(lanes[0].points.len());
        }
        assert!(counts.iter().all(|count| *count == counts[0]), "{counts:?}");
        assert!(counts[0] <= 7, "{counts:?}");
    }

    /// The volume goes along to another track, which had none and holds its own volume around
    /// it. The filter stays: it is a device of the first track.
    #[test]
    fn to_another_track_the_volume_goes_along_and_a_device_lane_stays() {
        let gain = lane(None, "gain_db", &[(BAR, -12.), (2 * BAR - 1, 0.)]);
        let sweep = lane(Some("dark"), "cutoff_hz", &[(BAR, 100.), (2 * BAR, 400.)]);
        let mut other = track(Vec::new());
        other.gain_db = -3.;
        let tracks = BTreeMap::from([
            (id("a/one"), track(vec![gain, sweep.clone()])),
            (id("a/two"), other),
        ]);
        let moved = moved(&tracks, &[step(BAR..2 * BAR, "a/two", 4 * BAR)], &travel).lanes;
        let one = &moved[&id("a/one")];
        assert_eq!(one.len(), 2);
        assert_eq!(one[1], sweep);
        let [two] = &moved[&id("a/two")][..] else {
            panic!("one lane");
        };
        assert_eq!(two.device, None);
        assert_eq!(
            points(two),
            [
                (4 * BAR - 1, -3.),
                (4 * BAR, -12.),
                (5 * BAR - 1, 0.),
                (5 * BAR, -3.)
            ]
        );
    }

    /// A lane whose points all hold one value holds its knob still: a clip over it takes
    /// nothing along, to its own track or another, and says it carries nothing. A lane just
    /// added holds the record value in one point at tick 0.
    #[test]
    fn a_lane_that_holds_one_value_carries_nothing() {
        let added = lane(None, "pan", &[(0, 0.5)]);
        let held = lane(None, "gain_db", &[(0, -6.), (BAR, -6.), (2 * BAR, -6.)]);
        let tracks = BTreeMap::from([
            (id("a/one"), track(vec![added, held])),
            (id("a/two"), track(Vec::new())),
        ]);
        for to in ["a/one", "a/two"] {
            let moved = moved(&tracks, &[step(0..2 * BAR, to, 4 * BAR)], &travel);
            assert_eq!(moved.carried, [Vec::<AutomationLane>::new()], "to {to}");
            for (track, lanes) in &moved.lanes {
                assert_eq!(lanes, &tracks[track].automation, "to {to}");
            }
        }
        // One point that moves away from the rest is a line, and goes along.
        let dip = lane(None, "gain_db", &[(0, -6.), (BAR, -12.), (2 * BAR, -6.)]);
        let tracks = BTreeMap::from([(id("a/one"), track(vec![dip]))]);
        let moved = moved(&tracks, &[step(0..2 * BAR, "a/one", 4 * BAR)], &travel);
        let [carried] = &moved.carried[..] else {
            panic!("one move");
        };
        assert_eq!(carried.len(), 1);
    }

    /// A paste puts the line down over what was there, and a cut leaves a straight line.
    #[test]
    fn a_cut_leaves_a_straight_line_and_a_paste_replaces() {
        let pan = lane(None, "pan", &[(0, -1.), (BAR / 2, 1.), (BAR, -1.)]);
        let mut state = track(vec![pan]);
        let one = id("a/one");
        let carried = Carried::under(&one, &state, Ticks(0)..Ticks(BAR), &travel);
        clear(&one, &mut state, &[(&carried, &one)], &travel);
        assert_eq!(points(&state.automation[0]), [(BAR, -1.)]);
        carried.place(&one, &mut state, Ticks(2 * BAR), &travel);
        let placed = &state.automation[0];
        // What the clip carries stays, its edge on its last tick too.
        assert_eq!(
            ticks(placed),
            [BAR, 2 * BAR, 2 * BAR + BAR / 2, 3 * BAR - 1, 3 * BAR]
        );
        for tick in [0, BAR / 4, BAR / 2, BAR - 1] {
            let (now, carried) = (
                line(placed, 2 * BAR + tick),
                line(&carried.lanes()[0], tick),
            );
            assert!(
                (now - carried).abs() < 1e-3,
                "at {tick}: {now} for {carried}"
            );
        }
        assert_eq!(line(placed, 3 * BAR), -1.);
    }

    /// A paste on the track it came from, after its device lane was taken out, makes the lane
    /// again around the record value of the device. Without the device it makes none.
    #[test]
    fn a_paste_makes_a_device_lane_again_while_the_device_has_the_number() {
        let sweep = lane(Some("dark"), "cutoff_hz", &[(0, 200.), (BAR / 2, 2000.)]);
        let one = id("a/one");
        let mut state = track(vec![sweep]);
        let carried = Carried::under(&one, &state, Ticks(0)..Ticks(BAR), &travel);
        state.automation.clear();
        carried.place(&one, &mut state, Ticks(2 * BAR), &travel);
        assert_eq!(
            points(&state.automation[0]),
            [
                (2 * BAR - 1, 1000.),
                (2 * BAR, 200.),
                (2 * BAR + BAR / 2, 2000.),
                (3 * BAR - 1, 2000.),
                (3 * BAR, 1000.)
            ]
        );

        let mut gone = track(Vec::new());
        let no_device = |_: &InstanceId, _: &str| None;
        carried.place(&one, &mut gone, Ticks(2 * BAR), &no_device);
        assert_eq!(gone.automation, []);
    }
}
