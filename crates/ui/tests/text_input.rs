//! The multi-line text input with keys, on the test text system, where every character is
//! 8.4 px wide at the 14 px text size.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AppContext, ClipboardItem, Context, Entity, EntityInputHandler, Focusable, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, point, px,
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

/// The selection as the IME sees it, in UTF-16.
fn selection(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> Range<usize> {
    input
        .update_in(cx, |input, window, cx| {
            input.selected_text_range(false, window, cx)
        })
        .map(|selection| selection.range)
        .unwrap_or_default()
}

fn utf16_len(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> usize {
    text(input, cx).encode_utf16().count()
}

#[gpui::test]
fn up_and_down_move_across_wrapped_rows_and_past_the_edges(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    let arrows: Rc<RefCell<Vec<Arrow>>> = Rc::default();
    input.update(cx, |input, _| {
        let arrows = arrows.clone();
        input.set_on_arrow_past_edge(move |_, arrow, _, _| arrows.borrow_mut().push(arrow));
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
fn three_backspaces_are_one_step_and_a_space_ends_a_typed_word(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    cx.simulate_keystrokes("a b space c d");
    assert_eq!(text(&input, cx), "ab cd");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "ab ");
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(text(&input, cx), "");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "ab ");
}

#[gpui::test]
fn the_caret_moves_by_characters_in_text_of_many_bytes(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));

    // Twelve CJK characters wrap after ten. Up keeps the x of two characters.
    set_text(&input, "日本語日本語日本語日本語", cx);
    cx.simulate_keystrokes("up x");
    assert_eq!(text(&input, cx), "日本x語日本語日本語日本語");
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(text(&input, cx), "日語日本語日本語日本語");

    // End of a hard line, and backspace over a two-byte character.
    set_text(&input, "üü\nab", cx);
    cx.simulate_keystrokes("up home end backspace");
    assert_eq!(text(&input, cx), "ü\nab");

    // An emoji is two characters wide on the test text system.
    set_text(&input, "😀a\n😀b", cx);
    cx.simulate_keystrokes("up x");
    assert_eq!(text(&input, cx), "😀ax\n😀b");
}

#[gpui::test]
fn a_composition_keeps_its_selection_inside_the_text(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    cx.simulate_keystrokes("a b space");
    // A Japanese IME: "k" shows as a marked "ｋ", then "a" turns it into "か", then it commits.
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "ｋ", Some(1..1), window, cx)
    });
    assert_eq!(selection(&input, cx), 4..4);
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "か", Some(1..1), window, cx)
    });
    assert_eq!(text(&input, cx), "ab か");
    assert_eq!(selection(&input, cx), 4..4);
    assert!(selection(&input, cx).end <= utf16_len(&input, cx));
    cx.simulate_keystrokes("shift-left cmd-c");
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("か"));
    input.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, "か", window, cx)
    });
    assert_eq!(text(&input, cx), "ab か");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "ab ");
    assert!(selection(&input, cx).end <= utf16_len(&input, cx));
}

#[gpui::test]
fn a_click_on_the_second_row_puts_the_caret_there(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    set_text(&input, "aaaa bbbb cccc dddd", cx);
    // Rows are 20 px high and characters 8.4 px wide: after "cc" on the second row.
    cx.simulate_click(point(px(18.), px(30.)), Modifiers::none());
    cx.simulate_keystrokes("x");
    assert_eq!(text(&input, cx), "aaaa bbbb ccxcc dddd");
}

#[gpui::test]
fn the_rows_scroll_to_the_caret_past_the_last_row_of_the_box(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(2).bare(true));
    // Four rows in a box of two, with the caret on the last: "c" and "d" show.
    set_text(&input, "a\nb\nc\nd", cx);
    cx.simulate_click(point(px(30.), px(5.)), Modifiers::none());
    cx.simulate_keystrokes("x");
    assert_eq!(text(&input, cx), "a\nb\ncx\nd");
}

