//! Markdown: the agent's answers. `Markdown::parse` turns the source into a small block model,
//! and `MarkdownText` draws it. An answer arrives streaming, so the caller parses the whole
//! message again on each batch; a 5 KB answer parses in well under a millisecond.
//!
//! No syntax colours and no selection. Tables and HTML are shown as their source, images as
//! their alt text.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Div, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    SharedString, StyleRefinement, StyledText, UnderlineStyle, Window, div, prelude::*, px,
};
use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use crate::theme::{ActiveTheme, Theme};
use crate::typography::MONO;

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
    /// Also a table, as its source text.
    Code(SharedString),
    Rule,
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
        let mut in_table = false;
        for (event, range) in Parser::new_ext(source, Options::ENABLE_TABLES).into_offset_iter() {
            match event {
                Event::Start(Tag::Table(_)) => {
                    in_table = true;
                    builder.table(source.get(range).unwrap_or_default());
                }
                Event::End(TagEnd::Table) => in_table = false,
                _ if in_table => {}
                event => builder.event(event),
            }
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

    /// A table becomes its source in monospace, which lines up without a grid. Inside a quote
    /// or a list item, every line after the first still has the quote marks and the indent.
    fn table(&mut self, source: &str) {
        let quotes = self
            .open
            .iter()
            .filter(|container| matches!(container, Container::Quote(_)))
            .count();
        let lines: Vec<&str> = source
            .trim_end()
            .lines()
            .map(|line| {
                let mut line = line.trim_start();
                for _ in 0..quotes {
                    line = line.strip_prefix('>').unwrap_or(line).trim_start();
                }
                line
            })
            .collect();
        self.push_block(Block::Code(lines.join("\n").into()));
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

impl RenderOnce for MarkdownText {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut renderer = Renderer {
            theme: cx.theme().clone(),
            next_id: 0,
        };
        self.base
            .id(self.id)
            .flex()
            .flex_col()
            .gap(px(12.))
            .min_w_0()
            .children(renderer.blocks(self.markdown.blocks()))
    }
}

struct Renderer {
    theme: Theme,
    /// Code blocks scroll and links take clicks, and both need an id of their own.
    next_id: usize,
}

impl Renderer {
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
            // A long line scrolls sideways rather than wraps, so a table keeps its columns. Up
            // and down still scroll the thread around it.
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
                .child(div().flex_none().whitespace_nowrap().child(code.clone()))
                .into_any_element(),
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

    fn inline(&mut self, inline: &Inline) -> AnyElement {
        let (code_fill, underline) = (self.theme.alpha_at(0.08), self.theme.gray_600);
        let highlights = inline.spans.iter().filter_map(|span| {
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
            (highlight != HighlightStyle::default()).then(|| (span.range.clone(), highlight))
        });
        let monospace = inline
            .spans
            .iter()
            .filter(|span| span.style.code)
            .map(|span| (span.range.clone(), SharedString::new_static(MONO)));
        let text = StyledText::new(inline.text.clone())
            .with_highlights(highlights)
            .with_font_family_overrides(monospace);

        let (ranges, urls): (Vec<_>, Vec<_>) = inline
            .spans
            .iter()
            .filter_map(|span| Some((span.range.clone(), span.style.link.clone()?)))
            .unzip();
        if ranges.is_empty() {
            return text.into_any_element();
        }
        InteractiveText::new(self.id("links"), text)
            .on_click(ranges, move |index, _window, cx| {
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
    fn a_table_is_its_source_in_a_code_block() {
        let source = "| Track | Gain |\n| --- | --- |\n| Bass | -3 dB |\n\nafter";
        let markdown = Markdown::parse(source);
        assert_eq!(
            markdown.blocks(),
            [
                Block::Code("| Track | Gain |\n| --- | --- |\n| Bass | -3 dB |".into()),
                Block::Paragraph(only_paragraph("after")),
            ]
        );
    }

    #[test]
    fn a_table_in_a_quote_loses_the_quote_marks() {
        let markdown = Markdown::parse("> | a | b |\n> | - | - |\n> | 1 | 2 |\n");
        assert_eq!(
            markdown.blocks(),
            [Block::Quote(vec![Block::Code(
                "| a | b |\n| - | - |\n| 1 | 2 |".into()
            )])]
        );
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
