//! The piano roll of `tooling/agent-eval/examples`, a page written in TypeScript, played with
//! the mouse and the keys: what moves while the pointer holds it moves in the window, and the
//! code hears each gesture once.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, point, px,
};
use serde_json::{Value, json};

use super::{ONLY_THE_TOOL, Opened, open};

const PIANO_ROLL: &str = include_str!("../../../../../tooling/agent-eval/examples/piano-roll.tsx");

/// Three notes: a at 0 on 86, b after it on 84, c later on 82.
fn three_notes() -> Value {
    json!([
        { "start": 0, "length": 960, "pitch": 86, "velocity": 100 },
        { "start": 960, "length": 960, "pitch": 84, "velocity": 80 },
        { "start": 2880, "length": 480, "pitch": 82, "velocity": 60 },
    ])
}

fn note(start: u32, length: u32, pitch: u8, velocity: u8) -> Value {
    json!({ "start": start, "length": length, "pitch": pitch, "velocity": velocity })
}

pub(super) fn open_roll<'a>(cx: &'a mut TestAppContext, notes: &Value) -> Opened<'a> {
    let record = json!({ "tool": "piano-roll", "state": { "notes": notes } }).to_string();
    open(
        cx,
        &[
            ONLY_THE_TOOL[0],
            ("state/piano-roll.json", &record),
            ("extensions/piano-roll.tsx", PIANO_ROLL),
        ],
    )
}

