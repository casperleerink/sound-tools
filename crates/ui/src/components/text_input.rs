//! Editable text field. GPUI ships no input widget, so this is the standard pattern: an
//! `Entity` implementing `EntityInputHandler` plus a custom `Element` that shapes the text and
//! paints selection and cursor. Create with `cx.new(|cx| TextInput::new(cx))`.
//!
//! It behaves as a browser's input does on macOS. One line by default, which scrolls sideways
//! to keep the caret in view. `multi_line(n)` wraps at the width of the box, grows up to `n`
//! rows and then scrolls, for the agent composer: enter submits and shift-enter adds a newline.
//! `enter_adds_line()` makes enter add a newline and cmd-enter submit, as in a text area.
//! `fill()` is such an input as tall as its box, for a text that is the whole view.
//!
//! The editing keys follow macOS text fields: option moves and deletes by words, cmd by rows
//! and to the ends of the text, shift with any of them selects, and the ctrl keys of macOS
//! (ctrl-a, ctrl-e, ctrl-k...) work. Up and down past the first or last row go to the ends of
//! the text, except in a menu's search, whose rows take them. cmd-z undoes the field's own
//! edits, never the project's. A double or triple click selects a word or a line, and a drag
//! after it goes on by words or lines. A right click shows Cut, Copy, Paste and Select All. A
//! press outside the field takes the focus from it, also where nothing else takes it, such as
//! a clip under a rename.
//!
//! Known limit: where a word too long for a row is broken inside, end stops one character
//! before the break, because the offset at the break belongs to the next row.

mod rows;
pub mod words;

use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Global,
    GlobalElementId, Hsla, KeyBinding, KeyContext, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollEvent, SharedString, Style, Task,
    TextAlign, TextRun, TextStyle, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions,
    div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::components::dropdown_menu::{
    self, DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
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

/// The key context of a text field. A binding of a plain key outside the field leaves it out,
/// so the field keeps the key.
pub const KEY_CONTEXT: &str = "TextInput";

struct BindingsInstalled;
impl Global for BindingsInstalled {}

/// Bind the editing keys once per process, scoped to the [`KEY_CONTEXT`].
fn install_bindings(cx: &mut App) {
    if cx.has_global::<BindingsInstalled>() {
        return;
    }
    cx.set_global(BindingsInstalled);
    let ctx = Some(KEY_CONTEXT);
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
        KeyBinding::new("secondary-a", SelectAll, ctx),
        KeyBinding::new("secondary-c", Copy, ctx),
        KeyBinding::new("secondary-x", Cut, ctx),
        KeyBinding::new("secondary-v", Paste, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("cmd-left", Home, ctx),
        KeyBinding::new("cmd-right", End, ctx),
        KeyBinding::new("shift-home", SelectToHome, ctx),
        KeyBinding::new("shift-end", SelectToEnd, ctx),
        KeyBinding::new("shift-cmd-left", SelectToHome, ctx),
        KeyBinding::new("shift-cmd-right", SelectToEnd, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("secondary-z", Undo, ctx),
        KeyBinding::new("shift-secondary-z", Redo, ctx),
    ]);
    let multi_line = format!("{KEY_CONTEXT} && multi_line");
    let one_line = format!("{KEY_CONTEXT} && !multi_line");
    let text_area = format!("{KEY_CONTEXT} && enter_adds_line");
    let field = format!("{KEY_CONTEXT} && !enter_adds_line");
    cx.bind_keys([
        KeyBinding::new("enter", Submit, Some(field.as_str())),
        KeyBinding::new("enter", Newline, Some(text_area.as_str())),
        KeyBinding::new("secondary-enter", Submit, Some(text_area.as_str())),
        KeyBinding::new("shift-enter", Newline, Some(multi_line.as_str())),
        KeyBinding::new("shift-enter", Submit, Some(one_line.as_str())),
    ]);
    let arrows = format!("{KEY_CONTEXT} && !{}", dropdown_menu::KEY_CONTEXT);
    let arrows = Some(arrows.as_str());
    cx.bind_keys([
        KeyBinding::new("up", Up, arrows),
        KeyBinding::new("down", Down, arrows),
        KeyBinding::new("shift-up", SelectUp, arrows),
        KeyBinding::new("shift-down", SelectDown, arrows),
        KeyBinding::new("cmd-up", TextStart, arrows),
        KeyBinding::new("cmd-down", TextEnd, arrows),
        KeyBinding::new("shift-cmd-up", SelectToTextStart, arrows),
        KeyBinding::new("shift-cmd-down", SelectToTextEnd, arrows),
    ]);
    // Elsewhere ctrl with a letter is the secondary modifier: ctrl-a selects all.
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("ctrl-a", Home, ctx),
            KeyBinding::new("ctrl-e", End, ctx),
            KeyBinding::new("ctrl-b", Left, ctx),
            KeyBinding::new("ctrl-f", Right, ctx),
            KeyBinding::new("ctrl-h", Backspace, ctx),
            KeyBinding::new("ctrl-d", Delete, ctx),
            KeyBinding::new("ctrl-k", DeleteToEnd, ctx),
            KeyBinding::new("ctrl-p", Up, arrows),
            KeyBinding::new("ctrl-n", Down, arrows),
        ]);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputSize {
    Sm,
    #[default]
    Md,
}

