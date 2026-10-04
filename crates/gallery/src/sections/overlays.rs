//! Tooltip, dropdown menu and split button. The select, a style of the dropdown menu, is in the
//! rack section, and the focus rings of the split button in the focus section.
//! `GALLERY_OPEN=dropdown` opens the dropdown menu at startup, and `GALLERY_OPEN=search` the
//! one with a search. Without either the second split button opens its menu, so the snapshot
//! shows it.

use gpui::{
    App, Entity, FontWeight, Global, IntoElement, ParentElement, Styled, Window, div, prelude::*,
    px,
};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem, Trigger};
use sound_ui::components::popover::Align;
use sound_ui::components::split_button::{SplitButton, SplitChoices};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::theme::ActiveTheme;

/// Stateful overlays must be created once, so they live in a global.
struct OverlaysState {
    menu: Entity<DropdownMenu>,
    search: Entity<DropdownMenu>,
    split: Entity<SplitButton>,
    split_open: Entity<SplitButton>,
}

impl Global for OverlaysState {}

/// The split button of the arrangement: `Add track`, and an instrument or an audio track.
pub(crate) fn add_track_button(name: &'static str, cx: &mut App) -> Entity<SplitButton> {
    cx.new(|cx| {
        let items = [
            MenuItem::new("instrument", "Instrument track").selectable(false),
            MenuItem::new("audio", "Audio track").selectable(false),
        ];
        let choices = SplitChoices {
            label: "Add track".into(),
            main_value: "instrument".into(),
            menu_label: "Instrument or audio track".into(),
            entries: vec![MenuEntry::Group(MenuGroup::new().items(items))],
        };
        SplitButton::new(name, choices, cx).icon("plus")
    })
}

fn models() -> Vec<MenuItem> {
    [
        ("opus-5", "Opus 5", "Deepest reasoning, slowest"),
        ("sonnet-4", "Sonnet 4.6", "Balanced default"),
        ("haiku-3", "Haiku 3.5", "Fast and cheap"),
        ("gpt-6", "GPT-6 Astra", "Long context"),
        ("gpt-5-sol", "GPT-5.6 Sol", "Tool heavy work"),
        ("gpt-5-luna", "GPT-5.6 Luna", "Short answers"),
        ("local-7b", "Local 7B", "Runs on this machine"),
    ]
    .into_iter()
    .map(|(value, label, description)| MenuItem::new(value, label).description(description))
    .collect()
}

fn menu_entries() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Group(MenuGroup::new().label("Model").items(models())),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Effort").items([
            MenuItem::new("low", "low"),
            MenuItem::new("medium", "medium"),
            MenuItem::new("high", "high").disabled(true),
        ])),
    ]
}

/// The parameters of a plugin, as the list on its card shows them, two of them on the card.
fn parameters() -> Vec<MenuEntry> {
    let names = [
        "Cutoff",
        "Resonance",
        "Drive",
        "Wave",
        "Detune",
        "Attack",
        "Decay",
        "Sustain",
        "Release",
        "Glide",
        "Bright",
        "Level",
    ];
    let items = names.iter().enumerate().map(|(index, name)| {
        MenuItem::new(index.to_string(), *name)
            .checked(index < 2)
            .selectable(false)
    });
    vec![MenuEntry::Group(MenuGroup::new().items(items))]
}

fn install(window: &mut Window, cx: &mut App) {
    let open = std::env::var("GALLERY_OPEN").unwrap_or_default();

    let menu = cx.new(|cx| {
        DropdownMenu::new("Opus 5 · high", menu_entries(), cx)
            .selected("opus-5")
            .align(Align::Start)
            .width(300.)
            .max_height(260.)
    });
    let search = cx.new(|cx| {
        DropdownMenu::new("Parameters", parameters(), cx)
            .searchable("Search", cx)
            .trigger(Trigger::Subtle)
            .width(260.)
            .max_height(260.)
    });
    let split = add_track_button("split", cx);
    let split_open = add_track_button("split-open", cx);
    match open.as_str() {
        "dropdown" => menu.update(cx, |this, cx| this.open(window, cx)),
        "search" => search.update(cx, |this, cx| this.open(window, cx)),
        _ => {
            let split_menu = split_open.read(cx).menu().clone();
            split_menu.update(cx, |this, cx| this.open(window, cx));
        }
    }

    cx.set_global(OverlaysState {
        menu,
        search,
        split,
        split_open,
    });
}

fn heading(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().gray_700)
        .child(label)
}

fn row(label: &'static str, cx: &App, child: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(8.))
        .child(heading(label, cx))
        .child(child)
}

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    if cx.try_global::<OverlaysState>().is_none() {
        install(window, cx);
    }
    let state = cx.global::<OverlaysState>();
    let (menu, search) = (state.menu.clone(), state.search.clone());
    let (split, split_open) = (state.split.clone(), state.split_open.clone());
    let theme = cx.theme();
    let (border, text, hover) = (theme.alpha_at(0.10), theme.gray_950, theme.alpha_at(0.10));

    let tooltip = div()
        .id("tooltip")
        .flex()
        .items_center()
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .text_size(px(14.))
        .text_color(text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child("Play")
        .tooltip(|_window, cx| Tooltip::new("Play").key("Space").view(cx));

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .child(row("Tooltip", cx, tooltip))
        .child(row("Dropdown menu", cx, menu))
        .child(row("Dropdown menu with a search", cx, search))
        // Last, so the open menu hangs over nothing.
        .child(row(
            "Split button: at rest, and its menu open",
            cx,
            div().flex().gap(px(48.)).child(split).child(split_open),
        ))
}
