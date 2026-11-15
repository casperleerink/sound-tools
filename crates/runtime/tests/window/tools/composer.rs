//! The piece of the Composer template (`templates/composer`), the whole piece on one line with a
//! band of marks over its rows: a bar made longer pushes what comes after it and nothing else, a
//! bar added or removed moves the rest, a section started at a bar takes the music after it
//! along, a section moved takes its music with it, and a part an agent left in the wrong place is
//! laid out again. A time signature or a tempo typed in place changes the bars it is typed for,
//! across section lines too; a rit. or accel. reaches the clock in steps, and a fermata as a
//! slower count. Each edit is one undo step.

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
const MARKS: f32 = 42.;
const BARS: f32 = 72.;
const FERMATA: f32 = 70.;
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

    /// The clock's tempo changes: the tick each is at and its tempo.
    fn tempos(&mut self) -> Vec<(u64, f64)> {
        let changes = self.project_file()["tempo_map"]["tempo_changes"].clone();
        (changes.as_array().unwrap().iter())
            .map(|change| {
                (
                    change["tick"].as_u64().unwrap(),
                    change["bpm"].as_f64().unwrap(),
                )
            })
            .collect()
    }

    /// Where the fermata over the count `count`, from 1, of the bar `bar`, from 0, is.
    fn fermata(&mut self, bar: u64, count: u64) -> Point<Pixels> {
        self.in_band(bar * BAR + (count - 1) * BEAT + BEAT / 2, 0., FERMATA)
    }
}

/// How many seconds the clock takes from `from` to `to`, along `tempos`.
fn seconds(tempos: &[(u64, f64)], from: u64, to: u64) -> f64 {
    let mut seconds = 0.;
    for (index, &(tick, bpm)) in tempos.iter().enumerate() {
        let next = tempos.get(index + 1).map_or(u64::MAX, |&(next, _)| next);
        let (start, end) = (tick.max(from), next.min(to));
        if end > start {
            seconds += (end - start) as f64 / BEAT as f64 * 60. / bpm;
        }
    }
    seconds
}

/// The tempo changes from `from` to `to` are steps every sixteenth along a straight line from
/// `bpm` at `from` to `reached` at `to`, each at the tempo of its middle.
fn assert_slide(tempos: &[(u64, f64)], from: u64, to: u64, bpm: f64, reached: f64) {
    let steps: Vec<(u64, f64)> = (tempos.iter())
        .filter(|(tick, _)| (from..to).contains(tick))
        .copied()
        .collect();
    let step = BEAT / 4;
    assert_eq!(steps.len() as u64, (to - from) / step, "{steps:?}");
    for (index, &(tick, tempo)) in steps.iter().enumerate() {
        let middle = (index as f64 + 0.5) * step as f64 / (to - from) as f64;
        assert_eq!(tick, from + index as u64 * step);
        assert!(
            (tempo - (bpm + (reached - bpm) * middle)).abs() < 0.002,
            "{tick}: {tempo}"
        );
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

/// Bar 2 chosen and T, 5/4: the notes before it and in it stay, the one after it in A and
/// the whole of B are a beat later, and the clock's bars, tempo change and section names follow.
#[gpui::test]
fn a_bar_made_longer_pushes_what_comes_after_it_and_nothing_else(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("t");
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

/// The name of B, chosen, dragged onto the bars of A: B lands before A, with its music, and
/// stays chosen where it now is, at the start, not the bars of A that are where it was.
#[gpui::test]
fn a_section_name_dragged_onto_another_section_lands_before_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let name = window.in_band(4 * BAR, 14., NAMES);
    let onto = window.in_band(BAR, 0., NAMES);
    window.click(name);
    window.until_playhead(4 * BAR);
    let before = mark(&mut window);
    window.drag(name, onto);
    window.until("the section moved", |window| {
        window.part("t00", "sec-b").0 == 0
    });
    window.until_playhead(0);
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
    window.keys("t");
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

/// Bar 2 chosen and shift-T, "100 bpm": that bar alone is at 100, and the bar after it at 120 again.
#[gpui::test]
fn a_tempo_typed_for_one_bar_changes_that_bar_only(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("shift-t");
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

/// The last bar of A and the first of B chosen, and shift-T, 100: both are at 100, in one undo step.
#[gpui::test]
fn a_tempo_typed_for_bars_across_a_section_line_is_one_change(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(3);
    window.keys("shift-right");
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("shift-t");
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
    window.keys("t");
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

/// Bars 2 and 3 chosen and alt-T, 60: they slow down in a straight line from 120 to 60, which
/// the clock plays in steps over exactly those bars; bar 4 is at 120 again, and the band says
/// "rit.".
#[gpui::test]
fn a_rit_typed_for_chosen_bars_slows_the_clock_down_over_them_only(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    window.keys("shift-right");
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit."));
    window.after_the_runs();
    let bars = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 1, "signature": "4/4", "bpm": 120, "to": 90 },
        { "bars": 1, "signature": "4/4", "bpm": 90, "to": 60 },
        { "bars": 1, "signature": "4/4", "bpm": 120 },
    ]);
    assert_eq!(window.sections()[0]["bars"], bars);
    let tempos = window.tempos();
    assert_eq!(tempos[0], (0, 120.));
    assert_slide(&tempos, BAR, 3 * BAR, 120., 60.);
    assert_eq!(
        tempos[tempos.len() - 2..],
        [(3 * BAR, 120.), (4 * BAR, 90.)]
    );
    assert_eq!(window.part("t00", "sec-a"), (0, IN_A.to_vec()));
    one_undo_step(&mut window, "Change the tempo gradually", &before);
    assert_eq!(window.problems(), []);
}

/// The last bar of A and the first of B chosen, and alt-T, 60: one straight line from 120 to 60
/// over both, in one undo step; the last bar of B keeps its 90.
#[gpui::test]
fn a_rit_over_bars_across_a_section_line_is_one_line_and_one_change(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(3);
    window.keys("shift-right");
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| {
        window.sections()[1]["bars"].as_array().unwrap().len() == 2
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 3, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 120, "to": 90 },
        ])
    );
    assert_eq!(
        window.sections()[1]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 90, "to": 60 },
            { "bars": 1, "signature": "4/4", "bpm": 90 },
        ])
    );
    let tempos = window.tempos();
    assert_slide(&tempos, 3 * BAR, 5 * BAR, 120., 60.);
    assert_eq!(tempos.last(), Some(&(5 * BAR, 90.)));
    one_undo_step(&mut window, "Change the tempo gradually", &before);
    assert_eq!(window.problems(), []);
}

