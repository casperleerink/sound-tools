//! How each entry of the thread looks: the composer's message as a bubble, the agent's turn as
//! plain text with its meta line, and quiet lines.

use std::time::Duration;

use gpui::{AnyElement, App, ClickEvent, SharedString, Window, div, prelude::*, px};
use sound_ui::ActiveTheme;
use sound_ui::components::indicator::{Indicator, IndicatorSize};

use crate::StepOutcome;
use crate::TurnOutcome;
use crate::conversation::Turn;

/// The text of the thread: 15 on 22, as the mockup.
const TEXT_SIZE: f32 = 15.;
const LINE_HEIGHT: f32 = 22.;
/// Meta lines and steps.
const SMALL_TEXT_SIZE: f32 = 12.;

pub fn message(text: &str, cx: &App) -> AnyElement {
    div()
        .p(px(14.))
        .rounded(px(12.))
        .bg(cx.theme().alpha_at(0.05))
        .text_size(px(TEXT_SIZE))
        .line_height(px(LINE_HEIGHT))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

pub fn notice(text: &str, cx: &App) -> AnyElement {
    div()
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(cx.theme().gray_700)
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// One turn: the meta line once it ended, the steps behind it, the text, and while it works
/// the working line and the question it waits on.
pub fn turn(
    turn: &Turn,
    index: usize,
    expanded: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    approval: Option<AnyElement>,
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
            .id(("worked-for", index))
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(color)
            .child(line)
            .when(has_steps, |line| {
                line.cursor_pointer()
                    .hover(move |style| style.text_color(bright))
                    .on_click(on_toggle)
            })
    });
    let steps = expanded.then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(dim)
            .children(turn.steps.iter().map(|step| {
                let suffix = match step.outcome {
                    Some(StepOutcome::Failed) => ", failed",
                    Some(StepOutcome::Denied) => ", denied",
                    Some(StepOutcome::Done) | None => "",
                };
                div().child(format!("{}{suffix}", step.title))
            }))
    });
    let streaming = (!turn.streaming.is_empty()).then_some(&turn.streaming);
    let text = turn.blocks.iter().chain(streaming).map(|block| {
        div()
            .text_size(px(TEXT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .child(SharedString::from(block.clone()))
    });
    let working = turn.end.is_none().then(|| {
        let title = turn
            .current_step()
            .map_or("Working", |step| step.title.as_str());
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                Indicator::new(("agent-working", index))
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
                    .child(SharedString::from(title.to_string())),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(meta)
        .children(steps)
        .children(text)
        .children(working)
        .children(approval)
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
