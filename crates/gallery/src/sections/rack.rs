//! Rack section: every control of a device card and the mixer strip, in each of its states, as
//! in `docs/reference/m3-step-0/mockups/components.png`: knob, volume, meter and gain
//! reduction, toggle, segmented control and select, and device cards with their header and a
//! display. The focus section beside it shows the focus ring of each, which one window can
//! show only one at a time.
//!
//! Every sample is live: its value is kept here and a drag, a key or a click changes it.

use gpui::{
    AnyElement, App, AppContext, Entity, FontWeight, IntoElement, ParentElement, Point,
    SharedString, Styled, Window, div, point, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::{Cell, ROW_HEIGHT};
use sound_ui::components::device_card::{Column, DeviceCard};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem, Trigger};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::meter::{GainReduction, Level, Meter};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::toggle::Toggle;
use sound_ui::components::volume::Volume;

/// A value of a sample, and where it was at the press of a drag, for escape.
#[derive(Clone, Copy)]
struct Live<V> {
    value: V,
    origin: Option<V>,
}

impl<V: Copy> Live<V> {
    fn new(value: V) -> Self {
        Self {
            value,
            origin: None,
        }
    }

    /// What every owner of a control does with a change: follow a drag, put the value back on
    /// escape, take a key step or a reset.
    fn follow(&mut self, change: ValueChange<V>) {
        match change {
            ValueChange::Drag(next) => {
                self.origin.get_or_insert(self.value);
                self.value = next;
            }
            ValueChange::DragEnd => self.origin = None,
            ValueChange::DragCancel => {
                if let Some(origin) = self.origin.take() {
                    self.value = origin;
                }
            }
            ValueChange::Set(next) => self.value = next,
        }
    }
}

/// The values of the samples. Built once, kept in element state.
pub(crate) struct RackState {
    knobs: [Live<f32>; 5],
    volumes: [Live<f32>; 5],
    levels: [Level; 5],
    toggles: [bool; 6],
    segment: SharedString,
    select: Entity<DropdownMenu>,
    pickers: [Entity<DropdownMenu>; 5],
    filter: [Live<f32>; 4],
    filter_power: [bool; 2],
    envelope: [Live<f32>; 4],
    reverb: [Live<f32>; 2],
    reverb_expanded: bool,
}

fn picker(name: &'static str, cx: &mut App) -> Entity<DropdownMenu> {
    let items = ["Synth", "Filter", "Compressor", "EQ", "Reverb"]
        .map(|device| MenuItem::new(device.to_lowercase(), device));
    cx.new(|cx| {
        DropdownMenu::new(
            name,
            vec![MenuEntry::Group(MenuGroup::new().items(items))],
            cx,
        )
        .selected(name.to_lowercase())
        .trigger(Trigger::Ghost)
        .width(200.)
    })
}

/// The shape of an EQ band, the list a select is for.
fn select(cx: &mut App) -> Entity<DropdownMenu> {
    let shapes = [
        "Bell",
        "Low shelf",
        "High shelf",
        "Low cut",
        "High cut",
        "Notch",
    ]
    .map(|shape| MenuItem::new(shape.to_lowercase(), shape));
    cx.new(|cx| {
        DropdownMenu::new(
            "Shape",
            vec![MenuEntry::Group(MenuGroup::new().items(shapes))],
            cx,
        )
        .selected("bell")
        .trigger(Trigger::Select)
        .width(160.)
    })
}

/// Level in dBFS, the same on both sides, with the peak line a little above.
fn level(db: f32, clipped: bool) -> Level {
    Level {
        now: [db, db - 1.5],
        peak: [db + 2., db + 1.],
        clipped,
    }
}

impl RackState {
    pub(crate) fn new(cx: &mut App) -> Self {
        let select = select(cx);
        Self {
            knobs: [480., -0.3, 0., 1., 480.].map(Live::new),
            volumes: [-3.5, 0., 0., 2., -3.5].map(Live::new),
            levels: [
                level(-24., false),
                level(-10., false),
                level(-2., false),
                level(1., true),
                Level::SILENT,
            ],
            toggles: [false, true, false, true, false, true],
            segment: "low".into(),
            select,
            pickers: [
                picker("Synth", cx),
                picker("Filter", cx),
                picker("Filter", cx),
                picker("Reverb", cx),
                picker("Gain", cx),
            ],
            filter: [1_200., 0.7, 0., 1.].map(Live::new),
            filter_power: [true, false],
            envelope: [0.005, 0.35, 0.25, 0.12].map(Live::new),
            reverb: [0.02, 2.4].map(Live::new),
            reverb_expanded: true,
        }
    }
}

