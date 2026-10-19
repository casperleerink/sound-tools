//! The mixer of `tooling/agent-eval/examples`, a page written in TypeScript: its meters follow
//! the engine, its faders turn the records of its children and its pan knob follows a cable,
//! all in the window, with no tree from Bun.

use gpui::{TestAppContext, point, px};
use serde_json::json;

use super::{Window, open};

const MIXER: &str = include_str!("../../../../../tooling/agent-eval/examples/mixer.tsx");
const SWAY: &str = include_str!("../../../../../tooling/agent-eval/examples/sway.ts");

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
pub(super) fn open_mixer(cx: &mut TestAppContext, count: usize) -> Option<Window<'_>> {
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
    }
    let files: Vec<(&str, &str)> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    open(cx, &files)
}

impl Window<'_> {
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
    let Some(mut window) = open_mixer(cx, 4) else {
        return;
    };
    window.space_at("fader-kick");
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    window.until("the meter of the kick to show its tone", |window| {
        window.play_and_draw(2_400);
        sound_typescript::shown_by_live_values("peak:kick").is_some_and(|peak| peak > 0.2)
    });
    // A tone at 0.3 through a utility at 0 dB.
    let master = sound_typescript::shown_by_live_values("peak:master").unwrap();
    assert!(master > 0.3, "{master}");
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
}

#[gpui::test]
fn a_fader_turns_the_volume_of_its_strip_as_one_undo_step_and_changes_no_node(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open_mixer(cx, 4) else {
        return;
    };
    let fader = window.space_at("fader-bass") + point(px(20.), px(110.));
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let batches = sound_typescript::heard_batches();
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
    // Bun still hears each move, as the behaviour of a parent runs again when a child changes.
    assert_eq!(sound_typescript::heard_batches(), batches);
    assert_eq!(sound_typescript::sent_updates_and_events().1, asked.1);
    let session = window.session.clone();
    window
        .cx
        .update(|_, cx| session.update(cx, |session, cx| session.undo(cx)));
    assert_eq!(window.record("mixer/bass-mix")["gain_db"], 0.0);
}

#[gpui::test]
fn a_pan_knob_shows_the_pan_a_cable_plays_and_not_its_record(cx: &mut TestAppContext) {
    let Some(mut window) = open_mixer(cx, 4) else {
        return;
    };
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
