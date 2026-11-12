//! A tool of the project whose children have children: a song of tracks of clips. Its page
//! reads a clip, its code edits a clip and takes a track out with all it holds, each one undo
//! step.

use gpui::TestAppContext;

use super::{Opened, open};

/// A song whose page is as wide as the start of the clip `verse` of the track `piano`, over
/// ten. `m` moves the verse a bar on, `d` takes the piano out, `a` adds a track with a clip.
const SONG: &str = r#"import { h, Space, tool, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", "*": "timeline" } as const;

export const track = tool({
  name: "track",
  title: "Track",
  when: "Never",
  doc: "Plays its clips on its synth.",
  state: {},
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => ({
    connections: [
      ...children
        .filter((child) => child.tool === "timeline")
        .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } })),
      { from: { child: "synth" }, to: { port: "audio" } },
    ],
  }),
});

const tracks = { "*": track } as const;

tool({
  name: "song",
  title: "Song",
  when: "Never",
  doc: "Plays its tracks.",
  state: {},
  children: tracks,
  behaviour: ({ children }): Declaration<typeof tracks> => ({
    connections: children.map((child) => ({ from: { child: child.name }, to: { port: "audio" } })),
  }),
  onKey: ({ update }, { key, down }) => {
    if (!down) return;
    if (key === "m") update("Move the verse", ({ children }) => {
      const verse = children.find((track) => track.name === "piano")?.children?.find((clip) => clip.name === "verse");
      if (verse?.tool === "timeline") verse.state.start += 3840;
    });
    if (key === "d") update("Take the piano out", (part) => {
      part.children = part.children.filter((track) => track.name !== "piano");
    });
    if (key === "a") update("Add a bass", ({ children }) => {
      children.push({
        name: "bass",
        tool: "track",
        state: {},
        children: [
          { name: "synth", tool: "instrument.synth", state: {} },
          { name: "line", tool: "timeline", state: { start: 0, length: 3840, notes: [] } },
        ],
      });
    });
  },
  view: ({ children }) =>
    h(Space, {
      key: "probe",
      height: 20,
      width: () => {
        const verse = children.find((track) => track.name === "piano")?.children.find((clip) => clip.name === "verse");
        return verse?.tool === "timeline" ? verse.state.start / 10 : 1;
      },
    }),
});
"#;

const PROJECT: &str = r#"{"format": 1, "extensions": ["instrument"],
  "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
  "connections": [{"from": {"instance": "song", "port": "audio"}, "to": {"device_output": 0}}]}"#;

fn open_song(cx: &mut TestAppContext) -> Opened<'_> {
    let files = [
        ("project.json", PROJECT),
        ("extensions/song.ts", SONG),
        (
            "state/song/instance.json",
            r#"{"tool": "song", "state": {}}"#,
        ),
        (
            "state/song/piano/instance.json",
            r#"{"tool": "track", "state": {}}"#,
        ),
        (
            "state/song/piano/synth.json",
            r#"{"tool": "instrument.synth", "state": {}}"#,
        ),
        (
            "state/song/piano/verse.json",
            r#"{"tool": "timeline", "state": {"start": 3840, "length": 3840, "notes": []}}"#,
        ),
    ];
    open(cx, &files)
}

impl Opened<'_> {
    /// The width of the space `probe`, once it shows.
    fn probe(&mut self) -> f32 {
        self.until("the probe", |window| window.find("space-probe").is_some());
        let bounds = self.cx.debug_bounds("space-probe").unwrap();
        f32::from(bounds.size.width)
    }

    fn exists(&self, path: &str) -> bool {
        self.folder.path().join(path).exists()
    }
}

#[gpui::test]
fn a_page_reads_a_clip_of_a_track_and_an_edit_of_it_is_one_undo_step(cx: &mut TestAppContext) {
    let mut window = open_song(cx);
    assert_eq!(window.probe(), 384.0);

    window.cx.simulate_keystrokes("m");
    window.until("the verse moved", |window| window.probe() == 768.0);
    window.after_the_runs();
    assert_eq!(window.record("song/piano/verse")["start"], 7680);
    assert_eq!(window.undo_label().as_deref(), Some("Move the verse"));

    // Undo tells Bun the clip as it was, and the page follows.
    assert!(window.undo().is_some());
    window.until("the verse back", |window| window.probe() == 384.0);
    assert_eq!(window.record("song/piano/verse")["start"], 3840);
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_track_taken_out_goes_with_what_it_holds_and_one_added_comes_with_its_clips(
    cx: &mut TestAppContext,
) {
    let mut window = open_song(cx);
    assert_eq!(window.probe(), 384.0);

    window.cx.simulate_keystrokes("d");
    window.until("the piano gone", |window| window.probe() == 1.0);
    window.after_the_runs();
    for path in ["instance.json", "synth.json", "verse.json"] {
        assert!(
            !window.exists(&format!("state/song/piano/{path}")),
            "{path}"
        );
    }
    assert!(window.undo().is_some());
    window.until("the piano back", |window| window.probe() == 384.0);
    assert!(window.exists("state/song/piano/verse.json"));

    window.cx.simulate_keystrokes("a");
    window.until("the bass", |window| {
        window.exists("state/song/bass/line.json")
    });
    window.after_the_runs();
    assert!(window.exists("state/song/bass/synth.json"));
    assert_eq!(window.undo_label().as_deref(), Some("Add a bass"));
    assert_eq!(window.problems(), []);
}

/// Tools a parent asks about: one whose sound reads its notes, one that plays its own steps in
/// time, and one whose code hears notes. The page says what `hearsNotes` answers of each, and
/// of a built-in instrument.
const HEARING: &str = r#"import { beat, h, hearsNotes, note, sine, tool } from "./sdk";

tool({ name: "voice", title: "Voice", when: "Never", doc: "Plays notes.", state: {}, sound: () => sine(note.freq).times(note.gate) });
tool({ name: "steps", title: "Steps", when: "Never", doc: "Plays its own steps.", state: {}, sound: () => sine(220).times(beat.mod(1).lt(0.5)) });
tool({ name: "listens", title: "Listens", when: "Never", doc: "Its code hears notes.", state: {}, sound: () => sine(220), onNotes: () => {} });

tool({
  name: "asks",
  title: "Asks",
  when: "Never",
  doc: "Says which tools notes play.",
  state: {},
  view: () => h("div", { key: "page" }, ...["voice", "steps", "listens", "instrument.synth"].map((name) => h("div", { key: `${name}-${hearsNotes(name)}` }, name))),
});
"#;

/// A parent learns whether notes play a tool: not one of the project that makes its own
/// notes, so a track can show its clips as not played.
#[gpui::test]
fn a_parent_learns_whether_notes_play_a_tool(cx: &mut TestAppContext) {
    let mut window = super::parts::page(cx, "asks", HEARING);
    for answer in [
        "voice-true",
        "steps-false",
        "listens-true",
        "instrument.synth-true",
    ] {
        window.until_found(&format!("div-{answer}"));
    }
    assert_eq!(window.problems(), []);
}
