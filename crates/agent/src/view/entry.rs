//! How each entry of the thread looks: the composer's message as a bubble, the agent's turn as
//! markdown with its meta line, and quiet lines.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, HighlightStyle, Hsla,
    SharedString, StyledText, Window, div, ease_out_quint, prelude::*, px, relative,
};
use sound_core::Problem;
use sound_ui::ActiveTheme;
use sound_ui::components::icon::Icon;
use sound_ui::components::indicator::{Indicator, IndicatorSize};

use super::markdown::{Markdown, MarkdownText};
use crate::conversation::Turn;
use crate::{StepKind, StepOutcome, TurnOutcome};

/// The text of the thread: 15 on 22, as the mockup.
const TEXT_SIZE: f32 = 15.;
const LINE_HEIGHT: f32 = 22.;
/// Meta lines and steps.
const SMALL_TEXT_SIZE: f32 = 12.;
const SMALL_LINE_HEIGHT: f32 = 16.;
/// How long a new block of a streaming answer takes to fade in.
const FADE_IN: Duration = Duration::from_millis(600);
/// One sweep of the bright band across the working line.
const SHIMMER: Duration = Duration::from_millis(2200);
/// How wide the band is, in characters.
const SHIMMER_WIDTH: f32 = 8.;

/// The message, and below it the names of what it had attached: a saved thread keeps no
/// pictures.
pub(super) fn message(text: &str, attachments: &[String], cx: &App) -> AnyElement {
    let muted = cx.theme().gray_700;
    let attached = attachments.iter().map(|name| {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(SMALL_TEXT_SIZE))
            .line_height(px(SMALL_LINE_HEIGHT))
            .text_color(muted)
            .child(Icon::new("paperclip").size(12.).flex_none())
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(name.clone())),
            )
    });
    div()
        .flex()
        .justify_end()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .max_w(relative(0.8))
                .p(px(14.))
                .rounded(px(12.))
                .bg(cx.theme().alpha_at(0.05))
                .text_size(px(TEXT_SIZE))
                .line_height(px(LINE_HEIGHT))
                .child(SharedString::from(text.to_string()))
                .children(attached),
        )
        .into_any_element()
}