fn state(window: &mut Window, cx: &mut App) -> Entity<RackState> {
    window.use_keyed_state("rack-state", cx, |_, cx| RackState::new(cx))
}

/// A callback that changes the state and draws again.
fn update<E: 'static>(
    state: &Entity<RackState>,
    f: impl Fn(&mut RackState, E) + 'static,
) -> impl Fn(E, &mut Window, &mut App) + 'static {
    let state = state.clone();
    move |event, _, cx| {
        state.update(cx, |state, cx| {
            f(state, event);
            cx.notify();
        })
    }
}

/// A click that changes the state and draws again.
fn click(
    state: &Entity<RackState>,
    f: impl Fn(&mut RackState) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let state = state.clone();
    move |_, _, cx| {
        state.update(cx, |state, cx| {
            f(state);
            cx.notify();
        })
    }
}

/// One component: a title and its samples.
fn block(
    title: &'static str,
    cx: &App,
    samples: impl IntoIterator<Item = AnyElement>,
) -> AnyElement {
    let text = cx.theme().gray_950;
    div()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .child(title),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(px(24.))
                .children(samples),
        )
        .into_any_element()
}

/// A sample with the name of its state under it.
fn sample(state: &'static str, cx: &App, element: impl IntoElement) -> AnyElement {
    let muted = cx.theme().gray_700;
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .child(element)
        .child(div().text_size(px(12.)).text_color(muted).child(state))
        .into_any_element()
}

fn hertz(value: f32) -> String {
    match value < 1_000. {
        true => format!("{} Hz", short(value)),
        false => format!("{} kHz", short(value / 1_000.)),
    }
}

fn percent(value: f32) -> String {
    format!("{}%", short(value * 100.))
}

fn seconds(value: f32) -> String {
    match value < 1. {
        true => format!("{} ms", short(value * 1_000.)),
        false => format!("{} s", short(value)),
    }
}

fn pan(value: f32) -> String {
    let side = if value < 0. { "L" } else { "R" };
    match value == 0. {
        true => "C".into(),
        false => format!("{}{side}", short(value.abs() * 100.)),
    }
}

const CUTOFF: KnobRange = KnobRange::logarithmic(20., 20_000.);
const PART: KnobRange = KnobRange::linear(0., 1.);

fn knobs(state: &Entity<RackState>, cx: &App) -> AnyElement {
    let values = state.read(cx).knobs.map(|live| live.value);
    let knob = |index: usize, id: &'static str| {
        Knob::new(id)
            .value(values[index])
            .on_change(update(state, move |s, change| {
                s.knobs[index].follow(change)
            }))
    };
    block(
        "Knob",
        cx,
        [
            sample(
                "rest",
                cx,
                knob(0, "cutoff")
                    .range(CUTOFF)
                    .default_value(2_000.)
                    .label("Cutoff")
                    .readout(hertz(values[0])),
            ),
            sample(
                "bipolar",
                cx,
                knob(1, "pan")
                    .range(KnobRange::linear(-1., 1.))
                    .bipolar(true)
                    .default_value(0.)
                    .label("Pan")
                    .readout(pan(values[1])),
            ),
            sample(
                "start",
                cx,
                knob(2, "drive")
                    .range(KnobRange::linear(0., 24.))
                    .label("Drive")
                    .readout(format!("{} dB", short(values[2]))),
            ),
            sample(
                "end",
                cx,
                knob(3, "mix")
                    .range(PART)
                    .label("Mix")
                    .readout(percent(values[3])),
            ),
            sample(
                "disabled",
                cx,
                Knob::new("rate")
                    .range(KnobRange::logarithmic(0.1, 20.))
                    .value(2.)
                    .label("Rate")
                    .readout("2 Hz")
                    .disabled(true),
            ),
        ],
    )
}

fn volumes(state: &Entity<RackState>, cx: &App) -> AnyElement {
    let rack = state.read(cx);
    let (values, levels) = (rack.volumes.map(|live| live.value), rack.levels);
    let volume = |index: usize| {
        Volume::new(("volume", index), values[index])
            .level(levels[index])
            .on_change(update(state, move |s, change| {
                s.volumes[index].follow(change)
            }))
            .on_clear_clip({
                let state = state.clone();
                move |_, cx| {
                    state.update(cx, |s, cx| {
                        s.levels[index].clipped = false;
                        cx.notify();
                    })
                }
            })
    };
    block(
        "Volume: the fader thumb on the meter",
        cx,
        [
            sample("quiet", cx, volume(0)),
            sample("normal", cx, volume(1)),
            sample("hot", cx, volume(2)),
            sample("clipped", cx, volume(3)),
            sample("silent", cx, volume(4)),
            sample(
                "disabled",
                cx,
                Volume::new("volume-off", -6.).disabled(true),
            ),
        ],
    )
}

