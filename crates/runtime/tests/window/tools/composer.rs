//! The piece of the Composer template (`templates/composer`): a bar made longer pushes what
//! comes after it and nothing else, a bar added or removed moves the rest, a section started at
//! a bar takes the music after it along, a section moved takes its music with it, and a part an
//! agent left in the wrong place is laid out again. Each edit is one undo step.

use std::path::Path;

use gpui::{Pixels, Point, TestAppContext, point, px};
use serde_json::{Value, json};

use super::{Opened, open_folder};
use crate::song::BAR;
use crate::support::{mark, one_undo_step, write_outside};

const BEAT: u64 = 960;
/// A row of notes is 64 points tall, as `piece-page.ts` has it.
const ROW: f32 = 64.;
/// The notes of `t00` in section A, one in each of its first three bars, and in B.
const IN_A: [u64; 3] = [0, BAR + BEAT / 2, 2 * BAR];
const IN_B: [u64; 1] = [0];

fn composer() -> &'static runtime::Template {
    runtime::templates::named("Composer").unwrap()
}

fn write_record(root: &Path, path: &str, tool: &str, state: Value) {
    let path = root.join("state/piece").join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let record = json!({ "tool": tool, "state": state });
    std::fs::write(path, record.to_string()).unwrap();
}

fn notes(starts: &[u64], pitch: u64) -> Value {
    let notes: Vec<Value> = (starts.iter())
        .map(|start| json!({ "start": start, "length": BEAT / 2, "pitch": pitch, "velocity": 100 }))
        .collect();
    notes.into()
}

/// A project of the Composer template with the piece an agent would write, laid out: section A
/// of four bars of 4/4 at 120, then B of two at 90; the instruments `t00`, with the notes
/// [`IN_A`] and [`IN_B`], and `t01`, with none.
fn open_piece(cx: &mut TestAppContext) -> Opened<'_> {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    runtime::templates::write(root, composer()).unwrap();
    let file = root.join("project.json");
    let mut project: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    project["tempo_map"] = json!({
        "time_signatures": [{ "signature": "4/4", "bars": 6 }],
        "tempo_changes": [{ "tick": 0, "bpm": 120.0 }, { "tick": 4 * BAR, "bpm": 90.0 }],
    });
    project["sections"] = json!([{ "bar": 1, "name": "A" }, { "bar": 5, "name": "B" }]);
    std::fs::write(&file, project.to_string()).unwrap();
    let sections = json!([
        { "id": "sec-a", "name": "A", "bars": [{ "bars": 4, "signature": "4/4", "bpm": 120 }] },
        { "id": "sec-b", "name": "B", "bars": [{ "bars": 2, "signature": "4/4", "bpm": 90 }] },
    ]);
    let piece = json!({ "sections": sections, "order": ["t00", "t01"] });
    write_record(root, "instance.json", "piece", piece);
    // The template's section and its instrument's records, for the second.
    std::fs::remove_dir_all(root.join("state/piece/t00")).unwrap();
    for (name, colour) in [("t00", "blue"), ("t01", "peach")] {
        write_record(
            root,
            &format!("{name}/instance.json"),
            "track",
            json!({ "colour": colour }),
        );
        write_record(
            root,
            &format!("{name}/synth.json"),
            "instrument.synth",
            json!({}),
        );
        write_record(root, &format!("{name}/mix.json"), "fader", json!({}));
        write_record(
            root,
            &format!("{name}/chain/instance.json"),
            "chain",
            json!({}),
        );
    }
    let part = |start: u64, length: u64, starts: &[u64]| json!({ "start": start, "length": length, "notes": notes(starts, 60) });
    write_record(root, "t00/sec-a.json", "timeline", part(0, 4 * BAR, &IN_A));
    write_record(
        root,
        "t00/sec-b.json",
        "timeline",
        part(4 * BAR, 2 * BAR, &IN_B),
    );
    let mut window = open_folder(cx, folder);
    window.wait_for_bun();
    window.after_the_runs();
    assert_eq!(window.problems(), []);
    // Laid out as the piece lays it out: it made no edit of its own.
    assert_eq!(window.undo_label(), None);
    window
}

