//! Foundation section: the palette, button, text input, kbd, indicator, card, empty state and
//! notice, with every variant, size and state.

use gpui::{
    AnyElement, App, AppContext, FontWeight, IntoElement, ParentElement, SharedString, Styled,
    Window, div, px,
};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::card::Card;
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::kbd::Kbd;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::components::text_input::{InputSize, TextInput};
use sound_ui::{ActiveTheme, Theme};

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
    let muted = theme.gray_700;
    let inputs = window.use_keyed_state("foundation-inputs", cx, |_, cx| {
        [
            cx.new(|cx| TextInput::new(cx).placeholder("Small").size(InputSize::Sm)),
            cx.new(|cx| TextInput::new(cx).placeholder("Project name")),
            cx.new(|cx| TextInput::new(cx).placeholder("Disabled").disabled(true)),
            cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("Ask the agent to build something")
                    .lines(3)
            }),
        ]
    });
    let [text_sm, text_md, text_disabled, composer] = inputs.read(cx).clone();
    let boxed = |width: f32, input| {
        div()
            .w(px(width))
            .flex_none()
            .child(input)
            .into_any_element()
    };

    // A persistent focus handle so the focus ring is live in the gallery.
    let focus = window
        .use_keyed_state("foundation-focus", cx, |window, cx| {
            let handle = cx.focus_handle();
            window.focus(&handle, cx);
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
        .child(palette(cx))
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
                            .variant(ButtonVariant::OutlineColor(lavender))
                            .into_any_element(),
                        Button::new("b-cghost", "Ghost")
                            .variant(ButtonVariant::GhostColor(lavender))
                            .into_any_element(),
                    ],
                ),
                row(
                    "green",
                    cx,
                    [
                        Button::new("b-gsolid", "Solid")
                            .variant(ButtonVariant::Solid(green))
                            .into_any_element(),
                        Button::new("b-gsubtle", "Subtle")
                            .variant(ButtonVariant::SubtleColor(green))
                            .into_any_element(),
                        Button::new("b-goutline", "Outline")
                            .variant(ButtonVariant::OutlineColor(green))
                            .into_any_element(),
                        Button::new("b-gghost", "Ghost")
                            .variant(ButtonVariant::GhostColor(green))
                            .into_any_element(),
                    ],
                ),
                row(
                    "red",
                    cx,
                    [
                        Button::new("b-rsolid", "Solid")
                            .variant(ButtonVariant::Solid(red))
                            .into_any_element(),
                        Button::new("b-rsubtle", "Subtle")
                            .variant(ButtonVariant::SubtleColor(red))
                            .into_any_element(),
                        Button::new("b-routline", "Outline")
                            .variant(ButtonVariant::OutlineColor(red))
                            .into_any_element(),
                        Button::new("b-rghost", "Ghost")
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
            "Text input",
            cx,
            [
                row("sizes", cx, [boxed(160., text_sm), boxed(240., text_md)]),
                row("disabled", cx, [boxed(240., text_disabled)]),
                row("composer", cx, [boxed(360., composer)]),
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
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("The plain card of the rack until step 1b."),
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
                [
                    EmptyState::new("No tools yet", "Ask the agent to build one.")
                        .action(Button::new("es-action", "New tool").icon("plus"))
                        .into_any_element(),
                ],
            )],
        ))
        .child(block(
            "Notice",
            cx,
            [row(
                "tones",
                cx,
                [
                    Notice::new("n-error", "instance arrangement/bass-2 does not exist")
                        .on_dismiss(|_, _, _| {})
                        .into_any_element(),
                    Notice::new("n-warning", "2 files are not live, see problems.txt")
                        .tone(NoticeTone::Warning)
                        .into_any_element(),
                    Notice::new(
                        "n-long",
                        "instance arrangement/warm-pad/verse-b: notes[3].start must be less than \
                         the clip length 3840, not 5760. A note start counts from the start of \
                         its clip, not from the start of the project",
                    )
                    .w(px(400.))
                    .on_dismiss(|_, _, _| {})
                    .into_any_element(),
                ],
            )],
        ))
}

/// The palette: every token with its hex value, the greys on the window and the colours with
/// what they mean. See DESIGN.md, "Colour".
fn palette(cx: &App) -> impl IntoElement {
    let theme: &Theme = cx.theme();
    let (window, label) = (theme.gray_100, theme.gray_800);
    let swatch = |name: &'static str, color: gpui::Hsla, meaning: &'static str| {
        let hex = gpui::Rgba::from(color);
        let hex = format!(
            "#{:02x}{:02x}{:02x}",
            (hex.r * 255.).round() as u8,
            (hex.g * 255.).round() as u8,
            (hex.b * 255.).round() as u8
        );
        div()
            .w(px(96.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .h(px(40.))
                    .rounded(px(6.))
                    .bg(color)
                    .border_1()
                    .border_color(theme.alpha_at(0.06)),
            )
            .child(div().text_size(px(12.)).child(name))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(label)
                    .child(format!("{hex} {meaning}")),
            )
            .into_any_element()
    };
    let greys = [
        ("gray-50", theme.gray_50, "inset"),
        ("gray-100", theme.gray_100, "window"),
        ("gray-200", theme.gray_200, "card"),
        ("gray-300", theme.gray_300, "knob face"),
        ("gray-400", theme.gray_400, ""),
        ("gray-500", theme.gray_500, ""),
        ("gray-600", theme.gray_600, "off"),
        ("gray-700", theme.gray_700, "muted"),
        ("gray-800", theme.gray_800, "labels"),
        ("gray-900", theme.gray_900, ""),
        ("gray-950", theme.gray_950, "values"),
    ];
    let colours = [
        ("green", theme.green, "sound"),
        ("yellow", theme.yellow, "hot, solo"),
        ("peach", theme.peach, "warn, mute"),
        ("red", theme.red, "record, clip"),
        ("lavender", theme.lavender, "focus"),
        ("blue", theme.blue, "track"),
        ("sapphire", theme.sapphire, "track"),
        ("sky", theme.sky, "track"),
        ("teal", theme.teal, "track"),
        ("maroon", theme.maroon, "track"),
        ("mauve", theme.mauve, "track"),
        ("pink", theme.pink, "track"),
        ("rosewater", theme.rosewater, "track"),
        ("flamingo", theme.flamingo, "track"),
    ];
    let wrap = |items: Vec<AnyElement>| {
        div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .bg(window)
            .children(items)
            .into_any_element()
    };
    block(
        "Palette",
        cx,
        [
            row(
                "greys",
                cx,
                [wrap(greys.map(|(n, c, m)| swatch(n, c, m)).into())],
            ),
            row(
                "colours",
                cx,
                [wrap(colours.map(|(n, c, m)| swatch(n, c, m)).into())],
            ),
        ],
    )
}
