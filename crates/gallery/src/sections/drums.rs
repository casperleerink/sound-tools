//! Drums section: the pad in each of its states, and a Drum pad card made of pads, as in
//! `docs/mockups/drum-pad.png`. The samples show states, so they hold
//! still: the card of the Drum pad is where they are pressed.

use gpui::AppContext;
use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, Window, div, px};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{Column, DeviceCard};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem, Trigger};
use sound_ui::components::knob::{Knob, KnobRange};
use sound_ui::components::pad::{PAD_GAP, Pad, PadGlyph};
use sound_ui::components::toggle::Toggle;

use super::rack::{block, sample};

fn pads(cx: &App) -> AnyElement {
    let states = [
        ("rest", Pad::new("rest", "Kick")),
        ("selected", Pad::new("selected", "Kick").selected(true)),
        ("sounding", Pad::new("sounding", "Kick").sounding(1.)),
        ("fading", Pad::new("fading", "Kick").sounding(0.4)),
        (
            "sample",
            Pad::new("sample", "Shaker").glyph(Some(PadGlyph::Sample)),
        ),
        (
            "file missing",
            Pad::new("missing", "Shaker").glyph(Some(PadGlyph::Missing)),
        ),
        ("drop target", Pad::new("drop", "Tom 6").drop_target(true)),
        (
            "a long name",
            Pad::new("long", "shaker-loop-long").glyph(Some(PadGlyph::Sample)),
        ),
    ];
    block("Pad", cx, states.map(|(state, pad)| sample(state, cx, pad)))
}

/// The names of the kit, bottom row first, with the shaker of the mockup on pad 48.
const NAMES: [&str; 16] = [
    "Kick",
    "Rim",
    "Snare",
    "Clap",
    "Snare 2",
    "Tom 1",
    "Hat",
    "Tom 2",
    "Pedal hat",
    "Tom 3",
    "Open hat",
    "Tom 4",
    "Shaker",
    "Crash",
    "Tom 6",
    "Ride",
];

/// The grid of a Drum pad while it plays: the kick and the hat sounding, the hat selected.
fn grid() -> impl IntoElement {
    let rows = (0..4).rev().map(|row| {
        let pads = (0..4).map(move |column| {
            let index = row * 4 + column;
            let sounding = match index {
                0 => 0.8,
                6 => 0.5,
                _ => 0.,
            };
            let glyph = (index == 12).then_some(PadGlyph::Sample);
            Pad::new(("grid", index), NAMES[index])
                .selected(index == 6)
                .sounding(sounding)
                .glyph(glyph)
        });
        div()
            .flex()
            .gap(px(PAD_GAP))
            .children(pads.collect::<Vec<_>>())
    });
    div()
        .flex()
        .flex_col()
        .gap(px(PAD_GAP))
        .children(rows.collect::<Vec<_>>())
}

fn knob(id: &'static str, label: &'static str, value: f32, readout: &'static str) -> Knob {
    Knob::new(id)
        .range(KnobRange::linear(0., 1.))
        .value(value)
        .label(label)
        .readout(readout)
}

fn cards(cx: &mut App) -> AnyElement {
    let title = |text: &'static str| div().child(text);
    let columns = |card: DeviceCard| {
        card.display(grid())
            .column(
                Column::new()
                    .top(knob("volume", "Volume", 0.8, "0 dB"))
                    .bottom(knob("decay", "Decay", 0.43, "180 ms")),
            )
            .column(
                Column::new()
                    .top(knob("pitch", "Pitch", 0.5, "0 st").bipolar(true))
                    .bottom(knob("pan", "Pan", 0.5, "C").bipolar(true)),
            )
    };
    let card = columns(DeviceCard::new("drum-pad", title("Drum pad")).expand(false, |_, _, _| {}));
    let select = cx.new(|cx| {
        let items = ["Kick", "Hat"].map(|name| MenuItem::new(name, name));
        DropdownMenu::new(
            "Sound",
            vec![MenuEntry::Group(MenuGroup::new().items(items))],
            cx,
        )
        .trigger(Trigger::Select)
        .trigger_width(104.)
        .selected("Hat")
    });
    let expanded =
        columns(DeviceCard::new("drum-pad-expanded", title("Drum pad")).expand(true, |_, _, _| {}))
            .hidden_column(
                Column::new()
                    .top(
                        Cell::new(select)
                            .span(2)
                            .label("Sound")
                            .value("Synthesized"),
                    )
                    .bottom(Cell::new(Toggle::new("choke", "On", true)).label("Choke")),
            )
            .hidden_column(Column::new());
    block(
        "Drum pad",
        cx,
        [
            sample(
                "playing: Kick and Hat sound, Hat selected, 452 pt",
                cx,
                card,
            ),
            sample("expanded, 581 pt: Sound and Choke", cx, expanded),
        ],
    )
}

pub fn section(_: &mut Window, cx: &mut App) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(pads(cx))
        .child(cards(cx))
}
