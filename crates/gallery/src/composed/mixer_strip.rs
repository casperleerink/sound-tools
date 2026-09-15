//! One mixer channel, 96 x 420: name with its accent dot, pan knob, level meter, fader, and mute
//! and solo toggles.
//!
//! `Slider` is horizontal only, so the fader sits under the meter instead of beside it. A rotated
//! layout would need a vertical slider in `crates/ui`.

use gpui::{
    Context, Entity, FontWeight, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, prelude::*, px,
};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::knob::Knob;
use sound_ui::components::meter::Meter;
use sound_ui::components::slider::Slider;
use sound_ui::{ActiveTheme, typography};

pub struct MixerStrip {
    pan: Entity<Knob>,
    level: Entity<Slider>,
    mute: bool,
    solo: bool,
    _subs: [Subscription; 2],
}

impl MixerStrip {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let pan = cx.new(|cx| {
            Knob::new(cx)
                .range(-50., 50.)
                .step(1.)
                .decimals(0)
                .value(0.)
                .size(40.)
                .label("Pan")
        });
        let level = cx.new(|cx| {
            Slider::new(cx)
                .range(-60., 6.)
                .step(0.5)
                .decimals(1)
                .value(-6.)
                .width(72.)
        });
        let subs = [
            cx.observe(&pan, |_, _, cx| cx.notify()),
            cx.observe(&level, |_, _, cx| cx.notify()),
        ];
        Self {
            pan,
            level,
            mute: false,
            solo: false,
            _subs: subs,
        }
    }

    fn toggle(&self, label: &'static str, id: &'static str, on: bool) -> Button {
        Button::new(id, label)
            .size(ButtonSize::Xs)
            .w(px(28.))
            .variant(if on {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Outline
            })
    }
}

impl Render for MixerStrip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, border, text, accent) = (
            theme.gray_200,
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.peach,
        );
        let readout = format!("{:.1} dB", self.level.read(cx).current());

        div()
            .w(px(96.))
            .h(px(420.))
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .p(px(12.))
            .rounded(px(10.))
            .bg(fill)
            .border_1()
            .border_color(border)
            .text_color(text)
            .font(typography::tabular())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(6.)).rounded_full().bg(accent))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Bass"),
                    ),
            )
            .child(self.pan.clone())
            .child(Meter::new(-12.).peak(-6.).height(180.).width(10.))
            .child(self.level.clone())
            .child(div().text_size(px(12.)).child(readout))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        self.toggle("M", "mute", self.mute)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mute = !this.mute;
                                cx.notify();
                            })),
                    )
                    .child(
                        self.toggle("S", "solo", self.solo)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.solo = !this.solo;
                                cx.notify();
                            })),
                    ),
            )
    }
}
