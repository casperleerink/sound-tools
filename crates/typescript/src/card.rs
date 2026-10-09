//! The card of an instance of a tool of the project, in a rack, or its page, the whole window:
//! the tree its code draws, with what needs the view drawn here: knobs, steps, meters, pads and
//! canvases.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle,
    KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, WeakEntity, Window, canvas, div, fill, point, prelude::*, px, size,
};
use serde_json::Value;
use sound_core::{InstanceId, ProjectEvent};
use sound_ui::components::device_card::{CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, decibels_readout, hertz_readout, milliseconds_readout, percent_readout, short,
};
use sound_ui::import::{choose_file, import_file};
use sound_ui::{ActiveTheme, ControlEdit, Session, weak_callback};

use crate::bun::PageSize;
use crate::tools::{Field, ToolInfo, Unit};
use crate::tree::{self, Controls, KnobNode};
use crate::window::{Live, Surface, state_of};

pub(crate) struct TypeScriptCard {
    live: Entity<Live>,
    session: Entity<Session>,
    id: InstanceId,
    /// The number the live host knows this card by.
    card: u64,
    /// The frame of its card in a rack; `None` for a page.
    frame: Option<CardFrame>,
    /// The drag of a knob on the record.
    edit: ControlEdit,
    /// The pad or the canvas the pointer holds since its press.
    held: Option<Held>,
    /// It has the keys while it, or a control in it, has the focus: a click on it gives it
    /// them, and a page takes them when it opens.
    focus: FocusHandle,
    /// The keys that went down and not up yet. They go up when it loses the keys, so a note
    /// a key holds is not held for ever.
    keys_down: BTreeSet<String>,
}

/// A surface of a card that follows the pointer from a press until the button comes up.
#[derive(Clone, PartialEq)]
enum Held {
    /// A pad, by the live controls it plays.
    Pad(String, String),
    /// A canvas, by its drag handler.
    Canvas(Option<usize>),
}

impl TypeScriptCard {
    pub(crate) fn new(
        live: Entity<Live>,
        session: Entity<Session>,
        id: InstanceId,
        frame: Option<CardFrame>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tool = session
            .read(cx)
            .project()
            .tool_of(&id)
            .unwrap_or_default()
            .to_string();
        let page = frame.is_none();
        let surface = if page {
            Surface::Page(None)
        } else {
            Surface::Card
        };
        let card = live.update(cx, |live, cx| live.add(id.clone(), tool, surface, cx));
        let focus = cx.focus_handle();
        let hears_keys = live.read(cx).info(card).is_some_and(|info| info.keys);
        if page && hears_keys {
            window.focus(&focus, cx);
        }
        cx.on_focus_out(&focus, window, |view, _, _, cx| view.let_go_of_keys(cx))
            .detach();
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
            held: None,
            focus,
            keys_down: BTreeSet::new(),
        }
    }

    /// A key went down or up while the card has the keys. Not a key with cmd, ctrl, alt or
    /// fn, which stays the window's, as cmd-z does, and not the repeats of a held key.
    fn key(&mut self, keystroke: &Keystroke, down: bool, cx: &mut Context<Self>) {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        let key = keystroke.key.clone();
        // A key that went down before the card had the keys has no up here.
        let changed = match down {
            true => self.keys_down.insert(key.clone()),
            false => self.keys_down.remove(&key),
        };
        if changed {
            let card = self.card;
            self.live
                .update(cx, |live, cx| live.key(card, &key, down, cx));
        }
        cx.stop_propagation();
    }

    /// The card lost the keys: every key it holds goes up.
    fn let_go_of_keys(&mut self, cx: &mut Context<Self>) {
        let card = self.card;
        for key in std::mem::take(&mut self.keys_down) {
            self.live
                .update(cx, |live, cx| live.key(card, &key, false, cx));
        }
    }

    /// Moves a live control, or fires a trigger with no value.
    fn control(&mut self, name: &str, value: Option<f32>, cx: &mut Context<Self>) {
        let id = self.id.clone();
        self.live
            .update(cx, |live, cx| live.control(&id, name, value, None, cx));
    }

    /// A handler of the tree, with where the pointer is on a canvas.
    fn event(&mut self, handler: usize, at: Option<(f32, f32)>, cx: &mut Context<Self>) {
        self.live.read(cx).event(self.card, handler, at);
    }
}

