//! What comes in, in the DAW of a new project on the offline engine with Bun: the computer keys
//! play the chosen track, R and the Record button record them and a simulated input into the
//! tracks as one step each, the click sounds, and the notice of an update.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Key;
use runtime::input::{Devices, Input, OpenedInput};
use sound_core::{CaptureWriter, LiveWriter};

use super::host::event;
use super::*;

/// How long a test waits for Bun and the threads of the input, at most.
const PATIENCE: Duration = Duration::from_secs(30);

fn window_of(folder: &Path) -> rune::Runtime<State> {
    let mut window = laid_out(opened(folder));
    window.on_key = Some(on_key);
    redraw(&mut window);
    window
}

/// A key goes down, and the frame after the one that hears it, which polls what it did.
fn press(window: &mut rune::Runtime<State>, chord: &str) {
    window.key(event(chord));
    redraw(window);
    redraw(window);
}

fn release(window: &mut rune::Runtime<State>, chord: &str) {
    window.key_up(event(chord));
    redraw(window);
    redraw(window);
}

/// Frames until `done` holds. Bun and the threads of the input answer in real time.
fn until(window: &mut rune::Runtime<State>, what: &str, done: impl Fn(&State) -> bool) {
    let start = Instant::now();
    while !done(&window.state) {
        assert!(
            start.elapsed() < PATIENCE,
            "{what} did not come; notice: {:?}; problems: {:?}",
            window.state.session.notice(),
            window.state.project().problems()
        );
        redraw(window);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The children of `track` of `tool`, by name.
fn children_of(state: &State, track: &str, tool: &str) -> Vec<String> {
    let track = InstanceId::new(track).unwrap();
    (state.project().instances())
        .filter(|(id, each)| id.parent().as_ref() == Some(&track) && *each == tool)
        .map(|(id, _)| id.name().to_string())
        .collect()
}

fn record_button(state: &State) -> Option<controls::Control> {
    let controls = controls::collect(state).controls;
    controls
        .into_iter()
        .find(|control| control.label == "Record")
}

#[test]
fn r_records_the_computer_keys_on_the_first_track_as_one_step_and_undo_waits_for_it() {
    let scratch = new_project();
    let mut window = window_of(&scratch.path().join("song"));
    // The song records: its Record button shows, off.
    assert_eq!(record_button(&window.state).unwrap().checked, Some(false));
    let clips = children_of(&window.state, "song/t00", "timeline");

    press(&mut window, "cmd-k");
    assert!(window.state.incoming.live.computer_keys());
    // `r` is the song's: the computer keys leave a key an action has.
    press(&mut window, "r");
    release(&mut window, "r");
    until(&mut window, "the take", |state| state.session.recording());
    assert_eq!(record_button(&window.state).unwrap().checked, Some(true));
    // `a` plays middle C into the take while it is held.
    press(&mut window, "a");
    for _ in 0..12 {
        redraw(&mut window);
    }
    release(&mut window, "a");
    press(&mut window, "cmd-z");
    assert_eq!(window.state.session.notice(), Some("Stop recording first."));

    act(
        &mut window.state,
        json!({ "label": "Record", "action": "press" }),
    );
    until(&mut window, "the clip of the take", |state| {
        children_of(state, "song/t00", "timeline").len() > clips.len()
    });
    assert!(!window.state.session.recording());
    let take = (children_of(&window.state, "song/t00", "timeline").into_iter())
        .find(|clip| !clips.contains(clip))
        .unwrap();
    until(&mut window, "the step of the take", |state| {
        state.project().undo_label().as_deref() == Some("Record")
    });
    let clip = InstanceId::new(&format!("song/t00/{take}")).unwrap();
    let clip: Value =
        serde_json::from_str(&window.state.project().state_json(&clip).unwrap()).unwrap();
    assert_eq!(clip["notes"][0]["pitch"], 60, "{clip}");
    assert_eq!(window.state.project().problems(), []);
    // One step: undo takes the take back and nothing else.
    press(&mut window, "cmd-z");
    assert_eq!(children_of(&window.state, "song/t00", "timeline"), clips);
}

/// An audio input with no device, into which the test writes a steady tone.
#[derive(Clone, Default)]
struct Microphone {
    writers: Arc<Mutex<Option<(CaptureWriter, LiveWriter)>>>,
    written: Arc<AtomicU64>,
    openings: Arc<AtomicU64>,
}

impl Microphone {
    fn devices(&self) -> Devices {
        let microphone = self.clone();
        Devices {
            open_input: Some(Arc::new(move || {
                let (capture, reader) = sound_core::capture(48_000, 2);
                let (live_writer, live) = sound_core::live_input(48_000, 2);
                *microphone.writers.lock().unwrap() = Some((capture, live_writer));
                microphone.written.store(0, Ordering::Relaxed);
                microphone.openings.fetch_add(1, Ordering::Relaxed);
                Ok(OpenedInput {
                    stream: None,
                    reader,
                    live,
                    notice: None,
                })
            })),
            ..Devices::default()
        }
    }

    /// Writes the input up to engine frame `until`, captured as that frame sounds.
    fn write_until(&self, until: u64) {
        let mut writers = self.writers.lock().unwrap();
        let Some((capture, live)) = writers.as_mut() else {
            return;
        };
        let first = self.written.load(Ordering::Relaxed);
        if until <= first {
            return;
        }
        let samples: Vec<f32> = (first..until).flat_map(|_| [0.5, 0.25]).collect();
        capture.write(&samples, first * 1_000_000_000 / 48_000, 0);
        live.write(&samples);
        self.written.store(until, Ordering::Relaxed);
    }
}

const AUDIO_TRACK: [(&str, &str); 4] = [
    (
        "audio-1/instance.json",
        r#"{"tool": "tape", "state": {"colour": "green", "title": "Audio 1"}}"#,
    ),
    (
        "audio-1/rec.json",
        r#"{"tool": "recorder", "state": {"channels": [0]}}"#,
    ),
    (
        "audio-1/chain/instance.json",
        r#"{"tool": "chain", "state": {}}"#,
    ),
    ("audio-1/mix.json", r#"{"tool": "fader", "state": {}}"#),
];

#[test]
fn an_armed_audio_track_opens_the_input_and_records_a_region_as_one_step() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    for (path, contents) in AUDIO_TRACK {
        let path = folder.join("state/song").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let mut window = window_of(&folder);
    let microphone = Microphone::default();
    let waker = Arc::new(|| {});
    window.state.incoming.live = (window.state.session)
        .background(|project| Input::new(project, microphone.devices(), waker));

    // Armed from the song's view: the input opens before anything plays.
    act(
        &mut window.state,
        json!({ "label": "Arm Audio 1", "action": "toggle" }),
    );
    until(&mut window, "the input", |_| {
        microphone.openings.load(Ordering::Relaxed) == 1
    });
    press(&mut window, "r");
    until(&mut window, "the take", |state| state.session.recording());
    // The engine plays half a second while the input brings what it heard.
    let start = Instant::now();
    while start.elapsed() < PATIENCE && window.state.session.status().frames < 24_000 {
        microphone.write_until(window.state.session.status().frames + 2_048);
        redraw(&mut window);
    }
    // Space pauses, which ends the take.
    press(&mut window, "space");
    until(&mut window, "the region of the take", |state| {
        microphone.write_until(state.session.status().frames + 2_048);
        !children_of(state, "song/audio-1", "region").is_empty()
    });
    until(&mut window, "the step of the take", |state| {
        state.project().undo_label().as_deref() == Some("Record")
    });
    assert_eq!(window.state.project().problems(), []);
    press(&mut window, "cmd-z");
    assert!(children_of(&window.state, "song/audio-1", "region").is_empty());
}

#[test]
fn the_computer_keys_play_the_plain_keys_no_one_takes_and_let_go_on_cmd() {
    let scratch = new_project();
    let mut window = window_of(&scratch.path().join("song"));
    let held = |state: &mut State, key: &str| state.computer_key(key, false, true, false);
    // Off, a plain key plays nothing.
    press(&mut window, "a");
    assert!(!held(&mut window.state, "a"));
    release(&mut window, "a");

    // Code turns them on as cmd-K does.
    window.state.session.set_computer_keys(true);
    redraw(&mut window);
    assert!(window.state.incoming.live.computer_keys());
    press(&mut window, "a");
    assert!(held(&mut window.state, "a"));
    // Every key up comes, and lets go of its note.
    release(&mut window, "a");
    assert!(!held(&mut window.state, "a"));
    // `t` is the song's action: the computer keys leave it.
    press(&mut window, "t");
    assert!(!held(&mut window.state, "t"));
    release(&mut window, "t");
    // Cmd going down lets go: macOS sends no key up under it.
    press(&mut window, "s");
    assert!(held(&mut window.state, "s"));
    window.update(|state, tx| on_modifiers(state, event("cmd-c").mods, tx));
    assert!(!held(&mut window.state, "s"));
    // So does losing the window.
    press(&mut window, "d");
    window.update(|state, tx| on_focus(state, false, tx));
    assert!(!held(&mut window.state, "d"));
    // Not while a text field has the keys.
    tab_to(&mut window, "tempo");
    key(&mut window, Key::Enter);
    assert!(window.state.typing_now());
    press(&mut window, "f");
    assert!(!held(&mut window.state, "f"));
}

/// The click is the session's, attached at open: a switch that turns on proves it is there,
/// as an unattached one stays off. It plays no part of the project.
#[test]
fn the_click_starts_off_and_turns_on_as_no_edit() {
    let scratch = new_project();
    let mut window = window_of(&scratch.path().join("song"));
    assert!(!window.state.session.transport().click);
    let undo = window.state.project().undo_label();
    act(
        &mut window.state,
        json!({ "label": "Click", "action": "toggle" }),
    );
    assert!(window.state.session.transport().click);
    assert_eq!(window.state.project().undo_label(), undo);
}

#[test]
fn an_update_that_did_not_install_says_why_and_offers_its_page() {
    let scratch = new_project();
    let mut window = window_of(&scratch.path().join("song"));
    window.state.incoming.ready = Some(runtime::update::Ready {
        version: runtime::update::Version::parse("9.0.0").unwrap(),
        action: runtime::update::Action::Failed {
            reason: "the disk is full".to_string(),
            page: "https://example.com/release".to_string(),
        },
    });
    redraw(&mut window);
    assert!(window.find("notice-update").is_some());
    let controls = controls::collect(&window.state).controls;
    control(&controls, "Download");
    click(&mut window, "notice-update-button");
    assert_eq!(window.take_url_requests(), ["https://example.com/release"]);
}