/// A rit. over bars 2 and 3, then "Steady tempo" picked in the field alt-T opens on them: they
/// are at 120 again, as the piece was. Undone to the rit., a tempo typed with shift-T, 100, also leaves
/// them steady, at 100.
#[gpui::test]
fn steady_tempo_or_a_tempo_typed_ends_a_rit(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let (bars, tempos) = (window.sections()[0]["bars"].clone(), window.tempos());
    window.choose_bar(1);
    window.keys("shift-right");
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit."));
    window.after_the_runs();

    let before = mark(&mut window);
    window.keys("alt-t");
    let steady = window.until_found("menu-Steady tempo");
    window.click(steady);
    window.until("steady", |window| !window.shows("rit."));
    window.after_the_runs();
    assert_eq!(window.sections()[0]["bars"], bars);
    assert_eq!(window.tempos(), tempos);
    one_undo_step(&mut window, "Keep the tempo steady", &before);

    window.wait_for_bun();
    window.keys("secondary-z");
    window.until("the rit. again", |window| window.shows("rit."));
    let before = mark(&mut window);
    window.keys("shift-t");
    window.type_in_field("100");
    window.until("steady", |window| !window.shows("rit."));
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 120 },
            { "bars": 2, "signature": "4/4", "bpm": 100 },
            { "bars": 1, "signature": "4/4", "bpm": 120 },
        ])
    );
    assert_eq!(
        window.tempos(),
        [(0, 120.), (BAR, 100.), (3 * BAR, 120.), (4 * BAR, 90.)]
    );
    one_undo_step(&mut window, "Change the tempo", &before);
    assert_eq!(window.problems(), []);
}

