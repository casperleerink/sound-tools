//! The multi-line text input with keys, on the test text system, where every character is
//! 8.4 px wide at the 14 px text size.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AppContext, Context, Entity, Focusable, IntoElement, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, px,
};
use sound_ui::components::text_input::{Arrow, TextInput};

/// Ten characters to a row.
const WIDTH: f32 = 85.;

struct Field {
    input: Entity<TextInput>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(WIDTH)).child(self.input.clone())
    }
}

fn open(
    cx: &mut TestAppContext,
    make: fn(&mut Context<TextInput>) -> TextInput,
) -> (Entity<TextInput>, &mut VisualTestContext) {
    cx.update(sound_ui::init);
    let (field, cx) = cx.add_window_view(|_, cx| Field {
        input: cx.new(make),
    });
    let input = field.read_with(cx, |field, _| field.input.clone());
    input.update_in(cx, |input, window, cx| {
        window.focus(&input.focus_handle(cx), cx)
    });
    cx.run_until_parked();
    (input, cx)
}

fn text(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    input.read_with(cx, |input, _| input.text().to_string())
}

fn set_text(input: &Entity<TextInput>, text: &'static str, cx: &mut VisualTestContext) {
    input.update(cx, |input, cx| input.set_text(text, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn up_and_down_move_across_wrapped_rows_and_past_the_edges(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    let arrows: Rc<RefCell<Vec<Arrow>>> = Rc::default();
    input.update(cx, |input, _| {
        let arrows = arrows.clone();
        input.set_on_arrow_past_edge(move |arrow, _, _| arrows.borrow_mut().push(arrow));
    });

    // Two rows, "aaaa bbbb " and "cccc dddd"; the caret is at the end.
    set_text(&input, "aaaa bbbb cccc dddd", cx);
    cx.simulate_keystrokes("up x");
    assert_eq!(text(&input, cx), "aaaa bbbbx cccc dddd");
    assert!(arrows.borrow().is_empty());

    // Up on the first row goes to the start and tells the host.
    set_text(&input, "aaaa bbbb cccc dddd", cx);
    cx.simulate_keystrokes("up up y");
    assert_eq!(text(&input, cx), "yaaaa bbbb cccc dddd");
    assert_eq!(*arrows.borrow(), [Arrow::Up]);

    // Down keeps the x where the presses began, also over a shorter row between.
    set_text(&input, "abcdefgh\nab\nabcdefgh", cx);
    cx.simulate_keystrokes("up up down down z");
    assert_eq!(text(&input, cx), "abcdefgh\nab\nabcdefghz");
    cx.simulate_keystrokes("down");
    assert_eq!(*arrows.borrow(), [Arrow::Up, Arrow::Down]);

    // Home and end go to the ends of the row, not of the text. The end of a wrapped row is
    // before its trailing space.
    set_text(&input, "aaaa bbbb cccc dd", cx);
    cx.simulate_keystrokes("up home 1");
    assert_eq!(text(&input, cx), "1aaaa bbbb cccc dd");
    set_text(&input, "aaaa bbbb cccc dd", cx);
    cx.simulate_keystrokes("up end 2");
    assert_eq!(text(&input, cx), "aaaa bbbb2 cccc dd");
}

#[gpui::test]
fn shift_enter_adds_a_line_and_enter_submits(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    let submitted: Rc<RefCell<Vec<String>>> = Rc::default();
    input.update(cx, |input, _| {
        let submitted = submitted.clone();
        input.set_on_submit(move |text, _, _| submitted.borrow_mut().push(text.to_string()));
    });
    cx.simulate_keystrokes("a shift-enter b enter");
    assert_eq!(text(&input, cx), "a\nb");
    assert_eq!(*submitted.borrow(), ["a\nb"]);
}

#[gpui::test]
fn cmd_z_takes_back_a_typed_word_and_shift_cmd_z_brings_it_again(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    cx.simulate_keystrokes("a b c left backspace");
    assert_eq!(text(&input, cx), "ac");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "abc");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "");
    cx.simulate_keystrokes("shift-cmd-z shift-cmd-z");
    assert_eq!(text(&input, cx), "ac");

    // A text the host set is a step too, so a sent message comes back.
    set_text(&input, "", cx);
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "ac");
}

#[gpui::test]
fn a_one_line_field_leaves_up_and_cmd_z_to_the_views_around_it(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a b up cmd-z c");
    assert_eq!(text(&input, cx), "abc");
}
