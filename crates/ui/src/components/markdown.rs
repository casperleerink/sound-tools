//! Markdown: the agent's answers. `Markdown::parse` turns the source into a small block model,
//! and `MarkdownText` draws it. An answer arrives streaming, so the caller parses the whole
//! message again on each batch; a 5 KB answer parses in well under a millisecond.
//!
//! No syntax colours and no selection. Tables and HTML are shown as their source, images as
//! their alt text.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Div, ElementId, FontStyle, FontWeight, HighlightStyle, Hsla, InteractiveText,
    SharedString, StyleRefinement, StyledText, UnderlineStyle, Window, div, prelude::*, px,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::theme::ActiveTheme;
use crate::typography::MONO;

/// A parsed message. Cheap to clone, so a view keeps it and hands it to every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Markdown {
    blocks: Arc<[Block]>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
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
    Code {
        /// Kept for syntax colours later; nothing reads it yet.
        language: Option<SharedString>,
        code: SharedString,
    },
    Rule,
}

/// Six heading levels are too many for a sidebar: `#` and `##` are large, the rest small.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingSize {
    Large,
    Small,
}

/// The text of a paragraph or heading, with its styles. The spans cover the text in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Inline {
    pub text: SharedString,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// Bytes of `Inline::text`.
    pub range: Range<usize>,
    pub style: SpanStyle,
}

/// All false and no link is plain text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<SharedString>,
}

