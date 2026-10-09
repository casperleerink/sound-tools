//! Tools of the project in the window, run by Bun: a key on a page and a note of the MIDI
//! keyboard reach the code of a tool, and what it plays or changes comes back. Bun is not part
//! of the build, so where it is not installed, as in CI, these tests say so and pass.

use std::time::{Duration, Instant};

use gpui::{AppContext, Entity, KeyUpEvent, Keystroke, TestAppContext, VisualTestContext};
use midi::Played;
use plugin_host::{Plugins, ScanCache};
use runtime::window::{DeviceAccess, Shell, bind_keys};
use runtime::{OFFLINE, views};
use sound_core::Engine;
use sound_notes::{Pitch, Velocity};
use sound_ui::{POLL_INTERVAL, Session};
use tempfile::TempDir;

use crate::plugin_hosts::scanner;
use crate::support::{id, peak};

/// A sine an octave and a fifth up from what is played, held while the key `a` is down, and
/// the last note of the MIDI keyboard kept in the record.
const KEYS: &str = r#"import { adsr, h, knob, note, sine, tool } from "./sdk";

tool({
  name: "keys",
  title: "Keys",
  when: "You want to play a sine from the computer keyboard",
  doc: "The key a holds A4.",
  kind: "instrument",
  state: { last: knob({ min: 0, max: 127, default: 0 }) },
  sound: () => sine(note.freq).times(adsr(note.gate, 1, 1, 1, 1)).times(0.2),
  onKey: ({ play, release }, { key, down }) => {
    if (key !== "a") return;
    if (down) play(69, { hold: true });
    else release(69);
  },
  onMidi: ({ update }, message) => {
    if (message.type !== "noteOn") return;
    update("Keep the note", (state) => {
      state.last = message.pitch;
    });
  },
  page: () => h("div", {}, "Play a"),
});
"#;

/// A project that is one experiment: `keys` at its top, connected to the device.
const EXPERIMENT: [(&str, &str); 3] = [
    (
        "project.json",
        r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "keys", "port": "audio"}, "to": {"device_output": 0}}]}"#,
    ),
    ("state/keys.json", r#"{"tool": "keys", "state": {}}"#),
    ("extensions/keys.ts", KEYS),
];

/// An effect that keeps the last note of the MIDI keyboard in its record.
const LISTENER: &str = r#"import { input, knob, tool } from "./sdk";

tool({
  name: "listener",
  title: "Listener",
  when: "You want to know the last note played",
  doc: "Passes the sound and keeps the last note.",
  state: { last: knob({ min: 0, max: 127, default: 0 }) },
  sound: () => input,
  onMidi: ({ update }, message) => {
    if (message.type !== "noteOn") return;
    update("Keep the note", (state) => {
      state.last = message.pitch;
    });
  },
});
"#;

/// Two tracks with a synth and a `listener` each.
const TWO_TRACKS: [(&str, &str); 8] = [
    (
        "project.json",
        r#"{"format": 1, "extensions": ["arrangement", "instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []}"#,
    ),
    (
        "state/arrangement/instance.json",
        r#"{"tool": "arrangement", "state": {}}"#,
    ),
    (
        "state/arrangement/one/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "One", "order": 0, "effects": ["listener"]}}"#,
    ),
    (
        "state/arrangement/one/instrument.json",
        r#"{"tool": "instrument.synth", "state": {}}"#,
    ),
    (
        "state/arrangement/one/listener.json",
        r#"{"tool": "listener", "state": {}}"#,
    ),
    (
        "state/arrangement/two/instance.json",
        r#"{"tool": "arrangement.track", "state": {"name": "Two", "order": 1, "effects": ["listener"]}}"#,
    ),
    (
        "state/arrangement/two/listener.json",
        r#"{"tool": "listener", "state": {}}"#,
    ),
    ("extensions/listener.ts", LISTENER),
];

struct Window<'a> {
    _folder: TempDir,
    engine: Engine,
    session: Entity<Session>,
    shell: Entity<Shell>,
    cx: &'a mut VisualTestContext,
}

