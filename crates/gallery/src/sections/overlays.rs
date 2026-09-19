//! Tooltip, popover, dropdown menu, select and dialog.
//! `GALLERY_OPEN=dropdown|select|popover|dialog` opens one overlay at startup.

use gpui::{
    App, Entity, FontWeight, Global, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::*, px,
};
use sound_ui::components::dialog::{Dialog, DialogAction};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::popover::{Align, Popover};
use sound_ui::components::select::Select;
use sound_ui::components::tooltip::{Tooltip, TooltipVariant};
use sound_ui::theme::ActiveTheme;

/// Stateful overlays must be created once, so they live in a global.
struct OverlaysState {
    popover: Entity<Popover>,
    menu: Entity<DropdownMenu>,
    select: Entity<Select>,
    dialog: Entity<Dialog>,
}

impl Global for OverlaysState {}

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
        MenuEntry::Group(
            MenuGroup::new()
                .label("Model")
                .max_height(180.)
                .items(models()),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Effort").items([
            MenuItem::new("low", "low"),
            MenuItem::new("medium", "medium"),
            MenuItem::new("high", "high").disabled(true),
        ])),
    ]
}

fn install(window: &mut Window, cx: &mut App) {
    let open = std::env::var("GALLERY_OPEN").unwrap_or_default();

    let popover = cx.new(|cx| {
        Popover::new(
            "Output device",
            |_window, cx| {
                let muted = cx.theme().gray_700;
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().child("Scarlett 2i2"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("48 kHz · 128 samples"),
                    )
                    .into_any_element()
            },
            cx,
        )
        .width(220.)
    });
    let menu = cx.new(|cx| {
        DropdownMenu::new("Opus 5 · high", menu_entries(), cx)
            .selected("opus-5")
            .align(Align::Start)
            .width(300.)
    });
    let select = cx.new(|cx| {
        Select::new(
            "Choose a scale",
            vec![
                MenuItem::new("major", "Major").icon("music"),
                MenuItem::new("minor", "Minor").icon("music"),
                MenuItem::new("dorian", "Dorian").icon("music"),
                MenuItem::new("locrian", "Locrian")
                    .icon("music")
                    .disabled(true),
            ],
            cx,
        )
        .selected("minor")
    });
    let dialog = cx.new(|cx| {
        Dialog::new(
            "Discard take?",
            "This take has not been bounced. Discarding removes it from the project.",
            cx,
        )
        .action(DialogAction::new("Cancel"))
        .action(DialogAction::new("Discard").primary(true))
    });

    match open.as_str() {
        "popover" => popover.update(cx, |this, cx| this.open(window, cx)),
        "dropdown" => menu.update(cx, |this, cx| this.open(window, cx)),
        "select" => select.update(cx, |this, cx| this.open(window, cx)),
        "dialog" => dialog.update(cx, |this, cx| this.open(window, cx)),
        _ => {}
    }

    cx.set_global(OverlaysState {
        popover,
        menu,
        select,
        dialog,
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
    let (popover, menu, select, dialog) = (
        state.popover.clone(),
        state.menu.clone(),
        state.select.clone(),
        state.dialog.clone(),
    );
    let theme = cx.theme();
    let (border, text, hover) = (theme.alpha_at(0.10), theme.gray_950, theme.alpha_at(0.10));

    let tooltips = div()
        .flex()
        .gap(px(12.))
        .child(
            div()
                .id("tooltip-default")
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
                .tooltip(|_window, cx| Tooltip::new("Play").key("Space").view(cx)),
        )
        .child(
            div()
                .id("tooltip-outline")
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
                .child("Bounce")
                .tooltip(|_window, cx| {
                    Tooltip::new("Bounce to disk")
                        .key("⌘")
                        .key("B")
                        .variant(TooltipVariant::Outline)
                        .view(cx)
                }),
        );

    let dialog_trigger = div()
        .id("dialog-trigger")
        .flex()
        .flex_none()
        .items_center()
        .h(px(32.))
        .px(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(SharedString::from("Discard take"))
        .on_click({
            let dialog = dialog.clone();
            move |_, window, cx| dialog.update(cx, |this, cx| this.open(window, cx))
        });

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .child(row("Tooltip", cx, tooltips))
        .child(row("Popover", cx, popover))
        .child(row("Dropdown menu", cx, menu))
        .child(row("Select", cx, select))
        .child(row("Dialog", cx, dialog_trigger))
        .child(dialog)
}