impl Opened<'_> {
    /// Where the strip over the rows has `x` points of the open section, half way down.
    fn on_strip(&mut self, x: f32) -> Point<Pixels> {
        self.space_at("strip") + point(px(x), px(22.))
    }

    /// Where the row of the `row`th instrument has `x` points of the open section.
    fn on_row(&mut self, x: f32, row: usize) -> Point<Pixels> {
        self.space_at("rows") + point(px(x), px(row as f32 * ROW + ROW / 2.))
    }

    /// Chooses the bar `bar` of the open section, from 0: a click on the first, then the arrow.
    fn choose_bar(&mut self, bar: usize) {
        let first = self.on_strip(10.);
        self.click(first);
        self.wait_for_bun();
        for _ in 0..bar {
            self.keys("right");
            self.wait_for_bun();
        }
        self.until_shows(&format!("Bar {}", bar + 1));
    }

    /// Waits for the page to show `text`, such as the head's caption of the chosen bars.
    fn until_shows(&mut self, text: &str) {
        let shell = self.shell.clone();
        self.until(text, |window| {
            window.cx.read(|cx| {
                let page = shell.read(cx).main_view().unwrap();
                sound_typescript::page_has_text(page, text, cx)
            })
        });
    }

    /// The start of the part of `track` in the section `id`, and where each of its notes starts.
    fn part(&mut self, track: &str, id: &str) -> (u64, Vec<u64>) {
        let record = self.record(&format!("piece/{track}/{id}"));
        let starts = (record["notes"].as_array().unwrap().iter())
            .map(|note| note["start"].as_u64().unwrap())
            .collect();
        (record["start"].as_u64().unwrap(), starts)
    }

    /// Where every note of `track` plays in the piece, in ticks from its start: what is heard.
    fn heard(&mut self, track: &str) -> Vec<(u64, u64)> {
        let parts: Vec<String> = self.project(|project| {
            (project.instances())
                .filter(|(id, tool)| {
                    *tool == "timeline"
                        && id
                            .parent()
                            .is_some_and(|parent| parent.as_str() == format!("piece/{track}"))
                })
                .map(|(id, _)| id.to_string())
                .collect()
        });
        let mut heard = Vec::new();
        for part in parts {
            let record = self.record(&part);
            let start = record["start"].as_u64().unwrap();
            for note in record["notes"].as_array().unwrap() {
                heard.push((
                    start + note["start"].as_u64().unwrap(),
                    note["length"].as_u64().unwrap(),
                ));
            }
        }
        heard.sort_unstable();
        heard
    }

    fn sections(&mut self) -> Value {
        self.record("piece")["sections"].clone()
    }
}

#[gpui::test]
fn a_new_composer_project_opens_its_piece_and_its_first_instrument_plays(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), composer()).unwrap();
    let mut window = open_folder(cx, folder);
    window.wait_for_bun();
    window.until_found("div-section-sec-section-1");
    window.after_the_runs();
    assert_eq!(window.problems(), []);
    // Its files are laid out as the piece lays them out: opening it made no edit.
    assert_eq!(window.undo_label(), None);

    // It opens with its first instrument chosen, which the keyboard plays.
    let session = window.session.clone();
    window.until("the instrument chosen", |window| {
        (window.cx).read(|cx| session.read(cx).selected().map(ToString::to_string))
            == Some("piece/t00".to_string())
    });
    window.play_midi(60);
    assert!(window.loudest(4_800) > 0.01);
}

/// The time signature of bar 2, typed in the head, from 4/4 to 5/4: the notes before it and in
/// it stay, the one after it in A and the whole of B are a beat later, and the clock's bars,
/// tempo change and section names follow.
#[gpui::test]
fn a_bar_made_longer_pushes_what_comes_after_it_and_nothing_else(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);

    let beats = window.until_found("space-section-beats");
    window.click(beats);
    // A field to type it in takes its place.
    window.until("the field", |window| {
        window.find("space-section-beats").is_none()
    });
    window.keys("secondary-a");
    window.cx.simulate_input("5");
    window.keys("enter");
    window.until("the bar made longer", |window| {
        window.part("t00", "sec-b").0 != 4 * BAR
    });
    window.after_the_runs();

    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, BAR + BEAT / 2, 2 * BAR + BEAT])
    );
    assert_eq!(window.part("t00", "sec-b"), (4 * BAR + BEAT, IN_B.to_vec()));
    assert_eq!(window.record("piece/t00/sec-a")["length"], 4 * BAR + BEAT);
    let bars = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 1, "signature": "5/4", "bpm": 120 },
        { "bars": 2, "signature": "4/4", "bpm": 120 },
    ]);
    assert_eq!(window.sections()[0]["bars"], bars);
    let project = window.project_file();
    let signatures = json!([
        { "signature": "4/4", "bars": 1 },
        { "signature": "5/4", "bars": 1 },
        { "signature": "4/4", "bars": 4 },
    ]);
    assert_eq!(project["tempo_map"]["time_signatures"], signatures);
    let tempos = json!([{ "tick": 0, "bpm": 120.0 }, { "tick": 4 * BAR + BEAT, "bpm": 90.0 }]);
    assert_eq!(project["tempo_map"]["tempo_changes"], tempos);
    assert_eq!(
        project["sections"],
        json!([{ "bar": 1, "name": "A" }, { "bar": 5, "name": "B" }])
    );
    one_undo_step(&mut window, "Change the time signature", &before);
    assert_eq!(window.problems(), []);
}

