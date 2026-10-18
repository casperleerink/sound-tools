//! Tools of the project in the window, run by Bun: a key on a page and a note of the MIDI
//! keyboard or the computer keys reach the code of a tool, and what it plays or changes comes
//! back; a save of a tool keeps the sample it plays. Bun is not part of the build, so where it
//! is not installed, these tests say so and pass.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{
    AppContext, Entity, FocusHandle, Focusable, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    Pixels, Point, TestAppContext, VisualTestContext, point,
};
use midi::Played;
use plugin_host::{Plugins, ScanCache};
use runtime::window::{DeviceAccess, Shell, bind_keys};
use runtime::{OFFLINE, views};
use sound_core::{Changes, Engine, Problem};
use sound_notes::{Pitch, Velocity};
use sound_ui::{POLL_INTERVAL, Session};
use tempfile::TempDir;

use crate::plugin_hosts::scanner;
use crate::support::{id, peak};

mod edits;

/// A sine held while the key `a` is down, the keys that went down counted in the record, and
/// the last note of the MIDI keyboard kept there.
const KEYS: &str = r#"import { adsr, h, knob, note, sine, tool } from "./sdk";

tool({
  name: "keys",
  title: "Keys",
  when: "You want to play a sine from the computer keyboard",
  doc: "The key a holds A4.",
  kind: "instrument",
  state: { last: knob({ min: 0, max: 127, default: 0 }), count: knob({ min: 0, max: 100, default: 0 }) },
  sound: () => sine(note.freq).times(adsr(note.gate, 1, 1, 1, 1)).times(0.2),
  onKey: ({ play, send, update }, { key, down }) => {
    if (down) {
      update("Count the key", ({ state }) => {
        state.count = (state.count ?? 0) + 1;
      });
    }
    if (key !== "a") return;
    if (down) play(69, { hold: true });
    else send({ type: "noteOff", pitch: 69 });
  },
  onNotes: ({ update }, event) => {
    if (event.type !== "noteOn") return;
    update("Keep the note", ({ state }) => {
      state.last = event.pitch;
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

/// A page that asks for the transport, at the top. A click keeps the tick it drew with.
const BEAT: [(&str, &str); 3] = [
    ONLY_THE_TOOL[0],
    (
        "state/beat.json",
        r#"{"tool": "beat", "state": {"tick": 0}}"#,
    ),
    (
        "extensions/beat.ts",
        r#"import { h, input, knob, tool } from "./sdk";

tool({
  name: "beat",
  title: "Beat",
  when: "You want to see the beat",
  doc: "Shows the bar and the beat.",
  state: { tick: knob({ min: 0, max: 1000000, default: 0 }) },
  sound: () => input,
  transport: true,
  page: ({ size, transport, update }) =>
    h(
      "div",
      {
        style: { width: size.width, height: size.height },
        onClick: () => update("Keep the tick", ({ state }) => { state.tick = transport.tick; }),
      },
      `${transport.bar}.${transport.beat}`,
    ),
});
"#,
    ),
];

/// A page that counts its clicks from the record it drew with, and shows nothing that changes.
const COUNTER: [(&str, &str); 3] = [
    ONLY_THE_TOOL[0],
    (
        "state/counter.json",
        r#"{"tool": "counter", "state": {"count": 0}}"#,
    ),
    (
        "extensions/counter.ts",
        r#"import { h, input, knob, tool } from "./sdk";

tool({
  name: "counter",
  title: "Counter",
  when: "You want to count clicks",
  doc: "Counts the clicks on its page.",
  state: { count: knob({ min: 0, max: 100, default: 0 }) },
  sound: () => input,
  page: ({ size, state, update }) =>
    h(
      "div",
      {
        style: { width: size.width, height: size.height },
        onClick: () => update("Count", ({ state: next }) => { next.count = state.count + 1; }),
      },
      "Count",
    ),
});
"#,
    ),
];

/// A page that counts in its memory the notes and the keys it hears, and keeps the count on a
/// click. Nothing else draws it again.
const EAR: [(&str, &str); 3] = [
    EXPERIMENT[0],
    ("state/keys.json", r#"{"tool": "keys", "state": {}}"#),
    (
        "extensions/keys.ts",
        r#"import { h, knob, tool } from "./sdk";

tool({
  name: "keys",
  title: "Ear",
  when: "You want to count what is played",
  doc: "Counts the notes and the keys it hears; a click on its page keeps the count.",
  kind: "instrument",
  state: { heard: knob({ min: 0, max: 100, default: 0 }) },
  memory: () => ({ heard: 0 }),
  onNotes: ({ memory }, event) => {
    if (event.type === "noteOn") memory.heard += 1;
  },
  onKey: ({ memory }, { down }) => {
    if (down) memory.heard += 1;
  },
  page: ({ size, memory, update }) =>
    h(
      "div",
      {
        style: { width: size.width, height: size.height },
        onClick: () => update("Keep the count", ({ state }) => { state.heard = memory.heard; }),
      },
      "Listen",
    ),
});
"#,
    ),
];

/// The tool `keys`, and no record of it yet.
const ONLY_THE_TOOL: [(&str, &str); 2] = [
    (
        "project.json",
        r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": []}"#,
    ),
    ("extensions/keys.ts", KEYS),
];

/// An effect that keeps the last note of the MIDI keyboard in its record, and how many it
/// holds.
const LISTENER: &str = r#"import { input, knob, tool } from "./sdk";

tool({
  name: "listener",
  title: "Listener",
  when: "You want to know the last note played",
  doc: "Passes the sound and keeps the last note and how many are held.",
  state: { last: knob({ min: 0, max: 127, default: 0 }), held: knob({ min: 0, max: 128, default: 0 }) },
  sound: () => input,
  onNotes: ({ update }, event) => {
    if (event.type === "noteOn") {
      update("Keep the note", ({ state }) => {
        state.last = event.pitch;
        state.held = (state.held ?? 0) + 1;
      });
    } else if (event.type === "noteOff") {
      update("Let go of the note", ({ state }) => {
        state.held = (state.held ?? 0) - 1;
      });
    }
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
    plugins: Plugins,
    session: Entity<Session>,
    shell: Entity<Shell>,
    cx: &'a mut VisualTestContext,
}

/// Opens the window, as the application does, on a project of these files and its tools.
/// `None` without Bun.
fn open<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Option<Window<'a>> {
    let folder = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        write(folder.path(), path, contents);
    }
    open_folder(cx, folder)
}

fn write(folder: &Path, path: &str, contents: &str) {
    let path = folder.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// [`open`] on a project folder that is written already.
fn open_folder(cx: &mut TestAppContext, folder: TempDir) -> Option<Window<'_>> {
    let plugins = Plugins::new(Vec::new(), scanner(), ScanCache::none());
    open_with(cx, folder, plugins)
}

/// [`open_folder`] with a plugin host that finds the repository's test plugins.
fn open_with_test_plugins<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> Option<Window<'a>> {
    let folder = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        write(folder.path(), path, contents);
    }
    let plugins = crate::plugin_hosts::test_plugin_host(folder.path());
    open_with(cx, folder, plugins)
}

fn open_with(cx: &mut TestAppContext, folder: TempDir, plugins: Plugins) -> Option<Window<'_>> {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return None;
    }
    // Bun's answers wake the window's tasks from a thread of their own.
    cx.executor().allow_parking();
    let (control, engine) = Engine::new(OFFLINE);
    let (project, extensions) =
        runtime::open_with_extensions(folder.path(), control, plugins.clone()).unwrap();
    assert_eq!(project.problems(), []);
    cx.update(sound_ui::init);
    let session = cx.new(|cx| Session::new(project, cx));
    cx.update(bind_keys);
    let (shell, cx) = cx.add_window_view({
        let (session, plugins) = (session.clone(), plugins.clone());
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
        plugins,
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

    /// One poll of the window, after a block of the engine: what Bun said is heard, and the
    /// tick of the window runs.
    fn poll(&mut self) {
        self.loudest(64);
        self.cx.executor().advance_clock(POLL_INTERVAL);
        self.cx.run_until_parked();
        let (session, plugins) = (self.session.clone(), self.plugins.clone());
        (self.cx).update(|_, cx| runtime::window::tick(&session, &plugins, &mut 0, cx));
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

    /// Polls for as long as Bun takes to answer, for a test that nothing happens.
    fn wait_for_bun(&mut self) {
        for _ in 0..10 {
            self.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The middle of a control that names itself for tests, after a whole frame. `None` when it
    /// is not on screen.
    fn find(&mut self, selector: &'static str) -> Option<Point<Pixels>> {
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
        self.cx.debug_bounds(selector).map(|bounds| bounds.center())
    }

    fn focused(&mut self) -> FocusHandle {
        self.cx.update(|window, cx| window.focused(cx)).unwrap()
    }

    /// Gives the focus to the window itself, out of a page or a card.
    fn focus_the_window(&mut self) {
        let shell = self.shell.clone();
        self.cx.update(|window, cx| {
            let handle = shell.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
    }

    /// A key going down, or repeating as the system marks it with `is_held`.
    fn key_down(&mut self, key: &str, is_held: bool) {
        self.cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held,
            prefer_character_input: false,
        });
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

    /// The record of `instance`.
    fn record(&mut self, instance: &str) -> serde_json::Value {
        let session = self.session.clone();
        let state = (self.cx)
            .read(|cx| session.read(cx).project().state_json(&id(instance)))
            .unwrap();
        serde_json::from_str(&state).unwrap()
    }

    fn problems(&mut self) -> Vec<Problem> {
        let session = self.session.clone();
        self.cx.read(|cx| session.read(cx).project().problems())
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
fn a_page_that_asks_for_the_transport_plays_it_and_draws_as_it_moves(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &BEAT) else {
        return;
    };
    assert!(window.find("record").is_none());
    let play = window.find("play").expect("the play button");
    window.cx.simulate_click(play, Modifiers::default());
    window.poll();
    assert!(window.playing());

    // A click keeps the tick of the tree it clicked: the page drew again as the playhead moved.
    let middle = window.cx.update(|window, _| window.viewport_size()) / 2.;
    let middle = point(middle.width, middle.height);
    window.until("a draw at a later tick", |window| {
        window.cx.simulate_click(middle, Modifiers::default());
        window.record("beat")["tick"]
            .as_f64()
            .is_some_and(|tick| tick > 0.)
    });
}

/// The tree stays the same, so a click runs the handler of the last draw, with the new record.
#[gpui::test]
fn a_click_on_a_page_that_drew_the_same_tree_again_sees_the_record_it_drew_with(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open(cx, &COUNTER) else {
        return;
    };
    let middle = window.cx.update(|window, _| window.viewport_size()) / 2.;
    let middle = point(middle.width, middle.height);
    let count = |window: &mut Window| window.record("counter")["count"].as_f64();
    // Clicked again until it counts: the first click may come before the first tree.
    for wanted in [1.0, 2.0] {
        window.until("the click counts", |window| {
            window.cx.simulate_click(middle, Modifiers::default());
            (0..40).any(|_| {
                window.poll();
                std::thread::sleep(Duration::from_millis(5));
                count(window) == Some(wanted)
            })
        });
    }
    window.wait_for_bun();
    assert_eq!(count(&mut window), Some(2.0));
}

/// The page draws again after its code hears a note or a key, and a click on it still runs.
#[gpui::test]
fn a_page_still_takes_clicks_after_its_code_hears_a_note_and_a_key(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EAR) else {
        return;
    };
    let middle = window.cx.update(|window, _| window.viewport_size()) / 2.;
    let middle = point(middle.width, middle.height);
    let kept = |window: &mut Window, wanted: f64| {
        window.until("the click keeps the count", |window| {
            window.cx.simulate_click(middle, Modifiers::default());
            (0..40).any(|_| {
                window.poll();
                std::thread::sleep(Duration::from_millis(5));
                window.record("keys")["heard"].as_f64() == Some(wanted)
            })
        });
    };
    window.play_midi(64);
    kept(&mut window, 1.0);
    window.cx.simulate_keystrokes("a");
    kept(&mut window, 2.0);
}

#[gpui::test]
fn a_page_that_does_not_ask_for_the_transport_has_no_play(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    assert!(window.find("play").is_none());
}

/// The computer keys play as a MIDI keyboard, but a page that hears keys keeps them.
#[gpui::test]
fn a_page_that_hears_keys_keeps_them_while_the_computer_keys_play(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    window.cx.simulate_keystrokes("secondary-k");
    window.cx.simulate_keystrokes("a");
    window.until("the key on the page", |window| {
        window.record("keys")["count"] == 1
    });
    window.wait_for_bun();
    assert_eq!(window.record("keys")["last"], serde_json::Value::Null);
}

/// A key that went down before the page had the keys starts nothing there as it repeats. A key
/// the computer keys hold stays theirs, also where the system does not mark a repeat (X11).
#[gpui::test]
fn a_key_that_went_down_elsewhere_repeats_into_nothing_on_the_page(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    let page = window.focused();
    window.cx.simulate_keystrokes("secondary-k");
    // The keys play where the window wired them, and the tool hears what reaches it.
    window.poll();
    window.poll();
    window.focus_the_window();
    window.cx.simulate_keystrokes("a");
    window.until("the note of the computer keys", |window| {
        window.record("keys")["last"] == 60
    });
    window.cx.update(|window, cx| window.focus(&page, cx));
    window.key_down("a", false);
    window.key_down("q", true);
    window.wait_for_bun();
    assert_eq!(window.record("keys")["count"], serde_json::Value::Null);
}

/// A key the page held repeats on once the focus left it, and starts no note of the computer
/// keys.
#[gpui::test]
fn a_key_the_page_held_repeats_into_nothing_on_the_computer_keys(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    window.cx.simulate_keystrokes("secondary-k");
    window.cx.simulate_keystrokes("a");
    window.until("the key on the page", |window| {
        window.record("keys")["count"] == 1
    });
    window.focus_the_window();
    window.key_down("a", true);
    window.wait_for_bun();
    assert_eq!(window.record("keys")["last"], serde_json::Value::Null);
}

#[gpui::test]
fn a_held_key_comes_up_with_ctrl_held_and_when_cmd_goes_down(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    // macOS sends no key up while cmd is held.
    window.cx.simulate_keystrokes("a");
    window.until("the note of the key", |window| window.loudest(480) > 0.1);
    window.cx.simulate_modifiers_change(Modifiers::command());
    window.until("the release as cmd goes down", |window| {
        window.loudest(480) == 0.0
    });
    window.cx.simulate_modifiers_change(Modifiers::none());

    window.cx.simulate_keystrokes("a");
    window.until("the note of the key", |window| window.loudest(480) > 0.1);
    // Ctrl went down while the key was held.
    let keystroke = Keystroke::parse("ctrl-a").unwrap();
    window.cx.simulate_event(KeyUpEvent { keystroke });
    window.until("the release of the key", |window| {
        window.loudest(480) == 0.0
    });
}

#[gpui::test]
fn every_key_counts_when_the_keys_come_faster_than_bun_answers(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    // All six go to Bun before the edit of the first comes back.
    window.cx.simulate_keystrokes("q w e r t y");
    window.until("six keys in the record", |window| {
        window.record("keys")["count"] == 6
    });
}

#[gpui::test]
fn a_page_that_opens_leaves_the_focus_where_it_is(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &ONLY_THE_TOOL) else {
        return;
    };
    // A control has the focus, as the composer of the agent does while it is typed in.
    let window_focus = window.cx.update(|window, cx| window.focused(cx));
    window.cx.simulate_keystrokes("tab");
    let focused = window.cx.update(|window, cx| window.focused(cx));
    assert!(focused.is_some() && focused != window_focus);

    // The agent adds the tool at the top, and its page opens.
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                project.set_json(&mut changes, id("keys"), "keys", serde_json::json!({}))?;
                project.commit("Add keys", changes)
            })
        })
    });
    window.poll();
    let shell = window.shell.clone();
    assert!(window.cx.read(|cx| shell.read(cx).main_view().is_some()));
    assert_eq!(window.cx.update(|window, cx| window.focused(cx)), focused);
}

#[gpui::test]
fn a_note_of_the_midi_keyboard_reaches_a_tool_at_the_top_and_still_sounds(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &EXPERIMENT) else {
        return;
    };
    // The keyboard plays into `keys`, the instance at the top.
    window.play_midi(64);
    window.until("the note in the record", |window| {
        window.record("keys")["last"] == 64
    });
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
        window.record("arrangement/one/listener")["last"] == 60
    });
    // Bun would have answered for both tracks at once.
    for _ in 0..10 {
        window.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        window.record("arrangement/two/listener")["last"],
        serde_json::Value::Null
    );
}

