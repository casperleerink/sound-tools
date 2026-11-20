//! The MIDI keyboard and the computer keys play into the `notes` of where the composer works,
//! and a tool hears them with `onNotes`.

use runtime::input::{Devices, KeyDown};
use serde_json::Value;

use crate::support::{Opened, id, open, write};

/// Keeps the last note it heard and how many it holds, and plays a sine of each.
const LISTENER: &str = r#"import { adsr, note, number, sine, tool } from "./sdk";

export const listener = tool({
  name: "listener",
  title: "Listener",
  when: "Never",
  doc: "Plays a sine of each note and keeps the last note and how many are held.",
  voices: 8,
  state: { last: number({ min: 0, max: 127, default: 0 }), held: number({ min: 0, max: 128, default: 0 }) },
  sound: () => sine(note.freq).times(adsr(note.gate, 0.001, 0.001, 1, 0.001)).times(0.2),
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

/// Two listeners at the top, `one` heard.
fn open_listeners() -> Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": [],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "one", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/listener.ts", LISTENER);
    write(root, "state/one.json", r#"{"tool": "listener", "state": {}}"#);
    write(root, "state/two.json", r#"{"tool": "listener", "state": {}}"#);
    open(folder, Devices::default())
}

fn plain(key: &str) -> KeyDown<'_> {
    KeyDown {
        key,
        modified: false,
        repeat: false,
        free: true,
    }
}

impl Opened {
    fn last(&self, instance: &str) -> Value {
        self.record(instance)["last"].clone()
    }

    fn held(&self, instance: &str) -> Value {
        self.record(instance)["held"].clone()
    }

    /// The engine plays a little, and the window polls.
    fn play_on(&mut self) {
        self.render(512);
        self.settle();
    }
}

#[test]
fn a_note_of_the_midi_keyboard_reaches_every_tool_at_the_top_and_sounds() {
    let mut opened = open_listeners();
    opened.play_midi(64);
    opened.until("the note in both", |opened| {
        opened.play_on();
        opened.last("one") == 64 && opened.last("two") == 64
    });
    // The instrument played it as well, as a keyboard plays any instrument.
    assert!(opened.loudest(480) > 0.1);
}

#[test]
fn the_selected_tool_hears_the_keyboard_and_another_does_not() {
    let mut opened = open_listeners();
    opened.selected = Some(id("one"));
    opened.play_midi(60);
    opened.until("the note in the first", |opened| {
        opened.play_on();
        opened.last("one") == 60
    });
    for _ in 0..10 {
        opened.play_on();
    }
    assert_eq!(opened.last("two"), Value::Null);
}

#[test]
fn a_tool_the_keyboard_leaves_hears_the_end_of_the_note_it_held() {
    let mut opened = open_listeners();
    opened.selected = Some(id("one"));
    opened.play_midi(60);
    opened.until("the note held in the first", |opened| {
        opened.play_on();
        opened.held("one") == 1
    });
    opened.selected = Some(id("two"));
    opened.until("the end of the note in the first", |opened| {
        opened.play_on();
        opened.held("one") == 0
    });
}

#[test]
fn the_selected_tool_hears_the_computer_keys() {
    let mut opened = open_listeners();
    opened.selected = Some(id("two"));
    assert!(opened.input.set_computer_keys(true));
    opened.settle();
    assert!(opened.input.computer_key_down(plain("a")));
    opened.until("the note in the second", |opened| {
        opened.play_on();
        opened.last("two") == 60
    });
    assert!(opened.input.computer_key_up("a"));
    opened.until("the note let go", |opened| {
        opened.play_on();
        opened.held("two") == 0
    });
    assert_eq!(opened.last("one"), Value::Null);
}

/// A computer key plays only when it is plain and the plain keys are free; a key they hold
/// stays theirs, and its repeats start nothing.
#[test]
fn the_computer_keys_take_only_plain_free_keys_and_keep_what_they_hold() {
    let mut opened = open_listeners();
    assert!(!opened.input.computer_key_down(plain("a")));
    opened.input.set_computer_keys(true);
    let modified = KeyDown {
        modified: true,
        ..plain("a")
    };
    assert!(!opened.input.computer_key_down(modified));
    let busy = KeyDown {
        free: false,
        ..plain("a")
    };
    assert!(!opened.input.computer_key_down(busy));
    assert!(!opened.input.computer_key_up("a"));

    // A repeat of a key that went down elsewhere is theirs and starts nothing.
    let repeat = KeyDown {
        repeat: true,
        ..plain("s")
    };
    assert!(opened.input.computer_key_down(repeat));
    // A key they hold stays theirs where the plain keys are not free.
    assert!(opened.input.computer_key_down(plain("a")));
    assert!(opened.input.computer_key_down(busy));
    for _ in 0..10 {
        opened.play_on();
    }
    assert_eq!(opened.held("one"), 1);
}

/// As cmd goes down or the window loses the keys, every note the computer keys hold ends:
/// their key ups will not come.
#[test]
fn letting_go_of_the_computer_keys_ends_every_note_they_hold() {
    let mut opened = open_listeners();
    opened.input.set_computer_keys(true);
    opened.settle();
    opened.input.computer_key_down(plain("a"));
    opened.input.computer_key_down(plain("s"));
    opened.until("two held", |opened| {
        opened.play_on();
        opened.held("one") == 2
    });
    opened.input.let_go_of_computer_keys();
    opened.until("none held", |opened| {
        opened.play_on();
        opened.held("one") == 0
    });
    // Off ends what they hold too.
    opened.input.computer_key_down(plain("d"));
    opened.until("one held", |opened| {
        opened.play_on();
        opened.held("one") == 1
    });
    assert!(!opened.input.set_computer_keys(false));
    opened.until("none held again", |opened| {
        opened.play_on();
        opened.held("one") == 0
    });
}

