//! The piece of the Composer template (`templates/composer`), the whole piece on one line with a
//! band of marks over its rows: a bar made longer pushes what comes after it and nothing else, a
//! bar added or removed moves the rest, a section started at a bar takes the music after it
//! along, a section moved takes its music with it, and a part an agent left in the wrong place is
//! laid out again. A time signature or a tempo typed in place changes the bars it is typed for,
//! across section lines too. Each edit is one undo step.

use std::path::Path;

use gpui::{Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, point, px};
use serde_json::{Value, json};
use sound_core::Ticks;

use super::{Opened, open_folder};
use crate::song::BAR;
use crate::support::{mark, one_undo_step, write_outside};

const BEAT: u64 = 960;
/// A row of notes is 64 points tall, as `piece-page.ts` has it.
const ROW: f32 = 64.;
/// The points the rows leave before the piece and after it as they open fitted to their width.
const FIT_BEFORE: f32 = 12.;
const FIT_AFTER: f32 = 120.;
/// How long the piece of [`open_piece`] is as it opens: the rows fit it to their width.
const OPENED: u64 = 6 * BAR;
/// How far down the band its lines are: the section names, the time signatures and tempos, and
/// under the bar numbers, empty band over the bars.
const NAMES: f32 = 17.;
const MARKS: f32 = 50.;
const BARS: f32 = 90.;
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
    /// Points per tick across the rows and the band, as the piece opens fitted to their width.
    fn across(&mut self) -> f32 {
        let rows = self.bounds("space-rows").unwrap();
        (f32::from(rows.size.width) - FIT_BEFORE - FIT_AFTER) / OPENED as f32
    }

    /// Where the band over the rows has `tick` of the piece, `y` points down, and `x` points on.
    fn in_band(&mut self, tick: u64, x: f32, y: f32) -> Point<Pixels> {
        let across = self.across();
        let band = self.space_at("marks");
        band + point(px(FIT_BEFORE + tick as f32 * across + x), px(y))
    }

    /// Where the row of the `row`th instrument has the middle of the bar `bar`, from 0.
    fn on_row(&mut self, bar: u64, row: usize) -> Point<Pixels> {
        let across = self.across();
        let x = FIT_BEFORE + (bar * BAR + BAR / 2) as f32 * across;
        self.space_at("rows") + point(px(x), px(row as f32 * ROW + ROW / 2.))
    }

    /// Where the buttons on chosen whole bars starting at `tick` are, `x` points on: a little
    /// under the last of the two rows, or over the bottom of the rows when they are taller.
    fn under_rows(&mut self, tick: u64, x: f32) -> Point<Pixels> {
        let across = self.across();
        let rows = self.bounds("space-rows").unwrap();
        let y = (2. * ROW + 6.).min(f32::from(rows.size.height) - 26.) + 10.;
        rows.origin + point(px(FIT_BEFORE + tick as f32 * across + x), px(y))
    }

    /// Chooses the bar `bar` of the piece whole, from 0, with a click in the band over it: the
    /// playhead goes to it.
    fn choose_bar(&mut self, bar: u64) {
        let at = self.in_band(bar * BAR + BAR / 2, 0., BARS);
        self.click(at);
        self.until_playhead(bar * BAR);
    }

    /// Chooses the bar `bar` of the `row`th instrument, with a click in its row.
    fn choose_music(&mut self, bar: u64, row: usize) {
        let at = self.on_row(bar, row);
        self.click(at);
        self.until_playhead(bar * BAR);
    }

    /// Types `text` into the field that opened, and enter.
    fn type_in_field(&mut self, text: &str) {
        self.until_found("menu-search");
        self.cx.simulate_input(text);
        self.keys("enter");
    }

    /// Whether the page shows `text`, also as a label over the bars.
    fn shows(&mut self, text: &str) -> bool {
        let shell = self.shell.clone();
        self.cx.read(|cx| {
            let page = shell.read(cx).main_view().unwrap();
            sound_typescript::page_has_text(page, text, cx)
        })
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
    // Its one section, named over its bars, and with no notes yet, how to write some.
    window.until("its section named", |window| window.shows(" Section 1 "));
    window.until("the hint", |window| {
        window.shows("Double-click a bar to write notes")
    });
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

/// Bar 2 chosen and shift-T, 5/4: the notes before it and in it stay, the one after it in A and
/// the whole of B are a beat later, and the clock's bars, tempo change and section names follow.
#[gpui::test]
fn a_bar_made_longer_pushes_what_comes_after_it_and_nothing_else(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("shift-t");
    window.type_in_field("5/4");
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

/// "+ bar" under chosen bar 2 adds one after it, chosen, and "remove" under that takes it out
/// again.
#[gpui::test]
fn the_buttons_on_chosen_bars_add_one_after_them_and_remove_them(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    // At the left edge of the chosen bars, "+ bar", and right of it "remove".
    let add = window.under_rows(BAR, 22.);
    window.click(add);
    window.until("a bar added", |window| {
        window.part("t00", "sec-b").0 == 5 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, BAR + BEAT / 2, 3 * BAR])
    );
    one_undo_step(&mut window, "Add a bar", &before);

    window.until_playhead(2 * BAR);
    let before = mark(&mut window);
    let remove = window.under_rows(2 * BAR, 76.);
    window.click(remove);
    window.until("the bar removed", |window| {
        window.part("t00", "sec-b").0 == 4 * BAR
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
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
    window.until_found("menu-search");
    window.keys("escape");
    one_undo_step(&mut window, "Start a section", &before);
    assert_eq!(window.problems(), []);
}

/// Shift-S on the first bar of B, which starts a section already, does nothing.
#[gpui::test]
fn a_section_is_not_started_where_one_starts(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(4);
    window.keys("shift-s");
    window.wait_for_bun();
    window.after_the_runs();
    assert_eq!(window.sections().as_array().unwrap().len(), 2);
    assert_eq!(window.undo_label(), None);
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

/// A click on the name of A chooses its bars, and alt-shift-down moves it after B: the parts of
/// both swap places.
#[gpui::test]
fn a_section_moved_takes_its_music_with_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let name = window.in_band(0, 14., NAMES);
    window.click(name);
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

/// The name of B dragged onto the bars of A: B lands before A, with its music.
#[gpui::test]
fn a_section_name_dragged_onto_another_section_lands_before_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let name = window.in_band(4 * BAR, 14., NAMES);
    let onto = window.in_band(BAR, 0., NAMES);
    let before = mark(&mut window);
    window.drag(name, onto);
    window.until("the section moved", |window| {
        window.part("t00", "sec-b").0 == 0
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (2 * BAR, IN_A.to_vec()));
    one_undo_step(&mut window, "Move a section", &before);
    assert_eq!(window.problems(), []);
}

/// A double click on the name of A opens it to type over: the new name is the section's and the
/// clock's, and its id, so its files, stay.
#[gpui::test]
fn a_section_named_in_place_keeps_its_files(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let name = window.in_band(0, 14., NAMES);
    let before = mark(&mut window);
    window.double_click(name);
    window.type_in_field("Theme");
    window.until("the section named", |window| {
        window.sections()[0]["name"] == "Theme"
    });
    window.after_the_runs();
    assert_eq!(window.sections()[0]["id"], "sec-a");
    assert_eq!(
        window.project_file()["sections"][0],
        json!({ "bar": 1, "name": "Theme" })
    );
    one_undo_step(&mut window, "Rename a section", &before);
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

/// `6/8` typed for bar 2 is one change of both its numbers: the bar is three quarters long, and
/// what comes after it moves up by one.
#[gpui::test]
fn a_whole_time_signature_typed_is_one_change(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("shift-t");
    window.type_in_field("6/8");
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

/// The last bar of B made 3/4 by hand, then a click on the 4/4 at the first bar and 2/4: every
/// bar up to that 3/4 is 2/4, in A and in B, and the 3/4 bar stays as it was. One undo step.
#[gpui::test]
fn a_time_signature_typed_on_its_mark_changes_its_run_up_to_the_next(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let mut piece = window.record("piece");
    piece["sections"][1]["bars"] = json!([
        { "bars": 1, "signature": "4/4", "bpm": 90 },
        { "bars": 1, "signature": "3/4", "bpm": 90 },
    ]);
    let record = json!({ "tool": "piece", "state": piece });
    write_outside(
        &mut window,
        "state/piece/instance.json",
        &record.to_string(),
    );
    window.until("the piece laid out", |window| {
        window.project_file()["tempo_map"]["time_signatures"][1]["signature"] == "3/4"
    });
    window.after_the_runs();
    let before = mark(&mut window);

    let signature = window.in_band(0, 10., MARKS);
    window.click(signature);
    window.type_in_field("2/4");
    window.until("the run made 2/4", |window| {
        window.part("t00", "sec-b").0 != 4 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([{ "bars": 4, "signature": "2/4", "bpm": 120 }])
    );
    assert_eq!(
        window.sections()[1]["bars"],
        json!([
            { "bars": 1, "signature": "2/4", "bpm": 90 },
            { "bars": 1, "signature": "3/4", "bpm": 90 },
        ])
    );
    // Each note keeps its place in its bar.
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![0, 2 * BEAT + BEAT / 2, 4 * BEAT])
    );
    assert_eq!(window.part("t00", "sec-b"), (8 * BEAT, IN_B.to_vec()));
    one_undo_step(&mut window, "Change the time signature", &before);
    assert_eq!(window.problems(), []);
}

/// Bar 2 chosen and T, "100 bpm": that bar alone is at 100, and the bar after it at 120 again.
#[gpui::test]
fn a_tempo_typed_for_one_bar_changes_that_bar_only(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("t");
    window.type_in_field("100 bpm");
    window.until("the tempo changed", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 3
    });
    window.after_the_runs();
    let bars = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 1, "signature": "4/4", "bpm": 100 },
        { "bars": 2, "signature": "4/4", "bpm": 120 },
    ]);
    assert_eq!(window.sections()[0]["bars"], bars);
    let tempos = json!([
        { "tick": 0, "bpm": 120.0 },
        { "tick": BAR, "bpm": 100.0 },
        { "tick": 2 * BAR, "bpm": 120.0 },
        { "tick": 4 * BAR, "bpm": 90.0 },
    ]);
    assert_eq!(window.project_file()["tempo_map"]["tempo_changes"], tempos);
    // Only the time moved: every note is where it was.
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
    one_undo_step(&mut window, "Change the tempo", &before);
    assert_eq!(window.problems(), []);
}

/// The last bar of A and the first of B chosen, and T, 100: both are at 100, in one undo step.
#[gpui::test]
fn a_tempo_typed_for_bars_across_a_section_line_is_one_change(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(3);
    window.keys("shift-right");
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("t");
    window.type_in_field("100");
    window.until("the tempo changed", |window| {
        window.sections()[1]["bars"].as_array().unwrap().len() == 2
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 3, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 100 },
        ])
    );
    assert_eq!(
        window.sections()[1]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 100 },
            { "bars": 1, "signature": "4/4", "bpm": 90 },
        ])
    );
    one_undo_step(&mut window, "Change the tempo", &before);
    assert_eq!(window.problems(), []);
}