/// Bar 2 chosen and F: a fermata on its last count, which the clock plays at half the tempo, so
/// the bar lasts one beat longer, 2.5 seconds at 120. F again opens its menu, so a key reaches
/// how long it holds; "Remove the fermata" there takes it off.
#[gpui::test]
fn a_fermata_holds_the_last_count_one_beat_longer_and_f_again_opens_its_menu(
    cx: &mut TestAppContext,
) {
    let mut window = open_piece(cx);
    let tempos = window.tempos();
    window.choose_bar(1);
    let before = mark(&mut window);
    window.keys("f");
    window.until("the fermata", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 3
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 120, "fermata": 4, "hold": 1 },
            { "bars": 2, "signature": "4/4", "bpm": 120 },
        ])
    );
    let held = window.tempos();
    assert_eq!(
        held,
        [
            (0, 120.),
            (BAR + 3 * BEAT, 60.),
            (2 * BAR, 120.),
            (4 * BAR, 90.)
        ]
    );
    assert_eq!(seconds(&held, BAR, 2 * BAR), 2.5);
    assert_eq!(seconds(&held, 2 * BAR, 3 * BAR), 2.);
    one_undo_step(&mut window, "Add a fermata", &before);

    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("f");
    let remove = window.until_found("menu-7");
    window.click(remove);
    window.until("no fermata", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 1
    });
    window.after_the_runs();
    assert_eq!(window.tempos(), tempos);
    one_undo_step(&mut window, "Remove a fermata", &before);
    assert_eq!(window.problems(), []);
}

/// Opens the piece with a fermata on the last count of bar 2.
fn with_a_fermata(cx: &mut TestAppContext) -> Opened<'_> {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    window.keys("f");
    window.until("the fermata", |window| {
        window.sections()[0]["bars"][1]["fermata"] == 4
    });
    window.after_the_runs();
    window
}

/// A click on the fermata of bar 2 and "2 beats longer": it holds two beats, its count a third
/// as fast. A click again and "Remove the fermata" takes it off.
#[gpui::test]
fn the_menu_of_a_fermata_changes_how_long_it_holds_and_removes_it(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    let sign = window.fermata(1, 4);
    let before = mark(&mut window);
    window.click(sign);
    let two = window.until_found("menu-2");
    window.click(two);
    window.until("held longer", |window| {
        window.sections()[0]["bars"][1]["hold"] == 2
    });
    window.after_the_runs();
    assert_eq!(window.sections()[0]["bars"][1]["fermata"], 4);
    assert_eq!(window.tempos()[1], (BAR + 3 * BEAT, 40.));
    assert_eq!(seconds(&window.tempos(), BAR, 2 * BAR), 3.);
    one_undo_step(&mut window, "Change a fermata", &before);

    window.wait_for_bun();
    let before = mark(&mut window);
    window.click(sign);
    let remove = window.until_found("menu-7");
    window.click(remove);
    window.until("no fermata", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 1
    });
    window.after_the_runs();
    one_undo_step(&mut window, "Remove a fermata", &before);
    assert_eq!(window.problems(), []);
}

/// The fermata of bar 2 dragged onto the second count of bar 3: it is there, and bar 2 has none.
#[gpui::test]
fn a_fermata_dragged_to_another_count_moves_there(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    let (sign, onto) = (window.fermata(1, 4), window.fermata(2, 2));
    let before = mark(&mut window);
    window.drag(sign, onto);
    window.until("the fermata moved", |window| {
        window.sections()[0]["bars"][1]["fermata"] == 2
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 2, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 120, "fermata": 2, "hold": 1 },
            { "bars": 1, "signature": "4/4", "bpm": 120 },
        ])
    );
    assert_eq!(window.tempos()[1], (2 * BAR + BEAT, 60.));
    one_undo_step(&mut window, "Move a fermata", &before);
    assert_eq!(window.problems(), []);
}

/// A right-click on the first count of bar 2, and "Fermata" in its menu: the fermata is on that
/// count.
#[gpui::test]
fn a_fermata_from_the_menu_of_a_bar_goes_on_the_count_under_the_pointer(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let count = window.in_band(BAR + BEAT / 2, 0., BARS);
    let before = mark(&mut window);
    window.right_click(count);
    let fermata = window.until_found("menu-8");
    window.click(fermata);
    window.until("the fermata", |window| {
        window.sections()[0]["bars"][1]["fermata"] == 1
    });
    window.after_the_runs();
    assert_eq!(window.tempos()[1], (BAR, 60.));
    one_undo_step(&mut window, "Add a fermata", &before);
    assert_eq!(window.problems(), []);
}

