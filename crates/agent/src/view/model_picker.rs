//! The model picker in the composer: the providers that are set up in a rail at the left, a
//! search, and the picked provider's models. Once a thread started on a provider the others
//! are off: a thread keeps its provider.

use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, ScrollHandle,
    SharedString, Window, div, img, prelude::*, px, rgb,
};
use sound_ui::ActiveTheme;
use sound_ui::ControlSize;
use sound_ui::components::icon::Icon;
use sound_ui::components::popover::{Align, Side, anchor, surface};
use sound_ui::components::text_input::{Arrow, TextInput};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::radius;
use sound_ui::{TextVariant, Typography};

use crate::{Model, Provider};

/// The panel's width: the rail and room for a model's name and its source.
const WIDTH: f32 = 340.;
/// The list scrolls past this, for a provider with hundreds of models.
const LIST_HEIGHT: f32 = 320.;

/// A provider as the picker offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Offered {
    pub provider: Provider,
    /// Its models, its default first, or none until it listed them.
    pub models: Vec<Model>,
    /// The composer's pick of its models, or `None` for its default.
    pub picked: Option<String>,
}

/// What the sidebar shows in the picker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Shown {
    /// The providers that are set up, in [`Provider::ALL`] order.
    pub offered: Vec<Offered>,
    /// The thread's provider: the trigger says its model.
    pub current: Option<Provider>,
    /// The provider the thread started on, whose models are the only ones it can pick.
    pub locked: Option<Provider>,
}

/// A model picked in the panel, with its provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModelPicked(pub Provider, pub String);

pub(super) struct ModelPicker {
    shown: Shown,
    open: bool,
    /// The provider the panel shows the models of.
    viewing: Option<Provider>,
    search: gpui::Entity<TextInput>,
    /// The row the keys are on, in the filtered list.
    highlighted: usize,
    /// Keeps the highlighted row in sight as the keys move it.
    scroll: ScrollHandle,
    trigger_focus: FocusHandle,
    /// Each provider of the rail is a tab stop, so the keys switch providers too.
    rail_focus: [FocusHandle; Provider::ALL.len()],
}

impl EventEmitter<ModelPicked> for ModelPicker {}

/// The name of the provider's logo in the icons.
fn logo_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude",
        Provider::Pi => "pi",
    }
}

/// The provider's logo, `size` points square: Claude's in its brand color, Pi's in its own
/// colors, which an icon cannot draw.
pub(super) fn logo(provider: Provider, size: f32) -> AnyElement {
    match provider {
        Provider::Claude => Icon::new("claude")
            .size(size)
            .color(rgb(0xd97757).into())
            .into_any_element(),
        Provider::Pi => img("icons/pi.svg").size(px(size)).into_any_element(),
    }
}

