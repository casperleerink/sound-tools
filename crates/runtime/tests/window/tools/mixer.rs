//! The mixer of `tooling/agent-eval/examples`, a page written in TypeScript: its meters follow
//! the engine, its faders turn the records of its children and its pan knob follows a cable,
//! all in the window, with no tree from Bun.

use gpui::{TestAppContext, point, px};
use serde_json::json;
use sound_ui::renders::Part;

use super::{Opened, open};

const MIXER: &str = include_str!("../../../../../tooling/agent-eval/examples/mixer.tsx");
const SWAY: &str = include_str!("../../../../../tooling/agent-eval/examples/sway.ts");
const METER: &str = r#"{"tool": "meter", "state": {}}"#;

/// The names of `count` strips.
pub(super) fn strip_names(count: usize) -> Vec<String> {
    let named = [
        "kick", "snare", "bass", "keys", "pad", "lead", "choir", "bells",
    ];
    (0..count)
        .map(|index| match named.get(index) {
            Some(name) => (*name).to_string(),
            None => format!("s{index}"),
        })
        .collect()
}

/// The mixer with `count` strips of tones, and a `sway` that swings the pan of the first strip
/// in name order four times a second.
pub(super) fn open_mixer(cx: &mut TestAppContext, count: usize) -> Opened<'_> {
    let project = json!({
        "format": 1,
        "extensions": ["tone", "utility"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [{ "from": { "instance": "mixer", "port": "audio" }, "to": { "device_output": 0 } }],
    })
    .to_string();
    let mut files = vec![
        ("project.json".to_string(), project),
        (
            "state/mixer/instance.json".to_string(),
            r#"{"tool": "mixer", "state": {}}"#.to_string(),
        ),
        (
            "state/mixer/master.json".to_string(),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ),
        (
            "state/mixer/sway.json".to_string(),
            r#"{"tool": "sway", "state": {"rate": 4}}"#.to_string(),
        ),
        ("extensions/mixer.tsx".to_string(), MIXER.to_string()),
        ("extensions/sway.ts".to_string(), SWAY.to_string()),
    ];
    for (index, name) in strip_names(count).iter().enumerate() {
        let tone = json!({ "tool": "tone", "state": { "frequency_hz": 55.0 * (index + 1) as f32, "gain": 0.3 } });
        files.push((format!("state/mixer/{name}.json"), tone.to_string()));
        files.push((
            format!("state/mixer/{name}-mix.json"),
            r#"{"tool": "utility", "state": {}}"#.to_string(),
        ));
        files.push((format!("state/mixer/{name}-meter.json"), METER.to_string()));
    }
    files.push((
        "state/mixer/master-meter.json".to_string(),
        METER.to_string(),
    ));
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    open(cx, &files)
}

impl Opened<'_> {
    /// Plays `frames` of the engine and draws the next frame of the page.
    pub(super) fn play_and_draw(&mut self, frames: usize) {
        self.loudest(frames);
        self.cx.executor().advance_clock(sound_ui::POLL_INTERVAL);
        self.find("space-fader-master");
    }

    /// The same, of a page of knobs.
    fn play_and_draw_knobs(&mut self, frames: usize) {
        self.loudest(frames);
        self.cx.executor().advance_clock(sound_ui::POLL_INTERVAL);
        self.find("space-knob-filter.cutoff_hz");
    }
}

#[gpui::test]
fn a_meter_shows_the_level_of_its_strip_as_it_plays_and_asks_bun_for_nothing(
    cx: &mut TestAppContext,
) {
    let mut window = open_mixer(cx, 4);
    window.space_at("fader-kick");
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    window.until("the meter of the kick to show its tone", |window| {
        window.play_and_draw(2_400);
        sound_typescript::shown_by_live_values("peak:kick-meter").is_some_and(|peak| peak > 0.2)
    });
    // A tone at 0.3 through a utility at 0 dB.
    let master = sound_typescript::shown_by_live_values("peak:master-meter").unwrap();
    assert!(master > 0.3, "{master}");
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
}

