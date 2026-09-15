//! Indicator: a status dot in three sizes, with an optional halo ring and a slow 3.5 s pulse
//! (down to 70% opacity) for "agent is working".

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, ElementId, Hsla, Window, div, prelude::*, pulsating_between, px,
};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IndicatorSize {
    Xs,
    Sm,
    #[default]
    Md,
}

impl IndicatorSize {
    fn dot(self) -> f32 {
        match self {
            Self::Xs => 6.,
            Self::Sm => 8.,
            Self::Md => 12.,
        }
    }

    fn ring(self) -> f32 {
        match self {
            Self::Xs => 2.,
            Self::Sm => 3.,
            Self::Md => 4.,
        }
    }
}

#[derive(IntoElement)]
pub struct Indicator {
    id: ElementId,
    size: IndicatorSize,
    color: Option<Hsla>,
    ring: bool,
    pulse: bool,
}

impl Indicator {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            size: IndicatorSize::default(),
            color: None,
            ring: false,
            pulse: false,
        }
    }

    pub fn size(mut self, size: IndicatorSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Draw a soft halo around the dot.
    pub fn ring(mut self, ring: bool) -> Self {
        self.ring = ring;
        self
    }

    /// Slow 3.5 s pulse, for "agent is working".
    pub fn pulse(mut self, pulse: bool) -> Self {
        self.pulse = pulse;
        self
    }
}

impl RenderOnce for Indicator {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().gray_600);
        let size = self.size;

        let dot = div().size(px(size.dot())).rounded_full().bg(color);
        let dot = if self.pulse {
            dot.with_animation(
                self.id,
                Animation::new(Duration::from_millis(3500))
                    .repeat()
                    .with_easing(pulsating_between(0.7, 1.0)),
                |dot, delta| dot.opacity(delta),
            )
            .into_any_element()
        } else {
            dot.into_any_element()
        };

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .when(self.ring, |d| {
                d.p(px(size.ring())).rounded_full().bg(color.opacity(0.10))
            })
            .child(dot)
    }
}
