//! Markdown: the agent's answers. `Markdown::parse` turns the source into a small block model,
//! and `MarkdownText` draws it. An answer arrives streaming, so the caller parses the whole
//! message again on each batch; a 5 KB answer parses in well under a millisecond.
//!
//! The text of one message can be selected: drag, or double- or triple-click for a word or a
//! line, and cmd-c copies it. A selection stays inside its message.
//!
//! No syntax colours. HTML is shown as its source, images as their alt text.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, ClipboardItem, DispatchPhase, Div, ElementId, Entity, FocusHandle, FontStyle,
    FontWeight, HighlightStyle, InteractiveText, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, StyleRefinement, StyledText, TextLayout,
    UnderlineStyle, Window, actions, canvas, div, prelude::*, px,
};
use pulldown_cmark::{Alignment, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use sound_ui::components::text_input::words;
use sound_ui::typography::MONO;
use sound_ui::{ActiveTheme, Theme};

actions!(agent_markdown, [Copy]);

/// The key context of a message, where cmd-c copies the selection.
pub(super) const KEY_CONTEXT: &str = "Markdown";

/// A parsed message. Cheap to clone, so a view keeps it and hands it to every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Markdown {
    blocks: Arc<[Block]>,
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Paragraph(Inline),
    Heading {
        size: HeadingSize,
        text: Inline,
    },
    /// `first_number` is `None` for a bullet list. Each item is a list of blocks, so an item
    /// can hold several paragraphs, a code block or a nested list.
    List {
        first_number: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Code(SharedString),
    Table(Table),
    Rule,
}

/// The first row is the header. Every row has one cell per column, `None` when it is empty.
#[derive(Clone, Debug, PartialEq)]
struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Option<Inline>>>,
}

/// Six heading levels are too many for a sidebar: `#` and `##` are large, the rest small.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeadingSize {
    Large,
    Small,
}

/// The text of a paragraph or heading, with its styles. The spans cover the text in order.
#[derive(Clone, Debug, PartialEq)]
struct Inline {
    text: SharedString,
    spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
struct Span {
    /// Bytes of `Inline::text`.
    range: Range<usize>,
    style: SpanStyle,
}

/// All false and no link is plain text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SpanStyle {
    bold: bool,
    italic: bool,
    code: bool,
    /// Only a link that opens: see `opens`.
    link: Option<SharedString>,
}

impl Markdown {
    /// Never fails: markdown has no syntax errors, and unfinished markdown, such as an open code
    /// fence while the answer streams, reads as far as it goes.
    pub fn parse(source: &str) -> Self {
        let mut builder = Builder::default();
        for event in Parser::new_ext(source, Options::ENABLE_TABLES) {
            builder.event(event);
        }
        Self {
            blocks: builder.finish().into(),
        }
    }

    /// One line the app writes, such as "Run `cargo build`": only the text between two
    /// backticks is styled, as code, and nothing else is markdown. A `*` in a command or a
    /// pattern stays a star, and a backtick with no partner shows as it is.
    pub fn inline_code(line: &str) -> Self {
        let mut inline = InlineBuilder::default();
        let parts: Vec<&str> = line.split('`').collect();
        let last = parts.len().saturating_sub(1);
        let code = SpanStyle {
            code: true,
            ..SpanStyle::default()
        };
        for (index, part) in parts.into_iter().enumerate() {
            match (index % 2 == 1, index < last) {
                (true, true) => inline.push(part, code.clone()),
                (true, false) => inline.push(&format!("`{part}"), SpanStyle::default()),
                (false, _) => inline.push(part, SpanStyle::default()),
            }
        }
        Self {
            blocks: inline.finish().map(Block::Paragraph).into_iter().collect(),
        }
    }

    fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// An open quote, list or list item, which collects the blocks inside it.
enum Container {
    Quote(Vec<Block>),
    List {
        first_number: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
}

#[derive(Default)]
struct Builder {
    document: Vec<Block>,
    open: Vec<Container>,
    /// The text of the paragraph or heading being read. A tight list item has text with no
    /// paragraph around it, so text starts one by itself and the next block ends it.
    inline: InlineBuilder,
    code: Option<String>,
    table: Option<Table>,
    bold: usize,
    italic: usize,
    /// One entry per open link, `None` for a link that does not open.
    links: Vec<Option<SharedString>>,
}

impl Builder {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some(code) => code.push_str(&text),
                None => self.text(&text, false),
            },
            Event::Code(text) => self.text(&text, true),
            Event::Html(text) | Event::InlineHtml(text) => self.text(&text, false),
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => self.push_block(Block::Rule),
            // Footnotes, math and task lists are off in the parser options.
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let url = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                };
                self.links
                    .push(opens(&url).then(|| SharedString::from(url)));
            }
            // The alt text follows as text.
            Tag::Image { .. } => {}
            Tag::BlockQuote(_) => self.open(Container::Quote(Vec::new())),
            Tag::List(first_number) => self.open(Container::List {
                first_number,
                items: Vec::new(),
            }),
            Tag::Item => self.open(Container::Item(Vec::new())),
            Tag::CodeBlock(_) => {
                self.end_paragraph();
                self.code = Some(String::new());
            }
            Tag::Table(alignments) => {
                self.end_paragraph();
                self.table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                });
            }
            // The cells of the header come with no row around them.
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            _ => self.end_paragraph(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {}
            TagEnd::Heading(level) => {
                if let Some(text) = std::mem::take(&mut self.inline).finish() {
                    let size = match level {
                        HeadingLevel::H1 | HeadingLevel::H2 => HeadingSize::Large,
                        _ => HeadingSize::Small,
                    };
                    self.push_block(Block::Heading { size, text });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(mut code) = self.code.take() {
                    code.truncate(code.trim_end_matches('\n').len());
                    self.push_block(Block::Code(code.into()));
                }
            }
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item => self.close(),
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline).finish();
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some(mut table) = self.table.take() {
                    // A short row would shift the cells after it into the wrong column.
                    for row in &mut table.rows {
                        row.resize(table.alignments.len(), None);
                    }
                    self.push_block(Block::Table(table));
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {}
            _ => self.end_paragraph(),
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        let style = SpanStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            link: self.links.last().cloned().flatten(),
        };
        self.inline.push(text, style);
    }

    fn open(&mut self, container: Container) {
        self.end_paragraph();
        self.open.push(container);
    }

    fn close(&mut self) {
        self.end_paragraph();
        let block = match self.open.pop() {
            Some(Container::Quote(blocks)) => Block::Quote(blocks),
            Some(Container::List {
                first_number,
                items,
            }) => Block::List {
                first_number,
                items,
            },
            Some(Container::Item(blocks)) => match self.open.last_mut() {
                Some(Container::List { items, .. }) => {
                    items.push(blocks);
                    return;
                }
                // An item outside a list does not happen; keep its text anyway.
                _ => Block::Quote(blocks),
            },
            None => return,
        };
        self.push_block(block);
    }

    fn end_paragraph(&mut self) {
        if let Some(text) = std::mem::take(&mut self.inline).finish() {
            self.push_block(Block::Paragraph(text));
        }
    }

    fn push_block(&mut self, block: Block) {
        self.end_paragraph();
        match self.open.last_mut() {
            Some(Container::Quote(blocks) | Container::Item(blocks)) => blocks.push(block),
            // Nothing comes between the start of a list and its first item.
            Some(Container::List { .. }) | None => self.document.push(block),
        }
    }

    fn finish(mut self) -> Vec<Block> {
        // The parser closes every tag it opens, even at the end of an unfinished message.
        while !self.open.is_empty() {
            self.close();
        }
        self.end_paragraph();
        self.document
    }
}

#[derive(Default)]
struct InlineBuilder {
    text: String,
    spans: Vec<Span>,
}

impl InlineBuilder {
    fn push(&mut self, text: &str, style: SpanStyle) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.range.end = end,
            _ => self.spans.push(Span {
                range: start..end,
                style,
            }),
        }
    }

    /// `None` when there is nothing to show. An HTML block ends in a line break.
    fn finish(mut self) -> Option<Inline> {
        let length = self.text.trim_end_matches('\n').len();
        if length == 0 {
            return None;
        }
        self.text.truncate(length);
        self.spans.retain_mut(|span| {
            span.range.end = span.range.end.min(length);
            !span.range.is_empty()
        });
        Some(Inline {
            text: self.text.into(),
            spans: self.spans,
        })
    }
}

