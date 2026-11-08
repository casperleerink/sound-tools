//! The MIDI tools of the DAW template's note editor and timeline: quantize with its strength
//! and swing, legato, a controller lane, a clip trimmed before its controller lane, and a MIDI
//! file dropped on a track. Each gesture is one undo step.

use gpui::{ExternalPaths, FileDropEvent, TestAppContext, point, px};
use serde_json::{Value, json};
use sound_core::Ticks;
use sound_notes::{Clip, Length, Note, Pitch, Velocity};

use super::Opened;
use super::song::open_song;
use crate::song::BAR;
use crate::support::write_outside;

/// The first clip of the second track, a bar long, with `notes` and the lanes of `extra`.
fn write_clip(window: &mut Opened<'_>, notes: &[(u64, u8)], extra: Value) {
    let notes: Vec<Value> = (notes.iter())
        .map(|(start, pitch)| json!({ "start": start, "length": 100, "pitch": pitch, "velocity": 100 }))
        .collect();
    let mut state = json!({ "start": 0, "length": BAR, "notes": notes });
    if let (Some(state), Some(extra)) = (state.as_object_mut(), extra.as_object()) {
        state.extend(extra.clone());
    }
    let record = json!({ "tool": "timeline", "state": state }).to_string();
    write_outside(window, "state/song/t01/c00.json", &record);
}

fn starts(window: &mut Opened<'_>) -> Vec<u64> {
    let notes = window.record("song/t01/c00")["notes"].clone();
    let notes = notes.as_array().cloned().unwrap_or_default();
    notes
        .iter()
        .filter_map(|note| note["start"].as_u64())
        .collect()
}

impl Opened<'_> {
    /// Picks `item` of the note menu, which its button in the editor's header opens.
    fn note_menu(&mut self, item: &'static str) {
        let button = self.until_found("button-note-menu");
        self.click(button);
        self.pick(item);
        self.wait_for_bun();
    }
}

/// On the default grid of a sixteenth: Q pulls every note onto it, the menu's 50 % halfway, and
/// a full swing pulls a note before the second sixteenth onto it a triplet late. Legato then
/// ends each note where the next starts.
#[gpui::test]
fn quantize_takes_its_strength_and_swing_and_legato_closes_the_gaps(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    write_clip(&mut window, &[(300, 60), (530, 62), (1000, 64)], json!({}));
    let clip = window.on_lane(BAR / 2, 1);
    window.double_click(clip);
    window.editor("t01/c00");

    window.keys("q");
    window.until("quantized", |window| starts(window) == [240, 480, 960]);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Quantize notes"));
    assert!(window.undo().is_some());

    // The menu: 0 Quantize, 1 to 3 the strengths, 4 to 7 the swings, 8 Humanize, 9 Legato.
    window.note_menu("menu-3");
    window.note_menu("menu-0");
    window.until("halfway", |window| starts(window) == [270, 505, 980]);
    assert!(window.undo().is_some());

    window.note_menu("menu-1");
    window.note_menu("menu-7");
    // A simulated key never comes up, so Q again would be a key held: the menu quantizes.
    window.note_menu("menu-0");
    window.until("swung", |window| starts(window) == [320, 480, 960]);

    window.note_menu("menu-9");
    window.until("legato", |window| {
        let notes = window.record("song/t01/c00")["notes"].clone();
        let lengths: Vec<u64> = (notes.as_array().cloned().unwrap_or_default().iter())
            .filter_map(|note| note["length"].as_u64())
            .collect();
        lengths == [160, 480, 100]
    });
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Legato"));
    assert_eq!(window.problems(), []);
}

/// A clip's controller lane shows in the lane picker, after the lanes of every clip, and a
/// double click adds a point to it. Taking its last point away takes the lane out.
#[gpui::test]
fn a_controller_lane_of_a_clip_is_picked_and_its_points_edited(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let lane = json!({ "controllers": { "74": [{ "tick": 1920, "value": 64 }] } });
    write_clip(&mut window, &[(0, 60)], lane);
    let clip = window.on_lane(BAR / 2, 1);
    window.double_click(clip);
    let editor = window.editor("t01/c00");
    let picker = window.until_found("div-editor-lane-pick");
    window.click(picker);
    // Velocity, Pedal, Bend, Mod wheel, Pressure, then Brightness.
    window.pick("menu-5");
    window.double_click(editor.lane(1000., 8.));
    window.until("a second point", |window| {
        window.record("song/t01/c00")["controllers"]["74"]
            .as_array()
            .map(Vec::len)
            == Some(2)
    });
    let added = window.record("song/t01/c00")["controllers"]["74"][0].clone();
    assert_eq!(added["tick"], 960);
    assert!(
        added["value"].as_u64().is_some_and(|value| value > 100),
        "{added}"
    );
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a point"));
    assert_eq!(window.problems(), []);
}

