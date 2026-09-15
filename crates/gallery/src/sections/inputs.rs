//! Inputs section: text input, checkbox, switch, segmented control, tabs, slider, numeric input,
//! knob and meter, with every variant, size and state.

use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    ParentElement, SharedString, Styled, Window, div, px,
};
use sound_ui::components::checkbox::{Checkbox, CheckboxSize};
use sound_ui::components::knob::Knob;
use sound_ui::components::meter::Meter;
use sound_ui::components::numeric_input::NumericInput;
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::slider::Slider;
use sound_ui::components::switch::{Switch, SwitchSize};
use sound_ui::components::tabs::Tabs;
use sound_ui::components::text_input::{InputSize, TextInput};
use sound_ui::{ActiveTheme, typography};

/// Everything in this section that holds state. Built once, kept in element state.
struct InputsState {
    text_sm: Entity<TextInput>,
    text_md: Entity<TextInput>,
    text_lg: Entity<TextInput>,
    text_disabled: Entity<TextInput>,
    composer: Entity<TextInput>,
    slider: Entity<Slider>,
    slider_labelled: Entity<Slider>,
    slider_disabled: Entity<Slider>,
    tempo: Entity<NumericInput>,
    gain: Entity<NumericInput>,
    knob_small: Entity<Knob>,
    knob_large: Entity<Knob>,
    checkbox_focus: FocusHandle,
    switch_focus: FocusHandle,
    checked: bool,
    indeterminate: bool,
    switched: bool,
    segment: SharedString,
    tab: SharedString,
}

impl InputsState {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        let text_md = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Project name")
                .size(InputSize::Md)
        });
        let handle = text_md.read(cx).focus_handle(cx);
        window.focus(&handle);
        Self {
            text_sm: cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("Small")
                    .size(InputSize::Sm)
            }),
            text_md,
            text_lg: cx.new(|cx| TextInput::new(cx).placeholder("Large").size(InputSize::Lg)),
            text_disabled: cx.new(|cx| TextInput::new(cx).placeholder("Disabled").disabled(true)),
            composer: cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("Ask the agent to build something")
                    .size(InputSize::Md)
                    .lines(3)
            }),
            slider: cx.new(|cx| Slider::new(cx).value(0.35)),
            slider_labelled: cx.new(|cx| {
                Slider::new(cx)
                    .range(-60., 6.)
                    .step(0.5)
                    .decimals(1)
                    .value(-8.)
                    .unit(" dB")
                    .label("Output")
            }),
            slider_disabled: cx.new(|cx| Slider::new(cx).value(0.6).disabled(true)),
            tempo: cx.new(|cx| {
                NumericInput::new(cx)
                    .range(20., 300.)
                    .value(120.)
                    .unit(" bpm")
                    .width(88.)
            }),
            gain: cx.new(|cx| {
                NumericInput::new(cx)
                    .range(-60., 6.)
                    .step(0.1)
                    .decimals(1)
                    .value(-6.)
                    .unit(" dB")
                    .width(88.)
            }),
            knob_small: cx.new(|cx| Knob::new(cx).value(0.3).size(36.).label("Drive")),
            knob_large: cx.new(|cx| {
                Knob::new(cx)
                    .range(0., 100.)
                    .step(1.)
                    .decimals(0)
                    .value(64.)
                    .unit("%")
                    .size(56.)
                    .label("Mix")
            }),
            checkbox_focus: cx.focus_handle(),
            switch_focus: cx.focus_handle(),
            checked: true,
            indeterminate: false,
            switched: true,
            segment: "bars".into(),
            tab: "mixer".into(),
        }
    }
}

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
        .gap(px(24.))
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
        .items_start()
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
                .gap(px(24.))
                .children(items),
        )
        .into_any_element()
}

