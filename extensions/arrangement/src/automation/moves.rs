//! What a clip takes along of the automation of its track when it moves, is cut or is copied:
//! the line of each lane under it, from its start to its end. Pure, apart from the two last
//! functions: the timeline writes what these give, and a drag can draw it before the drop.
//!
//! The line under a clip replaces the line where it lands. The values just outside the place it
//! leaves and the place it lands stay what they were, by a point on each edge, so nothing jumps
//! there, and the place it leaves becomes a straight line between its edges. An edge that the
//! line would pass anyway, flat on both sides, is left out, so moves do not pile up points.
//!
//! A line is cut on the travel of its knob, where it is straight, so a point on an edge sits on
//! the line that plays. A point that is cut out and put back keeps its value as it was saved.
//!
//! The volume and the pan go along to any track. A lane of a device stays with its track: the
//! device belongs to the track, and another track may have no such device, or one of another
//! tool under the same name.

use std::collections::BTreeMap;
use std::ops::Range;

use sound_core::{Changes, InstanceId, Parameter, Project, Ticks, ValueRange};
use sound_notes::{LaneValue, Point, cut, value_at};

use super::{AutomationLane, AutomationValue};
use crate::TrackState;
use crate::mixer::{Mix, Mixer};

/// The range of a number of a device, by the id of the device and the field of the number:
/// what the project knows now, see [`travel_in`]. `None` for a number it does not know.
pub type Travel<'a> = dyn Fn(&InstanceId, &str) -> Option<ValueRange> + 'a;

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
    /// The line of each lane of `state`, the record of `track`, inside `range`.
    pub fn under(
        track: &InstanceId,
        state: &TrackState,
        range: Range<Ticks>,
        travel: &Travel<'_>,
    ) -> Self {
        let lanes = state.automation.iter().filter_map(|lane| {
            let points = taken(&on_travel(track, lane, travel), range.clone());
            let points = (!points.is_empty()).then(|| values(points))?;
            Some(AutomationLane {
                device: lane.device.clone(),
                parameter: lane.parameter.clone(),
                points,
            })
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

    /// Whether `lane` goes along to the track `to`: the volume and the pan go anywhere, a lane
    /// of a device only to its own track.
    fn goes_to(&self, lane: &AutomationLane, to: &InstanceId) -> bool {
        lane.device.is_none() || self.track == *to
    }

    /// Only the lanes that go along to `to`, so a move to another track leaves the lanes of
    /// the devices where they are.
    fn going_to(mut self, to: &InstanceId) -> Self {
        let lanes = std::mem::take(&mut self.lanes);
        self.lanes = lanes
            .into_iter()
            .filter(|lane| self.goes_to(lane, to))
            .collect();
        self
    }

    /// `state`, the record of the track the clip was on, without what the clip took: each of
    /// those lanes becomes a straight line between the edges of where the clip was. What a
    /// move and a cut leave behind.
    pub fn clear(&self, state: &mut TrackState, travel: &Travel<'_>) {
        for lane in &mut state.automation {
            if self.lanes.iter().any(|carried| same_number(carried, lane)) {
                let points = on_travel(&self.track, lane, travel);
                lane.points = values(spliced(&points, self.range.clone(), Vec::new()));
            }
        }
    }

    /// `state`, the record of `to`, with the line put down from `start`, over what each lane
    /// had there. A volume or a pan that the track does not move yet gets a lane that holds
    /// its record value around what lands. A lane of a device goes only to the track it came
    /// from, and only while that track has the lane.
    pub fn place(
        &self,
        to: &InstanceId,
        state: &mut TrackState,
        start: Ticks,
        travel: &Travel<'_>,
    ) {
        let range = start..start + self.length();
        let mix = Mix::of(state);
        for carried in self.lanes.iter().filter(|lane| self.goes_to(lane, to)) {
            let inside = on_travel(to, carried, travel)
                .into_iter()
                .map(|point| Point {
                    tick: start + point.tick,
                    value: point.value,
                });
            let inside: Vec<_> = inside.collect();
            let mut lanes = state.automation.iter_mut();
            match lanes.find(|lane| same_number(carried, lane)) {
                Some(lane) => {
                    let points = on_travel(to, lane, travel);
                    lane.points = values(spliced(&points, range.clone(), inside));
                }
                None => {
                    let Some(parameter) = track_parameter(carried) else {
                        continue;
                    };
                    let record = (parameter.get)(&mix);
                    let around = [Point {
                        tick: range.end,
                        value: OnTravel::new(record, Some(ValueRange::of(parameter))),
                    }];
                    state.automation.push(AutomationLane {
                        device: None,
                        parameter: carried.parameter.clone(),
                        points: values(spliced(&around, range.clone(), inside)),
                    });
                }
            }
        }
    }
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

/// The lanes after clips move, from `tracks` as they were before the move: each clip takes the
/// line under it, the place it leaves becomes straight between its edges, and its line
/// replaces what was where it lands. Every clip takes its line from the tracks as they were,
/// so clips that move together keep their own lines, also where one lands on where another
/// was. A clip that stays where it was changes nothing.
///
/// Gives the lanes of every track a moving clip leaves or lands on, changed or not. A drag
/// calls it with the tracks of mouse down at every move, and so can a preview of the drop.
pub fn moved(
    tracks: &BTreeMap<InstanceId, TrackState>,
    moves: &[LaneMove],
    travel: &Travel<'_>,
) -> BTreeMap<InstanceId, Vec<AutomationLane>> {
    let moving = moves
        .iter()
        .filter(|step| step.from != step.to || step.range.start != step.start);
    let carried: Vec<(Carried, &LaneMove)> = moving
        .filter_map(|step| {
            let state = tracks.get(&step.from)?;
            let carried = Carried::under(&step.from, state, step.range.clone(), travel);
            Some((carried.going_to(&step.to), step))
        })
        .collect();
    let mut after = BTreeMap::new();
    for (carried, step) in &carried {
        if let Some(state) = working(&mut after, tracks, &step.from) {
            carried.clear(state, travel);
        }
    }
    for (carried, step) in &carried {
        if let Some(state) = working(&mut after, tracks, &step.to) {
            carried.place(&step.to, state, step.start, travel);
        }
    }
    let lanes = after.into_iter();
    lanes
        .map(|(track, state)| (track, state.automation))
        .collect()
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

/// The travel of the numbers of the devices of `project`, as their behaviours named them.
/// `Copy`, so it holds nothing to drop and a borrow of the project ends where it is last used.
pub fn travel_in(
    project: &Project,
) -> impl Fn(&InstanceId, &str) -> Option<ValueRange> + Copy + '_ {
    |device, field| {
        let numbers = project.automation(device)?;
        let number = numbers.iter().find(|number| number.field == field)?;
        Some(number.range)
    }
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

/// Whether two lanes move the same number.
fn same_number(a: &AutomationLane, b: &AutomationLane) -> bool {
    a.device == b.device && a.parameter == b.parameter
}

/// The volume or the pan of the track that a lane moves. `None` for a lane of a device.
fn track_parameter(lane: &AutomationLane) -> Option<&'static Parameter<Mix>> {
    let parameters = Mixer::AUTOMATION.parameters().iter().copied();
    parameters
        .filter(|_| lane.device.is_none())
        .find(|parameter| parameter.field == lane.parameter)
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

/// The points of a lane of `track` on the travel of its number.
fn on_travel(
    track: &InstanceId,
    lane: &AutomationLane,
    travel: &Travel<'_>,
) -> Vec<Point<OnTravel>> {
    let range = match &lane.device {
        None => track_parameter(lane).map(ValueRange::of),
        Some(device) => track
            .child(device)
            .ok()
            .and_then(|device| travel(&device, &lane.parameter)),
    };
    let points = lane.points.iter().map(|point| Point {
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
    let last = range.end.0.checked_sub(1).map(Ticks);
    for tick in [before, Some(range.start), last, Some(range.end)] {
        drop_flat(&mut spliced, tick);
    }
    spliced
}

/// Leaves out the point at `tick` when the line is the same without it: its neighbours have its
/// value, or it is first or last and its one neighbour has it. A lane keeps one point.
fn drop_flat(points: &mut Vec<Point<OnTravel>>, tick: Option<Ticks>) {
    let Some(index) = points.iter().position(|point| Some(point.tick) == tick) else {
        return;
    };
    let value = points[index].value.value;
    let same = |at: Option<usize>| {
        let point = at.and_then(|at| points.get(at));
        point.is_none_or(|point| point.value.value == value)
    };
    if points.len() > 1 && same(index.checked_sub(1)) && same(Some(index + 1)) {
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

    /// Where a lane of the first track stands at `tick`, on the line that plays.
    fn line(lane: &AutomationLane, tick: u64) -> f32 {
        let points = on_travel(&id("a/one"), lane, &travel);
        value_at(&points, Ticks(tick)).unwrap().value
    }

    fn track(lanes: Vec<AutomationLane>) -> TrackState {
        let mut track = TrackState::new("Track", crate::Colour::Blue, 0);
        track.automation = lanes;
        track
    }

    /// A cutoff of 20 Hz to 20 kHz on a logarithmic knob, as the filter has.
    fn travel(_: &InstanceId, field: &str) -> Option<ValueRange> {
        (field == "cutoff_hz").then_some(ValueRange::logarithmic(20., 20000.))
    }

    fn moved_on(lanes: Vec<AutomationLane>, range: Range<u64>, start: u64) -> Vec<AutomationLane> {
        let tracks = BTreeMap::from([(id("a/one"), track(lanes.clone()))]);
        let step = LaneMove {
            from: id("a/one"),
            range: Ticks(range.start)..Ticks(range.end),
            to: id("a/one"),
            start: Ticks(start),
        };
        // A clip that stays where it was leaves its track out.
        let mut moved = moved(&tracks, &[step], &travel);
        moved.remove(&id("a/one")).unwrap_or(lanes)
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
        let moved = moved_on(vec![pan.clone()], BAR..2 * BAR, 4 * BAR);
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
        let moved = moved_on(vec![gain.clone()], 0..2 * BAR, BAR / 2);
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

    /// A track whose lanes are flat around the clip gains no points from a move.
    #[test]
    fn a_flat_line_gets_no_points_from_a_move() {
        let flat = lane(None, "gain_db", &[(0, -6.), (BAR, -3.)]);
        let moved = moved_on(vec![flat.clone()], 4 * BAR..5 * BAR, 8 * BAR);
        assert_eq!(moved, std::slice::from_ref(&flat));
        // And a move that ends where it began changes nothing at all.
        let back = moved_on(vec![flat.clone()], 4 * BAR..5 * BAR, 4 * BAR);
        assert_eq!(back, [flat]);
    }

    #[test]
    fn a_track_with_no_lanes_has_nothing_to_move() {
        assert_eq!(moved_on(Vec::new(), 0..BAR, 2 * BAR), []);
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
        let moved = moved_on(vec![fade], BAR..3 * BAR, 4 * BAR);
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
        let tracks = BTreeMap::from([(id("a/one"), track(vec![steps]))]);
        let step = |from: u64, to: u64| LaneMove {
            from: id("a/one"),
            range: Ticks(from)..Ticks(from + BAR),
            to: id("a/one"),
            start: Ticks(to),
        };
        // Bar 1 to bar 2 and bar 2 to bar 3.
        let moved = moved(&tracks, &[step(0, BAR), step(BAR, 2 * BAR)], &travel);
        let lane = &moved[&id("a/one")][0];
        let at = |tick: u64| line(lane, tick);
        assert_eq!((at(BAR), at(2 * BAR - 1)), (-1., -1.));
        assert_eq!((at(2 * BAR), at(3 * BAR - 1)), (1., 1.));
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
        let step = LaneMove {
            from: id("a/one"),
            range: Ticks(BAR)..Ticks(2 * BAR),
            to: id("a/two"),
            start: Ticks(4 * BAR),
        };
        let moved = moved(&tracks, &[step], &travel);
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

    /// A paste puts the line down over what was there, and a cut leaves a straight line.
    #[test]
    fn a_cut_leaves_a_straight_line_and_a_paste_replaces() {
        let pan = lane(None, "pan", &[(0, -1.), (BAR / 2, 1.), (BAR, -1.)]);
        let mut state = track(vec![pan]);
        let carried = Carried::under(&id("a/one"), &state, Ticks(0)..Ticks(BAR), &travel);
        carried.clear(&mut state, &travel);
        assert_eq!(points(&state.automation[0]), [(BAR, -1.)]);
        carried.place(&id("a/one"), &mut state, Ticks(2 * BAR), &travel);
        let placed = &state.automation[0];
        let ticks: Vec<u64> = points(placed).iter().map(|(tick, _)| *tick).collect();
        assert_eq!(
            ticks,
            [BAR, 2 * BAR, 2 * BAR + BAR / 2, 3 * BAR - 1, 3 * BAR]
        );
        for tick in [0, BAR / 4, BAR / 2, BAR - 1] {
            assert_eq!(
                line(placed, 2 * BAR + tick),
                line(&carried.lanes()[0], tick)
            );
        }
        assert_eq!(line(placed, 3 * BAR), -1.);
    }
}
