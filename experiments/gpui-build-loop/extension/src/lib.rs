pub const REVISION: &str = "revision-0";
use gpui::{Context, Entity, SharedString, Window, div, prelude::*, px, rgb};

/// A throwaway custom view. Each entity owns its own editable state.
pub struct PulseEditor {
    name: SharedString,
    steps: [bool; 4],
    level: u8,
}

impl PulseEditor {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            steps: [true, false, true, false],
            level: 50,
        }
    }
}

impl Render for PulseEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .w(px(330.0))
            .rounded_lg()
            .bg(rgb(0x202933))
            .text_color(rgb(0xf2f5f7))
            .child(div().text_xl().child(self.name.clone()))
            .child("Click a step to toggle it")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .children(self.steps.iter().enumerate().map(|(index, enabled)| {
                        div()
                            .id(("step", index))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(58.0))
                            .rounded_md()
                            .cursor_pointer()
                            .bg(rgb(if *enabled { 0x75d5ac } else { 0x394653 }))
                            .text_color(rgb(if *enabled { 0x10231c } else { 0xffffff }))
                            .child(format!("{}", index + 1))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.steps[index] = !this.steps[index];
                                eprintln!(
                                    "EDIT {} steps={:?} level={}",
                                    this.name, this.steps, this.level
                                );
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("decrease")
                            .p_3()
                            .rounded_md()
                            .bg(rgb(0x394653))
                            .cursor_pointer()
                            .child("−")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.level = this.level.saturating_sub(10);
                                eprintln!(
                                    "EDIT {} steps={:?} level={}",
                                    this.name, this.steps, this.level
                                );
                                cx.notify();
                            })),
                    )
                    .child(format!("Level {}%", self.level))
                    .child(
                        div()
                            .id("increase")
                            .p_3()
                            .rounded_md()
                            .bg(rgb(0x394653))
                            .cursor_pointer()
                            .child("+")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.level = (this.level + 10).min(100);
                                eprintln!(
                                    "EDIT {} steps={:?} level={}",
                                    this.name, this.steps, this.level
                                );
                                cx.notify();
                            })),
                    ),
            )
    }
}

pub struct ExperimentView {
    editors: [Entity<PulseEditor>; 2],
}

impl ExperimentView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            editors: [
                cx.new(|_| PulseEditor::new("Pulse A")),
                cx.new(|_| PulseEditor::new("Pulse B")),
            ],
        }
    }
}

impl Render for ExperimentView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .bg(rgb(0x10171e))
            .text_color(rgb(0xf2f5f7))
            .child(
                div()
                    .text_xl()
                    .child(format!("Custom GPUI view experiment · {}", REVISION)),
            )
            .child("Two independently editable instances · no audio")
            .child(div().flex().gap_4().children(self.editors.iter().cloned()))
    }
}
