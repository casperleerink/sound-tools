//! Editable text field. GPUI ships no input widget, so this is the standard pattern: an
//! `Entity` implementing `EntityInputHandler` plus a custom `Element` that shapes the text and
//! paints selection and cursor. Heights 28/32/40 px, disabled renders at 40% opacity. Create
//! with `cx.new(|cx| TextInput::new(cx))`.
//!
//! One line by default. `multi_line(n)` wraps at the width of the box, grows up to `n` rows and
//! then scrolls, for the agent composer: enter submits, shift-enter adds a newline, up and down
//! move by rows, and cmd-z undoes.
//!
//! The editing keys follow macOS text fields: option moves and deletes by words, cmd by rows
//! (and cmd-up and down to the ends of a multi-line text), shift with any of them selects,
//! and a double or triple click selects a word or a line.
//!
//! Known limit: where a word too long for a row is broken inside, end stops one character
//! before the break, because the offset at the break belongs to the next row.

mod rows;
mod words;

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Global,
    GlobalElementId, Hsla, KeyBinding, KeyContext, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollWheelEvent, SharedString, Style,
    TextAlign, TextRun, TextStyle, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions,
    div, fill, point, prelude::*, px, relative, size,
};

use crate::theme::ActiveTheme;
use rows::Row;

/// Up or down from the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    Up,
    Down,
}

actions!(
    sound_text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToHome,
        DeleteToEnd,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        TextStart,
        TextEnd,
        SelectToTextStart,
        SelectToTextEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
        Up,
        Down,
        SelectUp,
        SelectDown,
        Newline,
        Undo,
        Redo,
    ]
);

struct BindingsInstalled;
impl Global for BindingsInstalled {}

