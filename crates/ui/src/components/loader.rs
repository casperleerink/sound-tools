//! Loader: a spinning `loader-circle` icon in three sizes, for "still working".

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, ElementId, Hsla, Transformation, Window, linear, percentage,
    prelude::*, px, svg,
};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LoaderSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl LoaderSize {
    fn px(self) -> f32 {
        match self {
            Self::Sm => 12.,
            Self::Md => 16.,
            Self::Lg => 20.,
        }
    }
}

#[derive(IntoElement)]
pub struct Loader {
    id: ElementId,
    size: LoaderSize,
    color: Option<Hsla>,
}

impl Loader {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            size: LoaderSize::default(),
            color: None,
        }
    }

    pub fn size(mut self, size: LoaderSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Loader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().gray_800);
        svg()
            .path("icons/loader-circle.svg")
            .size(px(self.size.px()))
            .flex_none()
            .text_color(color)
            .with_animation(
                self.id,
                Animation::new(Duration::from_millis(900))
                    .repeat()
                    .with_easing(linear),
                |icon, delta| icon.with_transformation(Transformation::rotate(percentage(delta))),
            )
    }
}
