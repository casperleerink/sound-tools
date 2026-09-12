use crate::ToneState;
use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px, rgb};
use sound_core::Instance;
use sound_ui::Session;

pub struct ToneEditor {
    session: Entity<Session>,
    instance: Instance<ToneState>,
    _subscription: Subscription,
}
impl ToneEditor {
    pub fn new(
        session: Entity<Session>,
        instance: Instance<ToneState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            instance,
            _subscription: subscription,
        }
    }
    fn adjust(&self, frequency: f32, gain: f32, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.change(cx, |project| {
                project.edit(&self.instance, "Adjust tone", |state| {
                    state.frequency_hz = (state.frequency_hz + frequency).clamp(20.0, 20_000.0);
                    state.gain = (state.gain + gain).clamp(0.0, 1.0);
                })
            })
        });
    }
}
impl Render for ToneEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self
            .session
            .read(cx)
            .project()
            .state(&self.instance)
            .cloned();
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .w(px(280.))
            .rounded_lg()
            .bg(rgb(0x203038))
            .text_color(rgb(0xe7f0ed))
            .child(format!("Tone · {}", self.instance.id()));
        if let Ok(state) = state {
            panel = panel
                .child(format!(
                    "{:.0} Hz · {:.0}%",
                    state.frequency_hz,
                    state.gain * 100.0
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("frequency-down")
                                .p_2()
                                .bg(rgb(0x34515a))
                                .cursor_pointer()
                                .child("−110 Hz")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.adjust(-110.0, 0.0, cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("frequency-up")
                                .p_2()
                                .bg(rgb(0x34515a))
                                .cursor_pointer()
                                .child("+110 Hz")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.adjust(110.0, 0.0, cx)),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("gain-down")
                                .p_2()
                                .bg(rgb(0x34515a))
                                .cursor_pointer()
                                .child("−10%")
                                .on_click(cx.listener(|this, _, _, cx| this.adjust(0.0, -0.1, cx))),
                        )
                        .child(
                            div()
                                .id("gain-up")
                                .p_2()
                                .bg(rgb(0x34515a))
                                .cursor_pointer()
                                .child("+10%")
                                .on_click(cx.listener(|this, _, _, cx| this.adjust(0.0, 0.1, cx))),
                        ),
                )
                .child(
                    div()
                        .id("delete")
                        .p_2()
                        .cursor_pointer()
                        .child("Delete tone")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.update(cx, |session, cx| {
                                session.change(cx, |project| project.delete(&this.instance))
                            })
                        })),
                );
        } else {
            panel = panel.child("Instance deleted");
        }
        panel
    }
}
