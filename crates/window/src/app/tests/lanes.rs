//! The automation lanes of the song, driven by the pointer: a point drags on the snap as one
//! step of undo and the sound plays where it went, a line bends by its handle, and many lanes of
//! many points scroll and zoom without the view.

use serde_json::{Value, json};
use sound_core::InstanceId;

use super::budget::{FRAME, measured, timed};
use super::{frame, laid_out, new_project, opened, redraw};
use crate::app::State;

/// The song's view, changed to show the lanes of every track as it opens: their toggle is the
/// heads' block.
fn lanes_shown(scratch: &tempfile::TempDir) {
    let song = scratch.path().join("song/extensions/song.tsx");
    let code = std::fs::read_to_string(&song).unwrap();
    let opening = "    const page = openPage(self, ctx);";
    assert!(code.contains(opening));
    let shown = format!(
        "    if (!ctx.memory.opened) ctx.memory.lanes = self.children.map((each) => each.name);\n{opening}"
    );
    std::fs::write(&song, code.replace(opening, &shown)).unwrap();
}

/// Track 1 with a lane `lane-cut` that moves its synth's cutoff from 8 kHz at bar 1 to 500 Hz at bar 3.
fn cutoff_lane() -> tempfile::TempDir {
    let scratch = new_project();
    lanes_shown(&scratch);
    let track = scratch.path().join("song/state/song/t00");
    let automation = json!([{ "lane": "lane-cut", "target": "synth.cutoff_hz" }]);
    let instance = json!({ "tool": "track", "state": { "title": "Track 1", "colour": "blue", "automation": automation } });
    std::fs::write(track.join("instance.json"), instance.to_string()).unwrap();
    let points = json!([{ "tick": 0, "value": 8000 }, { "tick": 7680, "value": 500 }]);
    let lane = json!({ "tool": "lane", "state": { "points": points, "scale": "log" } });
    std::fs::write(track.join("lane-cut.json"), lane.to_string()).unwrap();
    scratch
}

/// The points of `lane` of Track 1, as the engine plays them.
fn points(state: &State, lane: &str) -> Vec<Value> {
    let id = InstanceId::new(&format!("song/t00/{lane}")).unwrap();
    let record: Value = serde_json::from_str(&state.project().state_json(&id).unwrap()).unwrap();
    record["points"].as_array().unwrap().clone()
}

/// What the live texts show: a lane's head says what plays now.
fn shows(state: &State, text: &str) -> bool {
    state.lives.texts().iter().any(|(_, shown)| shown == text)
}

/// Presses `key`, moves by `by` points in a few steps, and lets go.
fn drag(window: &mut rune::Runtime<State>, key: &str, by: (f32, f32)) {
    let (x, y) = window.find(key).unwrap_or_else(|| panic!("no {key}"));
    window.pointer_down(x, y);
    redraw(window);
    for step in 1..=5 {
        let part = step as f32 / 5.0;
        window.pointer_move(x + by.0 * part, y + by.1 * part);
        redraw(window);
    }
    window.pointer_up(x + by.0, y + by.1);
    redraw(window);
    redraw(window);
}

/// The song with its lane `lane-cut` open: a press on it, where it has no point.
fn opened_lane(scratch: &tempfile::TempDir) -> rune::Runtime<State> {
    let mut window = laid_out(opened(&scratch.path().join("song")));
    for _ in 0..3 {
        redraw(&mut window);
    }
    let [x, y, _, h] = window.presented("lane-t00-lane-cut").unwrap().0;
    click(&mut window, (x + 600.0, y + h / 2.0));
    window
}

fn click(window: &mut rune::Runtime<State>, (x, y): (f32, f32)) {
    window.pointer_down(x, y);
    window.pointer_up(x, y);
    redraw(window);
    redraw(window);
}

