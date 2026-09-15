//! Transport: a floating pill at the bottom centre of a dark stage. Play/pause, stop, position,
//! a hairline seek strip with a draggable playhead, duration, and one lavender dot when a reload
//! is pending. Nothing else.

use gpui::{
    BoxShadow, Context, DragMoveEvent, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Render, Styled, Window, div, hsla, point, prelude::*, px,
};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::{ActiveTheme, typography};

/// Marker for gpui's drag machinery; the preview renders nothing.
struct SeekDrag;

struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

const STRIP: f32 = 200.;
const PLAYHEAD: f32 = 8.;
/// 4:32 at 120 bpm in 4/4.
const TOTAL_BEATS: f32 = 544.;

pub struct Transport {
    playing: bool,
    beat: f32,
    reload_pending: bool,
    drag_start: Option<(f32, f32)>,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            beat: 1.,
            reload_pending: false,
            drag_start: None,
        }
    }
}

impl Transport {
    pub fn reload_pending(&self) -> bool {
        self.reload_pending
    }

    pub fn set_reload_pending(&mut self, pending: bool, cx: &mut Context<Self>) {
        self.reload_pending = pending;
        cx.notify();
    }

    fn fraction(&self) -> f32 {
        (self.beat / TOTAL_BEATS).clamp(0., 1.)
    }

    /// Bars, beats and ticks, one-based.
    fn position(&self) -> String {
        let beat = self.beat.floor().max(0.);
        format!("{}.{}.1", (beat / 4.) as u32 + 1, (beat as u32 % 4) + 1)
    }

    fn set_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        self.beat = (fraction.clamp(0., 1.) * TOTAL_BEATS).round();
        cx.notify();
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_start = Some((f32::from(ev.position.x), self.fraction()));
        cx.notify();
    }

    fn on_drag_move(
        &mut self,
        ev: &DragMoveEvent<SeekDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_x, start_fraction)) = self.drag_start else {
            return;
        };
        let travel = (STRIP - PLAYHEAD).max(1.);
        let delta = (f32::from(ev.event.position.x) - start_x) / travel;
        self.set_fraction(start_fraction + delta, cx);
    }

    fn seek_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (track, fill) = (theme.alpha_at(0.10), theme.gray_950);
        let fraction = self.fraction();

        div()
            .id("seek")
            .relative()
            .flex_none()
            .w(px(STRIP))
            .h(px(PLAYHEAD))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_drag(SeekDrag, |_, _, _, cx| cx.new(|_| DragGhost))
            .on_drag_move(cx.listener(Self::on_drag_move))
            .child(
                div()
                    .absolute()
                    .top(px(PLAYHEAD / 2. - 0.5))
                    .left_0()
                    .w(px(STRIP))
                    .h(px(1.))
                    .bg(track),
            )
            .child(
                div()
                    .absolute()
                    .top(px(PLAYHEAD / 2. - 0.5))
                    .left_0()
                    .w(px((STRIP - PLAYHEAD) * fraction + PLAYHEAD / 2.))
                    .h(px(1.))
                    .bg(fill),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px((STRIP - PLAYHEAD) * fraction))
                    .size(px(PLAYHEAD))
                    .rounded_full()
                    .bg(fill),
            )
    }

    fn pill(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, border, text, muted, green, lavender) = (
            theme.gray_200.blend(theme.alpha_at(0.06)),
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.gray_700,
            theme.green,
            theme.lavender,
        );
        let playing = self.playing;

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(48.))
            .px(px(12.))
            .rounded_full()
            .bg(fill)
            .border_1()
            .border_color(border)
            .text_color(text)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.25),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-8.),
            }])
            .child(
                Button::icon_only("play", if playing { "pause" } else { "play" })
                    .variant(if playing {
                        ButtonVariant::SubtleColor(green)
                    } else {
                        ButtonVariant::GhostColor(green)
                    })
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.playing = !this.playing;
                        cx.notify();
                    })),
            )
            .child(
                Button::icon_only("stop", "square")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.playing = false;
                        this.beat = 0.;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .font(typography::tabular())
                    .text_size(px(14.))
                    .min_w(px(52.))
                    .child(self.position()),
            )
            .child(self.seek_strip(cx))
            .child(
                div()
                    .font(typography::tabular())
                    .text_size(px(14.))
                    .text_color(muted)
                    .child("4:32"),
            )
            .when(self.reload_pending, |d| {
                d.child(
                    Indicator::new("reload-pending")
                        .size(IndicatorSize::Xs)
                        .color(lavender),
                )
            })
    }
}

impl Render for Transport {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (stage, border) = (theme.gray_100, theme.alpha_at(0.10));

        div()
            .relative()
            .flex_none()
            .w(px(900.))
            .h(px(120.))
            .rounded(px(12.))
            .bg(stage)
            .border_1()
            .border_color(border)
            .child(
                div()
                    .absolute()
                    .bottom(px(20.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(self.pill(cx)),
            )
    }
}