/// Shift-B after bar 1 puts an empty bar in: what comes after it is a bar later. Delete on
/// bar 2 then takes it out, and again takes the next with its note: what comes after moves up.
#[gpui::test]
fn a_bar_added_moves_the_rest_later_and_one_removed_takes_its_notes(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(0);
    let before = mark(&mut window);
    window.keys("shift-b");
    window.until("a bar added", |window| {
        window.part("t00", "sec-b").0 == 5 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, 2 * BAR + BEAT / 2, 3 * BAR])
    );
    one_undo_step(&mut window, "Add a bar", &before);

    // The new bar is chosen; removed, the piece is as it was.
    window.keys("backspace");
    window.until("a bar removed", |window| {
        window.part("t00", "sec-b").0 == 4 * BAR
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));

    // Now bar 2 is chosen, with its note.
    let before = mark(&mut window);
    window.keys("backspace");
    window.until("its note gone", |window| {
        window.part("t00", "sec-b").0 == 3 * BAR
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (0, vec![0, BAR]));
    assert_eq!(
        window.project_file()["sections"][1],
        json!({ "bar": 4, "name": "B" })
    );
    one_undo_step(&mut window, "Remove a bar", &before);
    assert_eq!(window.problems(), []);
}

/// Shift-S on bar 3 starts a section there: the note of bar 3 is in its part, counted from its
/// start, and every note plays where it did.
#[gpui::test]
fn a_section_started_at_a_bar_takes_the_music_after_it_and_sounds_the_same(
    cx: &mut TestAppContext,
) {
    let mut window = open_piece(cx);
    let heard = window.heard("t00");
    window.choose_bar(2);
    let before = mark(&mut window);
    window.keys("shift-s");
    window.until("a section started", |window| {
        window.sections().as_array().unwrap().len() == 3
    });
    window.after_the_runs();

    let ids: Vec<Value> = (window.sections().as_array().unwrap().iter())
        .map(|section| section["id"].clone())
        .collect();
    let new = ids[1].as_str().unwrap().to_string();
    assert_eq!(ids, [json!("sec-a"), json!(new), json!("sec-b")]);
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A[..2].to_vec()));
    assert_eq!(window.part("t00", &new), (2 * BAR, vec![0]));
    assert_eq!(window.heard("t00"), heard);
    let names = window.project_file()["sections"].clone();
    assert_eq!(
        names
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name["bar"].clone())
            .collect::<Vec<_>>(),
        [1, 3, 5]
    );
    // Its name waits to be typed, in a field that takes cmd-z for itself.
    window.keys("escape");
    one_undo_step(&mut window, "Start a section", &before);
    assert_eq!(window.problems(), []);
}

