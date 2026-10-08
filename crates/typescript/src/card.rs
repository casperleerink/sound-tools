//! The card of an instance in a rack: the card a file of `extensions/` draws for its tool, or the
//! built-in card while there is none. Which one can change at every save of a file.

use gpui::{AnyElement, AnyView, Context, Div, ElementId, Entity, Window, div, prelude::*, px};
use sound_core::{InstanceId, ProjectEvent, ValueRange};
use sound_ui::components::device_card::{CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::components::knob::{
    Knob, decibels_readout, hertz_readout, milliseconds_readout, percent_readout, short,
};
use sound_ui::{ActiveTheme, ControlEdit, Session, weak_callback};

use crate::tools::Unit;
use crate::tree::{self, Controls, KnobNode};
use crate::window::Live;

pub(crate) struct TypeScriptCard {
    live: Entity<Live>,
    session: Entity<Session>,
    id: InstanceId,
    /// The number the live host knows this card by.
    card: u64,
    frame: CardFrame,
    built_in: Option<AnyView>,
    /// The drag of a knob.
    edit: ControlEdit,
}

impl TypeScriptCard {
    pub(crate) fn new(
        live: Entity<Live>,
        session: Entity<Session>,
        id: InstanceId,
        frame: CardFrame,
        built_in: Option<AnyView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let card = live.update(cx, |live, cx| live.add(id.clone(), cx));
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if *id == view.id => {
                view.live.update(cx, |live, cx| live.render(view.card, cx));
            }
            ProjectEvent::Deleted(id) if *id == view.id => view.edit.finish(&view.session, cx),
            _ => {}
        })
        .detach();
        cx.observe(&live, |_, _, cx| cx.notify()).detach();
        cx.on_release(|view, cx| {
            view.edit.finish(&view.session, cx);
            view.live.update(cx, |live, _| live.remove(view.card));
        })
        .detach();
        Self {
            live,
            session,
            id,
            card,
            frame,
            built_in,
            edit: ControlEdit::default(),
        }
    }
}

/// The controls of one draw of the card.
struct Drawing<'a, 'b> {
    state: serde_json::Value,
    card: u64,
    cx: &'a mut Context<'b, TypeScriptCard>,
}

impl Controls for Drawing<'_, '_> {
    fn clickable(&mut self, element: Div, id: ElementId, handler: usize) -> AnyElement {
        let card = self.card;
        element
            .id(id)
            .cursor_pointer()
            .on_click(self.cx.listener(move |view, _, _, cx| {
                view.live.update(cx, |live, _| live.click(card, handler));
            }))
            .into_any_element()
    }

    fn knob(&mut self, knob: &KnobNode) -> AnyElement {
        let KnobNode {
            path,
            label,
            min,
            max,
            default,
            unit,
        } = knob;
        let value = tree::number_at(&self.state, path).unwrap_or(*default);
        let (path, undo_label) = (path.clone(), format!("Change {}", label.to_lowercase()));
        // A frequency is heard in octaves, so its knob turns on a log scale.
        let range = match unit {
            Some(Unit::Hz) if *min > 0.0 => ValueRange::logarithmic(*min, *max),
            _ => ValueRange::linear(*min, *max),
        };
        let readout = match unit {
            Some(Unit::Hz) => hertz_readout(value),
            Some(Unit::Ms) => milliseconds_readout(value),
            Some(Unit::Db) => decibels_readout(value),
            Some(Unit::Percent) => percent_readout(value),
            None => short(value),
        };
        Knob::new(ElementId::Name(path.clone().into()))
            .range(range)
            .value(value)
            .default_value(*default)
            .label(label.clone())
            .readout(readout)
            .on_change(weak_callback(
                self.cx,
                move |view: &mut TypeScriptCard, change, cx| {
                    let (session, id) = (&view.session, &view.id);
                    let set = |state: &mut serde_json::Value, value| {
                        tree::set_number(state, &path, value)
                    };
                    view.edit
                        .apply_json(session, id, &undo_label, change, set, cx);
                },
            ))
            .into_any_element()
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let tool = project.tool_of(&self.id);
        let state = project
            .state_json(&self.id)
            .and_then(|json| serde_json::from_str(&json).ok());
        let live = self.live.read(cx);
        let (Some(tool), Some(state)) = (tool, state) else {
            return div().into_any_element();
        };
        if !live.has_card(tool) {
            return match &self.built_in {
                Some(view) => view.clone().into_any_element(),
                None => div().into_any_element(),
            };
        }
        let tree = live.tree(self.card).cloned();
        let body = match tree {
            Some(Ok(node)) => {
                let mut drawing = Drawing {
                    state,
                    card: self.card,
                    cx,
                };
                tree::draw(&node, &[], &mut drawing)
            }
            Some(Err(error)) => div()
                .text_color(cx.theme().red)
                .child(format!("ui: {error}"))
                .into_any_element(),
            // Until its first tree arrives, a moment after the card opens.
            None => div().into_any_element(),
        };
        let theme = cx.theme();
        let body = div()
            .text_size(px(12.))
            .text_color(theme.gray_950)
            .child(body);
        // At least as wide as a plain card, so the title fits.
        self.frame
            .card()
            .min_w(px(PLAIN_CARD_WIDTH))
            .child(body)
            .into_any_element()
    }
}
