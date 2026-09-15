//! Tabs: a plain row of labels, no track and no underline. The active tab is the one at full
//! opacity; inactive tabs sit at 40% and come up on hover. Controlled — the caller owns the active
//! value and gets an `on_change(value)`.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, Interactivity, SharedString, StyleRefinement,
    Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Tabs {
    base: Div,
    id: ElementId,
    tabs: Vec<(SharedString, SharedString, bool)>,
    active: SharedString,
    on_change: Option<ChangeHandler>,
}

impl Tabs {
    pub fn new(id: impl Into<ElementId>, active: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            tabs: Vec::new(),
            active: active.into(),
            on_change: None,
        }
    }

    /// Each tab is `(value, label)`.
    pub fn tabs(
        mut self,
        tabs: impl IntoIterator<Item = (impl Into<SharedString>, impl Into<SharedString>)>,
    ) -> Self {
        self.tabs = tabs
            .into_iter()
            .map(|(value, label)| (value.into(), label.into(), false))
            .collect();
        self
    }

    /// Append one tab that cannot be selected.
    pub fn disabled_tab(
        mut self,
        value: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        self.tabs.push((value.into(), label.into(), true));
        self
    }

    pub fn on_change(mut self, f: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Tabs {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Tabs {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for Tabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let text = cx.theme().gray_950;
        let active = self.active.clone();
        let on_change = self.on_change;

        let items = self
            .tabs
            .into_iter()
            .enumerate()
            .map(|(ix, (value, label, disabled))| {
                let is_active = value == active;
                let on_change = on_change.clone().filter(|_| !disabled);
                div()
                    .id(("tab", ix))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(28.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .map(|d| match (disabled, is_active) {
                        (true, _) => d.opacity(0.2).cursor_not_allowed(),
                        (false, true) => d,
                        (false, false) => d.opacity(0.4).cursor_pointer().hover(|s| s.opacity(1.)),
                    })
                    .when_some(on_change, |d, f| {
                        d.on_click(move |_: &ClickEvent, window, cx| f(value.clone(), window, cx))
                    })
                    .child(label)
            });

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .children(items)
    }
}
