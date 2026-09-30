//! The matrix section: one row per route, its source, its destination, its amount on a
//! bipolar slider with the number next to it, and a button that removes it. `Add route` under
//! the rows adds one, up to [`MAX_ROUTES`]. The rows are as tall as a control, and a longer
//! list scrolls with two fingers inside the height of the card.

use gpui::{Context, FontWeight, div, prelude::*, px};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::ROW_HEIGHT;
use sound_ui::components::device_card::Section;
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::icon::Icon;
use sound_ui::components::knob::KnobRange;
use sound_ui::components::slider::{self, Slider};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::{ActiveTheme, typography};

use super::WavetableView;
use crate::{Destination, MAX_ROUTES, ROUTE_AMOUNT, Route, Source, WavetableState};

const SOURCE_WIDTH: f32 = 96.;
const DESTINATION_WIDTH: f32 = 136.;
const AMOUNT_WIDTH: f32 = 64.;
const READOUT_WIDTH: f32 = 40.;
const GAP: f32 = 6.;
/// Between two rows.
const ROW_GAP: f32 = 4.;

/// What a new route is: an LFO on the position of the first oscillator, the morph most sounds
/// start with, at no amount, so adding one changes nothing until its amount moves.
const NEW_ROUTE: Route = Route {
    source: Source::Lfo1,
    destination: Destination::Osc1Position,
    amount: ROUTE_AMOUNT.default,
};

/// An amount as the row shows it: `+50%`, `-25%`, `0%`.
fn amount_readout(amount: f32) -> String {
    let percent = sound_ui::components::knob::short(amount.abs() * 100.);
    match amount {
        amount if amount > 0. => format!("+{percent}%"),
        amount if amount < 0. => format!("-{percent}%"),
        _ => format!("{percent}%"),
    }
}

impl WavetableView {
    /// A change of route `index`, one undo step named `label`. The route may be gone by the
    /// time a drag moves, removed from outside: then nothing changes.
    fn route_change<V>(
        &mut self,
        index: usize,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut Route, V),
        cx: &mut Context<Self>,
    ) {
        let set = |state: &mut WavetableState, value| {
            if let Some(route) = state.matrix.get_mut(index) {
                set(route, value)
            }
        };
        self.change(label, change, set, cx);
    }

    fn route(&self, index: usize, route: Route, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, text) = (theme.gray_700, theme.gray_950);
        let source = Self::select(
            ("route-source", index),
            route.source,
            cx,
            move |view, source, cx| {
                let set = |route: &mut Route, source| route.source = source;
                view.route_change(
                    index,
                    "Change route source",
                    ValueChange::Set(source),
                    set,
                    cx,
                );
            },
        )
        .trigger_width(SOURCE_WIDTH)
        .menu_width(200.);
        let pick = move |view: &mut Self, destination, cx: &mut Context<Self>| {
            let set = |route: &mut Route, destination| route.destination = destination;
            let change = ValueChange::Set(destination);
            view.route_change(index, "Change route destination", change, set, cx);
        };
        let destination = Self::select(("route-destination", index), route.destination, cx, pick)
            .trigger_width(DESTINATION_WIDTH)
            .menu_width(200.);
        let amount = Slider::new(("route-amount", index), AMOUNT_WIDTH)
            .range(KnobRange::linear(ROUTE_AMOUNT.min, ROUTE_AMOUNT.max))
            .bipolar(true)
            .value(route.amount)
            .default_value(ROUTE_AMOUNT.default)
            .on_change(sound_ui::weak_callback(cx, move |view, change, cx| {
                let set = |route: &mut Route, amount| route.amount = amount;
                view.route_change(index, "Change route amount", change, set, cx);
            }));
        let remove = Button::icon_only(("route-remove", index), "x")
            .debug_selector(move || format!("route-remove-{index}"))
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.remove_focus[index])
            .on_click(cx.listener(move |view, _, _, cx| {
                let remove = |state: &mut WavetableState, ()| {
                    if index < state.matrix.len() {
                        state.matrix.remove(index);
                    }
                };
                view.change("Remove route", ValueChange::Set(()), remove, cx);
            }));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(GAP))
            .h(px(slider::HEIGHT))
            .child(source)
            .child(Icon::new("arrow-right").size(12.).color(muted))
            .child(destination)
            .child(amount)
            .child(
                div()
                    .w(px(READOUT_WIDTH))
                    .flex()
                    .justify_end()
                    .font(typography::tabular())
                    .text_size(px(12.))
                    .text_color(text)
                    .child(amount_readout(route.amount)),
            )
            .child(
                div()
                    .id(("route-remove-tip", index))
                    .tooltip(|_, cx| Tooltip::new("Remove route").view(cx))
                    .child(remove),
            )
    }

    /// The routes, and `Add route` while there is room for one more.
    pub(super) fn matrix(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let rows: Vec<_> = state
            .matrix
            .iter()
            .take(MAX_ROUTES)
            .enumerate()
            .map(|(index, route)| self.route(index, *route, cx).into_any_element())
            .collect();
        let theme = cx.theme();
        let (background, hover, muted, ring) = (
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.gray_800,
            theme.lavender,
        );
        // As quiet as a select, and the same size.
        let add = (state.matrix.len() < MAX_ROUTES).then(|| {
            div()
                .id("route-add")
                .debug_selector(|| "route-add".to_string())
                .track_focus(&self.add_focus)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .h(px(slider::HEIGHT))
                .pl(px(6.))
                .pr(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(gpui::Hsla::transparent_black())
                .focus_visible(move |style| style.border_color(ring))
                .bg(background)
                .hover(move |style| style.bg(hover))
                .cursor_pointer()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .child(Icon::new("plus").size(12.).color(muted))
                .child("Add route")
                .on_click(cx.listener(|view, _, _, cx| {
                    let add = |state: &mut WavetableState, ()| {
                        if state.matrix.len() < MAX_ROUTES {
                            state.matrix.push(NEW_ROUTE);
                        }
                    };
                    view.change("Add route", ValueChange::Set(()), add, cx);
                }))
        });
        let list = div()
            .id("routes")
            .overflow_y_scroll()
            .h(px(ROW_HEIGHT * 2.))
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .children(rows)
            .children(add.map(|add| div().flex().child(add)));
        Section::new().child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_amount_has_its_sign() {
        assert_eq!(amount_readout(0.5), "+50%");
        assert_eq!(amount_readout(-0.25), "-25%");
        assert_eq!(amount_readout(0.), "0%");
    }
}