pub(super) fn notice(text: &str, cx: &App) -> AnyElement {
    div()
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(cx.theme().gray_700)
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// A line the app writes, such as a step or a question, with its commands as code.
pub(super) fn title(id: impl Into<ElementId>, line: &str) -> MarkdownText {
    MarkdownText::new(id, Markdown::inline_code(line))
}

/// The answers of the agent, its blocks parsed once per batch of events.
pub(super) fn answer(turn: &Turn) -> Vec<Markdown> {
    let streaming = (!turn.streaming.is_empty()).then_some(&turn.streaming);
    turn.blocks
        .iter()
        .chain(streaming)
        .map(|block| Markdown::parse(block))
        .collect()
}

/// One turn: the meta line once it ended, the steps behind it, the answer, and while it works
/// the working line. `below` comes last: the question it waits on, or once it ended the
/// problems it left.
pub(super) fn turn(
    turn: &Turn,
    index: usize,
    answer: &[Markdown],
    steps_open: bool,
    on_toggle_steps: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    below: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, bright, dim, red, lavender) = (
        theme.gray_700,
        theme.gray_900,
        theme.gray_600,
        theme.red,
        theme.lavender,
    );
    let meta = turn.end.as_ref().map(|end| {
        let (line, color, summary) = match &end.outcome {
            TurnOutcome::Completed => (
                format!("Worked for {}", duration(end.worked)),
                muted,
                turn.summary(),
            ),
            TurnOutcome::Interrupted => (
                format!("Stopped after {}", duration(end.worked)),
                muted,
                turn.summary(),
            ),
            TurnOutcome::Failed { message } if message.is_empty() => {
                ("The turn failed".to_string(), red, None)
            }
            TurnOutcome::Failed { message } => (message.clone(), red, None),
        };
        let has_steps = !turn.steps.is_empty();
        let chevron = if steps_open {
            "chevron-down"
        } else {
            "chevron-right"
        };
        div()
            .id("worked-for")
            .debug_selector(move || format!("agent-worked-for-{index}"))
            .flex()
            .items_start()
            .gap(px(4.))
            .text_size(px(SMALL_TEXT_SIZE))
            .line_height(px(SMALL_LINE_HEIGHT))
            .text_color(color)
            .when(has_steps, |line| {
                line.cursor_pointer()
                    .hover(move |style| style.text_color(bright))
                    .on_click(on_toggle_steps)
                    .child(Icon::new(chevron).size(12.).mt(px(2.)))
            })
            // A failure has no summary and may wrap; with a summary, the summary is cut.
            .child(
                div()
                    .min_w_0()
                    .when(summary.is_some(), |line| line.flex_none())
                    .child(line),
            )
            .children(summary.map(|summary| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(dim)
                    .child(format!("· {summary}"))
            }))
    });
    let steps = steps_open.then(|| {
        div()
            .flex()
            .flex_col()
            .text_size(px(SMALL_TEXT_SIZE))
            .line_height(px(SMALL_LINE_HEIGHT))
            .text_color(dim)
            .children(turn.steps.iter().map(|step| {
                // A step that did not do what it says says so in words, a failure in red.
                let outcome = match step.outcome {
                    Some(StepOutcome::Failed) => Some(("· failed", red)),
                    Some(StepOutcome::Denied) => Some(("· denied", muted)),
                    Some(StepOutcome::Done) | None => None,
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(20.))
                    .child(Icon::new(icon(&step.kind)).size(12.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(plain(step.finished_title())),
                    )
                    .children(
                        outcome
                            .map(|(word, color)| div().flex_none().text_color(color).child(word)),
                    )
            }))
    });
    let working = turn.end.is_none();
    let text = answer.iter().enumerate().map(|(block, markdown)| {
        let text = MarkdownText::new(("answer", block), markdown.clone());
        // Only while it streams, so a thread that opens shows its answers at once.
        if !working {
            return text.into_any_element();
        }
        div()
            .child(text)
            .with_animation(
                ("answer-fade-in", block),
                Animation::new(FADE_IN).with_easing(ease_out_quint()),
                |block, delta| block.opacity(delta),
            )
            .into_any_element()
    });
    // While a question waits, the question is what the agent does.
    let working = (working && turn.approvals.is_empty()).then(|| {
        let step = turn.current_step();
        let line = step.map_or("Working", |step| match step.outcome {
            None => step.running_title.as_str(),
            Some(_) => step.finished_title(),
        });
        let mark = match step {
            Some(step) => Icon::new(icon(&step.kind))
                .size(14.)
                .color(lavender)
                .into_any_element(),
            None => Indicator::new("agent-working")
                .size(IndicatorSize::Sm)
                .color(lavender)
                .pulse(true)
                .into_any_element(),
        };
        let worked = duration(turn.working_for(SystemTime::now()));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(TEXT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .child(mark)
            .child(shimmer(plain(line), lavender, bright))
            .child(
                div()
                    .flex_none()
                    .text_size(px(SMALL_TEXT_SIZE))
                    .text_color(muted)
                    .child(worked),
            )
    });
    // Ids inside are the turn's own, so two turns never share one.
    div()
        .id(("turn", index))
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(meta)
        .children(steps)
        .children(text)
        .children(working)
        .children(below)
        .into_any_element()
}

/// The problems a turn left that were not there before it: one peach line that opens to
/// `path: message` lines.
pub(super) fn problems(
    problems: &[Problem],
    index: usize,
    open: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (peach, muted) = (theme.peach, theme.gray_800);
    let files = problems
        .iter()
        .map(|problem| problem.path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let line = match files {
        1 => "1 file is not live".to_string(),
        files => format!("{files} files are not live"),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .text_size(px(SMALL_TEXT_SIZE))
        .line_height(px(SMALL_LINE_HEIGHT))
        .child(
            div()
                .id("problems")
                .debug_selector(move || format!("agent-problems-{index}"))
                .text_color(peach)
                .cursor_pointer()
                .on_click(on_toggle)
                .child(line),
        )
        .when(open, |lines| {
            lines.children(problems.iter().enumerate().map(|(number, problem)| {
                div()
                    .debug_selector(move || format!("agent-problem-{index}-{number}"))
                    .text_color(muted)
                    .child(format!("{}: {}", problem.path, problem.message))
            }))
        })
        .into_any_element()
}

/// The icon of a step's line.
fn icon(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Read { .. } => "eye",
        StepKind::Change { .. } => "pencil",
        StepKind::Run => "terminal",
        StepKind::Search => "search",
        StepKind::Other => "wrench",
    }
}

/// A title with its commands as plain text, for a line that is cut with an ellipsis: gpui cuts
/// only a text that is the direct child of the line, not markdown's blocks.
fn plain(title: &str) -> SharedString {
    title.replace('`', "").into()
}

/// One line in `color` that a band of `bright` sweeps across, again and again, while the agent
/// works on it.
fn shimmer(text: SharedString, color: Hsla, bright: Hsla) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .truncate()
        .text_color(color)
        .with_animation(
            "agent-working-line",
            Animation::new(SHIMMER).repeat(),
            move |line, delta| {
                let highlights = band(&text, delta, color, bright);
                line.child(StyledText::new(text.clone()).with_highlights(highlights))
            },
        )
}

/// The colour of each character the band lights at `delta` of its sweep. It starts and ends
/// beyond the text, so each sweep begins on the plain line.
fn band(
    text: &str,
    delta: f32,
    color: Hsla,
    bright: Hsla,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let length = text.chars().count() as f32;
    let center = delta * (length + 2. * SHIMMER_WIDTH) - SHIMMER_WIDTH;
    text.char_indices()
        .enumerate()
        .filter_map(|(number, (start, character))| {
            let light = 1. - (number as f32 - center).abs() / SHIMMER_WIDTH;
            (light > 0.).then(|| {
                let style = HighlightStyle {
                    color: Some(color.blend(bright.opacity(light))),
                    ..HighlightStyle::default()
                };
                (start..start + character.len_utf8(), style)
            })
        })
        .collect()
}

/// "12 s", or "2 min 5 s" from a minute on.
fn duration(worked: Duration) -> String {
    let seconds = worked.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min {} s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_seconds_then_minutes() {
        assert_eq!(duration(Duration::from_millis(12_400)), "12 s");
        assert_eq!(duration(Duration::from_secs(125)), "2 min 5 s");
    }
}