/// Bar 2, with a fermata on its fourth count, made 3/4: the fermata is on its third.
#[gpui::test]
fn a_shorter_time_signature_keeps_the_fermata_on_a_count_the_bar_has(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    let before = mark(&mut window);
    window.keys("t");
    window.type_in_field("3/4");
    window.until("the bar made shorter", |window| {
        window.sections()[0]["bars"][1]["signature"] == "3/4"
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"][1],
        json!({ "bars": 1, "signature": "3/4", "bpm": 120, "fermata": 3, "hold": 1 })
    );
    assert_eq!(
        window.tempos()[1..3],
        [(BAR + 2 * BEAT, 60.), (BAR + 3 * BEAT, 120.)]
    );
    one_undo_step(&mut window, "Change the time signature", &before);
    assert_eq!(window.problems(), []);
}

/// Shift-B on bar 2, which has a fermata, adds a bar with none. Bars 3 and 4 then slowing down to
/// 60 and shift-B: the bar added is steady at 60, and the bar after it at 120 as it was.
#[gpui::test]
fn a_bar_added_after_a_fermata_or_a_rit_is_steady_at_the_tempo_reached(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    let before = mark(&mut window);
    window.keys("shift-b");
    window.until("a bar added", |window| {
        window.part("t00", "sec-b").0 == 5 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 120, "fermata": 4, "hold": 1 },
            { "bars": 3, "signature": "4/4", "bpm": 120 },
        ])
    );
    one_undo_step(&mut window, "Add a bar", &before);

    window.wait_for_bun();
    window.choose_bar(2);
    window.keys("shift-right");
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit."));
    window.after_the_runs();
    let before = mark(&mut window);
    window.keys("shift-b");
    window.until("a bar added", |window| {
        window.part("t00", "sec-b").0 == 6 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"].as_array().unwrap()[4..],
        [
            json!({ "bars": 1, "signature": "4/4", "bpm": 60 }),
            json!({ "bars": 1, "signature": "4/4", "bpm": 120 }),
        ]
    );
    let tempos = window.tempos();
    assert_slide(&tempos, 2 * BAR, 4 * BAR, 120., 60.);
    let after: Vec<_> = (tempos.iter())
        .filter(|(tick, _)| *tick >= 4 * BAR)
        .copied()
        .collect();
    assert_eq!(after, [(4 * BAR, 60.), (5 * BAR, 120.), (6 * BAR, 90.)]);
    one_undo_step(&mut window, "Add a bar", &before);
    assert_eq!(window.problems(), []);
}

/// An agent writes a run that slows down from 120 to 90 over bars 2 and 3: the clock gets its
/// steps, and the band says "rit.".
#[gpui::test]
fn a_rit_an_agent_writes_in_the_record_reaches_the_clock_in_steps(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    let mut piece = window.record("piece");
    piece["sections"][0]["bars"] = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 2, "signature": "4/4", "bpm": 120, "to": 90 },
        { "bars": 1, "signature": "4/4", "bpm": 120 },
    ]);
    let record = json!({ "tool": "piece", "state": piece });
    write_outside(
        &mut window,
        "state/piece/instance.json",
        &record.to_string(),
    );
    window.until("the steps", |window| window.tempos().len() > 2);
    window.after_the_runs();
    let tempos = window.tempos();
    assert_slide(&tempos, BAR, 3 * BAR, 120., 90.);
    assert_eq!(
        tempos[tempos.len() - 2..],
        [(3 * BAR, 120.), (4 * BAR, 90.)]
    );
    assert!(window.shows("rit."));
    assert_eq!(window.problems(), []);
}

/// The last bar of B slowing down to 60 with a fermata, and the music of B repeated past it: the
/// two bars the piece gets are steady at 60, with no fermata.
#[gpui::test]
fn bars_music_needs_past_a_rit_and_a_fermata_are_steady_at_the_tempo_reached(
    cx: &mut TestAppContext,
) {
    let mut window = open_piece(cx);
    window.choose_bar(5);
    window.keys("f");
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit."));
    window.after_the_runs();
    window.choose_music(4, 0);
    window.keys("shift-right");
    window.wait_for_bun();
    window.keys("secondary-d");
    window.until("repeated", |window| {
        window.record("piece/t00/sec-b")["length"] == 4 * BAR
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[1]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 90 },
            { "bars": 1, "signature": "4/4", "bpm": 90, "to": 60, "fermata": 4, "hold": 1 },
            { "bars": 2, "signature": "4/4", "bpm": 60 },
        ])
    );
    let tempos = window.tempos();
    assert_eq!(tempos.last(), Some(&(6 * BAR, 60.)));
    assert_eq!(window.problems(), []);
}

