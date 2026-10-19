//! The clock of the project from a page: the transport of the SDK plays it, its tempo is an edit
//! of `project.json` and one undo step, its repeat and its click are the window's and no edit.

use gpui::{Modifiers, TestAppContext, point, px};
use sound_core::Ticks;

use super::{BEAT, Window, open};

impl Window<'_> {
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
    let Some(mut window) = open(cx, &BEAT) else {
        return;
    };
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
fn the_repeat_and_the_click_of_the_transport_are_no_edit(cx: &mut TestAppContext) {
    let Some(mut window) = open(cx, &BEAT) else {
        return;
    };
    let session = window.session.clone();
    let repeat = window.until_found("div-transport-repeat");
    window.cx.simulate_click(repeat, Modifiers::default());
    window.until("four bars repeat", |window| {
        let looped = window
            .cx
            .read(|cx| session.read(cx).looped().read(cx).clone());
        looped.on && looped.range == (Ticks(0)..Ticks(4 * 3840))
    });
    let click = window.until_found("div-transport-click");
    window.cx.simulate_click(click, Modifiers::default());
    window.until("the click", |window| {
        window.cx.read(|cx| session.read(cx).click_on())
    });
    assert_eq!(window.undo_label(), None);
}