/// The two bars of B of the first instrument, repeated: there is no bar after them, so the last
/// section gets two, and none of the music is cut.
#[gpui::test]
fn music_repeated_past_the_end_of_the_piece_gets_the_bars_it_needs(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_music(4, 0);
    window.keys("shift-right");
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("secondary-d");
    window.until("repeated", |window| {
        window.record("piece/t00/sec-b")["length"] != 2 * BAR
    });
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-b"), (4 * BAR, vec![0, 2 * BAR]));
    assert_eq!(window.record("piece/t00/sec-b")["length"], 4 * BAR);
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
    one_undo_step(&mut window, "Paste", &before);
    assert_eq!(window.problems(), []);
}

/// Bar 2 of the first instrument copied, and pasted on bar 3 of the second: its note lands
/// there, in a part the second instrument did not have.
#[gpui::test]
fn a_passage_copied_from_one_instrument_pastes_onto_another(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_music(1, 0);
    window.keys("secondary-c");
    window.keys("down");
    window.wait_for_bun();
    window.keys("right");
    window.until_playhead(2 * BAR);
    let before = mark(&mut window);
    window.keys("secondary-v");
    window.until("pasted", |window| window.has("piece/t01/sec-a"));
    window.after_the_runs();
    assert_eq!(window.part("t01", "sec-a"), (0, vec![2 * BAR + BEAT / 2]));
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
    one_undo_step(&mut window, "Paste", &before);
    assert_eq!(window.problems(), []);
}

