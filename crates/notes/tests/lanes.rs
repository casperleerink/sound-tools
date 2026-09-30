//! The expression lanes of a clip: the saved form, the rules, the line between points, and
//! the clip a take with the wheels becomes.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use sound_core::{State, Ticks};
use sound_notes::{
    Amount, Bend, Clip, Expression, Length, NoteEvent, Point, RawEvent, RawTake, thinned, value_at,
};

fn bend(tick: u64, value: i16) -> Point<Bend> {
    Point {
        tick: Ticks(tick),
        value: Bend::new(value).unwrap(),
    }
}

fn amount(tick: u64, value: u8) -> Point<Amount> {
    Point {
        tick: Ticks(tick),
        value: Amount::new(value).unwrap(),
    }
}

fn load(line: &str) -> Result<Clip, String> {
    let clip: Clip = serde_json::from_str(line).map_err(|error| error.to_string())?;
    clip.validate()?;
    Ok(clip)
}

#[test]
fn a_clip_with_lanes_round_trips_and_one_without_leaves_them_out() {
    let line = r#"{"start":0,"length":3840,"notes":[],"bend":[{"tick":0,"value":-8192},{"tick":960,"value":8191}],"mod_wheel":[{"tick":0,"value":64}],"pressure":[{"tick":480,"value":127}]}"#;
    let clip = load(line).unwrap();
    assert_eq!(clip.bend, [bend(0, -8192), bend(960, 8191)]);
    assert_eq!(clip.mod_wheel, [amount(0, 64)]);
    assert_eq!(clip.pressure, [amount(480, 127)]);
    assert_eq!(serde_json::to_string(&clip).unwrap(), line);

    let plain = r#"{"start":0,"length":3840,"notes":[]}"#;
    let clip = load(plain).unwrap();
    assert!(clip.bend.is_empty() && clip.mod_wheel.is_empty() && clip.pressure.is_empty());
    assert_eq!(serde_json::to_string(&clip).unwrap(), plain);
}

