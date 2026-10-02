//! How each entry of the thread looks: the composer's message as a bubble, the agent's turn as
//! markdown with its meta line, and quiet lines.

use std::collections::BTreeSet;
use std::time::Duration;

use gpui::{AnyElement, App, ClickEvent, ElementId, SharedString, Window, div, prelude::*, px};
use sound_core::Problem;
use sound_ui::ActiveTheme;
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::markdown::{Markdown, MarkdownText};

use crate::StepOutcome;
use crate::TurnOutcome;
use crate::conversation::Turn;

/// The text of the thread: 15 on 22, as the mockup.
const TEXT_SIZE: f32 = 15.;
const LINE_HEIGHT: f32 = 22.;
/// Meta lines and steps.
const SMALL_TEXT_SIZE: f32 = 12.;
const SMALL_LINE_HEIGHT: f32 = 16.;

pub(super) fn message(text: &str, cx: &App) -> AnyElement {
    div()
        .p(px(14.))
        .rounded(px(12.))
        .bg(cx.theme().alpha_at(0.05))
        .text_size(px(TEXT_SIZE))
        .line_height(px(LINE_HEIGHT))
        .child(SharedString::from(text.to_string()))
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
        let (line, color) = match &end.outcome {
            TurnOutcome::Completed => (format!("Worked for {}", duration(end.worked)), muted),
            TurnOutcome::Interrupted => (format!("Stopped after {}", duration(end.worked)), muted),
            TurnOutcome::Failed { message } if message.is_empty() => {
                ("The turn failed".to_string(), red)
            }
            TurnOutcome::Failed { message } => (message.clone(), red),
        };
        let has_steps = !turn.steps.is_empty();
        div()
            .id("worked-for")
            .debug_selector(move || format!("agent-worked-for-{index}"))
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(color)
            .child(line)
            .when(has_steps, |line| {
                line.cursor_pointer()
                    .hover(move |style| style.text_color(bright))
                    .on_click(on_toggle_steps)
            })
    });
    let steps = steps_open.then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .text_color(dim)
            .children(turn.steps.iter().enumerate().map(|(step_index, step)| {
                // A step that did not do what it says says so in words, a failure in red.
                let outcome = match step.outcome {
                    Some(StepOutcome::Failed) => Some(("· failed", red)),
                    Some(StepOutcome::Denied) => Some(("· denied", muted)),
                    Some(StepOutcome::Done) | None => None,
                };
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        title(("step", step_index), step.finished_title())
                            .min_w_0()
                            .text_size(px(SMALL_TEXT_SIZE))
                            .line_height(px(SMALL_LINE_HEIGHT)),
                    )
                    .children(outcome.map(|(word, color)| {
                        div()
                            .flex_none()
                            .text_size(px(SMALL_TEXT_SIZE))
                            .line_height(px(SMALL_LINE_HEIGHT))
                            .text_color(color)
                            .child(word)
                    }))
            }))
    });
    let text = answer
        .iter()
        .enumerate()
        .map(|(block, markdown)| MarkdownText::new(("answer", block), markdown.clone()));
    // While a question waits, the question is what the agent does.
    let working = (turn.end.is_none() && turn.approvals.is_empty()).then(|| {
        let line = turn
            .current_step()
            .map_or("Working", |step| match step.outcome {
                None => step.running_title.as_str(),
                Some(_) => step.finished_title(),
            });
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                Indicator::new("agent-working")
                    .size(IndicatorSize::Sm)
                    .color(lavender)
                    .pulse(true),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(TEXT_SIZE))
                    .line_height(px(LINE_HEIGHT))
                    .text_color(lavender)
                    // One line, a long command cut with an ellipsis. Plain text, as gpui cuts
                    // only a text that is the direct child of the line, not markdown's blocks.
                    .child(SharedString::from(line.replace('`', ""))),
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