/// A clip trimmed to end before its controller lane starts loses the lane, as the lane plays
/// only from its first point; its bend keeps the value it had at the new end.
#[gpui::test]
fn a_clip_trimmed_before_its_controller_lane_drops_it(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let lanes = json!({
        "controllers": { "7": [{ "tick": 2400, "value": 100 }] },
        "bend": [{ "tick": 0, "value": 0 }, { "tick": 3000, "value": 3000 }],
    });
    write_clip(&mut window, &[(0, 60)], lanes);
    let body = window.on_lane(BAR / 4, 1);
    window.click(body);
    // The right edge, inside the clip, half a clip to the left.
    let edge = window.on_lane(BAR, 1) - point(px(2.), px(0.));
    window.drag(edge, edge - point(px(48.), px(0.)));
    window.until("the end moved", |window| {
        window.record("song/t01/c00")["length"] == BAR / 2
    });
    let after = window.record("song/t01/c00");
    assert_eq!(after["controllers"], json!({}));
    assert_eq!(
        after["bend"],
        json!([{ "tick": 0, "value": 0 }, { "tick": 1919, "value": 1919 }])
    );
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Resize clips"));
    assert_eq!(window.problems(), []);
}

fn note(start: u64, pitch: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(480)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

impl Opened<'_> {
    /// Drops the MIDI file `bytes`, `name.mid`, from the system on bar 3 of track `row`.
    fn drop_midi(&mut self, name: &str, bytes: &[u8], row: usize) {
        let files = tempfile::tempdir().unwrap();
        let file = files.path().join(format!("{name}.mid"));
        std::fs::write(&file, bytes).unwrap();
        let to = self.on_lane(2 * BAR, row);
        self.cx.simulate_event(FileDropEvent::Entered {
            position: to - point(px(10.), px(0.)),
            paths: ExternalPaths(vec![file].into()),
        });
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
        self.cx
            .simulate_event(FileDropEvent::Pending { position: to });
        self.cx
            .simulate_event(FileDropEvent::Submit { position: to });
        self.cx.run_until_parked();
        self.wait_for_bun();
    }
}

/// A file of one part dropped on a track becomes a clip there; a file of two becomes two new
/// tracks, each named after its part.
#[gpui::test]
fn a_dropped_midi_file_becomes_a_clip_or_a_track_for_each_part(cx: &mut TestAppContext) {
    let mut window = open_song(cx, 2, 2);
    window.wait_for_bun();
    let clip = Clip::new(
        Ticks(0),
        Length::new(Ticks(BAR)).unwrap(),
        vec![note(0, 60), note(960, 64)],
    );
    let bytes = sound_notes::midi_file::write(&clip, "Riff", 120.0, (4, 4));
    window.drop_midi("riff", &bytes, 1);
    window.until("the clip", |window| window.has("song/t01/clip"));
    let added = window.record("song/t01/clip");
    assert_eq!(added["start"], 2 * BAR);
    assert_eq!(added["title"], "Riff");
    assert_eq!(added["notes"], serde_json::to_value(&clip.notes).unwrap());
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Import notes"));

    // Two channels of one track: two parts, numbered after the file.
    let mut track = Vec::new();
    for (channel, pitch) in [(0x90u8, 60u8), (0x91, 48)] {
        track.extend([0x00, channel, pitch, 100, 0x83, 0x60, channel, pitch, 0]);
    }
    track.extend([0x00, 0xFF, 0x2F, 0x00]);
    let mut file = b"MThd\0\0\0\x06\0\0\0\x01\x01\xE0MTrk".to_vec();
    file.extend((track.len() as u32).to_be_bytes());
    file.extend(track);
    window.drop_midi("duet", &file, 1);
    window.until("two tracks", |window| {
        window.has("song/track/clip") && window.has("song/track-2/clip")
    });
    assert_eq!(window.record("song/track")["title"], "duet 1");
    assert_eq!(window.record("song/track-2")["title"], "duet 2");
    assert_eq!(window.record("song/track-2/clip")["start"], 2 * BAR);
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Import notes"));
    assert_eq!(window.problems(), []);
}