/// The master meter of the transport pill: the same colours, lying down.
fn master_meter(state: &Entity<RackState>, cx: &App) -> AnyElement {
    let levels = state.read(cx).levels;
    let meter = |index: usize| {
        let state = state.clone();
        Meter::new(("master", index), levels[index])
            .horizontal()
            .on_clear_clip(move |_, cx| {
                state.update(cx, |s, cx| {
                    s.levels[index].clipped = false;
                    cx.notify();
                })
            })
    };
    block(
        "Master meter",
        cx,
        [
            sample("quiet", cx, meter(0)),
            sample("normal", cx, meter(1)),
            sample("hot", cx, meter(2)),
            sample("clipped", cx, meter(3)),
            sample("silent", cx, meter(4)),
        ],
    )
}

fn gain_reduction(cx: &App) -> AnyElement {
    block(
        "Gain reduction",
        cx,
        [0., 6.8, 18.].map(|db: f32| {
            let caption: &'static str = match db {
                0. => "none",
                18. => "18 dB",
                _ => "6.8 dB",
            };
            sample(caption, cx, GainReduction::new(db))
        }),
    )
}

fn choices(state: &Entity<RackState>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (peach, yellow) = (theme.peach, theme.yellow);
    let rack = state.read(cx);
    let (toggles, segment, select) = (rack.toggles, rack.segment.clone(), rack.select.clone());
    let toggle = |index: usize, label: &'static str| {
        Toggle::new(("toggle", index), label, toggles[index])
            .on_change(update(state, move |s, on| s.toggles[index] = on))
    };
    let segments = [
        ("low", "Low"),
        ("band", "Band"),
        ("high", "High"),
        ("notch", "Notch"),
    ];
    block(
        "Toggle, segmented control and select",
        cx,
        [
            sample("mute", cx, toggle(0, "M").color(peach)),
            sample("mute on", cx, toggle(1, "M").color(peach)),
            sample("solo", cx, toggle(2, "S").color(yellow)),
            sample("solo on", cx, toggle(3, "S").color(yellow)),
            sample("off", cx, toggle(4, "Freeze")),
            sample("on", cx, toggle(5, "Freeze")),
            sample(
                "disabled",
                cx,
                Toggle::new("toggle-off", "M", false).disabled(true),
            ),
            sample(
                "segmented",
                cx,
                SegmentedControl::new("type", segment.clone())
                    .options(segments)
                    .on_change(update(state, |s, value| s.segment = value)),
            ),
            sample(
                "disabled",
                cx,
                SegmentedControl::new("type-off", segment)
                    .options(segments)
                    .disabled(true),
            ),
            sample("select", cx, select),
        ],
    )
}

/// A low-pass response through the handle at the cutoff: flat, a bump of resonance, then a
/// steep fall. For the gallery only; the Filter draws its own.
fn low_pass(cutoff: f32, resonance: f32) -> Vec<Point<f32>> {
    let (x_cut, flat) = (CUTOFF.position(cutoff), 0.45);
    (0..=96)
        .map(|step| {
            let x = step as f32 / 96.;
            let y = if x <= x_cut {
                let bump = (-((x - x_cut) / 0.07).powi(2)).exp();
                flat + (resonance - flat) * bump
            } else {
                (resonance - (x - x_cut) * 5.).max(0.08)
            };
            point(x, y)
        })
        .collect()
}

/// The places across of 100 Hz, 1 kHz and 10 kHz.
fn decades() -> Vec<f32> {
    [100., 1_000., 10_000.]
        .map(|hz| CUTOFF.position(hz))
        .to_vec()
}