#[gpui::test]
fn a_fader_turns_the_volume_of_its_strip_as_one_undo_step_and_changes_no_node(
    cx: &mut TestAppContext,
) {
    let mut window = open_mixer(cx, 4);
    let fader = window.space_at("fader-bass") + point(px(20.), px(110.));
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let batches = sound_typescript::heard_batches();
    let questions = sound_typescript::asked_behaviours();
    window.press(fader);
    for step in 1..=11 {
        window.drag_to(fader - point(px(0.), px(step as f32 * 5.)));
    }
    // 55 points of 220 up while the pointer holds it: a quarter of the travel of -36 to 36 dB.
    assert_eq!(window.record("mixer/bass-mix")["gain_db"], 18.0);
    window.release(fader - point(px(0.), px(55.)));
    window.wait_for_bun();
    assert_eq!(window.undo_label().as_deref(), Some("Change the volume"));
    // Its page read the mute of the strip, not its volume: no change of a node, and no press.
    // Its behaviour reads the names and tools of its children, not their volumes: no question.
    // Bun still hears the record, which a binding reads.
    assert_eq!(sound_typescript::heard_batches(), batches);
    assert_eq!(sound_typescript::sent_updates_and_events().1, asked.1);
    assert_eq!(sound_typescript::asked_behaviours(), questions);
    let session = window.session.clone();
    window
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.undo(cx)));
    assert_eq!(window.record("mixer/bass-mix")["gain_db"], 0.0);
}

#[gpui::test]
fn a_pan_knob_shows_the_pan_a_cable_plays_and_not_its_record(cx: &mut TestAppContext) {
    let mut window = open_mixer(cx, 4);
    // The first strip in name order.
    window.space_at("pan-bass");
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    window.until("the pan of the bass to swing", |window| {
        window.play_and_draw(1_200);
        sound_typescript::shown_by_live_values("played:bass-mix.pan")
            .is_some_and(|pan| pan.abs() > 0.3)
    });
    assert_eq!(window.record("mixer/bass-mix")["pan"], 0.0);
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
}

/// A meter of a tone: a shape that falls after the sound.
const LEVEL: &str = r##"import { h, now, Rect, Space, tool, type Declaration } from "./sdk";

tool({
  name: "level",
  title: "Level",
  when: "A test",
  doc: "The meter `meter` hears the tone `tone`.",
  state: {},
  behaviour: (): Declaration => ({ connections: [{ from: { child: "tone" }, to: { child: "meter" } }] }),
  view: () => h("div", null,
    h(Space, { key: "level", width: 20, height: 100 },
      h(Rect, { x: 0, y: 0, width: 10, height: now.peak("meter").fall(0.5).times(100), color: "#7ee0a0" }),
    ),
  ),
});
"##;