/// Opens the window, as the application does, on a project of these files and its tools.
/// `None` without Bun.
fn open<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Option<Window<'a>> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    // Bun's answers wake the window's tasks from a thread of their own.
    cx.executor().allow_parking();
    let folder = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = folder.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let (control, engine) = Engine::new(OFFLINE);
    let plugins = Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let (project, extensions) =
        runtime::open_with_extensions(folder.path(), control, plugins.clone()).unwrap();
    assert_eq!(project.problems(), []);
    cx.update(sound_ui::init);
    let session = cx.new(|cx| Session::new(project, cx));
    cx.update(bind_keys);
    let (shell, cx) = cx.add_window_view({
        let session = session.clone();
        move |window, cx| {
            let (mut views, mut devices) = views(plugins.downgrade());
            let extensions = extensions.unwrap();
            sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
            let device = DeviceAccess::default();
            Shell::with_device(session, (views, devices), "Test".into(), device, window, cx)
        }
    });
    cx.run_until_parked();
    Some(Window {
        _folder: folder,
        engine,
        session,
        shell,
        cx,
    })
}

impl Window<'_> {
    /// Runs the engine for `frames` and gives the loudest sample it played.
    fn loudest(&mut self, frames: usize) -> f32 {
        let mut output = vec![0.0_f32; frames * OFFLINE.channels];
        for buffer in output.chunks_mut(64 * OFFLINE.channels) {
            self.engine.process_block(buffer);
        }
        peak(&output)
    }

    /// One poll of the window, after a block of the engine: what Bun said is heard.
    fn poll(&mut self) {
        self.loudest(64);
        self.cx.executor().advance_clock(POLL_INTERVAL);
        self.cx.run_until_parked();
    }

    /// Polls until `done` holds: Bun answers on a thread of its own, in real time.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            self.poll();
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{what} did not happen within 5 s");
    }

    fn playing(&mut self) -> bool {
        let session = self.session.clone();
        self.cx
            .read(|cx| session.read(cx).playhead().read(cx).playing)
    }

    /// Plays a key of the MIDI keyboard, once it plays where the window wired it.
    fn play_midi(&mut self, pitch: u8) {
        let shell = self.shell.clone();
        let transport = self.cx.read(|cx| shell.read(cx).transport().clone());
        let input = self.cx.read(|cx| transport.read(cx).midi_input()).unwrap();
        self.poll();
        self.poll();
        let pitch = Pitch::new(pitch).unwrap();
        let velocity = Velocity::new(100).unwrap();
        assert!(input.send(Played::On { pitch, velocity }));
    }

    /// The `last` note of the record of `instance`.
    fn last(&mut self, instance: &str) -> serde_json::Value {
        let session = self.session.clone();
        let state = (self.cx)
            .read(|cx| session.read(cx).project().state_json(&id(instance)))
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&state).unwrap()["last"].clone()
    }
}

#[gpui::test]
fn a_key_on_a_page_holds_a_note_until_it_comes_up_and_space_does_not_play(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    assert_eq!(window.loudest(4_800), 0.0);

    window.cx.simulate_keystrokes("a");
    window.until("the note of the key", |window| window.loudest(480) > 0.1);
    // Held past the 0.25 s of a note with no `hold`.
    window.loudest(24_000);
    assert!(window.loudest(4_800) > 0.1);

    // The page has the keys, so space is the tool's and does not play.
    window.cx.simulate_keystrokes("space");
    window.poll();
    assert!(!window.playing());

    let keystroke = Keystroke::parse("a").unwrap();
    window.cx.simulate_event(KeyUpEvent { keystroke });
    window.until("the release of the key", |window| {
        window.loudest(480) == 0.0
    });
}

#[gpui::test]
fn a_note_of_the_midi_keyboard_reaches_a_tool_at_the_top_and_still_sounds(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    // The keyboard plays into `keys`, the one instance at the top that takes notes.
    window.play_midi(64);
    window.until("the note in the record", |window| window.last("keys") == 64);
    // The instrument played it as well, as a keyboard plays any instrument.
    assert!(window.loudest(480) > 0.1);
}

#[gpui::test]
fn a_tool_on_the_track_the_keyboard_plays_hears_it_and_one_on_another_does_not(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open(cx, &TWO_TRACKS) else {
        return;
    };
    // Nothing is selected, so the keyboard plays the first track.
    window.play_midi(60);
    window.until("the note on the first track", |window| {
        window.last("arrangement/one/listener") == 60
    });
    // Bun would have answered for both tracks at once.
    for _ in 0..10 {
        window.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        window.last("arrangement/two/listener"),
        serde_json::Value::Null
    );
}