fn filter_card(state: &Entity<RackState>, index: usize, id: &'static str, cx: &App) -> DeviceCard {
    let rack = state.read(cx);
    let [cutoff, resonance, drive, mix] = rack.filter.map(|live| live.value);
    let (on, title) = (rack.filter_power[index], rack.pickers[1 + index].clone());
    let segment = rack.segment.clone();
    let knob = |which: usize, id: &'static str| {
        Knob::new(id)
            .value(rack.filter[which].value)
            .on_change(update(state, move |s, change| {
                s.filter[which].follow(change)
            }))
    };
    let handle = Handle::new(
        "filter-handle",
        Axis::new(CUTOFF, cutoff, 2_000.),
        Axis::new(PART, resonance, 0.5),
    )
    .on_change(update(state, |s, change: ValueChange<Point<f32>>| {
        s.filter[0].follow(x_of(change));
        s.filter[1].follow(y_of(change));
    }));
    let display = Display::new(("filter-display", index), 200.)
        .curve(low_pass(cutoff, resonance))
        .grid(decades(), vec![0.45])
        .handle(handle)
        .caption("100 · 1k · 10k")
        .child(
            SegmentedControl::new(("filter-type", index), segment)
                .options([
                    ("low", "Low"),
                    ("band", "Band"),
                    ("high", "High"),
                    ("notch", "Notch"),
                ])
                .on_change(update(state, |s, value| s.segment = value)),
        );
    DeviceCard::new(id, div().ml(px(-8.)).child(title))
        .expand(false, |_, _, _| {})
        .power(
            on,
            click(state, move |s| {
                s.filter_power[index] = !s.filter_power[index]
            }),
        )
        .close(|_, _, _| {})
        .display(display)
        .column(
            Column::new()
                .top(
                    knob(0, "filter-cutoff")
                        .range(CUTOFF)
                        .label("Cutoff")
                        .readout(hertz(cutoff)),
                )
                .bottom(
                    knob(2, "filter-drive")
                        .range(KnobRange::linear(0., 24.))
                        .label("Drive")
                        .readout(format!("{} dB", short(drive))),
                ),
        )
        .column(
            Column::new()
                .top(
                    knob(1, "filter-resonance")
                        .range(PART)
                        .label("Resonance")
                        .readout(percent(resonance)),
                )
                .bottom(
                    knob(3, "filter-mix")
                        .range(PART)
                        .label("Mix")
                        .readout(percent(mix)),
                ),
        )
}

fn synth_card(state: &Entity<RackState>, cx: &App) -> DeviceCard {
    let rack = state.read(cx);
    let [attack, decay, sustain, release] = rack.envelope.map(|live| live.value);
    let title = rack.pickers[0].clone();
    // The envelope over 1.6 s: attack, decay to the sustain, a hold, and the release.
    const SPAN: f32 = 1.6;
    const HOLD: f32 = 0.3;
    let across = |seconds: f32| seconds / SPAN;
    let time = |from: f32| KnobRange::linear(-from, SPAN - from);
    let curve = [
        point(0., 0.),
        point(across(attack), 1.),
        point(across(attack + decay), sustain),
        point(across(attack + decay + HOLD), sustain),
        point(across(attack + decay + HOLD + release), 0.),
    ];
    let peak = Handle::new(
        "attack",
        Axis::new(time(0.), attack, 0.005),
        Axis::fixed(1.),
    )
    .on_change(update(state, |s, change: ValueChange<Point<f32>>| {
        s.envelope[0].follow(x_of(change).map(|x| x.max(0.001)));
    }));
    let corner = Handle::new(
        "decay",
        Axis::new(time(attack), decay, 0.35),
        Axis::new(PART, sustain, 0.25),
    )
    .on_change(update(state, |s, change: ValueChange<Point<f32>>| {
        s.envelope[1].follow(x_of(change).map(|x| x.max(0.001)));
        s.envelope[2].follow(y_of(change));
    }));
    let end = Handle::new(
        "release",
        Axis::new(time(attack + decay + HOLD), release, 0.12),
        Axis::fixed(0.),
    )
    .on_change(update(state, |s, change: ValueChange<Point<f32>>| {
        s.envelope[3].follow(x_of(change).map(|x| x.max(0.001)));
    }));
    let caption = format!(
        "A {} · D {} · S {} · R {}",
        seconds(attack),
        seconds(decay),
        percent(sustain),
        seconds(release)
    );
    let display = Display::new("synth-display", 200.)
        .curve(curve)
        .handle(peak)
        .handle(corner)
        .handle(end)
        .caption(caption)
        .child(
            div().ml_auto().child(
                SegmentedControl::new("waveform", "square")
                    .options([("saw", "Saw"), ("square", "Square")]),
            ),
        );
    DeviceCard::new("synth", div().ml(px(-8.)).child(title))
        .expand(false, |_, _, _| {})
        .display(display)
        .column(
            Column::new()
                .top(
                    Knob::new("synth-cutoff")
                        .range(CUTOFF)
                        .value(480.)
                        .label("Cutoff")
                        .readout("480 Hz"),
                )
                .bottom(
                    Knob::new("synth-gain")
                        .value(0.15)
                        .label("Gain")
                        .readout("15%"),
                ),
        )
        .column(
            Column::new().top(
                Knob::new("synth-resonance")
                    .value(0.4)
                    .label("Resonance")
                    .readout("40%"),
            ),
        )
}