#[gpui::test]
fn a_meter_draws_over_its_page_while_it_hears_a_sound_and_until_it_rests_and_never_in_silence(
    cx: &mut TestAppContext,
) {
    let project = json!({
        "format": 1,
        "extensions": ["tone"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [],
    })
    .to_string();
    let mut window = open(
        cx,
        &[
            ("project.json", &project),
            (
                "state/level/instance.json",
                r#"{"tool": "level", "state": {}}"#,
            ),
            (
                "state/level/tone.json",
                r#"{"tool": "tone", "state": {"frequency_hz": 220.0, "gain": 0.0}}"#,
            ),
            ("state/level/meter.json", METER),
            ("extensions/level.ts", LEVEL),
        ],
    );
    window.until_found("space-level");
    assert_eq!(window.problems(), []);
    window.wait_for_bun();
    window.renders_in(10);
    assert_eq!(
        window.renders_in(60).all_of(Part::Over),
        0,
        "a second of silence"
    );

    let gain = |window: &mut Opened, gain: f64| {
        window.edit(|project| {
            let mut edit = project.begin("Gain");
            project.update_json(&mut edit, &crate::support::id("level/tone"), |state| {
                state["gain"] = gain.into();
            })?;
            project.finish(edit)
        });
    };
    gain(&mut window, 0.3);
    let renders = window.renders_in(60);
    assert!(renders.all_of(Part::Over) > 50, "a second of the tone");
    assert_eq!(renders.all_of(Part::Body), 0, "the page under it");
    let shown = sound_typescript::shown_by_live_values("peak:meter").unwrap();
    assert!(shown > 0.2, "{shown}");

    gain(&mut window, 0.0);
    let falling = window.renders_in(60).all_of(Part::Over);
    assert!(falling > 0, "the second after the tone, as the meter falls");
    // An ease falls in real time, which these frames outrun.
    let mut seconds = 1;
    while window.renders_in(60).all_of(Part::Over) > 0 {
        seconds += 1;
        assert!(seconds < 60, "the meter never rested");
    }
    assert_eq!(
        sound_typescript::shown_by_live_values("peak:meter"),
        Some(0.0)
    );
}

/// [`LEVEL`] with its meter and its falling shape in the window's title bar.
const TITLE_LEVEL: &str = r##"import { h, now, Rect, Space, tool, type Declaration } from "./sdk";

tool({
  name: "level",
  title: "Level",
  when: "A test",
  doc: "The meter `meter` hears the tone `tone`.",
  state: {},
  behaviour: (): Declaration => ({ connections: [{ from: { child: "tone" }, to: { child: "meter" } }] }),
  view: () => h("div", null,
    h("div", { titleBar: true },
        h(Space, { key: "level", width: 20, height: 20 },
        h(Rect, { x: 0, y: 0, width: 10, height: now.peak("meter").fall(0.5).times(20), color: "#7ee0a0" }),
      ),
    ),
    h("div", null, "The rest of the page"),
  ),
});
"##;

#[gpui::test]
fn a_meter_in_the_title_bar_draws_the_title_bar_alone(cx: &mut TestAppContext) {
    let project = json!({
        "format": 1,
        "extensions": ["tone"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [],
    })
    .to_string();
    let mut window = open(
        cx,
        &[
            ("project.json", &project),
            (
                "state/level/instance.json",
                r#"{"tool": "level", "state": {}}"#,
            ),
            (
                "state/level/tone.json",
                r#"{"tool": "tone", "state": {"frequency_hz": 220.0, "gain": 0.3}}"#,
            ),
            ("state/level/meter.json", METER),
            ("extensions/level.ts", TITLE_LEVEL),
        ],
    );
    window.until_found("space-level");
    assert_eq!(window.problems(), []);
    window.wait_for_bun();
    window.renders_in(10);
    assert_eq!(
        window.renders_in(60).all_of(Part::Body),
        0,
        "a second of the tone"
    );
    let shown = sound_typescript::shown_by_live_values("peak:meter").unwrap();
    assert!(shown > 0.2, "the title bar reads the meter: {shown}");

    window.edit(|project| {
        let mut edit = project.begin("Gain");
        project.update_json(&mut edit, &crate::support::id("level/tone"), |state| {
            state["gain"] = 0.0.into();
        })?;
        project.finish(edit)
    });
    window.renders_in(1);
    let mut seconds = 0;
    while sound_typescript::shown_by_live_values("peak:meter") != Some(0.0) {
        assert_eq!(
            window.renders_in(60).all_of(Part::Body),
            0,
            "the meter falls in the title bar alone"
        );
        seconds += 1;
        assert!(seconds < 60, "the meter never rested");
    }
}

/// The gain of a tone as it plays, as a shape.
const GAIN: &str = r##"import { h, now, Rect, Space, tool, type Declaration } from "./sdk";

tool({
  name: "gain",
  title: "Gain",
  when: "A test",
  doc: "Shows the gain of the tone `tone` as it plays.",
  state: {},
  behaviour: (): Declaration => ({ connections: [] }),
  view: () => h("div", null,
    h(Space, { key: "gain", width: 20, height: 100 },
      h(Rect, { x: 0, y: 0, width: 10, height: now.value("tone.gain").times(100), color: "#7ee0a0" }),
    ),
  ),
});
"##;

#[gpui::test]
fn a_number_a_page_shows_as_it_plays_draws_again_when_the_record_of_its_child_changes(
    cx: &mut TestAppContext,
) {
    let project = json!({
        "format": 1,
        "extensions": ["tone"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [],
    })
    .to_string();
    let mut window = open(
        cx,
        &[
            ("project.json", &project),
            (
                "state/gain/instance.json",
                r#"{"tool": "gain", "state": {}}"#,
            ),
            (
                "state/gain/tone.json",
                r#"{"tool": "tone", "state": {"frequency_hz": 220.0, "gain": 0.25}}"#,
            ),
            ("extensions/gain.ts", GAIN),
        ],
    );
    window.until_found("space-gain");
    assert_eq!(window.problems(), []);
    window.wait_for_bun();
    window.renders_in(10);
    let shown = || sound_typescript::shown_by_live_values("played:tone.gain");
    assert_eq!(shown(), Some(0.25));

    window.edit(|project| {
        let mut edit = project.begin("Gain");
        project.update_json(&mut edit, &crate::support::id("gain/tone"), |state| {
            state["gain"] = 0.5.into();
        })?;
        project.finish(edit)
    });
    window.renders_in(1);
    assert_eq!(shown(), Some(0.5));
}

/// A filter whose cutoff a lane moves, with a knob on the cutoff and one on the resonance.
const SWEPT: &str = r##"import { h, Knob, tool, type Declaration } from "./sdk";

const children = { filter: "filter", sweep: "lane" } as const;

tool({
  name: "swept",
  title: "Swept",
  when: "A test",
  doc: "The lane `sweep` moves the cutoff of the filter `filter`.",
  state: {},
  children,
  behaviour: (): Declaration<typeof children> => ({
    connections: [{ from: { child: "sweep", number: "value" }, to: { child: "filter", number: "cutoff_hz" } }],
  }),
  view: () => h("div", null, h(Knob, { field: "filter.cutoff_hz" }), h(Knob, { field: "filter.resonance" })),
});
"##;

/// The swept filter, whose lane plays 300 Hz at the start over a record of 1000 Hz.
fn open_swept(cx: &mut TestAppContext) -> Opened<'_> {
    let project = json!({
        "format": 1,
        "extensions": ["filter"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [],
    })
    .to_string();
    let lane = json!({ "tool": "lane", "state": { "points": [{ "tick": 0, "value": 300 }, { "tick": 3840, "value": 3000 }] } })
        .to_string();
    open(
        cx,
        &[
            ("project.json", &project),
            (
                "state/swept/instance.json",
                r#"{"tool": "swept", "state": {}}"#,
            ),
            (
                "state/swept/filter.json",
                r#"{"tool": "filter", "state": {"cutoff_hz": 1000.0, "resonance": 0.2}}"#,
            ),
            ("state/swept/sweep.json", &lane),
            ("extensions/swept.ts", SWEPT),
        ],
    )
}

#[gpui::test]
fn a_knob_on_a_field_shows_what_its_lane_plays_and_does_not_drag(cx: &mut TestAppContext) {
    let mut window = open_swept(cx);
    let cutoff = window.space_at("knob-filter.cutoff_hz") + point(px(28.), px(18.));
    assert_eq!(window.problems(), []);
    window.wait_for_bun();
    window.renders_in(10);
    let shown = |name: &str| sound_typescript::shown_by_live_values(name).unwrap();
    // Its record says 1000 Hz, and the lane plays 300 Hz at the start.
    let near = |shown: f32, played: f32| assert!((shown - played).abs() < 3.0, "{shown}");
    near(shown("played:filter.cutoff_hz"), 300.0);
    assert_eq!(shown("held:filter.cutoff_hz"), 1.0);
    assert_eq!(shown("held:filter.resonance"), 0.0);

    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, _| {
            session.engine().seek(sound_core::Ticks(3840));
        });
    });
    window.until("the knob to follow the lane", |window| {
        window.play_and_draw_knobs(1_200);
        (shown("played:filter.cutoff_hz") - 3000.0).abs() < 3.0
    });

    // The lane's number is not the composer's: a drag of its knob turns nothing.
    window.press(cutoff);
    window.drag_to(cutoff - point(px(0.), px(50.)));
    window.release(cutoff - point(px(0.), px(50.)));
    assert_eq!(window.record("swept/filter")["cutoff_hz"], 1000.0);
    assert_eq!(window.undo_label(), None);

    // The knob beside it, which no lane moves, turns as ever.
    let resonance = window.space_at("knob-filter.resonance") + point(px(28.), px(18.));
    window.press(resonance);
    window.drag_to(resonance - point(px(0.), px(50.)));
    window.release(resonance - point(px(0.), px(50.)));
    assert!(window.record("swept/filter")["resonance"].as_f64().unwrap() > 0.2);
    assert_eq!(window.undo_label().as_deref(), Some("Change resonance"));
}

