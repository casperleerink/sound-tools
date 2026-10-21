//! The text input with keys and the mouse, on the test text system, where every character is
//! 8.4 px wide at the 14 px text size.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AppContext, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, Focusable, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, Styled, TestAppContext, VisualTestContext, Window, div, point, px,
};
use sound_ui::components::text_input::{Arrow, TextInput};

/// Ten characters to a row. A one-line field with its border and padding shows 63 px of text.
const WIDTH: f32 = 85.;
/// Where the text of a one-line field starts: its border and padding.
const TEXT_LEFT: f32 = 11.;

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
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "abc");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "");
    cx.simulate_keystrokes("shift-secondary-z shift-secondary-z");
    assert_eq!(text(&input, cx), "ac");

    // A text the host set is a step too, so a sent message comes back.
    set_text(&input, "", cx);
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "ac");
}

#[gpui::test]
fn three_backspaces_are_one_step_and_a_space_ends_a_typed_word(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    cx.simulate_keystrokes("a b space c d");
    assert_eq!(text(&input, cx), "ab cd");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "ab ");
    cx.simulate_keystrokes("backspace backspace backspace");
    assert_eq!(text(&input, cx), "");
    cx.simulate_keystrokes("secondary-z");
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

    // An emoji of several code points is one step, as a thumb with a skin tone.
    set_text(&input, "a👍🏽b", cx);
    cx.simulate_keystrokes("left backspace");
    assert_eq!(text(&input, cx), "ab");
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
    cx.simulate_keystrokes("shift-left secondary-c");
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("か"));
    input.update_in(cx, |input, window, cx| {
        input.replace_text_in_range(None, "か", window, cx)
    });
    assert_eq!(text(&input, cx), "ab か");
    cx.simulate_keystrokes("secondary-z");
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
fn a_one_line_field_undoes_its_own_edits_and_up_and_down_go_to_its_ends(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a b space c d up x down y");
    assert_eq!(text(&input, cx), "xab cdy");
    cx.simulate_keystrokes("secondary-z secondary-z");
    assert_eq!(text(&input, cx), "ab cd");
    cx.simulate_keystrokes("shift-secondary-z");
    assert_eq!(text(&input, cx), "xab cd");
}

#[gpui::test]
fn cmd_z_never_empties_a_field_its_host_filled(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    set_text(&input, "Bass", cx);
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "Bass");
}

#[gpui::test]
fn a_one_line_field_turns_newlines_into_spaces(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    cx.simulate_keystrokes("a");
    // A copied line's own break goes.
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("b\r\nc\nd\n".into())));
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(text(&input, cx), "ab c d");
}

/// Where the text from `offset` is drawn, from the left top of the field's text.
fn place_of(input: &Entity<TextInput>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    input
        .update_in(cx, |input, window, cx| {
            input.bounds_for_range(offset..offset, Bounds::default(), window, cx)
        })
        .map(|bounds| bounds.origin)
        .unwrap_or_default()
}

#[gpui::test]
fn the_text_stays_put_from_the_placeholder_to_the_first_characters_and_back(
    cx: &mut TestAppContext,
) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).placeholder("Search"));
    let empty = place_of(&input, 0, cx);
    for keys in ["a", "b", "left", "backspace", "backspace"] {
        cx.simulate_keystrokes(keys);
        assert_eq!(place_of(&input, 0, cx), empty, "after {keys}");
    }
}

#[gpui::test]
fn a_long_line_scrolls_to_keep_the_caret_in_view(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    // 20 characters are 168 px, in a field that shows 63 px.
    cx.simulate_keystrokes("a b c d e f g h i j k l m n o p q r s t");
    let caret = place_of(&input, 20, cx).x;
    assert!(caret > px(0.) && caret <= px(61.), "caret at {caret:?}");
    cx.simulate_keystrokes("cmd-left");
    assert_eq!(place_of(&input, 0, cx).x, px(0.));
    cx.simulate_keystrokes("cmd-right");

    // A click lands on the character under it in the scrolled text: 107 px scrolled, and
    // 20 px into the text is after "o".
    cx.simulate_click(point(px(TEXT_LEFT + 20.), px(16.)), Modifiers::none());
    cx.simulate_keystrokes("X");
    assert_eq!(text(&input, cx), "abcdefghijklmnoXpqrst");
}

fn press(cx: &mut VisualTestContext, x: f32, click_count: usize) {
    cx.simulate_event(MouseDownEvent {
        position: point(px(x), px(5.)),
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count,
        first_mouse: false,
    });
}

fn drag_to(cx: &mut VisualTestContext, x: f32) {
    cx.simulate_event(MouseMoveEvent {
        position: point(px(x), px(5.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
}

fn release(cx: &mut VisualTestContext, x: f32) {
    cx.simulate_event(MouseUpEvent {
        position: point(px(x), px(5.)),
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
}

#[gpui::test]
fn a_drag_past_the_edge_of_the_field_selects_on(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    set_text(&input, "abcdefghijklmnopqrst", cx);
    cx.simulate_keystrokes("cmd-left");
    press(cx, TEXT_LEFT + 18., 1);
    // Far right of the field, where its element no longer hears the mouse.
    drag_to(cx, 300.);
    release(cx, 300.);
    cx.simulate_keystrokes("Z");
    assert_eq!(text(&input, cx), "abZ");
}

#[gpui::test]
fn a_drag_after_a_double_click_selects_whole_words(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |cx| TextInput::new(cx).multi_line(4).bare(true));
    set_text(&input, "ab cd ef", cx);
    // From inside "ab" to inside "ef": both words whole.
    press(cx, 5., 1);
    release(cx, 5.);
    press(cx, 5., 2);
    drag_to(cx, 55.);
    release(cx, 55.);
    cx.simulate_keystrokes("x");
    assert_eq!(text(&input, cx), "x");
}

#[gpui::test]
fn a_right_click_in_the_selection_offers_to_cut_it(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    set_text(&input, "Bass", cx);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_event(MouseDownEvent {
        position: point(px(TEXT_LEFT + 10.), px(16.)),
        button: MouseButton::Right,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    // Cut is the first row; enter is the menu's, not the field's.
    cx.simulate_keystrokes("down enter");
    assert_eq!(text(&input, cx), "");
    let cut = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(cut.as_deref(), Some("Bass"));
    // The focus is back in the field.
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(text(&input, cx), "Bass");
}

#[gpui::test]
fn a_press_outside_takes_the_focus_where_nothing_else_does(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::new);
    let focused = |cx: &mut VisualTestContext| {
        input.update_in(cx, |input, window, cx| {
            input.focus_handle(cx).is_focused(window)
        })
    };
    assert!(focused(cx));
    cx.simulate_click(point(px(40.), px(200.)), Modifiers::none());
    assert!(!focused(cx));
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
    cx.simulate_keystrokes("secondary-z");
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