#[gpui::test]
fn a_tool_on_the_track_hears_the_computer_keys(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &TWO_TRACKS) else {
        return;
    };
    window.cx.simulate_keystrokes("secondary-k");
    window.poll();
    window.poll();
    window.cx.simulate_keystrokes("a");
    window.until("the note on the first track", |window| {
        window.record("arrangement/one/listener")["last"] == 60
    });
}

#[gpui::test]
fn a_tool_on_the_track_the_keyboard_leaves_hears_the_end_of_the_note_it_held(
    cx: &mut TestAppContext,
) {
    let Some(mut window) = open(cx, &TWO_TRACKS) else {
        return;
    };
    window.play_midi(60);
    window.until("the note held on the first track", |window| {
        window.record("arrangement/one/listener")["held"] == 1
    });
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.select(Some(id("arrangement/two")), cx)
        })
    });
    window.until("the end of the note on the first track", |window| {
        window.record("arrangement/one/listener")["held"] == 0
    });
}

/// A player of `sine.wav` at `level`, over and over.
fn player(level: f32) -> String {
    format!(
        r#"import {{ phasor, sample, tool }} from "./sdk";

tool({{
  name: "player",
  title: "Player",
  when: "You want a sound file to play",
  doc: "Plays a sound file over and over.",
  kind: "source",
  state: {{ sound: sample() }},
  sound: ({{ sound }}) => sound.at(phasor(1).times(sound.length)).times({level}),
}});
"#
    )
}