/// Only web and mail links open. A link in an answer is written by the agent, and a `file:`
/// or app URL could start something on the machine.
fn opens(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

/// Draws a parsed message. Text is 15 on 22 unless the caller sets another size, and takes the
/// colour of its parent. It wraps to the width it is given.
#[derive(IntoElement)]
pub struct MarkdownText {
    base: Div,
    id: ElementId,
    markdown: Markdown,
}

impl MarkdownText {
    /// The id must be unique per message. The items of a gpui `list` share one namespace, and
    /// the scroll offset of a code block and the clicks on a link are kept under this id.
    pub fn new(id: impl Into<ElementId>, markdown: Markdown) -> Self {
        Self {
            base: div().text_size(px(15.)).line_height(px(22.)),
            id: id.into(),
            markdown,
        }
    }
}

impl Styled for MarkdownText {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// What is selected in one message, in bytes of its flat text: every text of the message one
/// after the other, with a newline between two. It shows while the message has focus.
struct Selection {
    focus: FocusHandle,
    /// The message it was made in. A streaming answer parses into a new one, whose offsets
    /// mean other text, so the selection goes away.
    markdown: Markdown,
    anchor: usize,
    head: usize,
    dragging: bool,
}

impl Selection {
    /// Empty when the message changed since.
    fn range(&self, markdown: &Markdown) -> Range<usize> {
        if self.markdown != *markdown {
            return 0..0;
        }
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// One text of a message as drawn, where it starts in the flat text and where it lies on screen.
struct Piece {
    start: usize,
    text: SharedString,
    layout: TextLayout,
}

impl Piece {
    fn end(&self) -> usize {
        self.start + self.text.len()
    }
}

impl RenderOnce for MarkdownText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let selection = window.use_keyed_state(self.id.clone(), cx, |_, cx| Selection {
            focus: cx.focus_handle(),
            markdown: Markdown::default(),
            anchor: 0,
            head: 0,
            dragging: false,
        });
        let (focus, selected) = {
            let selection = selection.read(cx);
            let shows = selection.focus.is_focused(window);
            (
                selection.focus.clone(),
                shows
                    .then(|| selection.range(&self.markdown))
                    .unwrap_or_default(),
            )
        };
        let mut renderer = Renderer {
            theme: cx.theme().clone(),
            next_id: 0,
            selection: selection.clone(),
            selected,
            pieces: Vec::new(),
            length: 0,
        };
        let blocks = renderer.blocks(self.markdown.blocks());
        let pieces: Rc<[Piece]> = renderer.pieces.into();
        self.base
            .id(self.id)
            .key_context(KEY_CONTEXT)
            .track_focus(&focus)
            .flex()
            .flex_col()
            .gap(px(12.))
            .min_w_0()
            .on_mouse_down(MouseButton::Left, {
                let (selection, pieces) = (selection.clone(), pieces.clone());
                let markdown = self.markdown.clone();
                move |event, _, cx| press(&selection, &markdown, &pieces, event, cx)
            })
            .on_action({
                let (selection, pieces) = (selection.clone(), pieces.clone());
                let markdown = self.markdown.clone();
                move |_: &Copy, _, cx| {
                    let range = selection.read(cx).range(&markdown);
                    let text = selected_text(&pieces, range);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
            })
            .children(blocks)
            // A drag goes on outside the message, so it listens to the whole window. The
            // canvas paints last, when every text knows where it lies.
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, (), window, cx| {
                        if selection.read(cx).dragging {
                            follow_drag(selection, pieces, window);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

/// A click puts the caret, a double-click selects a word and a triple-click a line.
fn press(
    selection: &Entity<Selection>,
    markdown: &Markdown,
    pieces: &[Piece],
    event: &MouseDownEvent,
    cx: &mut App,
) {
    let offset = offset_at(pieces, event.position);
    let around = |select: fn(&str, usize) -> Range<usize>| {
        let piece = pieces
            .iter()
            .find(|piece| (piece.start..=piece.end()).contains(&offset))?;
        let range = select(&piece.text, offset - piece.start);
        Some(piece.start + range.start..piece.start + range.end)
    };
    let range = match event.click_count {
        0 | 1 => Some(offset..offset),
        2 => around(words::word_at),
        _ => around(words::line_at),
    }
    .unwrap_or(offset..offset);
    selection.update(cx, |selection, cx| {
        selection.markdown = markdown.clone();
        selection.anchor = range.start;
        selection.head = range.end;
        selection.dragging = event.click_count <= 1;
        cx.notify();
    });
}

fn follow_drag(selection: Entity<Selection>, pieces: Rc<[Piece]>, window: &mut Window) {
    window.on_mouse_event({
        let selection = selection.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // The button came up where the window did not see it.
            if event.pressed_button != Some(MouseButton::Left) {
                selection.update(cx, |selection, cx| {
                    selection.dragging = false;
                    cx.notify();
                });
                return;
            }
            let head = offset_at(&pieces, event.position);
            selection.update(cx, |selection, cx| {
                if selection.head != head {
                    selection.head = head;
                    cx.notify();
                }
            });
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            selection.update(cx, |selection, cx| {
                selection.dragging = false;
                cx.notify();
            });
        }
    });
}

/// The offset in the flat text nearest to a point, in the row of texts it is on or below: one
/// text, or the cells of a table row, which share their top. In a gap below a text it is the
/// end of that text.
fn offset_at(pieces: &[Piece], position: Point<Pixels>) -> usize {
    let bounds = |piece: &Piece| piece.layout.bounds();
    let Some(row_top) = pieces
        .iter()
        .map(|piece| bounds(piece).top())
        .filter(|top| *top <= position.y)
        .reduce(|highest, top| if top > highest { top } else { highest })
    else {
        return 0;
    };
    let mut row = pieces.iter().filter(|piece| bounds(piece).top() == row_top);
    let piece = row
        .clone()
        .rfind(|piece| bounds(piece).left() <= position.x)
        .or_else(|| row.next());
    piece.map_or(0, |piece| {
        let (Ok(index) | Err(index)) = piece.layout.index_for_position(position);
        piece.start + index.min(piece.text.len())
    })
}

/// The selected text as it reads, each text on a line of its own.
fn selected_text(pieces: &[Piece], range: Range<usize>) -> String {
    pieces
        .iter()
        .filter_map(|piece| {
            let local = local(piece.start, &piece.text, &range);
            (!local.is_empty()).then(|| &piece.text[local])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The part of `range` inside a text that starts at `start`, in bytes of that text, its ends on
/// whole characters.
fn local(start: usize, text: &str, range: &Range<usize>) -> Range<usize> {
    let clamp = |offset: usize| text.floor_char_boundary(offset.saturating_sub(start));
    let local_start = clamp(range.start);
    local_start..clamp(range.end).max(local_start)
}

/// `range` cut where the selection starts and ends, each part with whether it is selected.
fn cut(range: Range<usize>, selected: &Range<usize>) -> impl Iterator<Item = (Range<usize>, bool)> {
    let start = selected.start.clamp(range.start, range.end);
    let end = selected.end.clamp(start, range.end);
    [
        (range.start..start, false),
        (start..end, true),
        (end..range.end, false),
    ]
    .into_iter()
    .filter(|(part, _)| !part.is_empty())
}

struct Renderer {
    theme: Theme,
    /// Code blocks scroll and links take clicks, and both need an id of their own.
    next_id: usize,
    selection: Entity<Selection>,
    /// In the flat text, empty when nothing shows.
    selected: Range<usize>,
    pieces: Vec<Piece>,
    /// Of the flat text so far.
    length: usize,
}

impl Renderer {
    /// A text of the message, its segments styled and the selection drawn over them. The
    /// segments cover the text in order.
    fn text(
        &mut self,
        text: &SharedString,
        segments: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> StyledText {
        let start = self.length;
        self.length += text.len() + 1;
        let selected = local(start, text, &self.selected);
        let fill = self.theme.blue.opacity(0.25);
        let highlights: Vec<_> = segments
            .into_iter()
            .flat_map(|(range, style)| {
                cut(range, &selected).map(move |(part, is_selected)| {
                    let style = HighlightStyle {
                        background_color: if is_selected {
                            Some(fill)
                        } else {
                            style.background_color
                        },
                        ..style
                    };
                    (part, style)
                })
            })
            .filter(|(_, style)| *style != HighlightStyle::default())
            .collect();
        let styled = StyledText::new(text.clone()).with_highlights(highlights);
        self.pieces.push(Piece {
            start,
            text: text.clone(),
            layout: styled.layout().clone(),
        });
        styled
    }

    fn id(&mut self, name: &'static str) -> ElementId {
        self.next_id += 1;
        (name, self.next_id).into()
    }

    fn blocks(&mut self, blocks: &[Block]) -> Vec<AnyElement> {
        blocks.iter().map(|block| self.block(block)).collect()
    }

    fn block(&mut self, block: &Block) -> AnyElement {
        match block {
            Block::Paragraph(text) => self.inline(text),
            Block::Heading { size, text } => {
                let (size, line_height) = match size {
                    HeadingSize::Large => (18., 26.),
                    HeadingSize::Small => (15., 22.),
                };
                div()
                    .text_size(px(size))
                    .line_height(px(line_height))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.inline(text))
                    .into_any_element()
            }
            Block::List {
                first_number,
                items,
            } => self.list(*first_number, items),
            Block::Quote(blocks) => div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .pl(px(12.))
                .border_l(px(2.))
                .border_color(self.theme.alpha_at(0.15))
                .text_color(self.theme.gray_800)
                .children(self.blocks(blocks))
                .into_any_element(),
            // A long line scrolls sideways rather than wraps. Up and down still scroll the
            // thread around it.
            Block::Code(code) => div()
                .id(self.id("code"))
                .flex()
                .overflow_x_scroll()
                .restrict_scroll_to_axis()
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(self.theme.alpha_at(0.05))
                .font_family(MONO)
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(self.theme.gray_900)
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .child(self.text(code, [(0..code.len(), HighlightStyle::default())])),
                )
                .into_any_element(),
            Block::Table(table) => self.table(table),
            Block::Rule => div()
                .h(px(1.))
                .my(px(4.))
                .bg(self.theme.alpha_at(0.10))
                .into_any_element(),
        }
    }

    fn list(&mut self, first_number: Option<u64>, items: &[Vec<Block>]) -> AnyElement {
        // The marker hangs to the left of the item, so wrapped lines align. Its column is as
        // wide as the widest number, so "100." fits too.
        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
        let digits = first_number
            .map(|first| first.saturating_add(count.saturating_sub(1)))
            .and_then(|last| last.checked_ilog10())
            .unwrap_or(0)
            + 1;
        let marker_width = 24. + 12. * digits.saturating_sub(2) as f32;
        let muted = self.theme.gray_700;
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(items.iter().zip(0u64..).map(|(item, index)| {
                let marker: SharedString = match first_number {
                    Some(first) => format!("{}.", first.saturating_add(index)).into(),
                    None => "•".into(),
                };
                div()
                    .flex()
                    .child(
                        div()
                            .flex_none()
                            .w(px(marker_width))
                            .text_color(muted)
                            .child(marker),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .children(self.blocks(item)),
                    )
            }))
            .into_any_element()
    }

    /// Each column is as wide as its widest cell. When the table is wider than the text, the
    /// wide columns share what is left and their cells wrap.
    fn table(&mut self, table: &Table) -> AnyElement {
        let columns = table.alignments.len();
        let (head_fill, rule) = (self.theme.alpha_at(0.04), self.theme.alpha_at(0.10));
        let mut cells = Vec::new();
        for (row, contents) in table.rows.iter().enumerate() {
            for (column, (cell, alignment)) in contents.iter().zip(&table.alignments).enumerate() {
                let cell = div()
                    .px(px(10.))
                    .py(px(6.))
                    .overflow_hidden()
                    .when(row == 0, |cell| {
                        cell.bg(head_fill)
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(column == 0, |cell| cell.rounded_tl(px(7.)))
                            .when(column + 1 == columns, |cell| cell.rounded_tr(px(7.)))
                    })
                    .when(row > 0, |cell| cell.border_t_1().border_color(rule))
                    // Aligned by layout, not by text: gpui finds the letter under the pointer
                    // as if aligned text were on the left. The text still wraps in its box.
                    .map(|cell| match alignment {
                        Alignment::Center => cell.flex().justify_center(),
                        Alignment::Right => cell.flex().justify_end(),
                        Alignment::Left | Alignment::None => cell,
                    })
                    .children(
                        cell.as_ref()
                            .map(|text| div().min_w_0().child(self.inline(text))),
                    );
                cells.push(cell.into_any_element());
            }
        }
        div()
            .self_start()
            .grid()
            .grid_cols_max_content(u16::try_from(columns).unwrap_or(u16::MAX))
            .border_1()
            .border_color(rule)
            .rounded(px(8.))
            .text_size(px(13.))
            .line_height(px(20.))
            .children(cells)
            .into_any_element()
    }

    fn inline(&mut self, inline: &Inline) -> AnyElement {
        let (code_fill, underline) = (self.theme.alpha_at(0.08), self.theme.gray_600);
        let segments = inline.spans.iter().map(|span| {
            let style = &span.style;
            let highlight = HighlightStyle {
                font_weight: style.bold.then_some(FontWeight::SEMIBOLD),
                font_style: style.italic.then_some(FontStyle::Italic),
                background_color: style.code.then_some(code_fill),
                underline: style.link.as_ref().map(|_| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(underline),
                    wavy: false,
                }),
                ..HighlightStyle::default()
            };
            (span.range.clone(), highlight)
        });
        let monospace = inline
            .spans
            .iter()
            .filter(|span| span.style.code)
            .map(|span| (span.range.clone(), SharedString::new_static(MONO)));
        let text = self
            .text(&inline.text, segments)
            .with_font_family_overrides(monospace);

        let (ranges, urls): (Vec<_>, Vec<_>) = inline
            .spans
            .iter()
            .filter_map(|span| Some((span.range.clone(), span.style.link.clone()?)))
            .unzip();
        if ranges.is_empty() {
            return text.into_any_element();
        }
        let selection = self.selection.clone();
        InteractiveText::new(self.id("links"), text)
            .on_click(ranges, move |index, _window, cx| {
                // A drag that ends on the link it started on selects its text.
                let selection = selection.read(cx);
                if selection.anchor != selection.head {
                    return;
                }
                if let Some(url) = urls.get(index) {
                    cx.open_url(url);
                }
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> SpanStyle {
        SpanStyle::default()
    }

    fn bold() -> SpanStyle {
        SpanStyle {
            bold: true,
            ..SpanStyle::default()
        }
    }

    fn italic() -> SpanStyle {
        SpanStyle {
            italic: true,
            ..SpanStyle::default()
        }
    }

    fn code() -> SpanStyle {
        SpanStyle {
            code: true,
            ..SpanStyle::default()
        }
    }

    fn link(url: &'static str) -> SpanStyle {
        SpanStyle {
            link: Some(url.into()),
            ..SpanStyle::default()
        }
    }

    /// The spans of a text as `(text, style)` pairs.
    fn spans(inline: &Inline) -> Vec<(&str, SpanStyle)> {
        inline
            .spans
            .iter()
            .map(|span| (&inline.text[span.range.clone()], span.style.clone()))
            .collect()
    }

    fn paragraph(block: &Block) -> &Inline {
        match block {
            Block::Paragraph(inline) => inline,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    fn only_paragraph(source: &str) -> Inline {
        let markdown = Markdown::parse(source);
        match markdown.blocks() {
            [Block::Paragraph(inline)] => inline.clone(),
            other => panic!("not one paragraph: {other:?}"),
        }
    }

    /// Every text in the blocks, nested ones too.
    fn inlines(blocks: &[Block]) -> Vec<&Inline> {
        blocks
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph(text) | Block::Heading { text, .. } => vec![text],
                Block::List { items, .. } => items.iter().flat_map(|item| inlines(item)).collect(),
                Block::Quote(blocks) => inlines(blocks),
                Block::Table(table) => table.rows.iter().flatten().flatten().collect(),
                Block::Code(_) | Block::Rule => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn inline_styles_become_spans_that_cover_the_text() {
        let inline = only_paragraph(
            "Set **Gain** to *half*, see `gain.rs` or [the docs](https://example.com/docs).",
        );
        assert_eq!(
            inline.text.as_ref(),
            "Set Gain to half, see gain.rs or the docs."
        );
        assert_eq!(
            spans(&inline),
            [
                ("Set ", plain()),
                ("Gain", bold()),
                (" to ", plain()),
                ("half", italic()),
                (", see ", plain()),
                ("gain.rs", code()),
                (" or ", plain()),
                ("the docs", link("https://example.com/docs")),
                (".", plain()),
            ]
        );
    }

    #[test]
    fn nested_styles_combine() {
        let inline = only_paragraph("***both*** and [**bold link**](https://a.b)");
        let both = SpanStyle {
            bold: true,
            italic: true,
            ..SpanStyle::default()
        };
        let bold_link = SpanStyle {
            bold: true,
            ..link("https://a.b")
        };
        assert_eq!(
            spans(&inline),
            [("both", both), (" and ", plain()), ("bold link", bold_link)]
        );
    }

    #[test]
    fn a_link_inside_bold_and_code_inside_a_link() {
        let inline = only_paragraph("**see [docs](https://a.b)** or [`gain`](https://c.d)");
        let bold_link = SpanStyle {
            bold: true,
            ..link("https://a.b")
        };
        let code_link = SpanStyle {
            code: true,
            ..link("https://c.d")
        };
        assert_eq!(
            spans(&inline),
            [
                ("see ", bold()),
                ("docs", bold_link),
                (" or ", plain()),
                ("gain", code_link),
            ]
        );
    }

    #[test]
    fn an_email_autolink_opens_mail() {
        let inline = only_paragraph("Write to <someone@example.com>.");
        assert_eq!(
            spans(&inline),
            [
                ("Write to ", plain()),
                ("someone@example.com", link("mailto:someone@example.com")),
                (".", plain()),
            ]
        );
    }

    /// A link in an answer is written by the agent, and a `file:` or app URL could start
    /// something on the machine.
    #[test]
    fn only_web_and_mail_links_open() {
        let inline = only_paragraph(
            "[a](https://a.b) [b](http://a.b) [c](mailto:a@b.c) [d](file:///etc/passwd) [e](gain.rs) <x-apple.systempreferences:x>",
        );
        let links: Vec<_> = inline
            .spans
            .iter()
            .filter_map(|span| span.style.link.as_deref())
            .collect();
        assert_eq!(links, ["https://a.b", "http://a.b", "mailto:a@b.c"]);
    }

    #[test]
    fn line_breaks_inside_a_paragraph() {
        let inline = only_paragraph("one\ntwo  \nthree");
        assert_eq!(inline.text.as_ref(), "one two\nthree");
        assert_eq!(spans(&inline), [("one two\nthree", plain())]);
    }

    #[test]
    fn headings_fold_to_two_sizes() {
        let markdown = Markdown::parse("# One\n## Two\n### Three\n###### Six");
        let sizes: Vec<_> = markdown
            .blocks()
            .iter()
            .map(|block| match block {
                Block::Heading { size, text } => (*size, text.text.as_ref()),
                other => panic!("not a heading: {other:?}"),
            })
            .collect();
        assert_eq!(
            sizes,
            [
                (HeadingSize::Large, "One"),
                (HeadingSize::Large, "Two"),
                (HeadingSize::Small, "Three"),
                (HeadingSize::Small, "Six"),
            ]
        );
    }

    #[test]
    fn lists_keep_their_numbers_and_nesting() {
        let markdown = Markdown::parse("3. first\n4. second\n   - inner\n   - more\n\n- bullet");
        let [
            Block::List {
                first_number: Some(3),
                items: numbered,
            },
            Block::List {
                first_number: None,
                items: bullets,
            },
        ] = markdown.blocks()
        else {
            panic!("not two lists: {:?}", markdown.blocks());
        };
        assert_eq!(numbered.len(), 2);
        assert_eq!(paragraph(&numbered[0][0]).text.as_ref(), "first");
        assert_eq!(paragraph(&numbered[1][0]).text.as_ref(), "second");
        let [
            _,
            Block::List {
                first_number: None,
                items: inner,
            },
        ] = numbered[1].as_slice()
        else {
            panic!("no nested list: {:?}", numbered[1]);
        };
        assert_eq!(inner.len(), 2);
        assert_eq!(paragraph(&inner[1][0]).text.as_ref(), "more");
        assert_eq!(paragraph(&bullets[0][0]).text.as_ref(), "bullet");
    }

    #[test]
    fn a_loose_item_holds_several_blocks() {
        let markdown = Markdown::parse("- one\n\n  two\n\n  ```\n  code\n  ```\n");
        let [Block::List { items, .. }] = markdown.blocks() else {
            panic!("not a list: {:?}", markdown.blocks());
        };
        assert_eq!(
            items[0],
            [
                Block::Paragraph(only_paragraph("one")),
                Block::Paragraph(only_paragraph("two")),
                Block::Code("code".into()),
            ]
        );
    }

    #[test]
    fn quotes_hold_blocks() {
        let markdown = Markdown::parse("> said\n>\n> - a\n\nafter");
        let [Block::Quote(inside), Block::Paragraph(after)] = markdown.blocks() else {
            panic!("not a quote and a paragraph: {:?}", markdown.blocks());
        };
        assert_eq!(paragraph(&inside[0]).text.as_ref(), "said");
        assert!(matches!(inside[1], Block::List { .. }));
        assert_eq!(after.text.as_ref(), "after");
    }

    #[test]
    fn a_list_item_holds_a_quote() {
        let markdown = Markdown::parse("- item\n\n  > quoted\n");
        let [Block::List { items, .. }] = markdown.blocks() else {
            panic!("not a list: {:?}", markdown.blocks());
        };
        assert_eq!(
            items[0],
            [
                Block::Paragraph(only_paragraph("item")),
                Block::Quote(vec![Block::Paragraph(only_paragraph("quoted"))]),
            ]
        );
    }

    #[test]
    fn code_blocks_keep_their_text() {
        let markdown = Markdown::parse(
            "```rust title\nfn main() {\n    **not bold**\n}\n```\n\n    indented\n",
        );
        assert_eq!(
            markdown.blocks(),
            [
                Block::Code("fn main() {\n    **not bold**\n}".into()),
                Block::Code("indented".into()),
            ]
        );
    }

    #[test]
    fn a_table_keeps_its_cells_and_alignment() {
        let source = "| Track | Gain |\n| --- | ---: |\n| **Bass** | -3 dB |\n| Lead |\n\nafter";
        let markdown = Markdown::parse(source);
        let [Block::Table(table), Block::Paragraph(after)] = markdown.blocks() else {
            panic!("not a table and a paragraph: {:?}", markdown.blocks());
        };
        assert_eq!(table.alignments, [Alignment::None, Alignment::Right]);
        let rows: Vec<Vec<Option<&str>>> = table
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.as_ref().map(|cell| cell.text.as_ref()))
                    .collect()
            })
            .collect();
        assert_eq!(
            rows,
            [
                [Some("Track"), Some("Gain")],
                [Some("Bass"), Some("-3 dB")],
                [Some("Lead"), None],
            ]
        );
        assert_eq!(
            table.rows[1][0].as_ref().map(spans),
            Some(vec![("Bass", bold())])
        );
        assert_eq!(after.text.as_ref(), "after");
    }

    #[test]
    fn a_table_in_a_quote() {
        let markdown = Markdown::parse("> | a | b |\n> | - | - |\n> | 1 | 2 |\n");
        let [Block::Quote(inside)] = markdown.blocks() else {
            panic!("not a quote: {:?}", markdown.blocks());
        };
        assert!(matches!(inside.as_slice(), [Block::Table(table)] if table.rows.len() == 2));
    }

    #[test]
    fn rules_images_and_html() {
        let markdown = Markdown::parse(
            "before\n\n---\n\n![a waveform](wave.png)\n\n<div>\nraw\n</div>\n\na <b>b</b>",
        );
        assert_eq!(
            markdown.blocks(),
            [
                Block::Paragraph(only_paragraph("before")),
                Block::Rule,
                Block::Paragraph(only_paragraph("a waveform")),
                Block::Paragraph(Inline {
                    text: "<div>\nraw\n</div>".into(),
                    spans: vec![Span {
                        range: 0..16,
                        style: plain()
                    }],
                }),
                Block::Paragraph(Inline {
                    text: "a <b>b</b>".into(),
                    spans: vec![Span {
                        range: 0..10,
                        style: plain()
                    }],
                }),
            ]
        );
    }

    #[test]
    fn empty_input_and_empty_blocks_show_nothing() {
        assert!(Markdown::parse("").blocks().is_empty());
        assert!(Markdown::parse("\n\n  \n").blocks().is_empty());
        assert!(Markdown::parse("#").blocks().is_empty());
    }

    /// While an answer streams, the message ends anywhere.
    #[test]
    fn an_unclosed_code_fence_holds_the_rest_of_the_message() {
        let markdown = Markdown::parse("Run this:\n\n```sh\ncargo build\ncargo te");
        assert_eq!(
            markdown.blocks(),
            [
                Block::Paragraph(only_paragraph("Run this:")),
                Block::Code("cargo build\ncargo te".into()),
            ]
        );
    }

    #[test]
    fn half_written_styles_show_as_written_until_they_close() {
        let inline = only_paragraph("Set **Gai");
        assert_eq!(spans(&inline), [("Set **Gai", plain())]);
        let inline = only_paragraph("see [the do");
        assert_eq!(spans(&inline), [("see [the do", plain())]);
        let inline = only_paragraph("see [the docs](https://exa");
        assert_eq!(inline.text.as_ref(), "see [the docs](https://exa");
        let inline = only_paragraph("the `gai");
        assert_eq!(spans(&inline), [("the `gai", plain())]);
    }

    /// gpui panics on a highlight that splits a character, so the spans must cover each text
    /// in order and start and end on character boundaries, wherever the message is cut.
    #[test]
    fn every_prefix_of_an_answer_has_spans_on_character_boundaries() {
        let answer = sample_answer();
        for (end, _) in answer.char_indices() {
            let markdown = Markdown::parse(&answer[..end]);
            for inline in inlines(markdown.blocks()) {
                let mut covered = 0;
                for span in &inline.spans {
                    assert_eq!(span.range.start, covered, "{inline:?}");
                    assert!(inline.text.is_char_boundary(span.range.start), "{inline:?}");
                    assert!(inline.text.is_char_boundary(span.range.end), "{inline:?}");
                    covered = span.range.end;
                }
                assert_eq!(covered, inline.text.len(), "{inline:?}");
            }
        }
    }

    struct Message(Markdown);

    impl Render for Message {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div()
                .w(px(400.))
                .debug_selector(|| "message".into())
                .child(MarkdownText::new("message", self.0.clone()))
        }
    }

    /// A drag from the top left to the bottom right selects the whole message, each text on a
    /// line of its own, and a double-click a word. When the answer streams on, its offsets mean
    /// other text, and the selection goes away.
    #[gpui::test]
    fn a_drag_selects_and_cmd_c_copies(cx: &mut gpui::TestAppContext) {
        use gpui::{KeyBinding, Modifiers, point};
        cx.update(|cx| {
            sound_ui::init(cx);
            cx.bind_keys([KeyBinding::new("cmd-c", Copy, Some(KEY_CONTEXT))]);
        });
        let source = "First **paragraph**.\n\n- Größe one\n\n```\ncode line\n```";
        let (message, cx) = cx.add_window_view(|_, _| Message(Markdown::parse(source)));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("message").expect("the message is drawn");
        let copied = |cx: &mut gpui::VisualTestContext| {
            cx.simulate_keystrokes("cmd-c");
            cx.read_from_clipboard().and_then(|item| item.text())
        };

        let (first, last) = (
            bounds.origin + point(px(1.), px(1.)),
            bounds.bottom_right() - point(px(1.), px(1.)),
        );
        cx.simulate_mouse_down(first, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(last, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(last, MouseButton::Left, Modifiers::none());
        assert_eq!(
            copied(cx).as_deref(),
            Some("First paragraph.\nGröße one\ncode line")
        );

        cx.simulate_event(MouseDownEvent {
            position: first,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_mouse_up(first, MouseButton::Left, Modifiers::none());
        assert_eq!(copied(cx).as_deref(), Some("First"));

        message.update(cx, |message, cx| {
            message.0 = Markdown::parse("**First** paragraph");
            cx.notify();
        });
        cx.write_to_clipboard(ClipboardItem::new_string("untouched".into()));
        assert_eq!(copied(cx).as_deref(), Some("untouched"));
    }

    #[test]
    fn a_line_of_the_app_styles_only_its_code() {
        let line = |text| match Markdown::inline_code(text).blocks() {
            [Block::Paragraph(inline)] => inline.clone(),
            other => panic!("not one paragraph: {other:?}"),
        };
        let run = line("Run `find state -name '*.json'` in **bold**");
        assert_eq!(
            spans(&run),
            [
                ("Run ", plain()),
                ("find state -name '*.json'", code()),
                (" in **bold**", plain()),
            ]
        );
        assert_eq!(spans(&line("Edited a`b")), [("Edited a`b", plain())]);
        assert_eq!(Markdown::inline_code("").blocks(), []);
    }

    /// Risk R7 of the agent sidebar plan: the sidebar parses the whole streaming message again
    /// on each frame. Measured on an M-series Mac: 5.1 KB in 58 µs in release and 400 µs in a
    /// debug build, against a 16 ms frame. Ignored so a slow CI runner cannot fail it; run
    /// `cargo test --release -p sound-ui long_answer -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_long_answer_parses_in_a_small_part_of_a_frame() {
        let mut answer = String::new();
        while answer.len() < 5_000 {
            answer.push_str(&sample_answer());
        }
        let runs = 200;
        let start = std::time::Instant::now();
        for _ in 0..runs {
            std::hint::black_box(Markdown::parse(std::hint::black_box(&answer)));
        }
        let each = start.elapsed() / runs;
        println!("{} bytes parse in {each:?}", answer.len());
        assert!(each < std::time::Duration::from_millis(4), "{each:?}");
    }

    fn sample_answer() -> String {
        [
            "## What changed\n\n",
            "The bass now has its own **filter** with *slow* attack, set in `bass/state.json`. ",
            "See [the guide](https://example.com/guide) or <help@example.com>.\n\n",
            "Größe 🎛️ **lauter** `ü` and [**ä** `ö`](https://example.com/ü).\n\n",
            "1. Opened the track\n2. Added a filter\n   - cutoff 800 Hz\n   - resonance 0.3\n\n",
            "> The kick still masks the bass below 60 Hz.\n\n",
            "```json\n{ \"cutoff\": 800, \"resonance\": 0.3 }\n```\n\n",
            "| Track | Gain |\n| --- | --- |\n| Bass | -3 dB |\n\n---\n\n",
        ]
        .concat()
    }
}
