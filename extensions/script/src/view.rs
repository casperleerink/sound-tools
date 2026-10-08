//! The card of a script: one knob per `param` line, in the order of the code, two to a
//! column. Past four columns the rest are behind expand. A script with no params says so.
//!
//! The knobs come from the code that plays, so a new `param` line gets its knob as soon as the
//! code compiles. A knob writes the value of its param into `values` of the record, through
//! [`ControlEdit`]: a drag is one gesture and one undo step.

use gpui::{Context, ElementId, Entity, Window, div, prelude::*, px};
use sound_core::{Instance, ProjectEvent, ValueRange};
use sound_ui::components::device_card::{CardFrame, Column, PLAIN_CARD_WIDTH};
use sound_ui::components::knob::{Knob, short};
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use sound_hum::ParameterSpec;

use crate::ScriptState;

/// What the rack calls a script that has no `name`.
pub const NAME: &str = "Script";

/// Columns of knobs the card shows before expand.
const SHOWN_COLUMNS: usize = 4;

/// Registers the card of the `script` tool and what a rack calls one. A script has no offer in
/// a picker: an empty one does nothing, so it comes from an agent that writes its code.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(ScriptView::new);
    devices.describe::<ScriptState>(|state| DeviceLabel {
        key: crate::EXTENSION.into(),
        name: match state.name.is_empty() {
            true => NAME.into(),
            false => state.name.clone().into(),
        },
    });
}

pub struct ScriptView {
    session: Entity<Session>,
    script: Instance<ScriptState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob.
    edit: ControlEdit,
    expanded: bool,
}

impl ScriptView {
    pub fn new(
        session: Entity<Session>,
        script: Instance<ScriptState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.script.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.script.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        Self {
            session,
            script,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
        }
    }

    fn knob(&self, parameter: &ParameterSpec, value: f32, cx: &mut Context<Self>) -> Knob {
        let name = parameter.name.clone();
        let label = label(&name);
        let undo_label = format!("Change {}", label.to_lowercase());
        Knob::new(ElementId::Name(name.clone().into()))
            .range(ValueRange::linear(parameter.min, parameter.max))
            .value(value)
            .default_value(parameter.default)
            .label(label)
            .readout(short(value))
            .on_change(weak_callback(cx, move |view: &mut Self, change, cx| {
                let set = |state: &mut ScriptState, value| {
                    state.values.insert(name.clone(), value);
                };
                let (session, script) = (&view.session, &view.script);
                view.edit
                    .apply(session, script, &undo_label, change, set, cx);
            }))
    }
}

/// `tape_time` is labelled `Tape time`.
fn label(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut characters = words.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

impl Render for ScriptView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self.session.read(cx).project().state(&self.script) else {
            return div().into_any_element();
        };
        // The record in the project is valid, so it compiles.
        let Ok((code, values)) = state.compile() else {
            return div().into_any_element();
        };
        if code.parameters.is_empty() {
            let line = div()
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(cx.theme().gray_800)
                .child("This script has no params.");
            let card = self.frame.card().w(px(PLAIN_CARD_WIDTH));
            return card.child(line).into_any_element();
        }
        let knobs: Vec<Knob> = code
            .parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| self.knob(parameter, value, cx))
            .collect();
        // At least as wide as a plain card, so a name such as `Tape echo` fits in the title.
        let mut card = self.frame.card().min_w(px(PLAIN_CARD_WIDTH));
        let mut knobs = knobs.into_iter();
        let mut index = 0;
        while let Some(top) = knobs.next() {
            let column = Column::new().top(top);
            let column = match knobs.next() {
                Some(bottom) => column.bottom(bottom),
                None => column,
            };
            card = match index < SHOWN_COLUMNS {
                true => card.column(column),
                false => card.hidden_column(column),
            };
            index += 1;
        }
        if index > SHOWN_COLUMNS {
            let expanded = self.expanded;
            card = card.expand(
                expanded,
                cx.listener(|view, _, _, cx| {
                    view.expanded = !view.expanded;
                    cx.notify();
                }),
            );
        }
        card.into_any_element()
    }
}
