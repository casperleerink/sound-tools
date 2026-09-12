use crate::TremoloState;
use gpui::{Context, Entity, Subscription, Window, div, prelude::*, px, rgb};
use sound_core::Instance;
use sound_ui::Session;

pub struct TremoloEditor {
    session: Entity<Session>,
    instance: Instance<TremoloState>,
    _subscription: Subscription,
}

impl TremoloEditor {
    pub fn new(
        session: Entity<Session>,
        instance: Instance<TremoloState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            instance,
            _subscription: subscription,
        }
    }

    fn adjust(
        &self,
        field: fn(&mut TremoloState) -> &mut f32,
        delta: f32,
        min: f32,
        max: f32,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            session.change(cx, |project| {
                project.edit(&self.instance, "Adjust tremolo", |state| {
                    let value = field(state);
                    *value = (*value + delta).clamp(min, max);
                })
            })
        });
    }
}

impl Render for TremoloEditor {
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
            .w(px(300.))
            .rounded_lg()
            .bg(rgb(0x30283c))
            .text_color(rgb(0xf1eafa))
            .child(format!("Tremolo · {}", self.instance.id()));
        if let Ok(state) = state {
            let controls = [
                (
                    "frequency",
                    format!("Voice {:.0} Hz", state.frequency_hz),
                    110.0,
                    20.0,
                    20_000.0,
                    (|s: &mut TremoloState| &mut s.frequency_hz)
                        as fn(&mut TremoloState) -> &mut f32,
                ),
                (
                    "gain",
                    format!("Gain {:.0}%", state.gain * 100.0),
                    0.1,
                    0.0,
                    1.0,
                    |s| &mut s.gain,
                ),
                (
                    "rate",
                    format!("Tremolo {:.1} Hz", state.rate_hz),
                    0.5,
                    0.1,
                    20.0,
                    |s| &mut s.rate_hz,
                ),
                (
                    "depth",
                    format!("Depth {:.0}%", state.depth * 100.0),
                    0.1,
                    0.0,
                    1.0,
                    |s| &mut s.depth,
                ),
            ];
            for (id, label, step, min, max, field) in controls {
                panel = panel.child(
                    div().id(id).flex().flex_col().gap_2().child(label).child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("down")
                                    .p_2()
                                    .bg(rgb(0x52425f))
                                    .cursor_pointer()
                                    .child("−")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.adjust(field, -step, min, max, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("up")
                                    .p_2()
                                    .bg(rgb(0x52425f))
                                    .cursor_pointer()
                                    .child("+")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.adjust(field, step, min, max, cx)
                                    })),
                            ),
                    ),
                );
            }
        } else {
            panel = panel.child("Instance deleted");
        }
        panel
    }
}
