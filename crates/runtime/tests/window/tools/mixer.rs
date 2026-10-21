//! The mixer of `tooling/agent-eval/examples`, a page written in TypeScript: its meters follow
//! the engine, its faders turn the records of its children and its pan knob follows a cable,
//! all in the window, with no tree from Bun.

use gpui::{TestAppContext, point, px};
use serde_json::json;

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
    /// Plays `frames` of the engine and draws a frame of the page.
    pub(super) fn play_and_draw(&mut self, frames: usize) {
        self.loudest(frames);
        self.find("space-fader-master");
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

/// A meter of a tone, as a bar of the window and as a shape that falls after the sound.
const LEVEL: &str = r##"import { h, now, PeakMeter, Rect, Space, tool, type Declaration } from "./sdk";

tool({
  name: "level",
  title: "Level",
  when: "A test",
  doc: "The meter `meter` hears the tone `tone`.",
  state: {},
  behaviour: (): Declaration => ({ connections: [{ from: { child: "tone" }, to: { child: "meter" } }] }),
  view: () => h("div", null,
    h(PeakMeter, { of: "meter" }),
    h(Space, { key: "level", width: 20, height: 100 },
      h(Rect, { x: 0, y: 0, width: 10, height: now.peak("meter").fall(0.5).times(100), color: "#7ee0a0" }),
    ),
  ),
});
"##;

#[gpui::test]
fn a_meter_draws_its_page_while_it_hears_a_sound_and_until_it_rests_and_never_in_silence(
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
    window.cards_drawn_in(10);
    assert_eq!(window.cards_drawn_in(60), 0, "a second of silence");

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
    assert!(window.cards_drawn_in(60) > 50, "a second of the tone");
    let shown = sound_typescript::shown_by_live_values("peak:meter").unwrap();
    assert!(shown > 0.2, "{shown}");

    gain(&mut window, 0.0);
    let falling = window.cards_drawn_in(60);
    assert!(
        falling > 50,
        "the second after the tone, as the meter falls: {falling}"
    );
    // An ease falls in real time, which these frames outrun.
    let mut seconds = 1;
    while window.cards_drawn_in(60) > 0 {
        seconds += 1;
        assert!(seconds < 60, "the meter never rested");
    }
    assert_eq!(
        sound_typescript::shown_by_live_values("peak:meter"),
        Some(0.0)
    );
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
      h(Rect, { x: 0, y: 0, width: 10, height: now.value("tone.gain", { played: true }).times(100), color: "#7ee0a0" }),
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
    window.cards_drawn_in(10);
    let shown = || sound_typescript::shown_by_live_values("played:tone.gain");
    assert_eq!(shown(), Some(0.25));

    window.edit(|project| {
        let mut edit = project.begin("Gain");
        project.update_json(&mut edit, &crate::support::id("gain/tone"), |state| {
            state["gain"] = 0.5.into();
        })?;
        project.finish(edit)
    });
    window.cards_drawn_in(1);
    assert_eq!(shown(), Some(0.5));
}