/// A note held from bar 2 into bar 3, and a section started at bar 3: the note ends at the new
/// section line, and what was left of it goes on in the new section, from its start.
#[gpui::test]
fn a_note_held_across_a_new_section_line_goes_on_in_the_new_section(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let held =
        json!([{ "start": 2 * BAR - BEAT / 2, "length": BEAT, "pitch": 67, "velocity": 100 }]);
    let part =
        json!({ "tool": "timeline", "state": { "start": 0, "length": 4 * BAR, "notes": held } });
    write_outside(&mut window, "state/piece/t01/sec-a.json", &part.to_string());
    window.until("the part", |window| window.has("piece/t01/sec-a"));
    window.choose_bar(2);
    window.keys("shift-s");
    window.until("a section started", |window| {
        window.sections().as_array().unwrap().len() == 3
    });
    window.after_the_runs();
    let new = window.sections()[1]["id"].as_str().unwrap().to_string();
    let lengths = |record: Value| -> Vec<(u64, u64)> {
        (record["notes"].as_array().unwrap().iter())
            .map(|note| {
                (
                    note["start"].as_u64().unwrap(),
                    note["length"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    assert_eq!(
        lengths(window.record("piece/t01/sec-a")),
        [(2 * BAR - BEAT / 2, BEAT / 2)]
    );
    assert_eq!(
        lengths(window.record(&format!("piece/t01/{new}"))),
        [(0, BEAT / 2)]
    );
    assert_eq!(window.problems(), []);
}

/// Alt-shift-down moves the open section A after B: the parts of both swap places.
#[gpui::test]
fn a_section_moved_takes_its_music_with_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let section = window.until_found("div-section-sec-a");
    window.click(section);
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("alt-shift-down");
    window.until("the section moved", |window| {
        window.part("t00", "sec-b").0 == 0
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (2 * BAR, IN_A.to_vec()));
    assert_eq!(
        window.project_file()["sections"],
        json!([{ "bar": 1, "name": "B" }, { "bar": 3, "name": "A" }])
    );
    one_undo_step(&mut window, "Move a section", &before);
    assert_eq!(window.problems(), []);
}

/// An agent writes a part with the wrong start and makes A two bars shorter in the piece's
/// record, leaving a note past its new end: the open window puts every part where its section
/// is, the clock's bars after them, and the note that no longer fits goes, so the part loads.
#[gpui::test]
fn what_an_agent_left_out_of_place_is_laid_out_again(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let part = json!({ "tool": "timeline", "state": { "start": 0, "length": 2 * BAR, "notes": notes(&[BEAT], 64) } });
    write_outside(&mut window, "state/piece/t01/sec-b.json", &part.to_string());
    window.until("the part laid out", |window| {
        window.part("t01", "sec-b").0 == 4 * BAR
    });

    let mut piece = window.record("piece");
    piece["sections"][0]["bars"][0]["bars"] = 2.into();
    let record = json!({ "tool": "piece", "state": piece });
    write_outside(
        &mut window,
        "state/piece/instance.json",
        &record.to_string(),
    );
    window.until("the piece laid out", |window| {
        window.part("t00", "sec-b").0 == 2 * BAR
    });
    window.after_the_runs();
    assert_eq!(window.part("t01", "sec-b"), (2 * BAR, vec![BEAT]));
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A[..2].to_vec()));
    assert_eq!(window.record("piece/t00/sec-a")["length"], 2 * BAR);
    let project = window.project_file();
    assert_eq!(
        project["tempo_map"]["time_signatures"],
        json!([{ "signature": "4/4", "bars": 4 }])
    );
    assert_eq!(
        project["tempo_map"]["tempo_changes"][1],
        json!({ "tick": 2 * BAR, "bpm": 90.0 })
    );
    assert_eq!(project["sections"][1], json!({ "bar": 3, "name": "B" }));
    assert_eq!(window.problems(), []);
}

/// `6/8` typed into the head for bar 2 is one change of both its numbers: the bar is three
/// quarters long, and what comes after it moves up by one.
#[gpui::test]
fn a_whole_time_signature_typed_is_one_change(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    let beats = window.until_found("space-section-beats");
    window.click(beats);
    window.until("the field", |window| {
        window.find("space-section-beats").is_none()
    });
    window.keys("secondary-a");
    window.cx.simulate_input("6/8");
    window.keys("enter");
    window.until("the bar made shorter", |window| {
        window.part("t00", "sec-b").0 != 4 * BAR
    });
    window.after_the_runs();
    let bars = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 1, "signature": "6/8", "bpm": 120 },
        { "bars": 2, "signature": "4/4", "bpm": 120 },
    ]);
    assert_eq!(window.sections()[0]["bars"], bars);
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, BAR + BEAT / 2, 2 * BAR - BEAT])
    );
    assert_eq!(window.part("t00", "sec-b"), (4 * BAR - BEAT, IN_B.to_vec()));
    one_undo_step(&mut window, "Change the time signature", &before);
    assert_eq!(window.problems(), []);
}

/// Bars 3 and 4 of the first instrument, repeated: there is no bar after them, so the section
/// gets two, and none of the music is cut.
#[gpui::test]
fn music_repeated_past_the_end_of_its_section_gets_the_bars_it_needs(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let first = window.on_row(10., 0);
    window.click(first);
    window.wait_for_bun();
    for bar in [2, 3] {
        window.keys("right");
        window.until_shows(&format!("Bar {bar}"));
    }
    window.keys("shift-right");
    window.until_shows("Bars 3–4");
    let before = mark(&mut window);
    window.keys("secondary-d");
    window.until("repeated", |window| {
        window.part("t00", "sec-b").0 != 4 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, BAR + BEAT / 2, 2 * BAR, 4 * BAR])
    );
    assert_eq!(window.record("piece/t00/sec-a")["length"], 6 * BAR);
    assert_eq!(window.part("t00", "sec-b"), (6 * BAR, IN_B.to_vec()));
    one_undo_step(&mut window, "Paste", &before);
    assert_eq!(window.problems(), []);
}

/// Bar 2 of the first instrument copied, and pasted on bar 3 of the second: its note lands
/// there, in a part the second instrument did not have.
#[gpui::test]
fn a_passage_copied_from_one_instrument_pastes_onto_another(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let first = window.on_row(10., 0);
    window.click(first);
    window.wait_for_bun();
    window.keys("right");
    window.until_shows("Bar 2");
    window.keys("secondary-c");
    window.keys("down");
    window.wait_for_bun();
    window.keys("right");
    window.until_shows("Bar 3");
    let before = mark(&mut window);
    window.keys("secondary-v");
    window.until("pasted", |window| window.has("piece/t01/sec-a"));
    window.after_the_runs();
    assert_eq!(window.part("t01", "sec-a"), (0, vec![2 * BAR + BEAT / 2]));
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
    one_undo_step(&mut window, "Paste", &before);
    assert_eq!(window.problems(), []);
}