impl InputSize {
    fn height(self) -> f32 {
        match self {
            Self::Sm => 28.,
            Self::Md => 32.,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Self::Sm => 6.,
            Self::Md => 8.,
        }
    }

    fn pad_x(self) -> f32 {
        match self {
            Self::Sm => 8.,
            Self::Md => 10.,
        }
    }
}

/// The height of one row of text.
const ROW_HEIGHT: f32 = 20.;
/// As drawn by macOS.
const CARET_WIDTH: Pixels = px(2.);
const CONTEXT_MENU_WIDTH: f32 = 200.;

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

/// A press and the drag after it: the range the press selected, which the drag keeps, and what
/// the drag selects by.
#[derive(Clone)]
struct Drag {
    anchor: Range<usize>,
    unit: DragUnit,
}

#[derive(Clone, Copy)]
enum DragUnit {
    Character,
    Word,
    Line,
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
    drag: Option<Drag>,
    size: InputSize,
    /// `Some(n)` wraps, grows up to `n` rows and then scrolls. `None` is one line.
    max_rows: Option<usize>,
    /// As tall as its box, see [`Self::fill`].
    fills: bool,
    enter_adds_line: bool,
    /// How far the text is scrolled: up in a multi-line input, left in a one-line one.
    scroll: Point<Pixels>,
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
    on_change: Option<SubmitHandler>,
    on_arrow_past_edge: Option<ArrowHandler>,
    /// The caret blinks from here, so it shows solid right after a move, an edit or a focus.
    blink_start: Instant,
    /// Repaints the caret at its next blink, while it shows.
    blink: Option<Task<()>>,
    /// Whether the field had the focus at the last paint.
    had_focus: bool,
    context_menu: Option<Entity<DropdownMenu>>,
}