/// Writes a second of a 441 Hz sine at `level` to `assets/audio/sine.wav`.
fn write_sine(folder: &Path, level: f32) {
    let path = folder.join("assets/audio/sine.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..44_100 {
        let sample = (frame as f32 * 441.0 / 44_100.0 * std::f32::consts::TAU).sin() * level;
        writer
            .write_sample((sample * f32::from(i16::MAX)) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

#[gpui::test]
fn a_save_of_a_tool_keeps_its_sample_playing_and_does_not_read_it_again(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().to_path_buf();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "player", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(&root, "project.json", project);
    let record = r#"{"tool": "player", "state": {"sound": "sine.wav"}}"#;
    write(&root, "state/player.json", record);
    write(&root, "extensions/player.ts", &player(1.0));
    write_sine(&root, 0.5);
    let Some(mut window) = open_folder(cx, folder) else {
        return;
    };
    let level = window.loudest(4_800);
    assert!((level - 0.5).abs() < 0.02, "{level}");

    // The file turns silent with the same size and time: only a new read would hear that.
    let sine = root.join("assets/audio/sine.wav");
    let modified = std::fs::metadata(&sine).unwrap().modified().unwrap();
    write_sine(&root, 0.0);
    let file = std::fs::File::options().write(true).open(&sine).unwrap();
    file.set_modified(modified).unwrap();

    // The save plays the new sound, still with the sine it read at the start.
    write(&root, "extensions/player.ts", &player(0.5));
    window.until("the saved tool at half the level", |window| {
        (window.loudest(4_800) - 0.25).abs() < 0.02
    });
}

/// An effect `name` that keeps the last note of the MIDI keyboard in its record and plays at
/// half the level with `half`. When it `hangs`, its tick hangs once the engine runs: its check
/// at load runs at time 0, so it loads.
fn noted(name: &str, hangs: bool) -> String {
    let tick = if hangs {
        "tick: ({ time }) => { while (time > 0) {} },"
    } else {
        ""
    };
    format!(
        r#"import {{ choice, input, knob, tool }} from "./sdk";

tool({{
  name: "{name}",
  title: "{name}",
  when: "You want to know the last note played",
  doc: "Keeps the last note.",
  state: {{ last: knob({{ min: 0, max: 127, default: 0 }}), mode: choice({{ options: ["full", "half"], default: "full" }}) }},
  sound: ({{ mode }}) => (mode === "half" ? input.times(0.5) : input),
  {tick}
  onNotes: ({{ update }}, event) => {{
    if (event.type !== "noteOn") return;
    update("Keep the note", ({{ state }}) => {{
      state.last = event.pitch;
    }});
  }},
}});
"#
    )
}

#[gpui::test]
fn a_tool_whose_tick_hangs_is_a_problem_the_others_play_on_and_a_fix_brings_it_back(
    cx: &mut TestAppContext,
) {
    let (heard, stuck) = (noted("heard", false), noted("stuck", true));
    // Both on one track, which plays its notes into each.
    let files = [
        TWO_TRACKS[0],
        TWO_TRACKS[1],
        (
            "state/arrangement/one/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "One", "order": 0, "effects": ["heard", "stuck"]}}"#,
        ),
        (
            "state/arrangement/one/heard.json",
            r#"{"tool": "heard", "state": {}}"#,
        ),
        (
            "state/arrangement/one/stuck.json",
            r#"{"tool": "stuck", "state": {}}"#,
        ),
        ("extensions/heard.ts", heard.as_str()),
        ("extensions/stuck.ts", stuck.as_str()),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let root = window._folder.path().to_path_buf();
    // The engine runs and the loops tick: Bun hangs in the tick of `stuck`.
    window.wait_for_bun();

    // The window does not wait for Bun: a new choice plays a sound Bun sent with its load.
    let started = Instant::now();
    let session = window.session.clone();
    window.cx.update(|_, cx| {
        let mode = |state: &mut serde_json::Value| state["mode"] = "half".into();
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Half");
                project.update_json(&mut edit, &id("arrangement/one/heard"), mode)?;
                project.finish(edit)
            })
        })
    });
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(window.problems(), []);

    window.until("the hang among the problems", |window| {
        let problems = window.problems();
        problems
            .iter()
            .any(|problem| problem.path == "extensions/stuck.ts")
    });
    // A new Bun runs the other tools.
    window.play_midi(64);
    window.until("the note in the other tool", |window| {
        window.record("arrangement/one/heard")["last"] == 64
    });

    write(&root, "extensions/stuck.ts", &noted("stuck", false));
    window.until("the fix", |window| window.problems().is_empty());
    window.play_midi(65);
    window.until("the note in the fixed tool", |window| {
        window.record("arrangement/one/stuck")["last"] == 65
    });
}