/// Bar 2 slows down to 60 and bar 3 speeds up from there to 120: the band says both, "rit." then
/// "accel.", not one mark over the two.
#[gpui::test]
fn a_rit_then_an_accel_are_two_marks(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit."));
    window.after_the_runs();
    window.choose_bar(2);
    window.keys("shift-t");
    window.type_in_field("60");
    window.until("bar 3 at 60", |window| {
        window.sections()[0]["bars"][2]["bpm"] == 60
    });
    window.after_the_runs();
    window.keys("alt-t");
    window.type_in_field("120");
    window.until("the accel.", |window| window.shows("accel."));
    assert!(window.shows("rit."));
    assert_eq!(window.problems(), []);
}

/// The first bar of B, at 90 after a bar at 120, slows down to 60: its mark says the tempo it
/// starts at, as no bar before it does.
#[gpui::test]
fn a_rit_that_starts_at_another_tempo_says_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(4);
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the tempo and the rit.", |window| {
        window.shows("= 90  rit. to 60")
    });
    assert_eq!(window.problems(), []);
}

/// A bar with a fermata of one beat made as slow as 15: the clock plays no slower than 10, so
/// the fermata holds half a beat, which its record says and the clock plays.
#[gpui::test]
fn a_fermata_holds_no_longer_than_its_tempo_can_play(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    window.keys("shift-t");
    window.type_in_field("15");
    window.until("bar 2 at 15", |window| {
        window.sections()[0]["bars"][1]["bpm"] == 15
    });
    window.after_the_runs();
    assert_eq!(window.sections()[0]["bars"][1]["hold"], 0.5);
    let tempos = window.tempos();
    assert!(tempos.contains(&(BAR + 3 * BEAT, 10.)));
    // Four beats at 15 are 16 seconds, and half a beat more is 2.
    assert_eq!(seconds(&tempos, BAR, 2 * BAR), 18.);
    assert_eq!(window.problems(), []);
}

/// A click on the time signature at the start of the piece chooses the bars it stands for, all
/// six, so the composer sees what the field changes: the playhead goes to the first, and a bar
/// added after the choice lands at the end of B, not after bar 2, which was chosen before.
#[gpui::test]
fn a_click_on_a_mark_chooses_the_bars_it_changes(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    let digits = window.in_band(0, 10., MARKS);
    window.click(digits);
    window.until_playhead(0);
    window.type_in_field("3/4");
    window.until("six bars of 3/4", |window| {
        window.sections()[1]["bars"][0]["signature"] == "3/4"
    });
    window.after_the_runs();
    window.keys("shift-b");
    window.until("a third bar in B", |window| {
        window.sections()[1]["bars"][0]["bars"] == 3
    });
    assert_eq!(window.sections()[0]["bars"][0]["bars"], 4);
    assert_eq!(window.problems(), []);
}

/// The fermata of bar 2, on its last count, moved a beat earlier from its menu, which F opens:
/// it is on count 3, in one undo step.
#[gpui::test]
fn a_fermata_moves_a_beat_earlier_from_its_menu(cx: &mut TestAppContext) {
    let mut window = with_a_fermata(cx);
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("f");
    let earlier = window.until_found("menu-5");
    window.click(earlier);
    window.until("on count 3", |window| {
        window.sections()[0]["bars"][1]["fermata"] == 3
    });
    window.after_the_runs();
    assert!(window.tempos().contains(&(BAR + 2 * BEAT, 60.)));
    one_undo_step(&mut window, "Move a fermata", &before);
    assert_eq!(window.problems(), []);
}

/// Alt-B and "3" with bar 2 chosen: three bars after it, as one undo step, and the notes after
/// them three bars later.
#[gpui::test]
fn bars_typed_as_a_number_are_added_in_one_step(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("alt-b");
    window.type_in_field("3");
    window.until("seven bars in A", |window| {
        window.sections()[0]["bars"][0]["bars"] == 7
    });
    window.after_the_runs();
    assert_eq!(
        window.part("t00", "sec-a"),
        (0, vec![IN_A[0], IN_A[1], IN_A[2] + 3 * BAR])
    );
    one_undo_step(&mut window, "Add bars", &before);
    assert_eq!(window.problems(), []);
}