/// What one draw of the card reads.
struct Drawing<'a, 'b> {
    live: Entity<Live>,
    /// The tool, as it last loaded.
    info: Option<ToolInfo>,
    /// The record, with each field it leaves out at its default.
    state: Value,
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
        element
            .id(id)
            .cursor_pointer()
            .on_click((self.cx).listener(move |view, _, _, cx| view.event(handler, None, cx)))
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
            (Some(path), _) => (self.state.get(path))
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            (None, Some(live)) => self.live_value(live).map(|(value, ..)| value),
            (None, None) => None,
        }
        .unwrap_or(*default);
        let range = crate::tools::knob_range(*min, *max, *unit);
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
                        let set = |state: &mut Value, value| {
                            tree::set_field(state, &path, tree::decimal(value));
                        };
                        (view.edit).apply_json(
                            &view.session,
                            &view.id,
                            &undo_label,
                            change,
                            set,
                            cx,
                        );
                    },
                ))
            }
            (None, Some(live)) => {
                let live = live.clone();
                knob.on_change(weak_callback(
                    self.cx,
                    move |view: &mut TypeScriptCard, change, cx| {
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

    fn steps(&mut self, path: &str, max: Option<f32>, playing: Option<&str>) -> AnyElement {
        // A step is off at the pattern's min and on at its max, unless the node says what on is.
        let pattern = self.info.as_ref().and_then(|info| info.field(path));
        let (off, on) = match pattern {
            Some(Field::Pattern { min, max: top, .. }) => (*min, max.unwrap_or(*top)),
            _ => (0.0, max.unwrap_or(1.0)),
        };
        let list = (self.state.get(path))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let list = Rc::new(list);
        let playing = playing
            .and_then(|watch| self.watches.get(watch))
            .map(|step| step.floor() as i64);
        let theme = self.cx.theme();
        let (on_color, off_color, light) = (theme.lavender, theme.gray_300, theme.gray_950);
        let cells = list.iter().enumerate().map(|(step, value)| {
            let is_on = value.as_f64().is_some_and(|value| value as f32 > off);
            let lit = playing == Some(step as i64);
            let (path, list) = (path.to_string(), list.clone());
            div()
                .id(ElementId::Name(format!("{path}-{step}").into()))
                .size(px(14.))
                .rounded(px(3.))
                .bg(if is_on { on_color } else { off_color })
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
                    // The whole list as it plays, with this step changed: the record may leave
                    // the pattern out.
                    let mut list = (*list).clone();
                    if let Some(item) = list.get_mut(step) {
                        *item = tree::decimal(if is_on { off } else { on });
                    }
                    view.session.update(cx, |session, cx| {
                        session.edit(cx, |project| {
                            let mut edit = project.begin(label);
                            project.update_json(&mut edit, &id, |state| {
                                tree::set_field(state, &path, Value::Array(list));
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

    fn sample(&mut self, path: &str, label: Option<&str>) -> AnyElement {
        let file = self.state.get(path).and_then(serde_json::Value::as_str);
        let shown = match file {
            Some(file) if !file.is_empty() => file.to_string(),
            _ => "No sound".to_string(),
        };
        let theme = self.cx.theme();
        let (quiet, button) = (theme.gray_700, theme.gray_300);
        let path = path.to_string();
        let choose = div()
            .id(ElementId::Name(format!("{path}-choose").into()))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(button)
            .cursor_pointer()
            .child("Choose…")
            .on_click(self.cx.listener(move |view, _, _, cx| {
                let path = path.clone();
                let session = view.session.clone();
                choose_file(&session, "Choose a sound", cx, move |view, file, cx| {
                    let session = view.session.clone();
                    import_file(&session, file, cx, move |view, imported, cx| {
                        let (id, name) = (view.id.clone(), imported.asset.to_string());
                        view.session.update(cx, |session, cx| {
                            session.edit(cx, |project| {
                                let mut edit = project.begin("Choose a sound");
                                project.update_json(&mut edit, &id, |state| {
                                    if let Some(fields) = state.as_object_mut() {
                                        fields.insert(path, name.into());
                                    }
                                })?;
                                project.finish(edit)
                            })
                        });
                    });
                });
            }));
        let mut row = div().flex().flex_row().items_center().gap(px(8.));
        if let Some(label) = label {
            row = row.child(label.to_string());
        }
        row.child(div().text_color(quiet).child(shown))
            .child(choose)
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

    fn canvas(&mut self, node: &tree::CanvasNode) -> AnyElement {
        let theme = self.cx.theme();
        let background = node.background.map_or(theme.gray_200, |color| color.0);
        let drawn: Vec<Drawn> = node.shapes.iter().map(Drawn::of).collect();
        let view = self.cx.entity().downgrade();
        let (on_press, on_drag) = (node.on_press, node.on_drag);
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, (), window: &mut Window, _: &mut App| {
                window.paint_quad(fill(bounds, background).corner_radii(px(6.)));
                for shape in &drawn {
                    shape.paint(bounds.origin, window);
                }
                if on_press.is_some() || on_drag.is_some() {
                    let held = Held::Canvas(on_drag);
                    follow_pointer(window, bounds, view, held, move |view, at, pressed, cx| {
                        if let Some(handler) = if pressed { on_press } else { on_drag } {
                            view.event(handler, Some(at), cx);
                        }
                    });
                }
            },
        )
        .w(px(node.width))
        .h(px(node.height))
        .into_any_element()
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
        let (x, y) = (x.to_string(), y.to_string());
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
                // Across moves `x` and up moves `y`, each over its range.
                let held = Held::Pad(x.clone(), y.clone());
                follow_pointer(window, bounds, view, held, move |view, at, _, cx| {
                    let (across, down) = at;
                    view.control(&x, Some(x_min + (x_max - x_min) * across), cx);
                    view.control(&y, Some(y_min + (y_max - y_min) * (1.0 - down)), cx);
                });
            },
        )
        .size(px(size_points))
        .into_any_element()
    }
}

/// Makes the surface at `bounds` follow the pointer: a press in it holds it, and `play` hears
/// the press and each drag after it until the button comes up, with where the pointer is (see
/// [`across_and_down`]) and whether it is the press.
fn follow_pointer(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    view: WeakEntity<TypeScriptCard>,
    held: Held,
    play: impl Fn(&mut TypeScriptCard, (f32, f32), bool, &mut Context<TypeScriptCard>) + 'static,
) {
    let play = Rc::new(play);
    window.on_mouse_event({
        let (view, held, play) = (view.clone(), held.clone(), play.clone());
        move |event: &MouseDownEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !bounds.contains(&event.position)
            {
                return;
            }
            view.update(cx, |view, cx| {
                view.held = Some(held.clone());
                play(view, across_and_down(bounds, event.position), true, cx);
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
                if view.held.as_ref() == Some(&held) {
                    play(view, across_and_down(bounds, event.position), false, cx);
                }
            })
            .ok();
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            view.update(cx, |view, _| view.held = None).ok();
        }
    });
}

/// Where `position` is in `bounds`, from 0 to 1 across and down. Outside, at the nearest edge,
/// so a drag past the edge holds there.
fn across_and_down(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
    let across = (position.x - bounds.origin.x) / bounds.size.width;
    let down = (position.y - bounds.origin.y) / bounds.size.height;
    (across.clamp(0.0, 1.0), down.clamp(0.0, 1.0))
}

/// A shape of a canvas, ready to paint.
enum Drawn {
    Quad {
        bounds: Bounds<Pixels>,
        radius: Pixels,
        color: gpui::Hsla,
    },
    Line {
        from: Point<Pixels>,
        to: Point<Pixels>,
        width: Pixels,
        color: gpui::Hsla,
    },
}

impl Drawn {
    fn of(shape: &tree::Shape) -> Self {
        match shape {
            tree::Shape::Circle {
                x,
                y,
                radius,
                color,
            } => Self::Quad {
                bounds: Bounds::new(
                    point(px(x - radius), px(y - radius)),
                    size(px(radius * 2.), px(radius * 2.)),
                ),
                radius: px(*radius),
                color: color.0,
            },
            tree::Shape::Rect {
                x,
                y,
                width,
                height,
                color,
                radius,
            } => Self::Quad {
                bounds: Bounds::new(point(px(*x), px(*y)), size(px(*width), px(*height))),
                radius: px(radius.unwrap_or(0.0)),
                color: color.0,
            },
            tree::Shape::Line {
                from,
                to,
                color,
                width,
            } => Self::Line {
                from: point(px(from[0]), px(from[1])),
                to: point(px(to[0]), px(to[1])),
                width: px(width.unwrap_or(1.0)),
                color: color.0,
            },
        }
    }

    fn paint(&self, origin: Point<Pixels>, window: &mut Window) {
        match self {
            Self::Quad {
                bounds,
                radius,
                color,
            } => {
                let bounds = Bounds::new(origin + bounds.origin, bounds.size);
                window.paint_quad(fill(bounds, *color).corner_radii(*radius));
            }
            Self::Line {
                from,
                to,
                width,
                color,
            } => {
                let mut line = gpui::PathBuilder::stroke(*width);
                line.move_to(origin + *from);
                line.line_to(origin + *to);
                if let Ok(path) = line.build() {
                    window.paint_path(path, *color);
                }
            }
        }
    }
}

impl Render for TypeScriptCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = state_of(self.session.read(cx).project(), &self.id) else {
            return div().into_any_element();
        };
        let live = self.live.read(cx);
        let info = live.info(self.card);
        let hears_keys = info.as_ref().is_some_and(|info| info.keys);
        let state = match &info {
            Some(info) => info.with_defaults(state),
            None => state,
        };
        let tree = live.tree(self.card).cloned();
        let watches = live.watches(self.card).cloned().unwrap_or_default();
        let body = match tree {
            Some(Ok(node)) => {
                let mut drawing = Drawing {
                    live: self.live.clone(),
                    info,
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
        let (text, background) = (theme.gray_950, theme.gray_100);
        // A click in what takes the keys gives them: the body of a card, a whole page.
        let takes_keys = |element: Div| {
            element.when(hears_keys, |element| {
                element
                    .track_focus(&self.focus)
                    .key_context(crate::KEY_CONTEXT)
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        view.key(&event.keystroke, true, cx)
                    }))
                    .on_key_up(cx.listener(|view, event: &KeyUpEvent, _, cx| {
                        view.key(&event.keystroke, false, cx)
                    }))
            })
        };
        let body = div().text_size(px(12.)).text_color(text).child(body);
        match &self.frame {
            // At least as wide as a plain card, so the title fits.
            Some(frame) => frame
                .card()
                .min_w(px(PLAIN_CARD_WIDTH))
                .child(takes_keys(body))
                .into_any_element(),
            None => {
                // The room inside the padding, which the page draws from.
                let (live, card) = (self.live.clone(), self.card);
                let measure = canvas(
                    move |bounds, _, cx| {
                        let size = PageSize {
                            width: f32::from(bounds.size.width).round() as u32,
                            height: f32::from(bounds.size.height).round() as u32,
                        };
                        cx.defer(move |cx| live.update(cx, |live, cx| live.resize(card, size, cx)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full();
                let room = div().relative().size_full().child(measure).child(body);
                takes_keys(div().size_full().p(px(24.)).bg(background).child(room))
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_is_found_across_and_down_and_held_at_the_edges() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(50.)));
        let at = |x: f32, y: f32| across_and_down(bounds, point(px(x), px(y)));
        assert_eq!(at(60., 45.), (0.5, 0.5));
        assert_eq!(at(10., 70.), (0.0, 1.0));
        assert_eq!(at(500., -5.), (1.0, 0.0));
    }
}