/// How long the caret stays on, and then off.
const BLINK: Duration = Duration::from_millis(530);

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
            drag: None,
            size: InputSize::default(),
            max_rows: None,
            fills: false,
            enter_adds_line: false,
            scroll: Point::default(),
            follow_caret: false,
            goal_x: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            bare: false,
            disabled: false,
            on_submit: None,
            on_cancel: None,
            on_change: None,
            on_arrow_past_edge: None,
            blink_start: Instant::now(),
            blink: None,
            had_focus: false,
            context_menu: None,
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

    /// Enter adds a newline and cmd-enter submits, in a [`Self::multi_line`] input.
    pub fn enter_adds_line(mut self) -> Self {
        self.enter_adds_line = true;
        self
    }

    /// A multi-line input as tall as its box, which scrolls past it, and whose enter adds a
    /// newline, as in a text editor.
    pub fn fill(mut self) -> Self {
        self.fills = true;
        self.multi_line(usize::MAX).enter_adds_line()
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

    pub fn set_disabled(&mut self, disabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if let Some(menu) = self.context_menu.clone()
                && menu.read(cx).is_open()
            {
                menu.update(cx, |menu, cx| menu.close(window, cx));
            }
            cx.notify();
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the text, with the caret at its end. This is an undo step, so a message cleared
    /// after sending comes back with cmd-z. Filling an empty field is none, so cmd-z never
    /// empties a field its host filled.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        if !self.content.is_empty() {
            self.record_undo(None, 0..0, 0);
        }
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

    /// Called with the new text after each edit (typing, deleting, pasting, undo), not after
    /// `set_text`.
    pub fn set_on_change(&mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.on_change = Some(Rc::new(f));
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

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo_stack.pop() {
            let current = self.snapshot();
            let before = current.content.clone();
            self.redo_stack.push(current);
            self.restore(snapshot, cx);
            self.changed(&before, window, cx);
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo_stack.pop() {
            let current = self.snapshot();
            let before = current.content.clone();
            self.undo_stack.push(current);
            self.restore(snapshot, cx);
            self.changed(&before, window, cx);
        }
    }

    fn changed(&self, before: &str, window: &mut Window, cx: &mut App) {
        if self.content.as_ref() != before
            && let Some(f) = self.on_change.clone()
        {
            f(&self.content.clone(), window, cx);
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

    /// Keep the text before an edit of `range`, unless the edit joins the last one. While the
    /// IME composes, the text from before the composition is already kept.
    fn record_undo(&mut self, kind: Option<EditKind>, range: Range<usize>, inserted: usize) {
        if self.marked_range.is_some() {
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
        self.blink_start = Instant::now();
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

    /// A one-line field drops the line break at the end of a copied line.
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n");
            let text = match self.max_rows {
                Some(_) => &text,
                None => text.trim_end_matches('\n'),
            };
            self.replace_text_in_range(None, text, window, cx);
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
        let Some(offset) = self.offset_at(event.position) else {
            return;
        };
        let (anchor, unit) = match event.click_count {
            2 => (words::word_at(&self.content, offset), DragUnit::Word),
            3.. => (words::line_at(&self.content, offset), DragUnit::Line),
            _ if event.modifiers.shift => {
                let anchor = match self.selection_reversed {
                    true => self.selected_range.end,
                    false => self.selected_range.start,
                };
                (anchor..anchor, DragUnit::Character)
            }
            _ => (offset..offset, DragUnit::Character),
        };
        self.drag = Some(Drag { anchor, unit });
        self.drag_to(offset, cx);
    }

    /// Selects from what the press selected to the character, word or line at `offset`.
    fn drag_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(Drag { anchor, unit }) = self.drag.clone() else {
            return;
        };
        let under = match unit {
            DragUnit::Character => offset..offset,
            DragUnit::Word => words::word_at(&self.content, offset),
            DragUnit::Line => words::line_at(&self.content, offset),
        };
        self.selected_range = anchor.start.min(under.start)..anchor.end.max(under.end);
        self.selection_reversed = under.start < anchor.start;
        self.caret_moved();
        cx.notify();
    }

    /// A drag selects past the edges of the field too, and the text scrolls after the caret.
    fn on_drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !event.dragging() {
            // Let go outside the window.
            self.drag = None;
        } else if let Some(offset) = self.offset_at(event.position) {
            self.drag_to(offset, cx);
        }
    }

    /// A right click inside the selection keeps it, elsewhere it moves the caret there first.
    fn on_right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus_handle, cx);
        if let Some(offset) = self.offset_at(event.position)
            && !(self.selected_range.start..=self.selected_range.end).contains(&offset)
        {
            self.move_to(offset, cx);
        }
        let entries = self.context_menu_entries(cx);
        let menu = match self.context_menu.clone() {
            Some(menu) => menu,
            None => {
                let menu = cx.new(|cx| {
                    DropdownMenu::new("", Vec::new(), cx)
                        .trigger(Trigger::Pointer)
                        .width(CONTEXT_MENU_WIDTH)
                });
                cx.subscribe_in(&menu, window, Self::on_context_menu_pick)
                    .detach();
                self.context_menu = Some(menu.clone());
                menu
            }
        };
        menu.update(cx, |menu, cx| {
            menu.set_entries(entries, cx);
            menu.open_at(event.position, window, cx);
        });
        cx.notify();
    }

    /// What macOS offers in the menu of a text field, each disabled where it would do nothing.
    fn context_menu_entries(&self, cx: &App) -> Vec<MenuEntry> {
        let selected = !self.selected_range.is_empty();
        let can_paste = (cx.read_from_clipboard().and_then(|item| item.text()))
            .is_some_and(|text| !text.is_empty());
        let all_selected = self.selected_range == (0..self.content.len());
        let command = |value: &'static str, label: &'static str, keys: &'static str, can: bool| {
            (MenuItem::new(value, label).shortcut(keys))
                .selectable(false)
                .disabled(!can)
        };
        vec![
            MenuEntry::Group(MenuGroup::new().items([
                command("cut", "Cut", "mod+x", selected),
                command("copy", "Copy", "mod+c", selected),
                command("paste", "Paste", "mod+v", can_paste),
            ])),
            MenuEntry::Separator,
            MenuEntry::Group(MenuGroup::new().item(command(
                "select-all",
                "Select All",
                "mod+a",
                !all_selected,
            ))),
        ]
    }

    fn on_context_menu_pick(
        &mut self,
        _: &Entity<DropdownMenu>,
        picked: &MenuPicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match picked.0.as_ref() {
            "cut" => self.cut(&Cut, window, cx),
            "copy" => self.copy(&Copy, window, cx),
            "paste" => self.paste(&Paste, window, cx),
            "select-all" => self.select_all_text(cx),
            _ => {}
        }
    }

    /// GPUI moves the focus only to what takes it, and a view may keep a press from the views
    /// around it, as a clip of the arrangement does. So when nothing took the focus by the end
    /// of the press, the field lets go of it, as a browser's input does: a rename commits.
    fn on_press_outside(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        let focus = self.focus_handle.clone();
        window.defer(cx, move |window, cx| {
            if focus.is_focused(window) {
                window.blur(cx);
            }
        });
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(layout), Some(bounds)) = (self.last_layout.as_ref(), self.last_bounds) else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.line_height);
        let scroll = match self.max_rows {
            Some(_) => point(
                px(0.),
                rows::clamp_scroll(
                    self.scroll.y - delta.y,
                    bounds.size.height,
                    layout.content_height(),
                ),
            ),
            None => point(
                rows::clamp_scroll(
                    self.scroll.x - delta.x,
                    bounds.size.width,
                    layout.content_width(),
                ),
                px(0.),
            ),
        };
        // Only text that can scroll takes the wheel, so over a short composer the panel
        // around it scrolls.
        if scroll != self.scroll {
            self.scroll = scroll;
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

    /// The caret offset nearest to a point in the window, `None` before the first frame.
    fn offset_at(&self, position: Point<Pixels>) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let at = position - bounds.origin + self.scroll;
        let row = layout
            .rows
            .get(rows::row_at(at.y, layout.line_height, layout.rows.len()))?;
        Some(self.clamp_offset(layout.offset_for_x(row, at.x)))
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

    /// Grapheme boundaries, as a browser steps: an emoji of several code points is one step.
    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let before = self.content.clone();
        let Some(inserted) = self.edit(range.clone(), new_text) else {
            return;
        };
        let cursor = range.start + inserted;
        self.selected_range = cursor..cursor;
        self.marked_range.take();
        cx.notify();
        self.changed(&before, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let before = self.content.clone();
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
        self.changed(&before, window, cx);
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
        let origin = bounds.origin - self.scroll;
        let top = origin.y + layout.line_height * index;
        Some(Bounds::from_corners(
            point(origin.x + layout.x_for(row, range.start), top),
            point(origin.x + layout.x_for(row, end), top + layout.line_height),
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
    /// Of the widest hard line, unwrapped.
    text_width: Pixels,
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
        let text_width = (lines.iter())
            .map(|line| line.unwrapped_layout.width)
            .fold(px(0.), Pixels::max);
        Self {
            rows: rows::rows(&text, &wraps),
            lines,
            line_height,
            text_width,
        }
    }

    /// The caret math works on the text, not on the placeholder shown when it is empty.
    fn of_empty_text(mut self) -> Self {
        self.rows = rows::rows("", &[]);
        self.text_width = px(0.);
        self
    }

    fn content_height(&self) -> Pixels {
        self.line_height * self.rows.len()
    }

    /// How far a one-line field scrolls: to a caret after the last character.
    fn content_width(&self) -> Pixels {
        self.text_width + CARET_WIDTH
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
    scroll: Point<Pixels>,
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
        if input.fills {
            style.size.height = relative(1.).into();
            return (window.request_layout(style, [], cx), ());
        }
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
        let input = self.input.read(cx);
        let focused = input.focus_handle.is_focused(window);
        let theme = cx.theme();
        let cursor_color = theme.blue;
        // As macOS shows the selection of a field that lost the focus.
        let selection_color = match focused {
            true => theme.blue.opacity(0.25),
            false => theme.alpha_at(0.10),
        };
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let wrap_width = input.max_rows.map(|_| bounds.size.width);
        let (text, runs) = input.display(&style, placeholder_color(cx));
        let mut layout = TextLayout::shape(text, &runs, font_size, wrap_width, line_height, window);
        if input.content.is_empty() {
            layout = layout.of_empty_text();
        }

        let caret = input.cursor_offset();
        let caret_row = rows::row_of(&layout.rows, caret);
        let caret_x = layout
            .rows
            .get(caret_row)
            .map_or(px(0.), |row| layout.x_for(row, caret));
        let scroll_axis = |scroll, caret_start, caret_size, view, content| match input.follow_caret
        {
            true => {
                rows::scroll_to_show(scroll, caret_start, caret_start + caret_size, view, content)
            }
            false => rows::clamp_scroll(scroll, view, content),
        };
        let scroll = match input.max_rows {
            Some(_) => point(
                px(0.),
                scroll_axis(
                    input.scroll.y,
                    line_height * caret_row,
                    line_height,
                    bounds.size.height,
                    layout.content_height(),
                ),
            ),
            None => point(
                scroll_axis(
                    input.scroll.x,
                    caret_x,
                    CARET_WIDTH,
                    bounds.size.width,
                    layout.content_width(),
                ),
                px(0.),
            ),
        };
        let left = bounds.left() - scroll.x;
        let row_top = |index: usize| bounds.top() - scroll.y + line_height * index;

        let selected_range = input.selected_range.clone();
        let (selections, cursor) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(
                    fill(
                        Bounds::new(
                            point(left + caret_x, row_top(caret_row)),
                            size(CARET_WIDTH, line_height),
                        ),
                        cursor_color,
                    )
                    .corner_radii(px(1.)),
                ),
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
            scroll,
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
        let focused = focus_handle.is_focused(window);
        let blink_start = match focused && !input.had_focus {
            true => Instant::now(),
            false => input.blink_start,
        };
        let blinked = blink_start.elapsed().as_nanos();
        let caret_on = (blinked / BLINK.as_nanos()).is_multiple_of(2);
        let next_blink = BLINK - Duration::from_nanos((blinked % BLINK.as_nanos()) as u64);
        // A multi-line input leaves room at the right for a caret at the very end of a full
        // row. A one-line input scrolls to show it.
        let caret_room = match input.max_rows {
            Some(_) => CARET_WIDTH,
            None => px(0.),
        };
        // A disabled field that has the focus, given by its host, takes no text either.
        if !input.disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        // Not on the field's own element, which hears the mouse only while it is over it.
        window.on_mouse_event({
            let input = self.input.clone();
            move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && input.read(cx).drag.is_some() {
                    input.update(cx, |input, cx| input.on_drag_move(event, cx));
                }
            }
        });
        window.on_mouse_event({
            let input = self.input.clone();
            move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && input.read(cx).drag.is_some() {
                    input.update(cx, |input, _| input.drag = None);
                }
            }
        });
        let Some(layout) = prepaint.layout.take() else {
            return;
        };
        let scroll = prepaint.scroll;
        let mask = ContentMask {
            bounds: Bounds::from_corners(
                bounds.origin,
                point(bounds.right() + caret_room, bounds.bottom()),
            ),
        };
        let caret = prepaint.cursor.take().filter(|_| focused);
        window.with_content_mask(Some(mask), |window| {
            for selection in prepaint.selections.drain(..) {
                window.paint_quad(selection);
            }
            let mut origin = bounds.origin - scroll;
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
            if caret_on && let Some(caret) = caret.clone() {
                window.paint_quad(caret);
            }
        });
        self.input.update(cx, |input, cx| {
            input.blink = caret.is_some().then(|| {
                cx.spawn(async move |input, cx| {
                    cx.background_executor().timer(next_blink).await;
                    input.update(cx, |_, cx| cx.notify()).ok();
                })
            });
            input.blink_start = blink_start;
            input.had_focus = focused;
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.scroll = scroll;
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
        key_context.add(KEY_CONTEXT);
        if multi_line {
            key_context.add("multi_line");
        }
        if self.enter_adds_line {
            key_context.add("enter_adds_line");
        }
        // The padding leaves room for the border, so the row fits the box exactly: a row taller
        // than its box scrolled 2 px at every key. A multi-line box takes its height from its
        // rows, so one row is as tall as a one-line field.
        let pad_y = (size.height() - ROW_HEIGHT - 2.) / 2.;

        let field = div()
            .w_full()
            .when(self.fills, |d| d.h_full())
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
            .text_size(px(14.))
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
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
                    .on_mouse_down_out(cx.listener(Self::on_press_outside))
                    .on_scroll(cx.listener(Self::on_scroll))
            })
            .child(TextElement { input: cx.entity() });
        // The menu is outside the field's key context, so enter and escape are the menu's.
        div()
            .w_full()
            .when(self.fills, |d| d.h_full())
            .child(field)
            .children(self.context_menu.clone())
    }
}
