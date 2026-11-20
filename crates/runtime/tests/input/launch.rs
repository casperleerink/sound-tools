//! Code launches a `timeline` child as a performer does: a number into its input `launch`
//! starts it at the next bar line and 0 stops it there; nothing is saved.

use crate::support::{open, write};
use runtime::input::Devices;

const LAUNCHER: &str = r#"import { h, tool } from "./sdk";

export const launcher = tool({
  name: "launcher", title: "Launcher", when: "Never", doc: "Launches a long note, and stops it.",
  state: {},
  behaviour: () => ({
    connections: [
      { from: { child: "bass", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { port: "audio" } },
    ],
  }),
  view: (_, { send, time }) => (
    <row>
      <box key="later" label="Later" onPress={() => send({ child: "bass", number: "launch" }, 1, { at: time + 3 })} />
      <box key="go" label="Go" onPress={() => send({ child: "bass", number: "launch" }, 1)} />
      <box key="stop" label="Stop" onPress={() => send({ child: "bass", number: "launch" }, 0)} />
      <box key="wrong" label="Wrong" onPress={() => send({ child: "bass", number: "volume" }, 1)} />
    </row>
  ),
});
"#;

/// Frames in a bar of 4/4 at 120 bpm.
const BAR: usize = 96_000;

/// The launcher at the top, playing its synth, with a bar long `bass` waiting for a launch.
fn open_launcher() -> crate::support::Opened {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "launcher", "port": "audio"}, "to": {"device_output": 0}}]}"#;
    write(root, "project.json", project);
    write(root, "extensions/theme.json", "{}");
    write(root, "extensions/launcher.tsx", LAUNCHER);
    write(
        root,
        "state/launcher/instance.json",
        r#"{"tool": "launcher", "state": {}}"#,
    );
    write(
        root,
        "state/launcher/synth.json",
        r#"{"tool": "instrument.synth", "state": {"release_seconds": 0.01}}"#,
    );
    let bass = r#"{"tool": "timeline", "state": {"length": 3840, "loop": true, "plays": false, "notes": [{"start": 0, "length": 3840, "pitch": 45, "velocity": 100}]}}"#;
    write(root, "state/launcher/bass.json", bass);
    open(folder, Devices::default())
}

#[test]
fn a_launch_from_code_starts_a_timeline_at_the_bar_line_and_a_stop_ends_it() {
    let mut opened = open_launcher();
    opened.start_playing();
    assert_eq!(opened.loudest(BAR / 2), 0.0);

    opened.press("launcher", "go");
    // Silent until the bar line, then it plays.
    opened.loudest(BAR);
    assert!(opened.loudest(BAR / 4) > 0.01);
    opened.press("launcher", "stop");
    opened.loudest(BAR);
    assert_eq!(opened.loudest(BAR / 4), 0.0);
    // Nothing is saved.
    assert_eq!(opened.record("launcher/bass")["plays"], false);
    let names_launch = |opened: &crate::support::Opened| {
        let problems = opened.project.problems();
        problems
            .iter()
            .any(|problem| problem.message.contains("`launch`"))
    };
    assert!(!names_launch(&opened));

    // A timeline takes no other number.
    opened.press("launcher", "wrong");
    assert!(names_launch(&opened), "{:?}", opened.project.problems());
}

/// A launch sent for later waits for its time, then for the bar line after it.
#[test]
fn a_launch_sent_for_later_waits_for_its_time() {
    let mut opened = open_launcher();
    opened.start_playing();
    opened.press("launcher", "later");
    // Three seconds is a bar and a half: silent until the bar line after it.
    assert_eq!(opened.loudest(BAR * 3 / 2), 0.0);
    opened.loudest(BAR / 2);
    assert!(opened.loudest(BAR / 4) > 0.01);
}
