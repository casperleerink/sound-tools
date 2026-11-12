//! Foundation section: the tokens, button, text input, kbd, indicator, empty state and notice,
//! with every variant, size and state.

use gpui::{
    AnyElement, App, AppContext, Entity, FontWeight, IntoElement, ParentElement, SharedString,
    Styled, Window, div, px,
};
use sound_ui::components::button::{Button, ButtonVariant};
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::kbd::Kbd;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::select::Select;
use sound_ui::components::text_input::TextInput;
use sound_ui::components::toggle::Toggle;
use sound_ui::{ActiveTheme, ControlSize, TextVariant, Theme, Typography, radius};

/// One component: a heading and its labelled rows.
fn block(
    title: &'static str,
    cx: &App,
    rows: impl IntoIterator<Item = AnyElement>,
) -> impl IntoElement {
    let muted = cx.theme().text_muted();
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .text_variant(TextVariant::Label)
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
    let muted = cx.theme().text_muted();
    div()
        .flex()
        .items_center()
        .gap(px(24.))
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_variant(TextVariant::Label)
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
    let inputs = window.use_keyed_state("foundation-inputs", cx, |_, cx| {
        [
            cx.new(|cx| TextInput::new(cx).placeholder("Project name")),
            cx.new(|cx| TextInput::new(cx).placeholder("Disabled").disabled(true)),
            cx.new(|cx| {
                let mut input = TextInput::new(cx);
                input.set_text("Bass through the tape delay, bounced", cx);
                input
            }),
            cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("Ask the agent to build something")
                    .multi_line(4)
            }),
            cx.new(|cx| {
                let mut input = TextInput::new(cx).multi_line(4);
                input.set_text(
                    "Give these three voices independent rhythms and let me stretch each pattern by dragging it.\nKeep the kick as it is.",
                    cx,
                );
                input
            }),
            cx.new(|cx| {
                let mut input = TextInput::new(cx).multi_line(4);
                input.set_text(
                    "Make the bass follow the kick.\nKeep it in the low octave.\nSidechain the pad to it.\nAdd a short reverb on the snare only.\nBounce bars 5 to 8.\nThen name the bounce Loop B.",
                    cx,
                );
                input
            }),
        ]
    });
    let [text_md, text_disabled, long, empty, three_rows, full] = inputs.read(cx).clone();
    let sized_inputs = window
        .use_keyed_state("foundation-sized-inputs", cx, |_, cx| {
            ControlSize::ALL
                .map(|size| cx.new(|cx| TextInput::new(cx).placeholder("Input").size(size)))
        })
        .read(cx)
        .clone();
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
        ("sm", ControlSize::Sm),
        ("md", ControlSize::Md),
        ("lg", ControlSize::Lg),
    ];

    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(tokens(sized_inputs, cx))
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
                            .variant(ButtonVariant::Ghost)
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
                row("empty", cx, [boxed(240., text_md)]),
                row("disabled", cx, [boxed(240., text_disabled)]),
                row("long, scrolled", cx, [boxed(160., long)]),
                row("multi-line", cx, [boxed(360., empty)]),
                row("3 rows", cx, [boxed(360., three_rows)]),
                row("full, scrolled", cx, [boxed(360., full)]),
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
                        .text_variant(TextVariant::Label)
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
                    Notice::new("n-error", "instance song/bass-2 does not exist")
                        .on_dismiss(|_, _, _| {})
                        .into_any_element(),
                    Notice::new(
                        "n-warning",
                        "2 files are not live. Ask the agent to fix them.",
                    )
                    .tone(NoticeTone::Warning)
                    .into_any_element(),
                    Notice::new("n-progress", "Exporting song.wav")
                        .tone(NoticeTone::Info)
                        .progress(0.4)
                        .w(px(240.))
                        .into_any_element(),
                    Notice::new("n-action", "Sound Tools 0.2.0 is ready")
                        .tone(NoticeTone::Info)
                        .action(Button::new("n-restart", "Restart"))
                        .into_any_element(),
                    Notice::new(
                        "n-long",
                        "instance song/warm-pad/verse-b: notes[3].start must be less than \
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

/// The tokens a view names: the colour roles and the hues with what they are for, the type
/// variants, the corners, and the three control sizes with every control at each.
fn tokens(sized_inputs: [Entity<TextInput>; 3], cx: &App) -> impl IntoElement {
    let theme: &Theme = cx.theme();
    let (window, outline) = (theme.background(), theme.border());
    let (label, fill) = (theme.text_secondary(), theme.fill_strong());
    let swatch = |name: &'static str, color: gpui::Hsla, meaning: &'static str| {
        div()
            .w(px(112.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .h(px(40.))
                    .rounded(radius::MD)
                    .bg(color)
                    .border_1()
                    .border_color(outline),
            )
            .child(div().text_variant(TextVariant::Label).child(name))
            .child(
                div()
                    .text_variant(TextVariant::Label)
                    .text_color(label)
                    .child(meaning),
            )
            .into_any_element()
    };
    // By the names a tool of a project gives them, so a name that stops resolving shows here.
    let roles = [
        ("background", "the window"),
        ("surface", "a card, a menu"),
        ("well", "a display in a card"),
        ("fill", "a control at rest"),
        ("fill-strong", "under the pointer"),
        ("selected", "the chosen row"),
        ("border", "a hairline"),
        ("border-strong", "an outline"),
        ("text", "text, a value"),
        ("text-secondary", "a label"),
        ("text-muted", "a hint, off"),
        ("text-inverse", "on a bright fill"),
    ]
    .map(|(name, meaning)| swatch(name, theme.named(name).unwrap_or(theme.red), meaning));
    let hues = [
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
    ]
    .map(|(name, color, meaning)| swatch(name, color, meaning));
    let wrap = |items: Vec<AnyElement>| {
        div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .bg(window)
            .children(items)
            .into_any_element()
    };
    let variants = TextVariant::ALL.map(|variant| {
        let name = format!("{variant:?}").to_lowercase();
        let sample = format!("The quiet rule 120.00 · {}", f32::from(variant.size()));
        row(
            name,
            cx,
            [div().text_variant(variant).child(sample).into_any_element()],
        )
    });
    let corner = |name: &'static str, radius: Option<gpui::Pixels>| {
        let shape = div().size(px(40.)).bg(fill);
        let shape = match radius {
            Some(radius) => shape.rounded(radius),
            None => shape.rounded_full(),
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .text_variant(TextVariant::Label)
            .text_color(label)
            .child(shape)
            .child(name)
            .into_any_element()
    };
    let options = [("low", "Low"), ("high", "High")];
    let sizes = ControlSize::ALL.into_iter().zip(sized_inputs).enumerate();
    let controls = sizes.map(|(ix, (size, input))| {
        let name = format!("{size:?} {}", size.height()).to_lowercase();
        row(
            name,
            cx,
            [
                Button::new(("size-button", ix), "Button")
                    .variant(ButtonVariant::Subtle)
                    .size(size)
                    .into_any_element(),
                Toggle::new(("size-toggle", ix), "Toggle", true)
                    .size(size)
                    .into_any_element(),
                SegmentedControl::new(("size-segments", ix), "low")
                    .options(options)
                    .size(size)
                    .into_any_element(),
                Select::new(("size-select", ix), "low")
                    .entries(vec![MenuEntry::Group(MenuGroup::new().items(
                        options.map(|(value, label)| MenuItem::new(value, label)),
                    ))])
                    .size(size)
                    .into_any_element(),
                div().w(px(120.)).child(input).into_any_element(),
            ],
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(block(
            "Colour",
            cx,
            [
                row("roles", cx, [wrap(roles.into())]),
                row("hues", cx, [wrap(hues.into())]),
            ],
        ))
        .child(block("Type", cx, variants))
        .child(block(
            "Corner",
            cx,
            [row(
                "radius",
                cx,
                [
                    corner("sm 4", Some(radius::SM)),
                    corner("md 6", Some(radius::MD)),
                    corner("lg 8", Some(radius::LG)),
                    corner("full", None),
                ],
            )],
        ))
        .child(block("Control size", cx, controls))
}