#[gpui::test]
fn a_one_line_field_leaves_up_and_cmd_z_to_the_views_around_it(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a b up cmd-z c");
    assert_eq!(text(&input, cx), "abc");
}

#[gpui::test]
fn a_one_line_field_turns_newlines_into_spaces(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a shift-enter");
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("b\r\nc\nd".into())));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(text(&input, cx), "a b c d");
}

#[gpui::test]
fn shift_backspace_still_deletes(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    cx.simulate_keystrokes("a shift-enter shift-backspace shift-backspace b");
    assert_eq!(text(&input, cx), "b");
    cx.simulate_keystrokes("left shift-delete");
    assert_eq!(text(&input, cx), "");
}

#[gpui::test]
fn option_moves_selects_and_deletes_by_words(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    set_text(&input, "one two three", cx);
    cx.simulate_keystrokes("alt-left alt-left x");
    assert_eq!(text(&input, cx), "one xtwo three");
    cx.simulate_keystrokes("alt-right alt-right y");
    assert_eq!(text(&input, cx), "one xtwo threey");

    set_text(&input, "one two three", cx);
    cx.simulate_keystrokes("alt-backspace");
    assert_eq!(text(&input, cx), "one two ");
    cx.simulate_keystrokes("alt-backspace");
    assert_eq!(text(&input, cx), "one ");
    // A deleted word is one undo step.
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text(&input, cx), "one two ");

    set_text(&input, "one two three", cx);
    cx.simulate_keystrokes("cmd-up alt-delete");
    assert_eq!(text(&input, cx), " two three");
    cx.simulate_keystrokes("shift-alt-right shift-alt-right backspace");
    assert_eq!(text(&input, cx), "");
}

#[gpui::test]
fn cmd_arrows_go_to_the_ends_of_the_row_and_the_text(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    // Two rows, "aaaa bbbb " and "cccc dd".
    set_text(&input, "aaaa bbbb cccc dd", cx);
    cx.simulate_keystrokes("cmd-left 1");
    assert_eq!(text(&input, cx), "aaaa bbbb 1cccc dd");
    cx.simulate_keystrokes("cmd-right 2");
    assert_eq!(text(&input, cx), "aaaa bbbb 1cccc dd2");
    cx.simulate_keystrokes("cmd-up 3");
    assert_eq!(text(&input, cx), "3aaaa bbbb 1cccc dd2");
    cx.simulate_keystrokes("cmd-down 4");
    assert_eq!(text(&input, cx), "3aaaa bbbb 1cccc dd24");

    set_text(&input, "ab\ncd", cx);
    cx.simulate_keystrokes("shift-cmd-left backspace");
    assert_eq!(text(&input, cx), "ab\n");
    set_text(&input, "ab\ncd", cx);
    cx.simulate_keystrokes("shift-cmd-up backspace");
    assert_eq!(text(&input, cx), "");

    // Cmd-backspace deletes to the start of the row, and at the start it joins the rows.
    set_text(&input, "ab\ncd", cx);
    cx.simulate_keystrokes("cmd-backspace");
    assert_eq!(text(&input, cx), "ab\n");
    cx.simulate_keystrokes("cmd-backspace");
    assert_eq!(text(&input, cx), "ab");
}

#[gpui::test]
fn a_one_line_field_takes_the_word_and_row_keys_too(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a b space c d alt-backspace e cmd-left f shift-cmd-right backspace");
    assert_eq!(text(&input, cx), "f");
}

fn click(cx: &mut VisualTestContext, x: f32, click_count: usize) {
    let position = point(px(x), px(5.));
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count,
    });
}

#[gpui::test]
fn a_double_click_selects_the_word_and_a_triple_click_the_line(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    set_text(&input, "ab cd\nef", cx);
    // Characters are 8.4 px wide: inside "cd".
    click(cx, 30., 1);
    click(cx, 30., 2);
    cx.simulate_keystrokes("x");
    assert_eq!(text(&input, cx), "ab x\nef");
    click(cx, 10., 3);
    cx.simulate_keystrokes("y");
    assert_eq!(text(&input, cx), "y\nef");
}
