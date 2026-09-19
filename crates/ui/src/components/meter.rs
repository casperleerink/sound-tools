//! Vertical level meter, -60 to 0 dBFS. Neutral fill, red above -3 dBFS, a thin peak line.
//! Stateless: the caller passes the current level and peak, both in dBFS.

use gpui::{App, Div, Interactivity, StyleRefinement, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

const FLOOR: f32 = -60.;
const HOT: f32 = -3.;

#[derive(IntoElement)]
pub struct Meter {
    base: Div,
    level: f32,
    peak: Option<f32>,
    height: f32,
    width: f32,
}

impl Meter {
    /// `level` in dBFS; anything at or below -60 reads as silence.
    pub fn new(level: f32) -> Self {
        Self {
            base: div(),
            level,
            peak: None,
            height: 120.,
            width: 8.,
        }
    }

    /// Peak hold line, in dBFS.
    pub fn peak(mut self, peak: f32) -> Self {
        self.peak = Some(peak);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl Styled for Meter {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Meter {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

/// Position of a dB value on the scale, 0 at the floor and 1 at 0 dBFS.
fn position(db: f32) -> f32 {
    ((db - FLOOR) / -FLOOR).clamp(0., 1.)
}

impl RenderOnce for Meter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track, fill, hot, peak_color) = (
            theme.alpha_at(0.05),
            theme.gray_800,
            theme.red,
            theme.gray_950,
        );
        let height = self.height;
        let level = position(self.level);
        let hot_start = position(HOT);
        let neutral = level.min(hot_start) * height;
        let over = (level - hot_start).max(0.) * height;

        self.base
            .relative()
            .flex_none()
            .w(px(self.width))
            .h(px(height))
            .rounded_sm()
            .bg(track)
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(px(neutral))
                    .bg(fill),
            )
            .when(over > 0., |d| {
                d.child(
                    div()
                        .absolute()
                        .bottom(px(hot_start * height))
                        .left_0()
                        .w_full()
                        .h(px(over))
                        .bg(hot),
                )
            })
            .when_some(self.peak, |d, peak| {
                let y = position(peak) * height;
                d.child(
                    div()
                        .absolute()
                        .bottom(px((y - 1.).max(0.)))
                        .left_0()
                        .w_full()
                        .h(px(1.))
                        .bg(if peak > HOT { hot } else { peak_color }),
                )
            })
    }
}