/// Bind the editing keys once per process, scoped to the `TextInput` key context.
fn install_bindings(cx: &mut App) {
    if cx.has_global::<BindingsInstalled>() {
        return;
    }
    cx.set_global(BindingsInstalled);
    let ctx = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        // Shift is still held after a shift-enter or a capital, and must not eat the key.
        KeyBinding::new("shift-backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("shift-delete", Delete, ctx),
        KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("alt-delete", DeleteWordRight, ctx),
        KeyBinding::new("cmd-backspace", DeleteToHome, ctx),
        KeyBinding::new("cmd-delete", DeleteToEnd, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("alt-left", WordLeft, ctx),
        KeyBinding::new("alt-right", WordRight, ctx),
        KeyBinding::new("shift-alt-left", SelectWordLeft, ctx),
        KeyBinding::new("shift-alt-right", SelectWordRight, ctx),
        KeyBinding::new("cmd-a", SelectAll, ctx),
        KeyBinding::new("cmd-c", Copy, ctx),
        KeyBinding::new("cmd-x", Cut, ctx),
        KeyBinding::new("cmd-v", Paste, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("cmd-left", Home, ctx),
        KeyBinding::new("cmd-right", End, ctx),
        KeyBinding::new("shift-home", SelectToHome, ctx),
        KeyBinding::new("shift-end", SelectToEnd, ctx),
        KeyBinding::new("shift-cmd-left", SelectToHome, ctx),
        KeyBinding::new("shift-cmd-right", SelectToEnd, ctx),
        KeyBinding::new("enter", Submit, ctx),
        KeyBinding::new("escape", Cancel, ctx),
    ]);
    // Only a multi-line input takes these, so a one-line field (a rename) leaves up, down and
    // cmd-z to the views around it, as before.
    let multi_line = Some("TextInput && multi_line");
    cx.bind_keys([
        KeyBinding::new("up", Up, multi_line),
        KeyBinding::new("down", Down, multi_line),
        KeyBinding::new("shift-up", SelectUp, multi_line),
        KeyBinding::new("shift-down", SelectDown, multi_line),
        KeyBinding::new("cmd-up", TextStart, multi_line),
        KeyBinding::new("cmd-down", TextEnd, multi_line),
        KeyBinding::new("shift-cmd-up", SelectToTextStart, multi_line),
        KeyBinding::new("shift-cmd-down", SelectToTextEnd, multi_line),
        KeyBinding::new("shift-enter", Newline, multi_line),
        KeyBinding::new("cmd-z", Undo, multi_line),
        KeyBinding::new("shift-cmd-z", Redo, multi_line),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl InputSize {
    fn height(self) -> f32 {
        match self {
            Self::Sm => 28.,
            Self::Md => 32.,
            Self::Lg => 40.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Sm => 6.,
            Self::Md => 8.,
            Self::Lg => 10.,
        }
    }

    fn pad_x(self) -> f32 {
        match self {
            Self::Sm => 8.,
            Self::Md => 10.,
            Self::Lg => 12.,
        }
    }

    fn text_size(self) -> f32 {
        match self {
            Self::Lg => 15.,
            _ => 14.,
        }
    }
}

/// The height of one row of text.
const ROW_HEIGHT: f32 = 20.;

type SubmitHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type ArrowHandler = Rc<dyn Fn(&str, Arrow, &mut Window, &mut App)>;

/// The text and selection before an edit, for undo.
struct Snapshot {
    content: SharedString,
    selected_range: Range<usize>,
}

/// Typing or deleting one character right where the last such edit left off joins its undo
/// step. A typed space ends the step, so cmd-z takes back a typed word and not a letter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    size: InputSize,
    /// `Some(n)` wraps, grows up to `n` rows and then scrolls. `None` is one line.
    max_rows: Option<usize>,
    /// How far the rows are scrolled up, in a multi-line input.
    scroll_top: Pixels,
    /// Set by every edit and caret move, so the next frame scrolls to the caret. The wheel
    /// clears it, so the rows stay where the composer scrolled them.
    follow_caret: bool,
    /// The x that a run of up and down presses keeps.
    goal_x: Option<Pixels>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    /// The last edit a next one may join, and where it left the caret.
    last_edit: Option<(EditKind, usize)>,
    bare: bool,
    disabled: bool,
    on_submit: Option<SubmitHandler>,
    on_cancel: Option<SubmitHandler>,
    on_arrow_past_edge: Option<ArrowHandler>,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        install_bindings(cx);
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: SharedString::default(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            size: InputSize::default(),
            max_rows: None,
            scroll_top: px(0.),
            follow_caret: false,
            goal_x: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            bare: false,
            disabled: false,
            on_submit: None,
            on_cancel: None,
            on_arrow_past_edge: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    /// Wrap at the width of the box and keep newlines. The box grows with the text from one
    /// row up to `max_rows`, and then scrolls.
    pub fn multi_line(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows.max(1));
        self
    }

    /// Drop the field's own surface, border, padding and focus ring, for use inside a card
    /// that already provides them (the agent composer).
    pub fn bare(mut self, bare: bool) -> Self {
        self.bare = bare;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the text, with the caret at its end. In a multi-line input this is an undo
    /// step, so a message cleared after sending comes back with cmd-z.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.record_undo(None, 0..0, 0);
        self.content = text.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.caret_moved();
        cx.notify();
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.select_range(0..self.content.len(), cx);
    }

    fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.selected_range = self.clamp_offset(range.start)..self.clamp_offset(range.end);
        self.selection_reversed = false;
        self.caret_moved();
        cx.notify();
    }

    /// Called on enter with the current text.
    pub fn set_on_submit(&mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.on_submit = Some(Rc::new(f));
    }

    /// Called on escape with the current text. Without it escape goes on to the views around
    /// the field.
    pub fn set_on_cancel(&mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.on_cancel = Some(Rc::new(f));
    }

    /// Called with the current text when up is pressed on the first row or down on the last,
    /// after the caret went to the start or the end. The agent composer recalls earlier
    /// messages with it. Like the other handlers it runs inside an update of this input, so
    /// defer a change to it.
    pub fn set_on_arrow_past_edge(
        &mut self,
        f: impl Fn(&str, Arrow, &mut Window, &mut App) + 'static,
    ) {
        self.on_arrow_past_edge = Some(Rc::new(f));
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    /// From the start of a selection, like left.
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let start = words::previous_word_start(&self.content, self.selected_range.start);
        self.move_to(start, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let end = words::next_word_end(&self.content, self.selected_range.end);
        self.move_to(end, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            words::previous_word_start(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            words::next_word_end(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all_text(cx);
    }

    /// The start of the caret's row, which is the start of the text in a one-line field.
    fn row_start(&self) -> usize {
        self.caret_row().map_or(0, |row| row.range.start)
    }

    fn row_end(&self) -> usize {
        self.caret_row()
            .map_or(self.content.len(), |row| row.caret_end)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.row_start(), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.row_end(), cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.row_start(), cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.row_end(), cx);
    }

    fn text_start(&mut self, _: &TextStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn text_end(&mut self, _: &TextEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_to_text_start(
        &mut self,
        _: &SelectToTextStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    fn select_to_text_end(&mut self, _: &SelectToTextEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(Arrow::Up, false, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(Arrow::Down, false, window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(Arrow::Up, true, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(Arrow::Down, true, window, cx);
    }

    /// One row up or down, at the x where the run of presses began. Past the first or last
    /// row the caret goes to the start or end of the text, and the host hears of it.
    fn move_vertically(
        &mut self,
        arrow: Arrow,
        select: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        let from = match (select || self.selected_range.is_empty(), arrow) {
            (true, _) => self.cursor_offset(),
            (false, Arrow::Up) => self.selected_range.start,
            (false, Arrow::Down) => self.selected_range.end,
        };
        let row_index = rows::row_of(&layout.rows, from);
        let Some(row) = layout.rows.get(row_index) else {
            return;
        };
        let goal_x = self.goal_x.unwrap_or_else(|| layout.x_for(row, from));
        let target_index = match arrow {
            Arrow::Up => row_index.checked_sub(1),
            Arrow::Down => Some(row_index + 1),
        };
        let target = target_index
            .and_then(|index| layout.rows.get(index))
            .map(|target| layout.offset_for_x(target, goal_x));
        let offset = target.unwrap_or(match arrow {
            Arrow::Up => 0,
            Arrow::Down => self.content.len(),
        });
        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        self.goal_x = Some(goal_x);
        if target.is_none()
            && !select
            && let Some(f) = self.on_arrow_past_edge.clone()
        {
            f(&self.content.clone(), arrow, window, cx);
        }
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(current);
            self.restore(snapshot, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(current);
            self.restore(snapshot, cx);
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.content = snapshot.content;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_edit = None;
        self.caret_moved();
        cx.notify();
    }

    /// Keep the text before an edit of `range`, unless the edit joins the last one. Only a
    /// multi-line input keeps a history: a one-line field leaves cmd-z alone. While the IME
    /// composes, the text from before the composition is already kept.
    fn record_undo(&mut self, kind: Option<EditKind>, range: Range<usize>, inserted: usize) {
        if self.max_rows.is_none() || self.marked_range.is_some() {
            return;
        }
        let joins = matches!(
            (kind, self.last_edit),
            (Some(kind), Some((last_kind, at)))
                if kind == last_kind && (range.start == at || range.end == at)
        );
        if !joins {
            let snapshot = self.snapshot();
            self.undo_stack.push(snapshot);
        }
        self.redo_stack.clear();
        self.last_edit = kind.map(|kind| (kind, range.start + inserted));
    }

    /// Every change of the text goes through here, so undo sees it. A one-line field turns
    /// newlines into spaces. Returns the length of what went in, or `None` for a range that
    /// is not on character boundaries of the text, which changes nothing.
    fn edit(&mut self, range: Range<usize>, new_text: &str) -> Option<usize> {
        let (Some(before), Some(removed), Some(after)) = (
            self.content.get(..range.start),
            self.content.get(range.clone()),
            self.content.get(range.end..),
        ) else {
            return None;
        };
        let new_text = if self.max_rows.is_some() {
            new_text.to_string()
        } else {
            new_text.replace('\n', " ")
        };
        let kind = if new_text.is_empty() {
            (removed.chars().count() == 1).then_some(EditKind::Deleting)
        } else {
            (range.is_empty() && new_text.chars().count() == 1 && new_text != "\n")
                .then_some(EditKind::Typing)
        };
        let content = format!("{before}{new_text}{after}");
        self.record_undo(kind, range, new_text.len());
        if new_text == " " {
            self.last_edit = None;
        }
        self.content = content.into();
        self.caret_moved();
        Some(new_text.len())
    }

    /// `offset` inside the text and on a character boundary. Rows from the last frame can be
    /// older than the text, after a `set_text` before the next frame.
    fn clamp_offset(&self, offset: usize) -> usize {
        self.content.floor_char_boundary(offset)
    }

    /// The row the caret is on, as the last frame laid it out.
    fn caret_row(&self) -> Option<&Row> {
        let layout = self.last_layout.as_ref()?;
        layout
            .rows
            .get(rows::row_of(&layout.rows, self.cursor_offset()))
    }

    fn caret_moved(&mut self) {
        self.goal_x = None;
        self.follow_caret = true;
    }

    /// Delete the selection, or without one the text from the caret to `offset`.
    fn delete_to(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(offset, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_to(self.previous_boundary(self.cursor_offset()), window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_to(self.next_boundary(self.cursor_offset()), window, cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = words::previous_word_start(&self.content, self.cursor_offset());
        self.delete_to(start, window, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = words::next_word_end(&self.content, self.cursor_offset());
        self.delete_to(end, window, cx);
    }

    /// At the start of a row it takes the newline before, as macOS does.
    fn delete_to_home(&mut self, _: &DeleteToHome, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.cursor_offset();
        let start = match self.row_start() {
            start if start < caret => start,
            _ => self.previous_boundary(caret),
        };
        self.delete_to(start, window, cx);
    }

    fn delete_to_end(&mut self, _: &DeleteToEnd, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.cursor_offset();
        let end = match self.row_end() {
            end if end > caret => end,
            _ => self.next_boundary(caret),
        };
        self.delete_to(end, window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn selected_text(&self) -> Option<String> {
        self.content
            .get(self.selected_range.clone())
            .filter(|selected| !selected.is_empty())
            .map(str::to_string)
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace("\r\n", "\n"), window, cx);
        }
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_submit.clone() {
            f(&self.content.clone(), window, cx);
        }
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        match self.on_cancel.clone() {
            Some(f) => f(&self.content.clone(), window, cx),
            // Escape means nothing to this field, so the view around it gets it: a panel
            // gives the focus back to where it was.
            None => cx.propagate(),
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let offset = self.offset_at(event.position).unwrap_or(0);
        // A double click selects the word and a triple click the line. Those don't drag, so a
        // twitch of the mouse keeps them.
        self.is_selecting = event.click_count < 2;
        match event.click_count {
            2 => self.select_range(words::word_at(&self.content, offset), cx),
            3.. => self.select_range(words::line_at(&self.content, offset), cx),
            _ if event.modifiers.shift => self.select_to(offset, cx),
            _ => self.move_to(offset, cx),
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.offset_at(event.position).unwrap_or(0), cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(layout), Some(bounds)) = (self.last_layout.as_ref(), self.last_bounds) else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.line_height).y;
        let scroll_top = rows::clamp_scroll(
            self.scroll_top - delta,
            bounds.size.height,
            layout.content_height(),
        );
        // Only rows that can scroll take the wheel, so over a short composer the panel
        // around it scrolls.
        if scroll_top != self.scroll_top {
            self.scroll_top = scroll_top;
            self.follow_caret = false;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        self.selected_range = offset..offset;
        self.caret_moved();
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// The caret offset at a point in the window, `None` above the text.
    fn offset_at(&self, position: Point<Pixels>) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let y = position.y - bounds.top() + self.scroll_top;
        let row = layout
            .rows
            .get(rows::row_at(y, layout.line_height, layout.rows.len())?)?;
        Some(self.clamp_offset(layout.offset_for_x(row, position.x - bounds.left())))
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp_offset(offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.caret_moved();
        cx.notify();
    }

    /// The text to show, the placeholder when empty, with the IME's marked text underlined.
    fn display(&self, style: &TextStyle, placeholder_color: Hsla) -> (SharedString, Vec<TextRun>) {
        let (text, color) = if self.content.is_empty() {
            (self.placeholder.clone(), placeholder_color)
        } else {
            (self.content.clone(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match self.marked_range.clone() {
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        (text, runs)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_from_utf16(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// Char boundaries, not graphemes: combining marks move one code point at a time.
    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        self.content.get(range).map(str::to_string)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let Some(inserted) = self.edit(range.clone(), new_text) else {
            return;
        };
        let cursor = range.start + inserted;
        self.selected_range = cursor..cursor;
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let Some(inserted) = self.edit(range.clone(), new_text) else {
            return;
        };
        let end = range.start + inserted;
        self.marked_range = (inserted > 0).then(|| range.start..end);
        // The IME places the selection inside the new text. A one-line field swapped its
        // newlines for spaces, which keeps every offset.
        self.selected_range = new_selected_range_utf16.map_or(end..end, |new| {
            range.start + utf8_from_utf16(new_text, new.start)
                ..range.start + utf8_from_utf16(new_text, new.end)
        });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let index = rows::row_of(&layout.rows, range.start);
        let row = layout.rows.get(index)?;
        // A range over more rows is placed by its first row.
        let end = range.end.min(row.range.end);
        let top = bounds.top() - self.scroll_top + layout.line_height * index;
        Some(Bounds::from_corners(
            point(bounds.left() + layout.x_for(row, range.start), top),
            point(
                bounds.left() + layout.x_for(row, end),
                top + layout.line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.offset_at(position)
            .map(|offset| self.offset_to_utf16(offset))
    }
}

/// The byte offset in `text` of a UTF-16 offset, as the IME counts.
fn utf8_from_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= offset_utf16 {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

/// The shaped text of a frame, kept for the caret math until the next one.
struct TextLayout {
    /// One per hard line.
    lines: Vec<WrappedLine>,
    rows: Vec<Row>,
    line_height: Pixels,
}

impl TextLayout {
    fn shape(
        text: SharedString,
        runs: &[TextRun],
        font_size: Pixels,
        wrap_width: Option<Pixels>,
        line_height: Pixels,
        window: &Window,
    ) -> Self {
        let lines = window
            .text_system()
            .shape_text(text.clone(), font_size, runs, wrap_width, None)
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        let wraps: Vec<Vec<usize>> = lines.iter().map(wrap_offsets).collect();
        Self {
            rows: rows::rows(&text, &wraps),
            lines,
            line_height,
        }
    }

    fn content_height(&self) -> Pixels {
        self.line_height * self.rows.len()
    }

    /// The x of `offset` from the left of its row.
    fn x_for(&self, row: &Row, offset: usize) -> Pixels {
        let Some(line) = self.lines.get(row.line) else {
            return px(0.);
        };
        let layout = &line.unwrapped_layout;
        layout.x_for_index(offset.saturating_sub(row.line_start))
            - layout.x_for_index(row.range.start.saturating_sub(row.line_start))
    }

    /// The caret offset on `row` closest to `x` from the left of the row.
    fn offset_for_x(&self, row: &Row, x: Pixels) -> usize {
        let Some(line) = self.lines.get(row.line) else {
            return row.range.start;
        };
        let layout = &line.unwrapped_layout;
        let row_x = layout.x_for_index(row.range.start.saturating_sub(row.line_start));
        (row.line_start + layout.closest_index_for_x(row_x + x))
            .clamp(row.range.start, row.caret_end)
    }
}

/// The offsets inside a hard line where it wrapped.
fn wrap_offsets(line: &WrappedLine) -> Vec<usize> {
    line.wrap_boundaries
        .iter()
        .filter_map(|boundary| {
            let run = line.unwrapped_layout.runs.get(boundary.run_ix)?;
            Some(run.glyphs.get(boundary.glyph_ix)?.index)
        })
        .collect()
}

/// Paints the shaped text plus selection and cursor, and installs the IME handler.
struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<TextLayout>,
    scroll_top: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn placeholder_color(cx: &App) -> Hsla {
    cx.theme().gray_950.opacity(0.4)
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let line_height = window.line_height();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let input = self.input.read(cx);
        let Some(max_rows) = input.max_rows else {
            style.size.height = line_height.into();
            return (window.request_layout(style, [], cx), ());
        };
        // The height follows the rows, and the rows follow the width, which only the layout
        // knows: so the text is shaped when the layout measures it.
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let (text, runs) = input.display(&text_style, placeholder_color(cx));
        let layout_id =
            window.request_measured_layout(style, move |known, available, window, _| {
                let width = known.width.or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let layout =
                    TextLayout::shape(text.clone(), &runs, font_size, width, line_height, window);
                size(
                    width.unwrap_or_default(),
                    line_height * layout.rows.len().clamp(1, max_rows),
                )
            });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.theme();
        let (cursor_color, selection_color) = (theme.blue, theme.blue.opacity(0.25));
        let input = self.input.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let wrap_width = input.max_rows.map(|_| bounds.size.width);
        let (text, runs) = input.display(&style, placeholder_color(cx));
        let mut layout = TextLayout::shape(text, &runs, font_size, wrap_width, line_height, window);
        // The caret math works on the text, not on the placeholder it shows when empty.
        if input.content.is_empty() {
            layout.rows = rows::rows("", &[]);
        }

        let caret = input.cursor_offset();
        let caret_row = rows::row_of(&layout.rows, caret);
        let scroll_top = if input.follow_caret {
            rows::scroll_to_show(
                input.scroll_top,
                caret_row,
                line_height,
                bounds.size.height,
                layout.content_height(),
            )
        } else {
            rows::clamp_scroll(
                input.scroll_top,
                bounds.size.height,
                layout.content_height(),
            )
        };
        let left = bounds.left();
        let row_top = |index: usize| bounds.top() - scroll_top + line_height * index;

        let selected_range = input.selected_range.clone();
        let (selections, cursor) = if selected_range.is_empty() {
            let x = layout
                .rows
                .get(caret_row)
                .map_or(px(0.), |row| layout.x_for(row, caret));
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(left + x, row_top(caret_row)),
                        size(px(1.5), line_height),
                    ),
                    cursor_color,
                )),
            )
        } else {
            let selections = rows::selection_spans(&layout.rows, &selected_range)
                .into_iter()
                .filter_map(|(index, span, continues)| {
                    let row = layout.rows.get(index)?;
                    let start = layout.x_for(row, span.start);
                    let mut end = layout.x_for(row, span.end);
                    // A selection that goes on to the next row fills this one to the edge.
                    if continues {
                        end = end.max(bounds.size.width);
                    }
                    Some(fill(
                        Bounds::from_corners(
                            point(left + start, row_top(index)),
                            point(left + end, row_top(index) + line_height),
                        ),
                        selection_color,
                    ))
                })
                .collect();
            (selections, None)
        };

        PrepaintState {
            layout: Some(layout),
            scroll_top,
            cursor,
            selections,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = self.input.read(cx);
        let focus_handle = input.focus_handle.clone();
        let multi_line = input.max_rows.is_some();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let Some(layout) = prepaint.layout.take() else {
            return;
        };
        let scroll_top = prepaint.scroll_top;
        // A multi-line input shows only the rows in its box. The mask leaves room at the
        // sides for a caret at the very end of a row.
        let mask = multi_line.then(|| ContentMask {
            bounds: Bounds::from_corners(
                point(bounds.left() - px(2.), bounds.top()),
                point(bounds.right() + px(2.), bounds.bottom()),
            ),
        });
        window.with_content_mask(mask, |window| {
            for selection in prepaint.selections.drain(..) {
                window.paint_quad(selection);
            }
            let mut origin = point(bounds.left(), bounds.top() - scroll_top);
            for line in &layout.lines {
                line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
                origin.y += line.size(layout.line_height).height;
            }
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.scroll_top = scroll_top;
            input.follow_caret = false;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (surface, border, border_active, text) = (
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.alpha_at(0.20),
            theme.gray_950,
        );
        let focused = self.focus_handle.is_focused(window);
        let size = self.size;
        let disabled = self.disabled;
        let bare = self.bare;
        let multi_line = self.max_rows.is_some();
        let mut key_context = KeyContext::default();
        key_context.add("TextInput");
        if multi_line {
            key_context.add("multi_line");
        }
        // A multi-line box takes its height from its rows. Its padding leaves room for the
        // border, so one row is as tall as a one-line field.
        let pad_y = if multi_line {
            (size.height() - ROW_HEIGHT - 2.) / 2.
        } else {
            (size.height() - ROW_HEIGHT) / 2.
        };

        div()
            .w_full()
            .when(!multi_line, |d| {
                d.h(px(if bare { ROW_HEIGHT } else { size.height() }))
            })
            .when(!bare, |d| {
                d.px(px(size.pad_x()))
                    .py(px(pad_y))
                    .rounded(px(size.radius()))
                    .bg(surface)
                    .border_1()
                    .border_color(border)
            })
            .flex()
            .flex_col()
            .text_color(text)
            .text_size(px(size.text_size()))
            .line_height(px(ROW_HEIGHT))
            .when(disabled, |d| d.opacity(0.4))
            .when(!disabled, |d| {
                d.key_context(key_context)
                    .track_focus(&self.focus_handle)
                    .cursor(CursorStyle::IBeam)
                    .when(!bare, |d| d.hover(move |s| s.border_color(border_active)))
                    .when(focused && !bare, |d| d.border_color(border_active))
                    .on_action(cx.listener(Self::backspace))
                    .on_action(cx.listener(Self::delete))
                    .on_action(cx.listener(Self::delete_word_left))
                    .on_action(cx.listener(Self::delete_word_right))
                    .on_action(cx.listener(Self::delete_to_home))
                    .on_action(cx.listener(Self::delete_to_end))
                    .on_action(cx.listener(Self::left))
                    .on_action(cx.listener(Self::right))
                    .on_action(cx.listener(Self::select_left))
                    .on_action(cx.listener(Self::select_right))
                    .on_action(cx.listener(Self::word_left))
                    .on_action(cx.listener(Self::word_right))
                    .on_action(cx.listener(Self::select_word_left))
                    .on_action(cx.listener(Self::select_word_right))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(Self::home))
                    .on_action(cx.listener(Self::end))
                    .on_action(cx.listener(Self::select_to_home))
                    .on_action(cx.listener(Self::select_to_end))
                    .on_action(cx.listener(Self::text_start))
                    .on_action(cx.listener(Self::text_end))
                    .on_action(cx.listener(Self::select_to_text_start))
                    .on_action(cx.listener(Self::select_to_text_end))
                    .on_action(cx.listener(Self::up))
                    .on_action(cx.listener(Self::down))
                    .on_action(cx.listener(Self::select_up))
                    .on_action(cx.listener(Self::select_down))
                    .on_action(cx.listener(Self::newline))
                    .on_action(cx.listener(Self::undo))
                    .on_action(cx.listener(Self::redo))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::cut))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::submit))
                    .on_action(cx.listener(Self::cancel))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .when(multi_line, |d| {
                        d.on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
                    })
            })
            .child(TextElement { input: cx.entity() })
    }
}