impl Markdown {
    /// Never fails: markdown has no syntax errors, and unfinished markdown, such as an open code
    /// fence while the answer streams, reads as far as it goes.
    pub fn parse(source: &str) -> Self {
        let mut builder = Builder::default();
        let mut in_table = false;
        for (event, range) in Parser::new_ext(source, Options::ENABLE_TABLES).into_offset_iter() {
            match event {
                // A table becomes its source in monospace, which lines up without a grid.
                Event::Start(Tag::Table(_)) => {
                    in_table = true;
                    let table = source.get(range).unwrap_or_default().trim_end();
                    builder.push_block(Block::Code {
                        language: None,
                        code: table.to_string().into(),
                    });
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

    pub fn blocks(&self) -> &[Block] {
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
    code: Option<(Option<SharedString>, String)>,
    bold: usize,
    italic: usize,
    links: Vec<SharedString>,
}

impl Builder {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some((_, code)) => code.push_str(&text),
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
            Tag::Link { dest_url, .. } => self.links.push(dest_url.to_string().into()),
            // The alt text follows as text.
            Tag::Image { .. } => {}
            Tag::BlockQuote(_) => self.open(Container::Quote(Vec::new())),
            Tag::List(first_number) => self.open(Container::List {
                first_number,
                items: Vec::new(),
            }),
            Tag::Item => self.open(Container::Item(Vec::new())),
            Tag::CodeBlock(kind) => {
                self.end_paragraph();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(|language| SharedString::from(language.to_string())),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((language, String::new()));
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
                if let Some((language, mut code)) = self.code.take() {
                    code.truncate(code.trim_end_matches('\n').len());
                    self.push_block(Block::Code {
                        language,
                        code: code.into(),
                    });
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
            link: self.links.last().cloned(),
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
        let blocks = match self.open.last_mut() {
            Some(Container::Quote(blocks) | Container::Item(blocks)) => blocks,
            // Between the start of a list and its first item nothing else comes.
            Some(Container::List { items, .. }) => {
                if items.is_empty() {
                    items.push(Vec::new());
                }
                match items.last_mut() {
                    Some(item) => item,
                    None => return,
                }
            }
            None => &mut self.document,
        };
        blocks.push(block);
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

/// Draws a parsed message. Text is 15 on 22 unless the caller sets another size, and takes the
/// colour of its parent. It wraps to the width it is given.
#[derive(IntoElement)]
pub struct MarkdownText {
    base: Div,
    id: ElementId,
    markdown: Markdown,
}

impl MarkdownText {
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
        let theme = cx.theme();
        let mut renderer = Renderer {
            colors: Colors {
                muted: theme.gray_700,
                quote: theme.gray_800,
                code: theme.gray_900,
                code_fill: theme.alpha_at(0.05),
                inline_code_fill: theme.alpha_at(0.08),
                rule: theme.alpha_at(0.10),
                quote_border: theme.alpha_at(0.15),
                underline: theme.gray_600,
            },
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

#[derive(Clone, Copy)]
struct Colors {
    muted: Hsla,
    quote: Hsla,
    code: Hsla,
    code_fill: Hsla,
    inline_code_fill: Hsla,
    rule: Hsla,
    quote_border: Hsla,
    underline: Hsla,
}

struct Renderer {
    colors: Colors,
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
        let colors = self.colors;
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
            } => div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(items.iter().zip(0u64..).map(|(item, index)| {
                    let marker: SharedString = match first_number {
                        Some(first) => format!("{}.", first.saturating_add(index)).into(),
                        None => "•".into(),
                    };
                    // The marker hangs to the left of the item, so wrapped lines align.
                    div()
                        .flex()
                        .child(
                            div()
                                .flex_none()
                                .w(px(24.))
                                .text_color(colors.muted)
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
                .into_any_element(),
            Block::Quote(blocks) => div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .pl(px(12.))
                .border_l(px(2.))
                .border_color(colors.quote_border)
                .text_color(colors.quote)
                .children(self.blocks(blocks))
                .into_any_element(),
            // A long line scrolls sideways rather than wraps, so a table keeps its columns.
            Block::Code { code, .. } => div()
                .id(self.id("code"))
                .flex()
                .overflow_x_scroll()
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(colors.code_fill)
                .font_family(MONO)
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(colors.code)
                .child(div().flex_none().whitespace_nowrap().child(code.clone()))
                .into_any_element(),
            Block::Rule => div()
                .h(px(1.))
                .my(px(4.))
                .bg(colors.rule)
                .into_any_element(),
        }
    }

    fn inline(&mut self, inline: &Inline) -> AnyElement {
        let colors = self.colors;
        let highlights = inline.spans.iter().filter_map(|span| {
            let style = &span.style;
            let highlight =
                HighlightStyle {
                    font_weight: style.bold.then_some(FontWeight::SEMIBOLD),
                    font_style: style.italic.then_some(FontStyle::Italic),
                    background_color: style.code.then_some(colors.inline_code_fill),
                    underline: style.link.as_ref().filter(|url| opens(url)).map(|_| {
                        UnderlineStyle {
                            thickness: px(1.),
                            color: Some(colors.underline),
                            wavy: false,
                        }
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
            .filter_map(|span| {
                let url = span.style.link.as_ref().filter(|url| opens(url))?;
                Some((span.range.clone(), url.clone()))
            })
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

/// Only web and mail links open. A link in an answer is written by the agent, and a `file:`
/// or app URL could start something on the machine.
fn opens(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
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
                Block::Code {
                    language: None,
                    code: "code".into()
                },
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
    fn code_blocks_keep_their_text_and_language() {
        let markdown = Markdown::parse(
            "```rust title\nfn main() {\n    **not bold**\n}\n```\n\n    indented\n",
        );
        assert_eq!(
            markdown.blocks(),
            [
                Block::Code {
                    language: Some("rust".into()),
                    code: "fn main() {\n    **not bold**\n}".into(),
                },
                Block::Code {
                    language: None,
                    code: "indented".into(),
                },
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
                Block::Code {
                    language: None,
                    code: "| Track | Gain |\n| --- | --- |\n| Bass | -3 dB |".into(),
                },
                Block::Paragraph(only_paragraph("after")),
            ]
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
                Block::Code {
                    language: Some("sh".into()),
                    code: "cargo build\ncargo te".into(),
                },
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

    #[test]
    fn every_prefix_of_an_answer_parses() {
        let answer = sample_answer();
        for (end, _) in answer.char_indices() {
            Markdown::parse(&answer[..end]);
        }
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(opens("https://example.com"));
        assert!(opens("mailto:someone@example.com"));
        assert!(!opens("file:///etc/passwd"));
        assert!(!opens("x-apple.systempreferences:"));
        assert!(!opens("gain.rs"));
    }

    /// Risk R7 of the agent sidebar plan: the sidebar parses the whole streaming message again
    /// on each frame. `cargo test --release -p sound-ui long_answer -- --nocapture` prints the
    /// time in a release build.
    #[test]
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
        // A frame is 16 ms. Even an unoptimized build stays far under it.
        assert!(each < std::time::Duration::from_millis(4), "{each:?}");
    }

    fn sample_answer() -> String {
        [
            "## What changed\n\n",
            "The bass now has its own **filter** with *slow* attack, set in `bass/state.json`. ",
            "See [the guide](https://example.com/guide).\n\n",
            "1. Opened the track\n2. Added a filter\n   - cutoff 800 Hz\n   - resonance 0.3\n\n",
            "> The kick still masks the bass below 60 Hz.\n\n",
            "```json\n{ \"cutoff\": 800, \"resonance\": 0.3 }\n```\n\n",
            "| Track | Gain |\n| --- | --- |\n| Bass | -3 dB |\n\n---\n\n",
        ]
        .concat()
    }
}
