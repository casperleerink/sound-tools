//! Foundation section: button, badge, chip, kbd, separator, label, loader, indicator, card,
//! empty state and alert, with every variant, size and state.

use gpui::{AnyElement, App, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use sound_ui::components::alert::{Alert, AlertVariant};
use sound_ui::components::badge::{Badge, BadgeSize, BadgeVariant};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::card::{Card, CardRow};
use sound_ui::components::chip::{Chip, ChipSize, ChipVariant};
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::kbd::Kbd;
use sound_ui::components::label::{Label, LabelVariant};
use sound_ui::components::loader::{Loader, LoaderSize};
use sound_ui::components::separator::Separator;
use sound_ui::{ActiveTheme, typography};

/// One component: a heading and its labelled rows.
fn block(
    title: &'static str,
    cx: &App,
    rows: impl IntoIterator<Item = AnyElement>,
) -> impl IntoElement {
    let muted = cx.theme().gray_700;
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(title),
        )
        .children(rows)
}

/// One labelled row of samples.
fn row(
    label: impl Into<SharedString>,
    cx: &App,
    items: impl IntoIterator<Item = AnyElement>,
) -> AnyElement {
    let muted = cx.theme().gray_600;
    div()
        .flex()
        .items_center()
        .gap(px(24.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_size(px(12.))
                .text_color(muted)
                .child(label.into()),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .children(items),
        )
        .into_any_element()
}

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let theme = cx.theme();
    let (lavender, green, red, peach) = (theme.lavender, theme.green, theme.red, theme.peach);

    // A persistent focus handle so the focus ring is live in the gallery.
    let focus = window
        .use_keyed_state("foundation-focus", cx, |window, cx| {
            let handle = cx.focus_handle();
            window.focus(&handle);
            handle
        })
        .read(cx)
        .clone();

    let sizes = [
        ("xs", ButtonSize::Xs),
        ("sm", ButtonSize::Sm),
        ("md", ButtonSize::Md),
        ("lg", ButtonSize::Lg),
    ];

    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(block(
            "Button",
            cx,
            [
                row(
                    "variants",
                    cx,
                    [
                        Button::new("b-primary", "Primary").into_any_element(),
                        Button::new("b-subtle", "Subtle")
                            .variant(ButtonVariant::Subtle)
                            .into_any_element(),
                        Button::new("b-outline", "Outline")
                            .variant(ButtonVariant::Outline)
                            .into_any_element(),
                        Button::new("b-ghost", "Ghost")
                            .variant(ButtonVariant::Ghost)
                            .into_any_element(),
                    ],
                ),
                row(
                    "colour",
                    cx,
                    [
                        Button::new("b-solid", "Solid")
                            .variant(ButtonVariant::Solid(lavender))
                            .into_any_element(),
                        Button::new("b-csubtle", "Subtle")
                            .variant(ButtonVariant::SubtleColor(lavender))
                            .into_any_element(),
                        Button::new("b-coutline", "Outline")
                            .variant(ButtonVariant::OutlineColor(green))
                            .into_any_element(),
                        Button::new("b-cghost", "Ghost")
                            .variant(ButtonVariant::GhostColor(red))
                            .into_any_element(),
                    ],
                ),
                row(
                    "sizes",
                    cx,
                    sizes.iter().enumerate().map(|(ix, (name, size))| {
                        Button::new(("b-size", ix), *name)
                            .variant(ButtonVariant::Subtle)
                            .size(*size)
                            .into_any_element()
                    }),
                ),
                row(
                    "with icon",
                    cx,
                    sizes.iter().enumerate().map(|(ix, (_, size))| {
                        Button::new(("b-icon", ix), "Play")
                            .icon("play")
                            .size(*size)
                            .into_any_element()
                    }),
                ),
                row(
                    "icon only",
                    cx,
                    sizes.iter().enumerate().map(|(ix, (_, size))| {
                        Button::icon_only(("b-iconly", ix), "pause")
                            .variant(ButtonVariant::Subtle)
                            .size(*size)
                            .into_any_element()
                    }),
                ),
                row(
                    "rounded",
                    cx,
                    [
                        Button::new("b-pill", "Pill")
                            .rounded(true)
                            .into_any_element(),
                        Button::new("b-pill-subtle", "Pill")
                            .variant(ButtonVariant::Subtle)
                            .rounded(true)
                            .into_any_element(),
                        Button::icon_only("b-pill-icon", "plus")
                            .variant(ButtonVariant::Outline)
                            .rounded(true)
                            .into_any_element(),
                    ],
                ),
                row(
                    "disabled",
                    cx,
                    [
                        Button::new("b-d-primary", "Primary")
                            .disabled(true)
                            .into_any_element(),
                        Button::new("b-d-subtle", "Subtle")
                            .variant(ButtonVariant::Subtle)
                            .disabled(true)
                            .into_any_element(),
                        Button::new("b-d-outline", "Outline")
                            .variant(ButtonVariant::Outline)
                            .disabled(true)
                            .into_any_element(),
                        Button::new("b-d-ghost", "Ghost")
                            .variant(ButtonVariant::Ghost)
                            .disabled(true)
                            .into_any_element(),
                    ],
                ),
                row(
                    "focus",
                    cx,
                    [Button::new("b-focus", "Focused")
                        .variant(ButtonVariant::Subtle)
                        .focus_handle(&focus)
                        .into_any_element()],
                ),
            ],
        ))
        .child(block(
            "Badge",
            cx,
            [
                row(
                    "variants",
                    cx,
                    [
                        Badge::new("Primary").into_any_element(),
                        Badge::new("Subtle")
                            .variant(BadgeVariant::Subtle)
                            .into_any_element(),
                        Badge::new("Ghost")
                            .variant(BadgeVariant::Ghost)
                            .into_any_element(),
                        Badge::new("Outline")
                            .variant(BadgeVariant::Outline)
                            .into_any_element(),
                    ],
                ),
                row(
                    "colour",
                    cx,
                    [
                        Badge::new("Solid")
                            .variant(BadgeVariant::Solid(green))
                            .into_any_element(),
                        Badge::new("Subtle")
                            .variant(BadgeVariant::SubtleColor(peach))
                            .into_any_element(),
                        Badge::new("Ghost")
                            .variant(BadgeVariant::GhostColor(red))
                            .into_any_element(),
                    ],
                ),
                row(
                    "sizes",
                    cx,
                    [
                        Badge::new("xs").size(BadgeSize::Xs).into_any_element(),
                        Badge::new("sm").size(BadgeSize::Sm).into_any_element(),
                        Badge::new("md").size(BadgeSize::Md).into_any_element(),
                        Badge::new("lg").size(BadgeSize::Lg).into_any_element(),
                    ],
                ),
                row(
                    "icon, pill",
                    cx,
                    [
                        Badge::new("Recording")
                            .icon("mic")
                            .variant(BadgeVariant::SubtleColor(red))
                            .into_any_element(),
                        Badge::new("Ready")
                            .icon("check")
                            .variant(BadgeVariant::SubtleColor(green))
                            .rounded(true)
                            .into_any_element(),
                        Badge::new("128")
                            .variant(BadgeVariant::Subtle)
                            .size(BadgeSize::Xs)
                            .rounded(true)
                            .font(typography::tabular())
                            .into_any_element(),
                    ],
                ),
            ],
        ))
        .child(block(
            "Chip",
            cx,
            [
                row(
                    "variants",
                    cx,
                    [
                        Chip::new("c-primary", "Primary").into_any_element(),
                        Chip::new("c-subtle", "Subtle")
                            .variant(ChipVariant::Subtle)
                            .into_any_element(),
                        Chip::new("c-solid", "Solid")
                            .variant(ChipVariant::Solid(lavender))
                            .into_any_element(),
                        Chip::new("c-csubtle", "Subtle")
                            .variant(ChipVariant::SubtleColor(lavender))
                            .into_any_element(),
                    ],
                ),
                row(
                    "sizes",
                    cx,
                    [
                        Chip::new("c-xs", "xs")
                            .size(ChipSize::Xs)
                            .variant(ChipVariant::Subtle)
                            .into_any_element(),
                        Chip::new("c-sm", "sm")
                            .size(ChipSize::Sm)
                            .variant(ChipVariant::Subtle)
                            .into_any_element(),
                        Chip::new("c-md", "md")
                            .size(ChipSize::Md)
                            .variant(ChipVariant::Subtle)
                            .into_any_element(),
                        Chip::new("c-lg", "lg")
                            .size(ChipSize::Lg)
                            .variant(ChipVariant::Subtle)
                            .into_any_element(),
                    ],
                ),
                row(
                    "removable",
                    cx,
                    [
                        Chip::new("c-rm", "Polyrhythm")
                            .on_remove(|_, _, _| {})
                            .into_any_element(),
                        Chip::new("c-rm-subtle", "Reverb")
                            .variant(ChipVariant::Subtle)
                            .on_remove(|_, _, _| {})
                            .into_any_element(),
                        Chip::new("c-rm-pill", "Sampler")
                            .variant(ChipVariant::SubtleColor(green))
                            .rounded(true)
                            .on_remove(|_, _, _| {})
                            .into_any_element(),
                    ],
                ),
            ],
        ))
        .child(block(
            "Kbd",
            cx,
            [row(
                "shortcuts",
                cx,
                [
                    Kbd::new("mod+k").into_any_element(),
                    Kbd::new("mod+shift+p").into_any_element(),
                    Kbd::new("alt+enter").into_any_element(),
                    Kbd::new("space").into_any_element(),
                ],
            )],
        ))
        .child(block(
            "Separator",
            cx,
            [
                row(
                    "horizontal",
                    cx,
                    [div()
                        .w(px(240.))
                        .child(Separator::horizontal())
                        .into_any_element()],
                ),
                row(
                    "vertical",
                    cx,
                    [div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .h(px(24.))
                        .child("Left")
                        .child(Separator::vertical())
                        .child("Right")
                        .into_any_element()],
                ),
            ],
        ))
        .child(block(
            "Label",
            cx,
            [row(
                "variants",
                cx,
                [
                    Label::new("Tempo").into_any_element(),
                    Label::new("Tempo is required")
                        .variant(LabelVariant::Error)
                        .into_any_element(),
                    Label::new("Tempo").disabled(true).into_any_element(),
                ],
            )],
        ))
        .child(block(
            "Loader",
            cx,
            [row(
                "sizes",
                cx,
                [
                    Loader::new("l-sm").size(LoaderSize::Sm).into_any_element(),
                    Loader::new("l-md").into_any_element(),
                    Loader::new("l-lg")
                        .size(LoaderSize::Lg)
                        .color(lavender)
                        .into_any_element(),
                ],
            )],
        ))
        .child(block(
            "Indicator",
            cx,
            [
                row(
                    "sizes",
                    cx,
                    [
                        Indicator::new("i-xs")
                            .size(IndicatorSize::Xs)
                            .into_any_element(),
                        Indicator::new("i-sm")
                            .size(IndicatorSize::Sm)
                            .into_any_element(),
                        Indicator::new("i-md").into_any_element(),
                    ],
                ),
                row(
                    "colour, ring",
                    cx,
                    [
                        Indicator::new("i-green")
                            .color(green)
                            .size(IndicatorSize::Sm)
                            .into_any_element(),
                        Indicator::new("i-red")
                            .color(red)
                            .size(IndicatorSize::Sm)
                            .ring(true)
                            .into_any_element(),
                        Indicator::new("i-peach")
                            .color(peach)
                            .ring(true)
                            .into_any_element(),
                    ],
                ),
                row(
                    "pulse",
                    cx,
                    [div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .child(
                            Indicator::new("i-pulse")
                                .color(lavender)
                                .size(IndicatorSize::Sm)
                                .pulse(true),
                        )
                        .child("Building Polyrhythm")
                        .into_any_element()],
                ),
            ],
        ))
        .child(block(
            "Card",
            cx,
            [row(
                "card",
                cx,
                [Card::new()
                    .title("Sampler")
                    .w(px(320.))
                    .child(
                        CardRow::new("Tempo").icon("music").child(
                            div()
                                .font(typography::tabular())
                                .text_size(px(14.))
                                .child("128 BPM"),
                        ),
                    )
                    .child(
                        CardRow::new("Output").icon("volume-2").child(
                            Badge::new("Built-in")
                                .size(BadgeSize::Xs)
                                .variant(BadgeVariant::Subtle),
                        ),
                    )
                    .into_any_element()],
            )],
        ))
        .child(block(
            "Empty state",
            cx,
            [row(
                "empty",
                cx,
                [EmptyState::new("No tools yet", "Ask the agent to build one.")
                    .action(Button::new("es-action", "New tool").icon("plus"))
                    .into_any_element()],
            )],
        ))
        .child(block(
            "Alert",
            cx,
            [row(
                "variants",
                cx,
                [
                    Alert::new("Save changes?", "Your edits will be written to the project.")
                        .into_any_element(),
                    Alert::new("Delete tool?", "This removes the tool and its presets.")
                        .variant(AlertVariant::Danger)
                        .action_label("Delete")
                        .into_any_element(),
                ],
            )],
        ))
}