#[gpui::test]
fn a_change_of_a_file_a_tool_imports_changes_its_sound(cx: &mut TestAppContext) {
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "hum", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    let hum = r#"import { sine, tool } from "./sdk";
import { LEVEL } from "./lib/level";

tool({
  name: "hum",
  title: "Hum",
  when: "You want a hum",
  doc: "A sine at the level of lib/level.ts.",
  kind: "source",
  state: {},
  sound: () => sine(441).times(LEVEL),
});
"#;
    let files = [
        ("project.json", project),
        ("state/hum.json", r#"{"tool": "hum", "state": {}}"#),
        ("extensions/hum.ts", hum),
        ("extensions/lib/level.ts", "export const LEVEL = 0.5;\n"),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let level = window.loudest(4_800);
    assert!((level - 0.5).abs() < 0.02, "{level}");

    let root = window._folder.path().to_path_buf();
    write(
        &root,
        "extensions/lib/level.ts",
        "export const LEVEL = 0.25;\n",
    );
    window.until("the hum at the new level", |window| {
        (window.loudest(4_800) - 0.25).abs() < 0.02
    });
}

/// A loop that never ends but keeps saying things, here moving a live control, is a hang of
/// its tool too.
#[gpui::test]
fn a_tick_that_never_ends_while_it_plays_is_a_problem(cx: &mut TestAppContext) {
    let flood = r#"import { input, live, tool } from "./sdk";

tool({
  name: "flood",
  title: "Flood",
  when: "You want a flood",
  doc: "Its loop never ends and moves a control all the time.",
  state: {},
  controls: { x: live({ min: 0, max: 1, default: 0 }) },
  sound: ({ x }) => input.times(x),
  tick: ({ time, set }) => {
    while (time > 0) set("x", 0);
  },
});
"#;
    let (_, project) = ONLY_THE_TOOL[0];
    let files = [
        ("project.json", project),
        ("state/flood.json", r#"{"tool": "flood", "state": {}}"#),
        ("extensions/flood.ts", flood),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    window.until("the flood among the problems", |window| {
        let problems = window.problems();
        problems
            .iter()
            .any(|problem| problem.path == "extensions/flood.ts")
    });
}

/// Code a timer of a tool runs between two requests hangs Bun: the request that waits then is
/// not to blame, and no tool is.
#[gpui::test]
fn a_hang_in_code_a_timer_runs_blames_no_tool(cx: &mut TestAppContext) {
    let timer = r#"import { input, tool } from "./sdk";

tool({
  name: "timer",
  title: "Timer",
  when: "You want a timer",
  doc: "Its loop starts a timer that never ends.",
  state: {},
  memory: () => ({ armed: false }),
  sound: () => input,
  tick: ({ memory, time }) => {
    if (time > 0 && !memory.armed) {
      memory.armed = true;
      setTimeout(() => { while (true) {} }, 0);
    }
  },
});
"#;
    let (_, project) = ONLY_THE_TOOL[0];
    let heard = noted("heard", false);
    let files = [
        ("project.json", project),
        ("state/heard.json", r#"{"tool": "heard", "state": {}}"#),
        ("state/timer.json", r#"{"tool": "timer", "state": {}}"#),
        ("extensions/heard.ts", heard.as_str()),
        ("extensions/timer.ts", timer),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    window.until("the hang among the problems", |window| {
        !window.problems().is_empty()
    });
    let problems = window.problems();
    let [problem] = problems.as_slice() else {
        panic!("{problems:?}");
    };
    assert_eq!(problem.path, "extensions/");
    assert!(problem.message.contains("no tool to blame"), "{problem:?}");
}

/// Plays the drone its record names into its own sound.
const BAND: &str = r#"import { text, tool } from "./sdk";

tool({
  name: "band",
  title: "Band",
  when: "You want one of two drones",
  doc: "Plays the drone `lead` names.",
  state: { lead: text({ default: "low" }) },
  behaviour: ({ state }) => ({ connections: [{ from: { child: state.lead }, to: { port: "audio" } }] }),
});
"#;

/// A sine at `hz`, all the time.
const DRONE: &str = r#"import { knob, sine, tool } from "./sdk";

tool({
  name: "drone",
  title: "Drone",
  when: "You want a drone",
  doc: "A sine.",
  kind: "source",
  state: { hz: knob({ min: 20, max: 2000, default: 220 }) },
  sound: ({ hz }) => sine(hz).times(0.25),
});
"#;

#[gpui::test]
fn an_owner_in_the_window_plays_its_new_wiring_once_bun_answers(cx: &mut TestAppContext) {
    let (_, project) = EXPERIMENT[0];
    let project = project.replace("\"keys\"", "\"band\"");
    let files = [
        ("project.json", project.as_str()),
        (
            "state/band/instance.json",
            r#"{"tool": "band", "state": {}}"#,
        ),
        ("state/band/low.json", r#"{"tool": "drone", "state": {}}"#),
        (
            "state/band/high.json",
            r#"{"tool": "drone", "state": {"hz": 880}}"#,
        ),
        ("extensions/band.ts", BAND),
        ("extensions/drone.ts", DRONE),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    // A tenth of a second of the left channel: how often it rises through zero.
    let rises = |window: &mut Window| {
        let mut output = vec![0.0_f32; 4_800 * OFFLINE.channels];
        window.engine.process_block(&mut output);
        let left: Vec<f32> = output.iter().step_by(OFFLINE.channels).copied().collect();
        left.windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count()
    };
    window.until("the low drone", |window| rises(window).abs_diff(22) <= 1);

    let session = window.session.clone();
    window.cx.update(|_, cx| {
        session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut edit = project.begin("Lead high");
                project.update_json(&mut edit, &id("band"), |state| {
                    state["lead"] = "high".into()
                })?;
                project.finish(edit)
            })
        })
    });
    // The edit plays what was declared before, until Bun answers.
    assert!(rises(&mut window).abs_diff(22) <= 1);
    window.until("the high drone", |window| rises(window).abs_diff(88) <= 1);
    assert_eq!(window.problems(), []);
}

/// A note a bar long that its sequencer `bass` loops on its child `synth` once a click on the
/// page launches it; a click while the page sees it play stops it. Nothing plays before.
const LAUNCHER: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "launcher",
  title: "Launcher",
  when: "You want to launch a pattern",
  doc: "A click launches a long note, another stops it.",
  state: {},
  behaviour: () => ({
    connections: [
      { from: { child: "bass", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { port: "audio" } },
    ],
  }),
  transport: true,
  page: ({ size, watches, set }) =>
    h("div", {
      style: { width: size.width, height: size.height },
      onClick: () => set({ child: "bass", live: "launch" }, watches["bass.playing"] === 0 ? -1 : 0),
    }, "Launch"),
});
"#;

/// Frames in a bar of 4/4 at 120 bpm.
const BAR_FRAMES: usize = 96_000;

#[gpui::test]
fn a_launch_from_a_page_lands_on_the_next_bar_line_and_a_stop_ends_the_notes(
    cx: &mut TestAppContext,
) {
    let (_, project) = EXPERIMENT[0];
    let project = project
        .replace("\"keys\"", "\"launcher\"")
        .replace("\"extensions\": []", "\"extensions\": [\"instrument\"]");
    let files = [
        ("project.json", project.as_str()),
        (
            "state/launcher/instance.json",
            r#"{"tool": "launcher", "state": {}}"#,
        ),
        (
            "state/launcher/synth.json",
            r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
        ),
        (
            "state/launcher/bass.json",
            r#"{"tool": "sequencer", "state": {"patterns": [{"length": 3840, "notes": [{"start": 0, "length": 3840, "pitch": 45, "velocity": 100}]}]}}"#,
        ),
        ("extensions/launcher.ts", LAUNCHER),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let play = window.find("play").expect("the play button");
    window.cx.simulate_click(play, Modifiers::default());
    let middle = window.cx.update(|window, _| window.viewport_size()) / 2.;
    let middle = point(middle.width, middle.height);
    let queued = |window: &mut Window| {
        let session = window.session.clone();
        let bass = id("launcher/bass");
        let watches = (window.cx).read(|cx| session.read(cx).project().watches(&bass));
        let queued = watches.iter().find(|(name, _)| name == "queued");
        queued.map(|(_, watch)| watch.get())
    };
    window.until("the sequencer", |window| queued(window).is_some());
    window.cx.simulate_click(middle, Modifiers::default());
    window.until("the launch", |window| queued(window) == Some(0.0));
    assert!(window.playing());

    // Frames from the bar line on, in blocks of 64 as the device asks: silent until it.
    let to_the_bar = |window: &mut Window| {
        let session = window.session.clone();
        let status = window
            .cx
            .update(|_, cx| session.update(cx, |session, _| session.engine().poll().unwrap()));
        let frame = status.playhead_frame.0 as usize;
        let bar = frame.div_ceil(BAR_FRAMES) * BAR_FRAMES;
        let before = window.loudest(bar - frame);
        (bar, before)
    };
    let (bar, before) = to_the_bar(&mut window);
    assert_eq!(before, 0.0, "before the bar line at {bar}");
    let mut first = vec![0.0_f32; 64 * OFFLINE.channels];
    window.engine.process_block(&mut first);
    let left: Vec<f32> = first.iter().step_by(OFFLINE.channels).copied().collect();
    let sounding = left.iter().position(|sample| sample.abs() > 1e-4);
    assert!(sounding.is_some_and(|frame| frame < 48), "{left:?}");

    window.until("the stop", |window| {
        window.cx.simulate_click(middle, Modifiers::default());
        queued(window) == Some(sound_notes::QUEUED_STOP)
    });
    // It plays on to the bar line, and is silent from a release after it.
    let (bar, before) = to_the_bar(&mut window);
    assert!(before > 0.1, "{before} before the bar line at {bar}");
    window.loudest(960);
    assert_eq!(window.loudest(2 * BAR_FRAMES), 0.0);
    assert_eq!(window.problems(), []);
}

/// A parent whose page plays its children, one click each: a note on the built-in `synth`, a
/// note on the tool `keys`, the live control and the trigger of `drop`; then a trigger `drop`
/// does not have, and a child it does not own.
const CONDUCTOR: &str = r#"import { h, tool } from "./sdk";

tool({
  name: "conductor",
  title: "Conductor",
  when: "You want to play children from a page",
  doc: "Each click plays the next child.",
  state: {},
  memory: () => ({ clicks: 0 }),
  behaviour: () => ({ connections: ["synth", "keys", "drop"].map((child) => ({ from: { child }, to: { port: "audio" } })) }),
  page: ({ size, memory, play, set, fire }) =>
    h("div", {
      style: { width: size.width, height: size.height },
      onClick: () => {
        memory.clicks += 1;
        if (memory.clicks === 1) play(45, { child: "synth", seconds: 0.1 });
        if (memory.clicks === 2) play(69, { child: "keys", seconds: 0.1 });
        if (memory.clicks === 3) {
          set({ child: "drop", live: "gain" }, 1);
          fire({ child: "drop", trigger: "hit" });
        }
        if (memory.clicks === 4) fire({ child: "drop", trigger: "hat" });
        if (memory.clicks === 5) play(60, { child: "drum" });
      },
    }, "Play"),
});
"#;

/// Noise that jumps up when `hit` fires and falls away, silent until `gain` is up.
const DROP: &str = r#"import { feedback, live, max, noise, tool, trigger } from "./sdk";

tool({
  name: "drop",
  title: "Drop",
  when: "You want a hit",
  doc: "A burst of noise.",
  kind: "source",
  state: {},
  controls: { hit: trigger(), gain: live({ min: 0, max: 1, default: 0 }) },
  sound: ({ hit, gain }) => {
    const level = feedback();
    level.set(max(hit, level.times(0.9995)));
    return noise().times(level).times(gain);
  },
});
"#;

#[gpui::test]
fn a_page_of_a_parent_plays_notes_on_two_children_and_the_controls_of_a_third(
    cx: &mut TestAppContext,
) {
    let (_, project) = EXPERIMENT[0];
    let project = project
        .replace("\"keys\"", "\"conductor\"")
        .replace("\"extensions\": []", "\"extensions\": [\"instrument\"]");
    let files = [
        ("project.json", project.as_str()),
        (
            "state/conductor/instance.json",
            r#"{"tool": "conductor", "state": {}}"#,
        ),
        (
            "state/conductor/synth.json",
            r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
        ),
        (
            "state/conductor/keys.json",
            r#"{"tool": "keys", "state": {}}"#,
        ),
        (
            "state/conductor/drop.json",
            r#"{"tool": "drop", "state": {}}"#,
        ),
        ("extensions/conductor.ts", CONDUCTOR),
        ("extensions/keys.ts", KEYS),
        ("extensions/drop.ts", DROP),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    let middle = window.cx.update(|window, _| window.viewport_size()) / 2.;
    let middle = point(middle.width, middle.height);
    window.wait_for_bun();
    for child in ["synth", "keys", "drop"] {
        assert_eq!(window.loudest(4_800), 0.0, "before the click for {child}");
        window.cx.simulate_click(middle, Modifiers::default());
        window.until(child, |window| window.loudest(480) > 0.05);
        // The notes end; the noise of `drop` only fades.
        if child != "drop" {
            window.until("silence", |window| window.loudest(4_800) == 0.0);
        }
    }
    assert_eq!(window.problems(), []);

    let problem = |message: &str| Problem {
        path: "extensions/conductor.ts".to_string(),
        message: format!("tool conductor: a performer plays the child {message}"),
    };
    window.cx.simulate_click(middle, Modifiers::default());
    window.until("the problem of the trigger", |window| {
        !window.problems().is_empty()
    });
    let trigger = problem("\"drop\", which has no trigger hat: its triggers are hit");
    assert_eq!(window.problems(), std::slice::from_ref(&trigger));
    window.cx.simulate_click(middle, Modifiers::default());
    // A file shows its newest problem.
    window.until("the problem of the child", |window| {
        window.problems() != std::slice::from_ref(&trigger)
    });
    let child =
        problem("\"drum\", which conductor does not own: its children are drop, keys, synth");
    assert_eq!(window.problems(), [child]);
}

/// An instrument that keeps every note event it hears in its record, with its time.
const HEARER: &str = r#"import { list, note, number, sine, text, tool } from "./sdk";

tool({
  name: "hearer",
  title: "Hearer",
  when: "You want to know what reaches an instrument",
  doc: "Keeps every note event it hears, with its time.",
  kind: "instrument",
  state: { heard: list({ type: text(), at: number({ min: 0, max: 100000 }) }) },
  sound: () => sine(note.freq).times(note.gate).times(0.1),
  onNotes: ({ update }, event) => {
    update("Hear", ({ state }) => {
      state.heard.push({ type: event.type, at: event.at });
    });
  },
});
"#;

impl Window<'_> {
    /// The engine frame of the next block, the clock `time` and `at` count on.
    fn engine_frame(&mut self) -> u64 {
        let session = self.session.clone();
        (self.cx).update(|_, cx| {
            session.update(cx, |session, _| session.engine().poll().unwrap().frames)
        })
    }

    /// The right channel of the next `frames`, in blocks of 64 as the device asks.
    fn right(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0_f32; frames * OFFLINE.channels];
        for buffer in output.chunks_mut(64 * OFFLINE.channels) {
            self.engine.process_block(buffer);
        }
        let right = output.iter().skip(1).step_by(OFFLINE.channels);
        right.copied().collect()
    }
}

/// The frame of a time in seconds on the clock of the sound, at the rate of the tests.
fn frame_of(seconds: f64) -> u64 {
    (seconds * f64::from(OFFLINE.sample_rate)).round() as u64
}

#[gpui::test]
fn an_instrument_hears_the_clip_of_its_track_at_the_samples_it_sounds(cx: &mut TestAppContext) {
    let clip = r#"{"tool": "arrangement.clip", "state": {"start": 0, "length": 3840,
  "notes": [{"start": 1000, "length": 500, "pitch": 60, "velocity": 100}, {"start": 2000, "length": 100, "pitch": 62, "velocity": 100}],
  "pedal": [{"start": 1500, "value": 127}]}}"#;
    let files = [
        TWO_TRACKS[0],
        TWO_TRACKS[1],
        (
            "state/arrangement/one/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "One", "order": 0}}"#,
        ),
        (
            "state/arrangement/one/instrument.json",
            r#"{"tool": "hearer", "state": {}}"#,
        ),
        ("state/arrangement/one/take.json", clip),
        ("extensions/hearer.ts", HEARER),
    ];
    let Some(mut window) = open(cx, &files) else {
        return;
    };
    window.wait_for_bun();
    let start = window.engine_frame();
    let session = window.session.clone();
    (window.cx).update(|_, cx| session.update(cx, |session, _| session.engine().play()));
    // Past the last note: 2100 ticks, 25 frames each at 120 bpm.
    window.loudest(54_000);
    let heard = |window: &mut Window| window.record("arrangement/one/instrument")["heard"].clone();
    window.until("every event of the clip", |window| {
        heard(window)
            .as_array()
            .is_some_and(|heard| heard.len() >= 5)
    });
    let heard: Vec<(String, u64)> = (heard(&mut window).as_array().unwrap().iter())
        .map(|event| {
            let at = frame_of(event["at"].as_f64().unwrap());
            (event["type"].as_str().unwrap().to_string(), at - start)
        })
        .collect();
    let expected = [
        ("noteOn", 25_000),
        ("cc", 37_500),
        ("noteOff", 37_500),
        ("noteOn", 50_000),
        ("noteOff", 52_500),
    ];
    let expected: Vec<(String, u64)> = (expected.iter())
        .map(|(kind, frame)| (kind.to_string(), *frame))
        .collect();
    assert_eq!(heard, expected);
}

/// A parent that sends controller 74 into its plugin child half a second after a key on its
/// page, and keeps that time in its record.
const CONDUCTOR_OF_A_PLUGIN: &str = r#"import { h, number, tool } from "./sdk";

tool({
  name: "conductor",
  title: "Conductor",
  when: "You want to move the controllers of a plugin",
  doc: "A key on its page moves controller 74 of its plugin.",
  state: { at: number({ min: 0, max: 100000 }) },
  behaviour: () => ({ connections: [{ from: { child: "piano" }, to: { port: "audio" } }] }),
  onKey: ({ send, update, time }, { key, down }) => {
    if (key !== "a" || !down) return;
    const at = time + 0.5;
    send({ type: "cc", controller: 74, value: 1 }, { child: "piano", at });
    update("Keep the time", ({ state }) => {
      state.at = at;
    });
  },
  page: ({ size }) => h("div", { style: { width: size.width, height: size.height } }, "Conduct"),
});
"#;

#[gpui::test]
fn a_controller_a_parent_sends_reaches_its_plugin_child_on_its_sample(cx: &mut TestAppContext) {
    // SAFETY: nextest runs one test per process and no other thread reads the environment yet.
    unsafe { std::env::set_var(test_plugin_support::SHOW_VARIABLE, "controller") };
    let plugin = crate::support::test_plugin_record(plugin_host::PluginFormat::Clap, "piano");
    let files = [
        (
            "project.json",
            r#"{"format": 1, "extensions": ["plugin-host"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "conductor", "port": "audio"}, "to": {"device_output": 0}}]}"#,
        ),
        (
            "state/conductor/instance.json",
            r#"{"tool": "conductor", "state": {}}"#,
        ),
        ("state/conductor/piano.json", plugin.as_str()),
        ("extensions/conductor.ts", CONDUCTOR_OF_A_PLUGIN),
    ];
    let Some(mut window) = open_with_test_plugins(cx, &files) else {
        return;
    };
    window.until("the plugin", |window| window.problems().is_empty());
    window.cx.simulate_keystrokes("a");
    window.until("the time it was sent at", |window| {
        (window.record("conductor")["at"].as_f64()).is_some_and(|at| at > 0.0)
    });
    let sent = frame_of(window.record("conductor")["at"].as_f64().unwrap());
    let now = window.engine_frame();
    // The plugin shows the controller on its right channel.
    let right = window.right(usize::try_from(sent - now).unwrap() + 1_000);
    let shown = right.iter().position(|sample| *sample > 0.25).unwrap() as u64;
    assert_eq!(now + shown, sent);
}
