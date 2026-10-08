//! The card of an instance of a tool of the project, in a rack: the tree its code draws, with
//! the controls that need the view drawn here: knobs, steps, meters and pads.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, div, fill, point,
    prelude::*, px, size,
};
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
    /// The drag of a knob on the record.
    edit: ControlEdit,
    /// The live controls of the pad the pointer holds.
    pad: Option<(String, String)>,
}

impl TypeScriptCard {
    pub(crate) fn new(
        live: Entity<Live>,
        session: Entity<Session>,
        id: InstanceId,
        frame: CardFrame,
        cx: &mut Context<Self>,
    ) -> Self {
        let tool = session
            .read(cx)
            .project()
            .tool_of(&id)
            .unwrap_or_default()
            .to_string();
        let card = live.update(cx, |live, cx| live.add(id.clone(), tool, cx));
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
            edit: ControlEdit::default(),
            pad: None,
        }
    }

    /// Moves a live control, or fires a trigger with no value.
    fn control(&mut self, name: &str, value: Option<f32>, cx: &mut Context<Self>) {
        let card = self.card;
        self.live
            .update(cx, |live, cx| live.control(card, name, value, cx));
    }
}

/// What one draw of the card reads.
struct Drawing<'a, 'b> {
    live: Entity<Live>,
    state: serde_json::Value,
    watches: BTreeMap<String, f32>,
    card: u64,
    cx: &'a mut Context<'b, TypeScriptCard>,
}

