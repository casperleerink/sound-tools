//! The clock of the project from a page: the transport of the SDK plays it, its tempo is an edit
//! of `project.json` and one undo step, its click and the computer keys are the window's and no
//! edit.

use gpui::{Modifiers, TestAppContext, point, px};
use sound_core::Ticks;

use super::{BEAT, Opened, open};

impl Opened<'_> {
    fn tempo(&mut self) -> f64 {
        let session = self.session.clone();
        self.cx.read(|cx| {
            let file = session.read(cx).project().project_file();
            file.tempo_map.change_at(Ticks(0)).bpm.bpm()
        })
    }
}

#[gpui::test]
fn a_drag_on_the_tempo_of_the_transport_is_one_undo_step(cx: &mut TestAppContext) {
    let mut window = open(cx, &BEAT);
    let tempo = window.space_at("transport-tempo") + point(px(10.), px(8.));
    window.press(tempo);
    window.drag_to(tempo + point(px(0.), px(-10.)));
    // Twenty points up: ten beats a minute faster.
    let end = tempo + point(px(0.), px(-20.));
    window.drag_to(end);
    window.release(end);
    window.until("the tempo", |window| window.tempo() == 130.0);
    window.after_the_runs();
    assert_eq!(window.undo().as_deref(), Some("Change the tempo"));
    assert_eq!(window.tempo(), 120.0);
}

#[gpui::test]
fn the_click_and_the_computer_keys_of_the_transport_are_no_edit(cx: &mut TestAppContext) {
    let mut window = open(cx, &BEAT);
    let session = window.session.clone();
    let click = window.until_found("button-click");
    window.cx.simulate_click(click, Modifiers::default());
    window.until("the click", |window| {
        window.cx.read(|cx| session.read(cx).click_on())
    });
    let keys = window.until_found("button-computer-keys");
    window.cx.simulate_click(keys, Modifiers::default());
    window.until("the computer keys", |window| {
        window.cx.read(|cx| session.read(cx).computer_keys_on())
    });
    assert_eq!(window.undo_label(), None);
}
