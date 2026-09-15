//! Card: a 16 px padded surface with an optional title, and `CardRow` for the label/value rows
//! inside it (label with optional icon on the left, any element on the right).

use gpui::{
    AnyElement, App, BoxShadow, Div, FontWeight, SharedString, StyleRefinement, Window, div, hsla,
    point, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

#[derive(IntoElement)]
pub struct Card {
    base: Div,
    title: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            base: div(),
            title: None,
            children: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, text) = (theme.gray_200, theme.alpha_at(0.10), theme.gray_950);

        self.base
            .flex()
            .flex_col()
            .p(px(16.))
            .rounded(px(10.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .text_color(text)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.10),
                offset: point(px(0.), px(8.)),
                blur_radius: px(16.),
                spread_radius: px(-8.),
            }])
            .when_some(self.title, |card, title| {
                card.child(
                    div()
                        .pb(px(4.))
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
            })
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct CardRow {
    base: Div,
    label: SharedString,
    icon: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl CardRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            label: label.into(),
            icon: None,
            children: Vec::new(),
        }
    }

    /// Lucide icon name, drawn before the label.
    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

impl Styled for CardRow {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for CardRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for CardRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (border, icon_color) = (theme.alpha_at(0.05), theme.gray_600);

        self.base
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(14.))
                    .when_some(self.icon, |row, name| {
                        row.child(Icon::new(name).size(16.).color(icon_color))
                    })
                    .child(div().truncate().child(self.label)),
            )
            .children(self.children)
    }
}