impl ModelPicker {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Search models")
                .size(ControlSize::Md)
                .bare(true)
        });
        let weak = cx.weak_entity();
        search.update(cx, |input, _| {
            // The handlers run inside the field's own update, which these change.
            let on = |weak: gpui::WeakEntity<Self>,
                      act: fn(&mut Self, &mut Window, &mut Context<Self>)| {
                move |_: &str, window: &mut Window, cx: &mut App| {
                    let weak = weak.clone();
                    window.defer(cx, move |window, cx| {
                        weak.update(cx, |picker, cx| act(picker, window, cx)).ok();
                    });
                }
            };
            input.set_on_submit(on(weak.clone(), Self::pick_highlighted));
            input.set_on_cancel(on(weak.clone(), Self::close));
            input.set_on_change(on(weak.clone(), |picker, _, cx| {
                picker.highlighted = 0;
                cx.notify();
            }));
            let weak = weak.clone();
            input.set_on_arrow_past_edge(move |_, arrow, _, cx| {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |picker, cx| picker.move_highlight(arrow, cx))
                        .ok();
                });
            });
        });
        Self {
            shown: Shown::default(),
            open: false,
            viewing: None,
            search,
            highlighted: 0,
            scroll: ScrollHandle::new(),
            trigger_focus: cx.focus_handle().tab_stop(true),
            rail_focus: Provider::ALL.map(|_| cx.focus_handle().tab_stop(true)),
        }
    }

    pub(super) fn set_shown(&mut self, shown: Shown, cx: &mut Context<Self>) {
        if self.shown != shown {
            self.shown = shown;
            cx.notify();
        }
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.viewing = self.shown.locked.or(self.shown.current);
        self.highlighted = 0;
        let search = self.search.clone();
        search.update(cx, |input, cx| input.set_text("", cx));
        window.focus(&search.focus_handle(cx), cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        window.focus(&self.trigger_focus, cx);
        cx.notify();
    }

    fn offered(&self, provider: Option<Provider>) -> Option<&Offered> {
        let provider = provider?;
        (self.shown.offered.iter()).find(|offered| offered.provider == provider)
    }

    /// The models the panel lists: the viewed provider's, those whose name or source holds
    /// the search.
    fn listed(&self, cx: &App) -> Vec<&Model> {
        let Some(offered) = self.offered(self.viewing) else {
            return Vec::new();
        };
        let search = self.search.read(cx).text().trim().to_lowercase();
        (offered.models.iter())
            .filter(|model| {
                let source = model.source.as_deref().unwrap_or_default();
                search.is_empty()
                    || model.name.to_lowercase().contains(&search)
                    || source.to_lowercase().contains(&search)
            })
            .collect()
    }

    fn move_highlight(&mut self, arrow: Arrow, cx: &mut Context<Self>) {
        let last = self.listed(cx).len().saturating_sub(1);
        self.highlighted = match arrow {
            Arrow::Up => self.highlighted.saturating_sub(1),
            Arrow::Down => (self.highlighted + 1).min(last),
        };
        self.scroll.scroll_to_item(self.highlighted);
        cx.notify();
    }

    fn pick_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self
            .listed(cx)
            .get(self.highlighted)
            .map(|model| model.id.clone());
        if let (Some(provider), Some(id)) = (self.viewing, picked) {
            self.pick(provider, id, window, cx);
        }
    }

    fn pick(
        &mut self,
        provider: Provider,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shown.locked.is_some_and(|locked| locked != provider) {
            return;
        }
        cx.emit(ModelPicked(provider, id));
        self.close(window, cx);
    }

    /// What the trigger says: the model the agent runs. Until the provider lists its models,
    /// the id that is picked, or "Default".
    fn label(&self) -> String {
        let Some(offered) = self.offered(self.shown.current) else {
            return "No agent".to_string();
        };
        label(offered.picked.as_deref(), &offered.models)
    }

    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (selected, hover) = (theme.selected(), theme.fill());
        let (locked, ring) = (self.shown.locked, theme.lavender);
        let icons = self.shown.offered.iter().map(|offered| {
            let provider = offered.provider;
            let index = (Provider::ALL.iter()).position(|each| *each == provider);
            let focus = index.and_then(|index| self.rail_focus.get(index)).cloned();
            let icon = logo_name(provider);
            let off = locked.is_some_and(|locked| locked != provider);
            let tip = match off {
                true => format!("{}: start a new thread to switch", provider.name()),
                false => provider.name().to_string(),
            };
            div()
                .id(SharedString::from(format!("model-picker-{}", icon)))
                .debug_selector(move || format!("model-picker-{icon}"))
                .flex()
                .items_center()
                .justify_center()
                .size(px(32.))
                .rounded(radius::LG)
                .when(self.viewing == Some(provider), |rail| rail.bg(selected))
                .when(off, |rail| rail.opacity(0.4))
                .when(!off, |rail| {
                    rail.cursor_pointer().hover(move |s| s.bg(hover))
                })
                .border_1()
                .border_color(gpui::transparent_black())
                .when_some(focus, |rail, focus| {
                    rail.track_focus(&focus)
                        .focus_visible(move |s| s.border_color(ring))
                })
                .child(logo(provider, 16.))
                .tooltip(move |_, cx| Tooltip::new(tip.clone()).view(cx))
                .on_key_down(
                    cx.listener(move |picker, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if !off && (key == "enter" || key == "space") {
                            picker.view(provider, cx);
                            // The keys go on in the search of its models.
                            window.focus(&picker.search.focus_handle(cx), cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .on_click(cx.listener(move |picker, _, _, cx| {
                    if !off {
                        picker.view(provider, cx);
                    }
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(6.))
            .border_r_1()
            .border_color(theme.border_strong())
            .children(icons)
    }

    fn view(&mut self, provider: Provider, cx: &mut Context<Self>) {
        self.viewing = Some(provider);
        self.highlighted = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, strong, highlight) = (theme.text_muted(), theme.text(), theme.fill_strong());
        let viewing = self.viewing;
        let picked = self.offered(viewing).map(|offered| offered.picked.clone());
        let rows: Vec<_> = (self.listed(cx).into_iter().enumerate())
            .map(|(index, model)| {
                let checked = match &picked {
                    Some(Some(picked)) => *picked == model.id,
                    Some(None) => index == 0 && self.search.read(cx).text().is_empty(),
                    None => false,
                };
                let second = (model.source.clone())
                    .or_else(|| (!model.description.is_empty()).then(|| model.description.clone()));
                let id = model.id.clone();
                let key = match viewing {
                    Some(provider) => logo_name(provider),
                    None => "",
                };
                let selector = format!("model-picker-{key}-{id}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(radius::MD)
                    .cursor_pointer()
                    .when(index == self.highlighted, |row| row.bg(highlight))
                    .hover(move |s| s.bg(highlight))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(strong)
                                    .child(model.name.clone()),
                            )
                            .children(second.map(|second| {
                                div()
                                    .truncate()
                                    .text_variant(TextVariant::Label)
                                    .text_color(muted)
                                    .child(second)
                            })),
                    )
                    .when(checked, |row| {
                        row.child(Icon::new("check").size(14.).color(strong))
                    })
                    .on_click(cx.listener(move |picker, _, window, cx| {
                        if let Some(provider) = viewing {
                            picker.pick(provider, id.clone(), window, cx);
                        }
                    }))
            })
            .collect();
        let empty = match rows.is_empty() {
            false => None,
            true if self.search.read(cx).text().trim().is_empty() => Some("No models yet."),
            true => Some("No model matches."),
        };
        div()
            .id("model-picker-list")
            .flex_1()
            .min_w_0()
            .max_h(px(LIST_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(4.))
            .children(rows)
            .children(empty.map(|empty| div().p(px(10.)).text_color(muted).child(empty)))
    }

    fn panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border_strong();
        let search = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .h(px(40.))
            .border_b_1()
            .border_color(border)
            .child(Icon::new("search").size(14.).color(cx.theme().text_muted()))
            .child(div().flex_1().child(self.search.clone()));
        surface(cx)
            .id("model-picker")
            .w(px(WIDTH))
            .text_variant(TextVariant::Body)
            .flex()
            .on_mouse_down_out(cx.listener(|picker, _, window, cx| picker.close(window, cx)))
            // One provider needs no rail.
            .when(self.shown.offered.len() > 1, |panel| {
                panel.child(self.rail(cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(search)
                    .child(self.list(cx)),
            )
    }
}

/// What the trigger says for the models of one provider and its pick.
fn label(picked: Option<&str>, models: &[Model]) -> String {
    let model = match picked {
        Some(id) => models.iter().find(|model| model.id == *id),
        None => models.first(),
    };
    match (model, picked) {
        (Some(model), _) => model.short_name.clone(),
        (None, Some(id)) => id.to_string(),
        (None, None) => "Default".to_string(),
    }
}

impl Focusable for ModelPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.trigger_focus.clone()
    }
}

impl Render for ModelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (text, muted, ring, hover) = (
            theme.text(),
            theme.text_muted(),
            theme.lavender,
            theme.fill(),
        );
        let logo = self.shown.current.map(|provider| logo(provider, 14.));
        let trigger = div()
            .id("model-picker-trigger")
            .debug_selector(|| "model-menu".to_string())
            .track_focus(&self.trigger_focus)
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .h(px(32.))
            .px(px(8.))
            .rounded(radius::LG)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(move |s| s.border_color(ring))
            .text_variant(TextVariant::Heading)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .children(logo)
            .child(div().min_w_0().truncate().child(self.label()))
            .child(
                div()
                    .flex_none()
                    .child(Icon::new("chevron-down").size(14.).color(muted)),
            )
            .on_click(cx.listener(|picker, _, window, cx| {
                if picker.open {
                    picker.close(window, cx);
                } else {
                    picker.open(window, cx);
                }
            }));
        div()
            .relative()
            .flex()
            .min_w_0()
            .child(trigger)
            .when(self.open, |picker| {
                picker.child(anchor(Side::Top, Align::Start, self.panel(cx)))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_says_the_model_it_runs() {
        let model = |id: &str, short_name: &str| Model {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            short_name: short_name.to_string(),
            source: None,
        };
        let models = [model("default", "Opus 5.5"), model("haiku", "Haiku 4.5")];
        assert_eq!(label(None, &models), "Opus 5.5");
        assert_eq!(label(Some("haiku"), &models), "Haiku 4.5");
        // Before the provider lists its models.
        assert_eq!(label(None, &[]), "Default");
        assert_eq!(label(Some("haiku"), &[]), "haiku");
    }
}