/// Right on the last bar of A chooses the first of B, bar 5 of the piece; left goes back to the
/// last bar of A.
#[gpui::test]
fn the_arrows_step_across_a_section_line(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(3);
    window.keys("right");
    window.until_playhead(4 * BAR);
    window.keys("left");
    window.until_playhead(3 * BAR);
    assert_eq!(window.problems(), []);
}

/// Bar 2 made 1/8 drops its note, half a beat in: the band says so, and one undo step brings it
/// back.
#[gpui::test]
fn a_bar_made_too_short_for_a_note_says_so(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("shift-t");
    window.type_in_field("1/8");
    let notice = "1 note did not fit and was removed. Undo brings it back.";
    window.until("the notice", |window| window.shows(notice));
    window.after_the_runs();
    assert_eq!(window.part("t00", "sec-a"), (0, vec![0, BAR + BEAT / 2]));
    one_undo_step(&mut window, "Change the time signature", &before);
    // Undone, the note is back, so the band no longer says it went.
    window.wait_for_bun();
    window.keys("secondary-z");
    window.until("the notice gone", |window| !window.shows(notice));
    assert_eq!(window.problems(), []);
}

/// The first bar of an instrument, opened with enter: the note editor shows it and the bar
/// after it, from the start of the section. A click on the first bar of B shows B's part.
#[gpui::test]
fn the_first_bar_opens_the_note_editor_on_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_music(0, 0);
    window.keys("enter");
    window.until_found(&format!("space-editor-t00/sec-a/0-{}", 2 * BAR));
    window.choose_music(4, 0);
    window.until_found(&format!("space-editor-t00/sec-b/0-{}", 2 * BAR));
    assert_eq!(window.problems(), []);
}

/// Bar 3 chosen, the rows scrolled far past it by hand, and play: the rows go back to it, so a
/// click a little in from their left edge is in bar 3.
#[gpui::test]
fn the_rows_go_to_the_playhead_as_the_piece_starts_to_play(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(2);
    let rows = window.space_at("rows");
    for _ in 0..5 {
        window.cx.simulate_event(ScrollEvent {
            position: rows + point(px(300.), px(20.)),
            delta: ScrollDelta::Pixels(point(px(-600.), px(0.))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        });
    }
    let session = window.session.clone();
    let playhead =
        move |window: &mut Opened| window.cx.read(|cx| *session.read(cx).playhead().read(cx));
    window.keys("space");
    window.until("it plays on", |window| {
        playhead(window).tick > Ticks(2 * BAR)
    });
    window.keys("space");
    window.until("it stops", |window| !playhead(window).playing);
    window.click(rows + point(px(34.), px(20.)));
    window.until_playhead(2 * BAR);
}