/// A fixed-width wrapper so full-width inputs do not stretch across the gallery.
fn boxed(width: f32, child: impl IntoElement) -> AnyElement {
    div().w(px(width)).flex_none().child(child).into_any_element()
}

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let state = window.use_keyed_state("inputs-state", cx, |window, cx| InputsState::new(window, cx));
    let s = state.read(cx);
    let (checked, indeterminate, switched) = (s.checked, s.indeterminate, s.switched);
    let (segment, tab) = (s.segment.clone(), s.tab.clone());
    let (checkbox_focus, switch_focus) = (s.checkbox_focus.clone(), s.switch_focus.clone());
    let (text_sm, text_md, text_lg, text_disabled, composer) = (
        s.text_sm.clone(),
        s.text_md.clone(),
        s.text_lg.clone(),
        s.text_disabled.clone(),
        s.composer.clone(),
    );
    let (slider, slider_labelled, slider_disabled) = (
        s.slider.clone(),
        s.slider_labelled.clone(),
        s.slider_disabled.clone(),
    );
    let (tempo, gain) = (s.tempo.clone(), s.gain.clone());
    let (knob_small, knob_large) = (s.knob_small.clone(), s.knob_large.clone());
    let muted = cx.theme().gray_600;

    let toggle_checked = {
        let state = state.clone();
        move |value: bool, _: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| {
                s.checked = value;
                s.indeterminate = false;
                cx.notify();
            });
        }
    };
    let toggle_indeterminate = {
        let state = state.clone();
        move |_: bool, _: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| {
                s.indeterminate = !s.indeterminate;
                cx.notify();
            });
        }
    };
    let toggle_switch = {
        let state = state.clone();
        move |value: bool, _: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| {
                s.switched = value;
                cx.notify();
            });
        }
    };
    let pick_segment = {
        let state = state.clone();
        move |value: SharedString, _: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| {
                s.segment = value;
                cx.notify();
            });
        }
    };
    let pick_tab = {
        let state = state.clone();
        move |value: SharedString, _: &mut Window, cx: &mut App| {
            state.update(cx, |s, cx| {
                s.tab = value;
                cx.notify();
            });
        }
    };

    let checkbox_sizes = [CheckboxSize::Sm, CheckboxSize::Md, CheckboxSize::Lg];
    let switch_sizes = [SwitchSize::Sm, SwitchSize::Md, SwitchSize::Lg];

    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(block(
            "Text input",
            cx,
            [
                row(
                    "sizes",
                    cx,
                    [
                        boxed(160., text_sm),
                        boxed(200., text_md),
                        boxed(240., text_lg),
                    ],
                ),
                row("disabled", cx, [boxed(200., text_disabled)]),
                row("composer", cx, [boxed(360., composer)]),
            ],
        ))
        .child(block(
            "Checkbox",
            cx,
            [
                row(
                    "states",
                    cx,
                    [
                        Checkbox::new("cb-live", checked)
                            .indeterminate(indeterminate)
                            .focus_handle(&checkbox_focus)
                            .on_change(toggle_checked.clone())
                            .into_any_element(),
                        Checkbox::new("cb-mixed", false)
                            .indeterminate(true)
                            .on_change(toggle_indeterminate)
                            .into_any_element(),
                        Checkbox::new("cb-off", false).into_any_element(),
                    ],
                ),
                row(
                    "sizes",
                    cx,
                    checkbox_sizes.into_iter().enumerate().map(|(ix, size)| {
                        Checkbox::new(("cb-size", ix), checked)
                            .size(size)
                            .on_change(toggle_checked.clone())
                            .into_any_element()
                    }),
                ),
                row(
                    "disabled",
                    cx,
                    [
                        Checkbox::new("cb-d-on", true)
                            .disabled(true)
                            .into_any_element(),
                        Checkbox::new("cb-d-off", false)
                            .disabled(true)
                            .into_any_element(),
                    ],
                ),
            ],
        ))
        .child(block(
            "Switch",
            cx,
            [
                row(
                    "sizes",
                    cx,
                    switch_sizes.into_iter().enumerate().map(|(ix, size)| {
                        Switch::new(("sw-size", ix), switched)
                            .size(size)
                            .on_change(toggle_switch.clone())
                            .into_any_element()
                    }),
                ),
                row(
                    "focus",
                    cx,
                    [Switch::new("sw-focus", switched)
                        .focus_handle(&switch_focus)
                        .on_change(toggle_switch.clone())
                        .into_any_element()],
                ),
                row(
                    "disabled",
                    cx,
                    [
                        Switch::new("sw-d-on", true).disabled(true).into_any_element(),
                        Switch::new("sw-d-off", false)
                            .disabled(true)
                            .into_any_element(),
                    ],
                ),
            ],
        ))
        .child(block(
            "Segmented control",
            cx,
            [
                row(
                    "default",
                    cx,
                    [SegmentedControl::new("seg", segment.clone())
                        .options([("bars", "Bars"), ("beats", "Beats"), ("time", "Time")])
                        .on_change(pick_segment)
                        .into_any_element()],
                ),
                row(
                    "disabled",
                    cx,
                    [SegmentedControl::new("seg-d", segment)
                        .options([("bars", "Bars"), ("beats", "Beats"), ("time", "Time")])
                        .disabled(true)
                        .into_any_element()],
                ),
            ],
        ))
        .child(block(
            "Tabs",
            cx,
            [row(
                "default",
                cx,
                [Tabs::new("tabs", tab)
                    .tabs([("mixer", "Mixer"), ("edit", "Edit"), ("graph", "Graph")])
                    .disabled_tab("render", "Render")
                    .on_change(pick_tab)
                    .into_any_element()],
            )],
        ))
        .child(block(
            "Slider",
            cx,
            [
                row("plain", cx, [slider.into_any_element()]),
                row("labelled", cx, [slider_labelled.into_any_element()]),
                row("disabled", cx, [slider_disabled.into_any_element()]),
            ],
        ))
        .child(block(
            "Numeric input",
            cx,
            [row(
                "drag or type",
                cx,
                [tempo.into_any_element(), gain.into_any_element()],
            )],
        ))
        .child(block(
            "Knob",
            cx,
            [row(
                "sizes",
                cx,
                [knob_small.into_any_element(), knob_large.into_any_element()],
            )],
        ))
        .child(block(
            "Meter",
            cx,
            [row(
                "levels",
                cx,
                [-60., -24., -12., -6., -1.].map(|level: f32| {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(Meter::new(level).peak(level + 2.))
                        .child(
                            div()
                                .font(typography::tabular())
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(format!("{level:.0}")),
                        )
                        .into_any_element()
                }),
            )],
        ))
}