impl Opened<'_> {
    /// Where `tick` and the middle of the row of `pitch` are in the roll.
    pub(super) fn on_roll(&mut self, tick: f32, pitch: u8) -> Point<Pixels> {
        let row = 127.0 - f64::from(pitch) + 0.5;
        self.in_space("roll", (f64::from(tick), row))
    }

    /// How far `ticks` across and `rows` down are in the roll.
    fn on_roll_by(&mut self, ticks: f32, rows: f32) -> Point<Pixels> {
        let roll = self.units("roll");
        let (across, down) = (roll.across as f32, roll.down as f32);
        point(px(ticks * across), px(rows * down))
    }

    fn pointer_down(&mut self, at: Point<Pixels>, modifiers: Modifiers, clicks: usize) {
        self.cx.simulate_mouse_move(at, None, modifiers);
        self.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers,
            button: MouseButton::Left,
            click_count: clicks,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    /// Moves the held pointer from `from` by `by` in `steps` moves.
    fn pointer_drag(
        &mut self,
        from: Point<Pixels>,
        by: Point<Pixels>,
        steps: usize,
        modifiers: Modifiers,
    ) -> Point<Pixels> {
        for step in 1..=steps {
            let to = from + by * (step as f32 / steps as f32);
            self.cx
                .simulate_mouse_move(to, MouseButton::Left, modifiers);
            self.cx.run_until_parked();
        }
        from + by
    }

    fn pointer_up(&mut self, at: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// A click, and Bun's answer to it.
    fn click_at(&mut self, at: Point<Pixels>, modifiers: Modifiers, clicks: usize) {
        self.pointer_down(at, modifiers, clicks);
        self.pointer_up(at, modifiers);
        self.wait_for_bun();
    }

    /// A drag from `from` by `by`, let go, and Bun's answer to it.
    fn drag_by(&mut self, from: Point<Pixels>, by: Point<Pixels>, modifiers: Modifiers) {
        self.pointer_down(from, Modifiers::none(), 1);
        self.wait_for_bun();
        let to = self.pointer_drag(from, by, 8, modifiers);
        self.pointer_up(to, modifiers);
        self.wait_for_bun();
    }

    /// A key such as `cmd-c`, and Bun's answer to it.
    fn chord(&mut self, key: &str) {
        self.cx.simulate_keystrokes(key);
        self.wait_for_bun();
    }

    fn notes(&mut self) -> Value {
        self.record("piano-roll")["notes"].clone()
    }

    fn notes_become(&mut self, what: &str, notes: Value) {
        self.until(what, |window| window.notes() == notes);
        self.after_the_runs();
    }
}

#[gpui::test]
fn a_double_click_adds_a_note_on_the_grid_and_another_on_it_removes_it(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let empty = window.on_roll(1940.0, 85);
    window.click_at(empty, Modifiers::none(), 2);
    let mut notes = three_notes();
    notes.as_array_mut().unwrap().push(note(1920, 480, 85, 100));
    window.notes_become("the note to be added", notes);
    assert_eq!(window.undo_label().as_deref(), Some("Add a note"));
    window.click_at(empty, Modifiers::none(), 2);
    window.notes_become("the note to be removed", three_notes());
    assert_eq!(window.undo_label().as_deref(), Some("Remove a note"));
}

#[gpui::test]
fn a_drag_moves_the_selected_notes_on_the_grid_as_one_undo_step_and_bun_hears_it_once(
    cx: &mut TestAppContext,
) {
    let mut window = open_roll(cx, &three_notes());
    let (a, b) = (window.on_roll(100.0, 86), window.on_roll(1100.0, 84));
    let by = window.on_roll_by(520., 1.);
    window.click_at(a, Modifiers::none(), 1);
    window.click_at(b, Modifiers::shift(), 1);
    window.pointer_down(a, Modifiers::none(), 1);
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    // 520 ticks right and a row down: a lands on the sixteenth at 480.
    let to = window.pointer_drag(a, by, 20, Modifiers::none());
    assert_eq!(sound_typescript::sent_updates_and_events(), asked);
    window.pointer_up(to, Modifiers::none());
    window.notes_become(
        "the selected notes to move",
        json!([
            note(480, 960, 85, 100),
            note(1440, 960, 83, 80),
            note(2880, 480, 82, 60)
        ]),
    );
    assert_eq!(window.undo_label().as_deref(), Some("Move notes"));
}

#[gpui::test]
fn a_drag_with_alt_copies_the_selected_notes(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let c = window.on_roll(3000.0, 82);
    window.click_at(c, Modifiers::none(), 1);
    let by = window.on_roll_by(960., -2.);
    window.drag_by(c, by, Modifiers::alt());
    let mut notes = three_notes();
    notes.as_array_mut().unwrap().push(note(3840, 480, 84, 60));
    window.notes_become("the note to be copied", notes);
    assert_eq!(window.undo_label().as_deref(), Some("Copy notes"));
}

#[gpui::test]
fn a_drag_of_the_end_of_a_note_resizes_it_to_the_grid(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    // The end of b is at 1920; 330 ticks on puts it on 2160.
    let end_of_b = window.on_roll(1900.0, 84);
    let by = window.on_roll_by(330., 0.);
    window.drag_by(end_of_b, by, Modifiers::none());
    window.notes_become(
        "the note to be resized",
        json!([
            note(0, 960, 86, 100),
            note(960, 1200, 84, 80),
            note(2880, 480, 82, 60)
        ]),
    );
    assert_eq!(window.undo_label().as_deref(), Some("Resize notes"));
}

#[gpui::test]
fn a_marquee_selects_what_it_covers_and_delete_removes_it(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let from = window.on_roll(2000.0, 87);
    let by = window.on_roll_by(1500., 6.);
    window.drag_by(from, by, Modifiers::none());
    window.chord("backspace");
    window.notes_become(
        "the covered note to be deleted",
        json!([note(0, 960, 86, 100), note(960, 960, 84, 80)]),
    );
    assert_eq!(window.undo_label().as_deref(), Some("Delete notes"));
}

#[gpui::test]
fn copy_paste_duplicate_and_select_all_work_from_the_keys(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let a = window.on_roll(100.0, 86);
    window.click_at(a, Modifiers::none(), 1);
    window.chord("cmd-c");
    window.chord("cmd-v");
    // After the last note, which ends at 3360.
    let mut notes = three_notes();
    notes.as_array_mut().unwrap().push(note(3360, 960, 86, 100));
    window.notes_become("the note to be pasted", notes.clone());
    assert_eq!(window.undo_label().as_deref(), Some("Paste notes"));
    // The pasted note is selected, and goes again right after itself.
    window.chord("cmd-d");
    notes.as_array_mut().unwrap().push(note(4320, 960, 86, 100));
    window.notes_become("the note to be duplicated", notes);
    assert_eq!(window.undo_label().as_deref(), Some("Duplicate notes"));
    window.chord("cmd-a");
    window.chord("delete");
    window.notes_become("every note to be deleted", json!([]));
}

#[gpui::test]
fn a_drag_of_a_bar_in_the_lane_changes_the_velocity(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let bar_of_c = window.control("shape-velocity-2");
    // 31 points up of the lane's 62: half of 127 more.
    window.drag_by(bar_of_c, point(px(0.), px(-31.)), Modifiers::none());
    window.notes_become(
        "the velocity to change",
        json!([
            note(0, 960, 86, 100),
            note(960, 960, 84, 80),
            note(2880, 480, 82, 124)
        ]),
    );
    assert_eq!(window.undo_label().as_deref(), Some("Change velocity"));
}

#[gpui::test]
fn escape_lets_go_of_a_drag_and_its_code_hears_nothing(cx: &mut TestAppContext) {
    let mut window = open_roll(cx, &three_notes());
    let a = window.on_roll(100.0, 86);
    window.pointer_down(a, Modifiers::none(), 1);
    window.wait_for_bun();
    let asked = sound_typescript::sent_updates_and_events();
    let by = window.on_roll_by(1000., 0.);
    let to = window.pointer_drag(a, by, 8, Modifiers::none());
    window.cx.simulate_keystrokes("escape");
    window.pointer_up(to, Modifiers::none());
    window.wait_for_bun();
    assert_eq!(window.notes(), three_notes());
    // Escape reached the page, and no drag ended: no event went to Bun.
    assert_eq!(sound_typescript::sent_updates_and_events().1, asked.1);
}
