//! Separator: a one pixel `alpha/10` divider, horizontal (full width) or vertical (full height).

use gpui::{App, Div, StyleRefinement, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SeparatorOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(IntoElement)]
pub struct Separator {
    base: Div,
    orientation: SeparatorOrientation,
}

impl Separator {
    pub fn horizontal() -> Self {
        Self {
            base: div(),
            orientation: SeparatorOrientation::Horizontal,
        }
    }

    pub fn vertical() -> Self {
        Self {
            base: div(),
            orientation: SeparatorOrientation::Vertical,
        }
    }
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Separator {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let line = cx.theme().alpha_at(0.10);
        self.base
            .flex_none()
            .bg(line)
            .map(|d| match self.orientation {
                SeparatorOrientation::Horizontal => d.h(px(1.)).w_full(),
                SeparatorOrientation::Vertical => d.w(px(1.)).h_full(),
            })
    }
}