/// A rit. over bars 2 and 3, from 120 to 60, and a bar added after bar 2: the three bars are one
/// rit. to 60, its line spread over them. The added bar removed, it is over two bars again.
#[gpui::test]
fn a_bar_added_inside_a_rit_joins_it_and_one_removed_leaves_it_whole(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(1);
    window.keys("shift-right");
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit. to 60"));
    window.after_the_runs();
    let rit = window.sections()[0]["bars"].clone();
    window.choose_bar(1);
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("shift-b");
    window.until("five bars in A", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 5
    });
    window.after_the_runs();
    let bars = json!([
        { "bars": 1, "signature": "4/4", "bpm": 120 },
        { "bars": 1, "signature": "4/4", "bpm": 120, "to": 100 },
        { "bars": 1, "signature": "4/4", "bpm": 100, "to": 80 },
        { "bars": 1, "signature": "4/4", "bpm": 80, "to": 60 },
        { "bars": 1, "signature": "4/4", "bpm": 120 },
    ]);
    assert_eq!(window.sections()[0]["bars"], bars);
    assert_slide(&window.tempos(), BAR, 4 * BAR, 120., 60.);
    one_undo_step(&mut window, "Add a bar", &before);

    window.wait_for_bun();
    // The added bar is chosen: removed, the rit. is as it was.
    window.keys("backspace");
    window.until("four bars in A", |window| {
        window.sections()[0]["bars"] == rit
    });
    assert_eq!(window.problems(), []);
}

/// A section started at bar 3, then enter on that bar: the note editor shows the part of the
/// new section, where the bar now is, not that of A.
#[gpui::test]
fn the_note_editor_opens_on_the_section_just_started(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(2);
    window.keys("shift-s");
    window.type_in_field("C");
    window.until("the section named", |window| {
        window.sections()[1]["name"] == "C"
    });
    window.after_the_runs();
    let new = window.sections()[1]["id"].as_str().unwrap().to_string();
    window.wait_for_bun();
    window.keys("enter");
    window.until_found(&format!("space-editor-t00/{new}/0-{}", 2 * BAR));
    assert_eq!(window.problems(), []);
}

/// Shift-B on the last bar of the piece: the bar it adds is the chosen one, so the playhead is
/// on it, and a second shift-B adds one after that. Alt-shift-down on A, chosen, moves it after
/// B, where it stays chosen.
#[gpui::test]
fn what_a_key_adds_or_moves_is_what_stays_chosen(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(5);
    window.keys("shift-b");
    window.until_playhead(6 * BAR);
    window.after_the_runs();
    window.wait_for_bun();
    let name = window.in_band(0, 14., NAMES);
    window.click(name);
    window.until_playhead(0);
    window.keys("alt-shift-down");
    window.until("A after B", |window| window.sections()[1]["id"] == "sec-a");
    window.until_playhead(3 * BAR);
    assert_eq!(window.problems(), []);
}

/// A rit. over the last bar of A and the first of B, from 120 to 60, and a bar added after the
/// last bar of A: the three bars, on both sides of the section line, are one rit. to 60. The
/// added bar removed, the rit. is as it was.
#[gpui::test]
fn a_bar_added_inside_a_rit_across_a_section_line_joins_it(cx: &mut TestAppContext) {
    let mut window = open_piece(cx);
    window.choose_bar(3);
    window.keys("shift-right");
    window.keys("alt-t");
    window.type_in_field("60");
    window.until("the rit.", |window| window.shows("rit. to 60"));
    window.after_the_runs();
    let rit = window.sections();
    window.choose_bar(3);
    window.wait_for_bun();
    let before = mark(&mut window);
    window.keys("shift-b");
    window.until("five bars in A", |window| {
        window.sections()[0]["bars"].as_array().unwrap().len() == 3
    });
    window.after_the_runs();
    assert_eq!(
        window.sections()[0]["bars"],
        json!([
            { "bars": 3, "signature": "4/4", "bpm": 120 },
            { "bars": 1, "signature": "4/4", "bpm": 120, "to": 100 },
            { "bars": 1, "signature": "4/4", "bpm": 100, "to": 80 },
        ])
    );
    assert_eq!(
        window.sections()[1]["bars"],
        json!([
            { "bars": 1, "signature": "4/4", "bpm": 80, "to": 60 },
            { "bars": 1, "signature": "4/4", "bpm": 90 },
        ])
    );
    assert_slide(&window.tempos(), 3 * BAR, 6 * BAR, 120., 60.);
    one_undo_step(&mut window, "Add a bar", &before);

    window.wait_for_bun();
    window.keys("backspace");
    window.until("the rit. as it was", |window| window.sections() == rit);
    assert_eq!(window.problems(), []);
}
