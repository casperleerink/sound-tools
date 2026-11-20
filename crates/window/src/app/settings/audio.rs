//! Audio: the output and the input device, the system's default first.

use rune::{El, Icon, col, el, icon, row, text};
use sound_core::DeviceChoice;

use super::{Act, MIC, Page, VOLUME, control};
use crate::app::State;
use crate::controls::Control;

/// The audio devices of this machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeviceList {
    pub(crate) outputs: Vec<DeviceChoice>,
    pub(crate) inputs: Vec<DeviceChoice>,
}

const DEFAULT: &str = "System default";

/// The rows of a list: its name, whether it is the chosen one, and what is said beside it.
fn rows(state: &State, devices: &DeviceList, output: bool) -> Vec<(String, bool, Option<String>)> {
    let (list, chosen) = match output {
        true => (&devices.outputs, &state.settings.chosen.output),
        false => (&devices.inputs, &state.settings.chosen.input),
    };
    let each = list
        .iter()
        .map(|device| (Some(device.id.as_str()), device.name.as_str()));
    let all = std::iter::once((None, DEFAULT)).chain(each);
    all.map(|(id, name)| {
        let picked = id == chosen.as_deref();
        // Beside the picked output, what plays when its name does not say. Only the output in
        // use is known: the engine reports no input.
        let playing = &state.settings.playing;
        let note = (output && picked && !playing.is_empty())
            .then(|| match (id, name == playing) {
                (None, _) => Some(playing.clone()),
                (Some(_), true) => None,
                (Some(_), false) => Some(format!("Not available. Playing on {playing}")),
            })
            .flatten();
        (name.to_string(), picked, note)
    })
    .collect()
}

fn key(output: bool, row: usize) -> String {
    let list = if output { "output" } else { "input" };
    format!("settings-{list}-{row}")
}

pub(super) fn view(state: &State, page: &Page) -> El {
    let theme = &state.theme;
    let muted = theme.color("text-muted");
    let intro =
        text("System default follows your computer. Picking a device starts the app again.")
            .font(theme.text("body"))
            .color(muted)
            .wrap();
    let Some(devices) = &page.devices else {
        return col().gap(theme.space(4)).child(intro).child(
            text("Looking for devices…")
                .font(theme.text("body"))
                .color(muted),
        );
    };
    let card = |output: bool, title: &str, glyph: &'static Icon| {
        let rows = rows(state, devices, output);
        let last = rows.len().saturating_sub(1);
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(index, (name, picked, note))| {
                device_row(state, output, index, &name, picked, note, index < last)
            });
        col()
            .w_full()
            .gap(2.0)
            .px(20.0)
            .pt(20.0)
            .pb(8.0)
            .radius(theme.radius("lg").unwrap_or(8.0))
            .border(1.0, theme.color("border-strong"))
            .child(
                row()
                    .gap(theme.space(2))
                    .items_center()
                    .child(icon(glyph, 16.0).color(theme.color("text")).a11y_hidden())
                    .child(
                        text(title)
                            .font(theme.text("heading"))
                            .color(theme.color("text")),
                    ),
            )
            .child(
                col()
                    .key(if output {
                        "settings-outputs"
                    } else {
                        "settings-inputs"
                    })
                    .role("radiogroup")
                    .label(title)
                    .focus_group()
                    .w_full()
                    .children(rows),
            )
    };
    col()
        .gap(theme.space(4))
        .child(intro)
        .child(card(true, "Output", &VOLUME))
        .child(card(false, "Input", &MIC))
}

fn device_row(
    state: &State,
    output: bool,
    index: usize,
    name: &str,
    picked: bool,
    note: Option<String>,
    line: bool,
) -> El {
    let theme = &state.theme;
    let accent = theme.color("focus");
    let dot = el()
        .size(16.0, 16.0)
        .radius(8.0)
        .items_center()
        .justify_center()
        .a11y_hidden();
    let dot = match picked {
        true => dot
            .fill(accent)
            .child(el().size(6.0, 6.0).radius(3.0).fill(theme.color("well"))),
        false => dot.border(1.5, theme.color("text-muted")),
    };
    let mut e = row()
        .key(key(output, index))
        .role("radio")
        .label(name)
        .checked(picked)
        .focus_selected(picked)
        .w_full()
        .h(48.0)
        .gap(10.0)
        .items_center()
        .focusable(true)
        .on_press_tx(move |s: &mut State, tx| s.pick_device(output, index, tx))
        .child(dot)
        .child(
            text(name.to_string())
                .font(theme.text("body"))
                .color(theme.color(if picked { "text" } else { "text-secondary" }))
                .truncate()
                .grow(1.0)
                .min_w(0.0)
                .a11y_hidden(),
        );
    if line {
        e = e.border_sides([0.0, 0.0, 1.0, 0.0], theme.color("border-strong"));
    }
    if let Some(note) = note {
        e = e.description(note.clone()).child(
            text(note)
                .font(theme.text("label"))
                .color(theme.color("text-secondary"))
                .a11y_hidden(),
        );
    }
    theme.kit.interactive(e)
}

pub(super) fn controls(state: &State, page: &Page) -> Vec<Control> {
    let Some(devices) = &page.devices else {
        return Vec::new();
    };
    let list = |output: bool, title: &str| {
        let rows = rows(state, devices, output).into_iter().enumerate();
        rows.map(move |(row, (name, picked, note))| Control {
            checked: Some(picked),
            value: note.map(serde_json::Value::String),
            ..control(
                key(output, row),
                &format!("{title}: {name}"),
                "radio",
                Act::Device { output, row },
            )
        })
        .collect::<Vec<_>>()
    };
    let mut controls = list(true, "Output");
    controls.extend(list(false, "Input"));
    controls
}