/// The first point dragged 31 points later lands on the snap, a sixteenth, at bar 1.2: lower,
/// one step of undo "Move a point", and the cutoff plays from there; undo puts it back.
#[test]
fn a_point_dragged_lands_on_the_snap_as_one_step_and_the_sound_plays_from_there() {
    let scratch = cutoff_lane();
    let mut window = opened_lane(&scratch);
    assert!(
        shows(&window.state, "8 kHz"),
        "{:?}",
        window.state.lives.texts()
    );
    let label = window.state.project().undo_label();
    drag(&mut window, "point-t00-lane-cut-0", (31.0, 10.0));
    let moved = points(&window.state, "lane-cut");
    // 31 points at a fortieth of a tick a point is 1240 ticks: the nearest sixteenth is 1200.
    assert_eq!(moved[0]["tick"], 1200, "{moved:?}");
    let value = moved[0]["value"].as_f64().unwrap();
    // 10 points down a lane of 48 is about a fifth of three decades: near 1.9 kHz.
    assert!(value > 1500.0 && value < 2500.0, "{value}");
    assert_eq!(
        window.state.project().undo_label().as_deref(),
        Some("Move a point")
    );
    // Before the first point the lane holds it: what plays at the start is the new value.
    let khz = format!("{:.1} kHz", value / 1000.0);
    assert!(
        !shows(&window.state, "8 kHz"),
        "{:?}",
        window.state.lives.texts()
    );
    assert!(
        shows(&window.state, &khz) || shows(&window.state, &format!("{:.2} kHz", value / 1000.0)),
        "{khz} in {:?}",
        window.state.lives.texts()
    );

    window.state.undo().unwrap();
    redraw(&mut window);
    assert_eq!(
        points(&window.state, "lane-cut")[0],
        json!({ "tick": 0, "value": 8000 })
    );
    assert_eq!(window.state.project().undo_label(), label);
}

/// While a point drags, the line through it follows, with no view in Bun.
#[test]
fn the_line_follows_a_point_as_it_drags() {
    let scratch = cutoff_lane();
    let mut window = opened_lane(&scratch);
    let before = window.state.lives.drawn();
    assert!(!before.is_empty());
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    let (x, y) = window.find("point-t00-lane-cut-7680").unwrap();
    window.pointer_down(x, y);
    redraw(&mut window);
    window.pointer_move(x - 40.0, y - 10.0);
    redraw(&mut window);
    let during = window.state.lives.drawn();
    assert_ne!(during, before, "the line did not move");
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
}

/// The handle in the middle of the line, dragged up, bends it, one step "Bend a line"; a double
/// click on it straightens it again.
#[test]
fn a_line_bends_by_its_handle_and_a_double_click_straightens_it() {
    let scratch = cutoff_lane();
    let mut window = opened_lane(&scratch);
    // Its handle shows beside a chosen point.
    let point = window.find("point-t00-lane-cut-0").unwrap();
    click(&mut window, point);
    drag(&mut window, "bend-t00-lane-cut-0", (0.0, -4.0));
    let bent = points(&window.state, "lane-cut");
    let curve = bent[0]["curve"].as_f64().unwrap_or_default();
    // Up on a falling line holds the high longer.
    assert!(curve > 0.0 && curve < 1.0, "{bent:?}");
    assert_eq!(
        window.state.project().undo_label().as_deref(),
        Some("Bend a line")
    );

    let (x, y) = window.find("bend-t00-lane-cut-0").unwrap();
    for _ in 0..2 {
        window.pointer_down(x, y);
        window.pointer_up(x, y);
    }
    redraw(&mut window);
    redraw(&mut window);
    assert_eq!(
        points(&window.state, "lane-cut")[0],
        json!({ "tick": 0, "value": 8000 })
    );
    assert_eq!(
        window.state.project().undo_label().as_deref(),
        Some("Straighten a line")
    );
}