/// The sideways value of a change of a handle.
fn x_of(change: ValueChange<Point<f32>>) -> ValueChange<f32> {
    map(change, |point| point.x)
}

fn y_of(change: ValueChange<Point<f32>>) -> ValueChange<f32> {
    map(change, |point| point.y)
}

fn map(change: ValueChange<Point<f32>>, f: fn(Point<f32>) -> f32) -> ValueChange<f32> {
    match change {
        ValueChange::Drag(point) => ValueChange::Drag(f(point)),
        ValueChange::Set(point) => ValueChange::Set(f(point)),
        ValueChange::DragEnd => ValueChange::DragEnd,
        ValueChange::DragCancel => ValueChange::DragCancel,
    }
}

trait MapValue {
    fn map(self, f: impl Fn(f32) -> f32) -> Self;
}

impl MapValue for ValueChange<f32> {
    fn map(self, f: impl Fn(f32) -> f32) -> Self {
        match self {
            ValueChange::Drag(value) => ValueChange::Drag(f(value)),
            ValueChange::Set(value) => ValueChange::Set(f(value)),
            other => other,
        }
    }
}

fn reverb_card(state: &Entity<RackState>, cx: &App) -> DeviceCard {
    let rack = state.read(cx);
    let [pre_delay, decay] = rack.reverb.map(|live| live.value);
    let (title, expanded) = (rack.pickers[3].clone(), rack.reverb_expanded);
    const SPAN: f32 = 4.;
    let time = KnobRange::linear(0., SPAN);
    let curve = [point(pre_delay / SPAN, 1.), point(decay / SPAN, 0.)];
    let start = Handle::new(
        "pre-delay",
        Axis::new(time, pre_delay, 0.02),
        Axis::fixed(1.),
    )
    .hollow(true)
    .on_change(update(state, |s, change: ValueChange<Point<f32>>| {
        s.reverb[0].follow(x_of(change));
    }));
    let end = Handle::new("reverb-decay", Axis::new(time, decay, 2.), Axis::fixed(0.)).on_change(
        update(state, |s, change: ValueChange<Point<f32>>| {
            s.reverb[1].follow(x_of(change).map(|x| x.max(0.1)));
        }),
    );
    let display = Display::new("reverb-display", 200.)
        .curve(curve)
        .handle(start)
        .handle(end)
        .caption(format!(
            "Pre-delay {} · Decay {}",
            seconds(pre_delay),
            seconds(decay)
        ));
    let knob = |id: &'static str, label: &'static str, value: f32| {
        Knob::new(id)
            .value(value)
            .label(label)
            .readout(percent(value))
    };
    DeviceCard::new("reverb", div().ml(px(-8.)).child(title))
        .expand(
            expanded,
            click(state, |s| s.reverb_expanded = !s.reverb_expanded),
        )
        .power(true, |_, _, _| {})
        .close(|_, _, _| {})
        .display(display)
        .column(
            Column::new()
                .top(knob("size", "Size", 0.6))
                .bottom(knob("width", "Width", 1.)),
        )
        .column(
            Column::new()
                .top(knob("damping", "Damping", 0.5))
                .bottom(knob("reverb-mix", "Mix", 0.25)),
        )
        .hidden_column(
            Column::new()
                .top(
                    Knob::new("low-cut")
                        .range(CUTOFF)
                        .value(200.)
                        .label("Low cut")
                        .readout("200 Hz"),
                )
                .bottom(knob("diffusion", "Diffusion", 0.7)),
        )
        .hidden_column(
            Column::new().top(
                Knob::new("high-cut")
                    .range(CUTOFF)
                    .value(8_000.)
                    .label("High cut")
                    .readout("8 kHz"),
            ),
        )
        .hidden_column(
            Column::new().top(Cell::new(Toggle::new("freeze", "Off", false)).label("Freeze")),
        )
}