#[test]
fn a_lane_that_breaks_the_rules_does_not_load_and_says_why() {
    let error = |lanes: &str| {
        load(&format!(r#"{{"start":0,"length":960,"notes":[],{lanes}}}"#)).unwrap_err()
    };
    assert!(
        error(r#""bend":[{"tick":0,"value":8192}]"#)
            .starts_with("bend must be from -8192 to 8191, not 8192")
    );
    assert!(
        error(r#""pressure":[{"tick":0,"value":-1}]"#)
            .starts_with("amount must be from 0 to 127, not -1")
    );
    assert!(error(r#""bend":[{"tick":0,"value":0.5}]"#).contains("invalid type"));
    assert!(error(r#""bend":[{"start":0,"value":0}]"#).contains("unknown field `start`"));
    assert_eq!(
        error(r#""mod_wheel":[{"tick":0,"value":1},{"tick":960,"value":2}]"#),
        "mod_wheel[1].tick must be less than the clip length 960, not 960. A point counts from the start of its clip, not from the start of the project"
    );
    assert_eq!(
        error(r#""bend":[{"tick":480,"value":1},{"tick":480,"value":2}]"#),
        "bend[1].tick must be after the tick of the point before it, 480, not 480. The points of a lane are in tick order, one per tick"
    );
    assert!(
        error(r#""pressure":[{"tick":480,"value":1},{"tick":240,"value":2}]"#)
            .starts_with("pressure[1].tick must be after")
    );
}

/// Before the first point the lane holds its value, between two points it moves in a straight
/// line, and after the last it holds.
#[test]
fn a_lane_moves_in_straight_lines_between_its_points() {
    let lane = [bend(480, 0), bend(1440, 8000), bend(1920, -8000)];
    let at = |tick| value_at(&lane, Ticks(tick)).unwrap().value();
    assert_eq!(at(0), 0);
    assert_eq!(at(480), 0);
    assert_eq!(at(720), 2000);
    assert_eq!(at(960), 4000);
    assert_eq!(at(1440), 8000);
    assert_eq!(at(1680), 0);
    assert_eq!(at(100_000), -8000);
    assert_eq!(value_at::<Bend>(&[], Ticks(0)), None);
}

/// A line that rises by one over three ticks rounds to the nearest value on every tick.
#[test]
fn a_value_between_two_steps_rounds_to_the_nearest() {
    let lane = [amount(0, 0), amount(3, 1)];
    let values: Vec<u8> = (0..4)
        .map(|tick| value_at(&lane, Ticks(tick)).unwrap().value())
        .collect();
    assert_eq!(values, [0, 0, 1, 1]);
}

#[test]
fn thinning_drops_the_points_on_a_straight_line() {
    let ramp: Vec<Point<Amount>> = (0..=127)
        .map(|value| amount(value * 10, value as u8))
        .collect();
    assert_eq!(thinned(&ramp), [amount(0, 0), amount(1270, 127)]);
    // Up and down again keeps the top.
    let mut peak = ramp;
    peak.extend(
        (0..127)
            .rev()
            .map(|value| amount(2540 - value * 10, value as u8)),
    );
    assert_eq!(
        thinned(&peak),
        [amount(0, 0), amount(1270, 127), amount(2540, 0)]
    );
    assert_eq!(thinned::<Amount>(&[]), []);
    assert_eq!(thinned(&[amount(5, 3)]), [amount(5, 3)]);
}

/// A curve keeps enough points that the line through them passes every recorded point within
/// one step: for the bend that is one step of the coarse half of MIDI's bend, 128.
#[test]
fn a_thinned_curve_stays_within_one_step_of_every_recorded_point() {
    let curve: Vec<Point<Bend>> = (0..2000)
        .map(|tick| {
            let phase = tick as f64 / 2000.0 * std::f64::consts::TAU;
            bend(tick, (phase.sin() * 8000.0) as i16)
        })
        .collect();
    let kept = thinned(&curve);
    assert!(kept.len() < curve.len() / 10, "{} points", kept.len());
    for point in &curve {
        let on_line = value_at(&kept, point.tick).unwrap().value();
        assert!(
            (on_line - point.value.value()).abs() <= 128,
            "at {:?}: {on_line} and {:?}",
            point.tick,
            point.value
        );
    }
}

/// A shorter clip cannot hold a point past its end. The lane keeps the value it had at the new
/// end, so what is left moves as it did.
#[test]
fn a_shorter_clip_keeps_the_value_its_lane_had_at_the_new_end() {
    let mut clip = Clip::new(Ticks(0), Length::new(Ticks(3840)).unwrap(), Vec::new());
    clip.bend = vec![bend(0, 0), bend(2000, 8000)];
    clip.mod_wheel = vec![amount(0, 100), amount(1000, 0)];
    clip.pressure = vec![amount(3000, 50)];
    clip.set_length(Length::new(Ticks(1001)).unwrap());
    assert_eq!(clip.bend, [bend(0, 0), bend(1000, 4000)]);
    assert_eq!(clip.mod_wheel, [amount(0, 100), amount(1000, 0)]);
    assert_eq!(clip.pressure, [amount(1000, 50)]);
    assert_eq!(clip.validate(), Ok(()));
}

#[test]
fn an_expression_moves_to_another_with_one_event_per_value_that_differs() {
    let bent = Expression {
        bend: Bend::new(100).unwrap(),
        ..Expression::REST
    };
    let moves: Vec<NoteEvent> = Expression::REST.moves_to(bent).collect();
    assert_eq!(moves, [NoteEvent::Bend(Bend::new(100).unwrap())]);
    assert_eq!(bent.moves_to(bent).count(), 0);
    let all: Vec<NoteEvent> = bent.moves_to(Expression::REST).collect();
    assert_eq!(all, [NoteEvent::Bend(Bend::MIDDLE)]);
    let wheels = Expression {
        bend: Bend::MIDDLE,
        mod_wheel: Amount::new(1).unwrap(),
        pressure: Amount::new(2).unwrap(),
    };
    assert_eq!(bent.moves_to(wheels).count(), 3);
}

fn wheel(kind: &str, time_us: u64, value: i64) -> RawEvent {
    let line = format!(
        r#"{{"kind":"{kind}","time_us":{time_us},"sounded_us":{time_us},"value":{value}}}"#
    );
    serde_json::from_str(&line).unwrap()
}

/// The wheels of a take are in its file one message per line, like a note, and read back.
#[test]
fn a_take_holds_the_wheels_one_message_per_line() {
    let take = RawTake {
        end_us: 1_000_000,
        events: vec![
            wheel("bend", 0, -8192),
            wheel("mod_wheel", 10, 64),
            wheel("pressure", 20, 127),
        ],
        ..RawTake::default()
    };
    let json = take.json();
    assert!(
        json.contains(r#"{"kind":"bend","time_us":0,"sounded_us":0,"value":-8192},"#),
        "{json}"
    );
    assert!(json.contains(r#""kind":"mod_wheel""#), "{json}");
    let read: RawTake = serde_json::from_str(&json).unwrap();
    assert_eq!(read, take);
    assert_eq!(read.validate(), Ok(()));
}

/// A take's wheels become thinned lanes: one point per tick, the last move of the tick, and a
/// straight move is its two ends.
#[test]
fn the_wheels_of_a_take_become_thinned_lanes_of_its_clip() {
    // At the default 120 bpm a tick is 520.8 microseconds; these land on whole ticks.
    let tick_us = |tick: u64| tick * 500_000 / 960;
    let mut events = vec![
        wheel("pressure", tick_us(0), 10),
        // Two moves on one tick: the last one counts.
        wheel("pressure", tick_us(0) + 1, 20),
        wheel("mod_wheel", tick_us(480), 90),
    ];
    events.extend(
        (0..=64).map(|step: u64| wheel("bend", tick_us(960 + step * 10), step as i64 * 128)),
    );
    events.push(RawEvent::On {
        time_us: tick_us(960),
        sounded_us: tick_us(960),
        pitch: 60,
        velocity: 100,
    });
    let take = RawTake {
        end_us: tick_us(3840),
        events,
        ..RawTake::default()
    };
    let clock = sound_core::Clock::new(sound_core::TempoMap::default(), 48_000);
    let clip = take.clip(|time_us| clock.tick_at_micros(time_us)).unwrap();
    assert_eq!(clip.pressure, [amount(0, 20)]);
    assert_eq!(clip.mod_wheel, [amount(480, 90)]);
    assert_eq!(clip.bend, [bend(960, 0), bend(1600, 8191)]);
    assert_eq!(clip.notes.len(), 1);
    assert_eq!(clip.validate(), Ok(()));
}