impl Drawing<'_, '_> {
    /// Where the live control `name` was put last, and its range.
    fn live_value(&self, name: &str) -> Option<(f32, f32, f32)> {
        self.live.read(self.cx).live_value(self.card, name)
    }
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
            live,
            label,
            min,
            max,
            default,
            unit,
        } = knob;
        let value = match (path, live) {
            (Some(path), _) => tree::number_at(&self.state, path),
            (None, Some(live)) => self.live_value(live).map(|(value, ..)| value),
            (None, None) => None,
        }
        .unwrap_or(*default);
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
        let name = path.clone().or_else(|| live.clone()).unwrap_or_default();
        let knob = Knob::new(ElementId::Name(name.into()))
            .range(range)
            .value(value)
            .default_value(*default)
            .label(label.clone())
            .readout(readout);
        let knob = match (path, live) {
            (Some(path), _) => {
                let (path, undo_label) = (path.clone(), format!("Change {}", label.to_lowercase()));
                knob.on_change(weak_callback(
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
            }
            (None, Some(live)) => {
                let live = live.clone();
                knob.on_change(weak_callback(
                    self.cx,
                    move |view: &mut TypeScriptCard,
                          change: sound_ui::components::gesture::ValueChange,
                          cx| {
                        use sound_ui::components::gesture::ValueChange;
                        if let ValueChange::Drag(value) | ValueChange::Set(value) = change {
                            view.control(&live, Some(value), cx);
                        }
                    },
                ))
            }
            (None, None) => knob,
        };
        knob.into_any_element()
    }

    fn steps(&mut self, path: &str, max: f32, playing: Option<&str>) -> AnyElement {
        let values: Vec<f32> = tree::numbers_at(&self.state, path).unwrap_or_default();
        let playing = playing
            .and_then(|watch| self.watches.get(watch))
            .map(|step| step.floor() as i64);
        let theme = self.cx.theme();
        let (on, off, light) = (theme.lavender, theme.gray_300, theme.gray_950);
        let cells = values.iter().enumerate().map(|(step, value)| {
            let is_on = *value > 0.0;
            let path = path.to_string();
            let lit = playing == Some(step as i64);
            div()
                .id(ElementId::Name(format!("{path}-{step}").into()))
                .size(px(14.))
                .rounded(px(3.))
                .bg(if is_on { on } else { off })
                .border(px(2.))
                .border_color(if lit {
                    light
                } else {
                    gpui::transparent_black()
                })
                .cursor_pointer()
                .on_click(self.cx.listener(move |view, _, _, cx| {
                    let id = view.id.clone();
                    let label = if is_on {
                        "Turn a step off"
                    } else {
                        "Turn a step on"
                    };
                    let next = if is_on { 0.0 } else { max };
                    view.session.update(cx, |session, cx| {
                        session.edit(cx, |project| {
                            let mut edit = project.begin(label);
                            project.update_json(&mut edit, &id, |state| {
                                tree::set_number(state, &format!("{path}.{step}"), next);
                            })?;
                            project.finish(edit)
                        })
                    });
                }))
        });
        div()
            .flex()
            .flex_row()
            .gap(px(3.))
            .children(cells)
            .into_any_element()
    }

    fn meter(&mut self, watch: &str, label: Option<&str>) -> AnyElement {
        let level = self
            .watches
            .get(watch)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let theme = self.cx.theme();
        let (empty, full) = (theme.gray_300, theme.green);
        let bar = div()
            .w(px(8.))
            .h(px(64.))
            .rounded(px(2.))
            .bg(empty)
            .flex()
            .flex_col()
            .justify_end()
            .child(div().w_full().h(px(64. * level)).rounded(px(2.)).bg(full));
        let mut meter = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .child(bar);
        if let Some(label) = label {
            meter = meter.child(label.to_string());
        }
        meter.into_any_element()
    }

    fn pad(&mut self, x: &str, y: &str, size_points: f32) -> AnyElement {
        let at = |name: &str| self.live_value(name).unwrap_or((0.0, 0.0, 1.0));
        let (x_now, x_min, x_max) = at(x);
        let (y_now, y_min, y_max) = at(y);
        let fraction =
            |value: f32, min: f32, max: f32| ((value - min) / (max - min)).clamp(0.0, 1.0);
        let dot = (fraction(x_now, x_min, x_max), fraction(y_now, y_min, y_max));
        let theme = self.cx.theme();
        let (background, mark) = (theme.gray_300, theme.lavender);
        let view = self.cx.entity().downgrade();
        let names = (x.to_string(), y.to_string());
        let ranges = ((x_min, x_max), (y_min, y_max));
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, (), window: &mut Window, _: &mut App| {
                window.paint_quad(fill(bounds, background).corner_radii(px(6.)));
                let dot_size = px(10.);
                let center = point(
                    bounds.origin.x + bounds.size.width * dot.0,
                    bounds.origin.y + bounds.size.height * (1.0 - dot.1),
                );
                let dot_bounds = Bounds::new(
                    point(center.x - dot_size / 2., center.y - dot_size / 2.),
                    size(dot_size, dot_size),
                );
                window.paint_quad(fill(dot_bounds, mark).corner_radii(dot_size / 2.));
                // The pointer at `position` puts both controls where it is in the square.
                let play = move |position: Point<Pixels>,
                                 view: &mut TypeScriptCard,
                                 cx: &mut Context<TypeScriptCard>| {
                    let across =
                        ((position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
                    let up =
                        1.0 - ((position.y - bounds.origin.y) / bounds.size.height).clamp(0.0, 1.0);
                    let ((x_min, x_max), (y_min, y_max)) = ranges;
                    let Some((x, y)) = view.pad.clone() else {
                        return;
                    };
                    view.control(&x, Some(x_min + (x_max - x_min) * across), cx);
                    view.control(&y, Some(y_min + (y_max - y_min) * up), cx);
                };
                window.on_mouse_event({
                    let (view, names) = (view.clone(), names.clone());
                    move |event: &MouseDownEvent, phase, _, cx| {
                        if phase != DispatchPhase::Bubble
                            || event.button != MouseButton::Left
                            || !bounds.contains(&event.position)
                        {
                            return;
                        }
                        view.update(cx, |view, cx| {
                            view.pad = Some(names.clone());
                            play(event.position, view, cx);
                        })
                        .ok();
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Bubble || !event.dragging() {
                            return;
                        }
                        view.update(cx, |view, cx| {
                            if view.pad.as_ref().is_some_and(|pad| *pad == names) {
                                play(event.position, view, cx);
                            }
                        })
                        .ok();
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            view.update(cx, |view, _| view.pad = None).ok();
                        }
                    }
                });
            },
        )
        .size(px(size_points))
        .into_any_element()
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let state = project
            .state_json(&self.id)
            .and_then(|json| serde_json::from_str(&json).ok());
        let Some(state) = state else {
            return div().into_any_element();
        };
        let live = self.live.read(cx);
        let tree = live.tree(self.card).cloned();
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let body = match tree {
            Some(Ok(node)) => {
                let mut drawing = Drawing {
                    live: self.live.clone(),
                    state,
                    watches,
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