fn plugin_card(cx: &App) -> DeviceCard {
    let muted = cx.theme().gray_800;
    DeviceCard::new("plugin", div().child("Surge XT"))
        .expand(false, |_, _, _| {})
        .power(true, |_, _, _| {})
        .close(|_, _, _| {})
        .w(px(200.))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(ROW_HEIGHT * 2.))
                .flex()
                .flex_col()
                .items_start()
                .child(
                    Button::new("open-window", "Open window")
                        .variant(ButtonVariant::Subtle)
                        .size(ButtonSize::Sm),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(126.))
                        .text_size(px(12.))
                        .line_height(px(14.))
                        .text_color(muted)
                        .child("CLAP · Surge Synth Team"),
                ),
        )
}

fn cards(state: &Entity<RackState>, cx: &App) -> AnyElement {
    block(
        "Device card, its header and a display",
        cx,
        [
            sample("instrument: expand", cx, synth_card(state, cx)),
            sample(
                "effect: expand, power, close",
                cx,
                filter_card(state, 0, "filter", cx),
            ),
            sample("effect off", cx, filter_card(state, 1, "filter-off", cx)),
            sample("expanded, a hollow handle", cx, reverb_card(state, cx)),
            sample("plugin", cx, plugin_card(cx)),
        ],
    )
}

pub fn section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let state = state(window, cx);
    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(knobs(&state, cx))
        .child(volumes(&state, cx))
        .child(master_meter(&state, cx))
        .child(gain_reduction(cx))
        .child(choices(&state, cx))
        .child(cards(&state, cx))
}

/// The values of the focus section.
struct FocusState {
    cutoff: Live<f32>,
    volume: Live<f32>,
    mute: bool,
    segment: SharedString,
    select: Entity<DropdownMenu>,
    on: bool,
}

/// The focus section: one of each control that the keyboard reaches, in tab order, so that the
/// snapshot can give each the focus in turn.
pub fn focus_section(window: &mut Window, cx: &mut App) -> impl IntoElement {
    let focus = window.use_keyed_state("focus-state", cx, |_, cx| FocusState {
        cutoff: Live::new(480.),
        volume: Live::new(-3.5),
        mute: false,
        segment: "low".into(),
        select: select(cx),
        on: true,
    });
    let change = |f: fn(&mut FocusState, ValueChange)| {
        let focus = focus.clone();
        move |change, _: &mut Window, cx: &mut App| {
            focus.update(cx, |state, cx| {
                f(state, change);
                cx.notify();
            })
        }
    };
    let state = focus.read(cx);
    let (cutoff, volume, mute) = (state.cutoff.value, state.volume.value, state.mute);
    let (segment, select, on) = (state.segment.clone(), state.select.clone(), state.on);
    let peach = cx.theme().peach;
    let toggle = {
        let focus = focus.clone();
        move |mute, _: &mut Window, cx: &mut App| {
            focus.update(cx, |state, cx| {
                state.mute = mute;
                cx.notify();
            })
        }
    };
    let pick = {
        let focus = focus.clone();
        move |segment, _: &mut Window, cx: &mut App| {
            focus.update(cx, |state, cx| {
                state.segment = segment;
                cx.notify();
            })
        }
    };
    let power = {
        let focus = focus.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            focus.update(cx, |state, cx| {
                state.on = !state.on;
                cx.notify();
            })
        }
    };
    block(
        "Focus from the keyboard, one at a time",
        cx,
        [
            sample(
                "knob",
                cx,
                Knob::new("focus-knob")
                    .range(CUTOFF)
                    .value(cutoff)
                    .label("Cutoff")
                    .readout(hertz(cutoff))
                    .on_change(change(|state, change| state.cutoff.follow(change))),
            ),
            sample(
                "volume",
                cx,
                Volume::new("focus-volume", volume)
                    .on_change(change(|state, change| state.volume.follow(change))),
            ),
            sample(
                "toggle",
                cx,
                Toggle::new("focus-toggle", "M", mute)
                    .color(peach)
                    .on_change(toggle),
            ),
            sample(
                "segmented",
                cx,
                SegmentedControl::new("focus-segments", segment)
                    .options([("low", "Low"), ("band", "Band"), ("high", "High")])
                    .on_change(pick),
            ),
            sample("select", cx, select),
            sample(
                "card header",
                cx,
                DeviceCard::new("focus-card", "Gain")
                    .power(on, power)
                    .column(
                        Column::new().top(
                            Knob::new("focus-gain")
                                .value(0.5)
                                .label("Gain")
                                .readout("0 dB"),
                        ),
                    ),
            ),
        ],
    )
}
