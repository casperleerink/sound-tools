//! Segmented control: a single-select pill group on an `alpha/5` track. Controlled — the caller
//! owns the selected value and gets an `on_change(value)`. Unselected items sit at 40% opacity and
//! come to 100% on hover.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, Div, ElementId, FontWeight, Interactivity, SharedString, StyleRefinement,
    Window, div, prelude::*, px,
};

use crate::theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SegmentedControl {
    base: Div,
    id: ElementId,
    options: Vec<(SharedString, SharedString)>,
    value: SharedString,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl SegmentedControl {
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            options: Vec::new(),
            value: value.into(),
            disabled: false,
            on_change: None,
        }
    }

    /// Each option is `(value, label)`.
    pub fn options(
        mut self,
        options: impl IntoIterator<Item = (impl Into<SharedString>, impl Into<SharedString>)>,
    ) -> Self {
        self.options = options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for SegmentedControl {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for SegmentedControl {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track, selected_bg, border, hover_bg, text) = (
            theme.alpha_at(0.05),
            theme.gray_50,
            theme.alpha_at(0.10),
            theme.alpha_at(0.05),
            theme.gray_950,
        );
        let disabled = self.disabled;
        let value = self.value.clone();
        let on_change = self.on_change.filter(|_| !disabled);

        let items = self
            .options
            .into_iter()
            .enumerate()
            .map(|(ix, (val, label))| {
                let selected = val == value;
                let on_change = on_change.clone();
                div()
                    .id(("segment", ix))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(28.))
                    .px(px(10.))
                    .rounded_full()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text)
                    .when(selected, |d| {
                        d.bg(selected_bg).border_1().border_color(border)
                    })
                    .when(!selected, |d| {
                        d.border_1()
                            .border_color(gpui::Hsla::transparent_black())
                            .opacity(0.4)
                            .hover(|s| s.bg(hover_bg).opacity(1.))
                    })
                    .when(!disabled, |d| d.cursor_pointer())
                    .when_some(on_change, |d, f| {
                        d.on_click(move |_: &ClickEvent, window, cx| f(val.clone(), window, cx))
                    })
                    .child(label)
            });

        self.base
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .p(px(4.))
            .rounded_full()
            .bg(track)
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .children(items)
    }
}