/// Two tracks of 32 lanes, each of 200 points on a wavetable's numbers: the song opens, and a
/// scroll and a zoom of it run no view in Bun and stay within the window's budgets.
#[test]
fn sixty_four_lanes_of_two_hundred_points_scroll_and_zoom_without_the_view() {
    let scratch = new_project();
    lanes_shown(&scratch);
    let numbers = [
        "position",
        "effect_amount",
        "gain",
        "pan",
        "detune_cents",
        "semitone",
        "octave",
    ];
    let targets: Vec<String> = ["osc_1", "osc_2"]
        .iter()
        .flat_map(|osc| {
            numbers
                .iter()
                .map(move |number| format!("synth.{osc}.{number}"))
        })
        .chain(
            [
                "synth.sub.gain",
                "synth.unison.amount",
                "synth.lfo_1.rate_hz",
                "synth.lfo_2.rate_hz",
                "synth.voicing.glide_seconds",
                "synth.env_3.sustain",
                "mix.pan",
                "mix.gain_db",
            ]
            .map(String::from),
        )
        .chain((1..=2).flat_map(|filter| {
            ["cutoff_hz", "resonance", "drive_db"]
                .map(|number| format!("synth.filter_{filter}.{number}"))
        }))
        .chain(["amp_env", "env_2"].iter().flat_map(|env| {
            ["attack_seconds", "sustain"].map(|number| format!("synth.{env}.{number}"))
        }))
        .take(32)
        .collect();
    assert_eq!(targets.len(), 32);
    for track in ["t00", "t01"] {
        let folder = scratch.path().join("song/state/song").join(track);
        std::fs::write(
            folder.join("synth.json"),
            json!({ "tool": "wavetable", "state": {} }).to_string(),
        )
        .unwrap();
        let automation: Vec<Value> = (targets.iter().enumerate())
            .map(|(index, target)| json!({ "lane": format!("lane-{index}"), "target": target }))
            .collect();
        let title = if track == "t00" { "Track 1" } else { "Track 2" };
        let instance = json!({ "tool": "track", "state": { "title": title, "colour": "blue", "automation": automation } });
        std::fs::write(folder.join("instance.json"), instance.to_string()).unwrap();
        for index in 0..targets.len() {
            let points: Vec<Value> = (0..200).map(|point| json!({ "tick": point * 480, "value": if point % 2 == 0 { 0.25 } else { 0.75 } })).collect();
            let lane = json!({ "tool": "lane", "state": { "points": points, "scale": "linear" } });
            std::fs::write(folder.join(format!("lane-{index}.json")), lane.to_string()).unwrap();
        }
    }
    let mut window = laid_out(opened(&scratch.path().join("song")));
    for _ in 0..3 {
        redraw(&mut window);
    }
    assert!(window.find("lane-t00-lane-0").is_some());
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    let [x, y, w, h] = window.presented("song").unwrap().0;
    let at = (x + w / 2.0, y + h / 2.0);
    let builds = window.state.planes.builds.get();
    let mut step = -1.0;
    let scroll = measured(|| {
        step = -step;
        let wheel = rune::WheelEvent::new(step, step).at(at.0, at.1);
        timed(|| {
            window.wheel_event(wheel);
            window.frame(1.0 / 60.0);
        })
    });
    assert_eq!(
        window.state.planes.builds.get(),
        builds,
        "a scroll built the plane again"
    );
    let mut zoom_in = true;
    let zoom = measured(|| {
        let by = if zoom_in { 0.25 } else { -0.2 };
        zoom_in = !zoom_in;
        timed(|| {
            window.magnify_event(rune::MagnifyEvent::new(by).at(at.0, at.1));
            window.frame(1.0 / 60.0);
        })
    });
    let later = window.state.last + 0.1;
    frame(&mut window.state, later);
    assert_eq!(
        window.state.tools.as_ref().unwrap().view_runs(),
        runs,
        "Bun ran the view"
    );
    eprintln!("64 lanes of 200 points: scroll {scroll:?}, zoom {zoom:?} a frame");
    assert!(scroll < FRAME, "a scroll frame cost {scroll:?}");
    assert!(zoom < FRAME, "a zoom frame cost {zoom:?}");
}

/// What a new lane offers comes from the facts of each device, an effect in the chain as well
/// as the instrument: the numbers a lane moves, not those it cannot, such as a pad's pitch.
#[test]
fn a_new_lane_offers_the_numbers_a_lane_moves_of_the_instrument_and_each_effect() {
    let scratch = new_project();
    lanes_shown(&scratch);
    let track = scratch.path().join("song/state/song/t00");
    let pads = json!({ "tool": "drum-pad", "state": { "pads": { "36": { "volume_db": -3.0 } } } });
    std::fs::write(track.join("synth.json"), pads.to_string()).unwrap();
    let eq = json!({ "tool": "eq", "state": { "bands": [{}, { "gain_db": -4.0 }] } });
    std::fs::write(track.join("chain/eq.json"), eq.to_string()).unwrap();
    let chain = json!({ "tool": "chain", "state": { "order": ["eq"] } });
    std::fs::write(track.join("chain/instance.json"), chain.to_string()).unwrap();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    for _ in 0..30 {
        redraw(&mut window);
    }
    let (x, y) = window.find("add-lane-t00").expect("an Add lane");
    window.pointer_down(x, y);
    window.pointer_up(x, y);
    redraw(&mut window);
    let offers = |target: &str| window.find(&format!("add-lane-t00-{target}")).is_some();
    assert!(offers("synth.pads.36.volume_db"));
    assert!(!offers("synth.pads.36.pitch_semitones"));
    assert!(offers("chain/eq.bands.1.gain_db"));
    assert!(offers("chain/eq.output_gain_db"));
}