/// Typing goes the way of a drag: the number its lane moves refuses a typed value too, and
/// the knob beside it takes one as one undo step.
#[gpui::test]
fn a_value_typed_on_a_knob_its_lane_moves_changes_nothing(cx: &mut TestAppContext) {
    let mut window = open_swept(cx);
    let value_of = |window: &mut Opened<'_>, field: &str| {
        window.space_at(&format!("knob-{field}")) + point(px(32.), px(62.))
    };
    let cutoff = value_of(&mut window, "filter.cutoff_hz");
    window.wait_for_bun();
    window.click(cutoff);
    window.until_found("div-knob-filter.cutoff_hz-typed");
    window.keys("cmd-a 2 0 0 0 enter");
    window.until_found("space-knob-filter.cutoff_hz");

    let resonance = value_of(&mut window, "filter.resonance");
    window.click(resonance);
    window.until_found("div-knob-filter.resonance-typed");
    window.keys("cmd-a 5 0 enter");
    window.until("the resonance typed", |window| {
        window.record("swept/filter")["resonance"].as_f64() == Some(0.5)
    });
    // The value typed first came first, and changed nothing.
    assert_eq!(window.record("swept/filter")["cutoff_hz"], 1000.0);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Change resonance"));
    window.undo();
    assert_eq!(window.undo_label(), None);
    assert_eq!(window.problems(), []);
}
