//! A space: shapes in units of its own, which it scrolls and zooms, and what the pointer does on it.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, ElementId, ExternalPaths, Hitbox, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point, ScrollEvent,
    WeakEntity, Window, canvas, div, point, prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::tooltip::Tooltip;
use sound_ui::focus::TabStop;

use crate::bun::EventKind;
use crate::live::SpaceView;
use crate::tree::{self, Cursor, Drag, Props, SpaceProps};

use super::controls::{Control, Keys};
use super::drawing::Drawing;
use super::input::{Carried, Hears, KeyTurned, heard};
use super::paint::{Painted, found, paint, prepaint};
use super::shapes::{Placed, Placing, Pointer, Shown};
use super::{Lifted, TypeScriptCard};

/// What the tooltip of a control says while a lane or a cable moves its field, as the window's
/// own knobs say it.
const HELD_BY_A_LANE: &str = "Follows the automation of the track";

impl Drawing<'_> {
    fn place(
        &self,
        children: &[usize],
        key: &str,
        view: &SpaceView,
    ) -> (Vec<Placed>, Vec<Option<Pointer>>) {
        let mut placing = Placing {
            sources: self.sources(),
            table: self.table,
            shown: Shown::of(view),
            hovered: (self.hovered.as_ref())
                .filter(|(space, _)| space == key)
                .map(|(_, shape)| *shape),
            placed: Vec::new(),
            pointed: Vec::new(),
        };
        let mut indexes = self.indexes.borrow_mut();
        let version = self.table.version();
        if indexes
            .get(key)
            .is_none_or(|index| index.version != version)
        {
            indexes.insert(key.to_string(), placing.index(children, version));
        }
        if let Some(index) = indexes.get_mut(key) {
            placing.indexed(index);
        }
        (placing.placed, placing.pointed)
    }

    pub(super) fn space(
        &mut self,
        slot: usize,
        id: u64,
        space: &SpaceProps,
        children: &[usize],
    ) -> AnyElement {
        let (key, view, fit) = self.space_view(id, space);
        let selector = format!("space-{key}");
        let name = key.clone();
        let inner = match self.lifting.filter(|(lifted, _)| lifted.contains(&id)) {
            // The layer over the page draws it where this lays it out.
            Some((_, places)) => {
                let place = Lifted {
                    slot,
                    bounds: Bounds::default(),
                    mask: Bounds::default(),
                };
                places.borrow_mut().insert(id, place);
                let places = places.clone();
                canvas(
                    move |bounds, window, _| {
                        if let Some(place) = places.borrow_mut().get_mut(&id) {
                            (place.bounds, place.mask) = (bounds, window.content_mask().bounds);
                        }
                    },
                    |_, _, _, _| {},
                )
                .size_full()
                .into_any_element()
            }
            None => self.space_element(id, space, children, (key, view, fit)),
        };
        let holder = div().debug_selector(move || selector).child(inner);
        let holder = match space.width {
            Some(width) => holder.w(px(width)),
            None => holder.w_full(),
        };
        let holder = match space.height {
            Some(height) => holder.h(px(height)),
            None => holder.h_full(),
        };
        let holder = if space.grow == Some(true) {
            holder.flex_grow(1.).min_h_0()
        } else {
            holder.flex_none()
        };
        let turned =
            (self.turned_by_keys(id, space, children)).filter(|_| space.tab_stop != Some(false));
        match turned {
            Some(turned) => {
                let held = (turned.turns.value.as_ref())
                    .is_some_and(|field| self.feeds.held.contains(field));
                // Kept by the card, which reads from it whether the keys are on the control.
                let names =
                    (std::iter::once(name.clone()).chain(turned.turns.value.clone())).collect();
                let mut kept = self.control_focus.borrow_mut();
                let (focus, _) = (kept.entry(name.clone()))
                    .and_modify(|(_, old)| old.clone_from(&names))
                    .or_insert_with(|| (self.cx.focus_handle().tab_stop(true), names.clone()));
                let focus = focus.clone();
                drop(kept);
                let control = Control {
                    keys: Keys::Turn(turned),
                    menu: None,
                };
                // Round corners for its ring, as the window's controls have.
                let holder = holder
                    .id(ElementId::Name(name.clone().into()))
                    .rounded(px(6.))
                    .when(held, |holder| {
                        holder.tooltip(|_, cx| Tooltip::new(HELD_BY_A_LANE).view(cx))
                    });
                let holder = self.control(holder, id, control);
                TabStop::new(format!("tab-{name}"), holder)
                    .focus_handle(&focus)
                    .into_any_element()
            }
            None => holder.into_any_element(),
        }
    }

    /// What a space turns with the keys: the one number that it or one shape in it drags, a
    /// knob's or a fader's, as a control of the window does. A space of more drags, such as a
    /// timeline, has none.
    fn turned_by_keys(&self, id: u64, space: &SpaceProps, children: &[usize]) -> Option<KeyTurned> {
        let shapes = (children.iter()).filter_map(|child| {
            let node = self.table.get(*child)?;
            match &node.props {
                Props::Shape(shape) => Some((node.id, self.table.drag(shape.drag)?)),
                _ => None,
            }
        });
        let mut turned = (self
            .table
            .drag(space.drag)
            .map(|drag| (id, drag))
            .into_iter())
        .chain(shapes)
        .flat_map(|(node, drag)| {
            [drag.x.clone(), drag.y.clone()]
                .into_iter()
                .enumerate()
                .filter_map(move |(axis, turns)| {
                    let turns = turns?;
                    Some(KeyTurned { turns, node, axis })
                })
        });
        let only = turned.next()?;
        turned.next().is_none().then_some(only)
    }

    /// The key of a space, and where it is scrolled to and how far zoomed now, which the spaces
    /// that follow it see; whether it draws again after its paint, for them to see where it
    /// moved itself to.
    pub(super) fn space_view(&mut self, id: u64, space: &SpaceProps) -> (String, SpaceView, bool) {
        let key = self.element_name("space", space.key.as_deref(), id);
        let (unit_x, unit_y) = space.unit();
        let extent = space.extent.unwrap_or_default();
        let scroll = space.scroll.unwrap_or_default();
        let first = |scroll: Option<f32>, extent: Option<[f32; 2]>| {
            scroll.or(extent.map(|[low, _]| low)).map_or(0.0, f64::from)
        };
        let mut view = self.spaces.get(&key).copied().unwrap_or(SpaceView {
            left: first(scroll.x, extent.x),
            top: first(scroll.y, extent.y),
            ..SpaceView::default()
        });
        (view.unit_x, view.unit_y) = (unit_x, unit_y);
        // Fitted once it has a width, which the paint of its first draw gives; the spaces that
        // follow it see that in the draw after.
        let fit = space.fit.filter(|_| !view.fitted);
        if let Some(fit) = fit
            && view.width > 0.0
        {
            view = fitted(view, fit, space.zoom.unwrap_or_default().x);
        }
        let follow = space.follow.clone().unwrap_or_default();
        let view = followed(clamped(view, &extent), &follow, &self.spaces);
        let turns = (space.turns.as_ref()).map(|turns| self.sources().number(turns));
        let kept = kept_in_view(view, space.show, turns, &extent);
        let moved = (kept.left, kept.top) != (view.left, view.top);
        // Over the page, a draw keeps nothing of its own: where it moved and what it saw stay
        // here.
        if space.show.is_some() || space.turns.is_some() {
            let mut laid_out = self.laid_out.borrow_mut();
            if let Some(view) = laid_out.get_mut(&key) {
                (view.left, view.top) = (kept.left, kept.top);
                (view.shown, view.turns_at) = (kept.shown, kept.turns_at);
            }
            if moved {
                move_leaders(&mut laid_out, &kept, &follow);
            }
        }
        if moved {
            move_leaders(&mut self.spaces, &kept, &follow);
        }
        self.spaces.insert(key.clone(), kept);
        (key, kept, fit.is_some() || moved)
    }

    /// The shapes of a space as they are now, and what it does with the pointer and drops.
    pub(super) fn space_element(
        &mut self,
        id: u64,
        space: &SpaceProps,
        children: &[usize],
        (key, view, draws_again): (String, SpaceView, bool),
    ) -> AnyElement {
        let (placed, pointed) = self.place(children, &key, &view);
        let frame = Rc::new(SpaceFrame {
            key,
            node: id,
            view,
            placed,
            pointed,
            background: space.background.map(|color| color.0),
            radius: space.radius.map_or(0.0, |radius| radius.0),
            hears: Hears::of(space.on_press, space.on_release, space.on_click),
            drag: self.table.drag(space.drag).cloned(),
            cursor: space.cursor,
            extent: space.extent.unwrap_or_default(),
            zoom: space.zoom.unwrap_or_default(),
            follow: space.follow.clone().unwrap_or_default(),
        });
        let card = self.card.clone();
        let spaces = self.laid_out.clone();
        // Where it was painted, for where a drop falls in its units.
        let painted_at = Rc::new(Cell::new(None));
        let element = canvas(
            {
                let frame = frame.clone();
                move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                    prepaint(&frame, bounds, window)
                }
            },
            {
                let (frame, painted_at) = (frame.clone(), painted_at.clone());
                move |bounds: Bounds<Pixels>,
                      (hitbox, painted): (Hitbox, Rc<Vec<Painted>>),
                      window: &mut Window,
                      cx: &mut App| {
                    painted_at.set(Some(bounds));
                    // The size it was laid out at, for live values of what shows and the next scroll.
                    let size = (
                        f64::from(f32::from(bounds.size.width)),
                        f64::from(f32::from(bounds.size.height)),
                    );
                    let first = spaces.borrow_mut().get_mut(&frame.key).is_some_and(|view| {
                        let first = view.width == 0.0 && size.0 > 0.0;
                        (view.width, view.height) = size;
                        first
                    });
                    // The card draws again, for the spaces that follow it and the live values
                    // that read it to see its fit or where it moved itself, or its size on its
                    // first paint. Only the first paint: drawing again on every resize made
                    // `tools::nested` fail, for a cause not found.
                    if draws_again || first {
                        let card = card.clone();
                        window.on_next_frame(move |_, cx| {
                            card.update(cx, |_, cx| cx.notify()).ok();
                        });
                    }
                    paint(&frame, bounds, &hitbox, &painted, window, cx);
                    listen(&frame, &card, bounds, hitbox, painted, window);
                }
            },
        );
        let holder = div().size_full().child(element.size_full());
        if space.on_drop != Some(true) {
            return holder.into_any_element();
        }
        let element_id = ElementId::Name(format!("{}-drop", frame.key).into());
        let units = move |window: &Window| match painted_at.get() {
            Some(bounds) => frame.units_at(bounds, window.mouse_position()),
            None => (0.0, 0.0),
        };
        let lavender = self.cx.theme().lavender;
        let dropped_files = units.clone();
        holder
            .id(element_id)
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(lavender.opacity(0.1)))
            .on_drop(
                self.listener(move |view, paths: &ExternalPaths, window, cx| {
                    let at = dropped_files(window);
                    view.drop_files(id, paths.paths().to_vec(), at, cx);
                }),
            )
            .on_drop(self.listener(move |view, carried: &Carried, window, cx| {
                let dropped = super::input::dropped(&[], &[], Some(&carried.0), units(window));
                view.event(id, EventKind::Drop, dropped, cx);
            }))
            .into_any_element()
    }
}

/// Everything a space paints and hears the pointer with, for one draw.
pub(super) struct SpaceFrame {
    pub(super) key: String,
    /// The node of the space, which hears a press and a drag on it.
    pub(super) node: u64,
    pub(super) view: SpaceView,
    pub(super) placed: Vec<Placed>,
    /// What each shape does with the pointer, by its place.
    pub(super) pointed: Vec<Option<Pointer>>,
    pub(super) background: Option<Hsla>,
    pub(super) radius: f32,
    pub(super) hears: Hears,
    pub(super) drag: Option<Rc<Drag>>,
    pub(super) cursor: Option<Cursor>,
    extent: tree::Ranges,
    zoom: tree::Ranges,
    follow: tree::Follow,
}

impl SpaceFrame {
    /// The units at a point of the window.
    pub(super) fn units_at(&self, bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f64, f64) {
        let (across, down) = self.view.scale();
        (
            self.view.left + f64::from(f32::from(position.x - bounds.origin.x)) / across,
            self.view.top + f64::from(f32::from(position.y - bounds.origin.y)) / down,
        )
    }

    pub(super) fn point_at(&self, bounds: Bounds<Pixels>, (x, y): (f64, f64)) -> Point<Pixels> {
        let (across, down) = self.view.scale();
        bounds.origin
            + point(
                px(((x - self.view.left) * across) as f32),
                px(((y - self.view.top) * down) as f32),
            )
    }

    /// The view after a wheel: it scrolls over the extent, and zooms with cmd.
    fn scrolled(&self, event: &ScrollEvent, bounds: Bounds<Pixels>) -> SpaceView {
        let delta = event.delta.pixel_delta(px(32.));
        if event.modifiers.secondary() {
            let factor = (f64::from(f32::from(delta.y)) * 0.01).exp();
            return self.zoomed(factor, bounds, event.position);
        }
        let (across, down) = self.view.scale();
        let (mut dx, mut dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        if event.modifiers.shift && dx == 0.0 {
            (dx, dy) = (dy, 0.0);
        }
        let view = SpaceView {
            left: self.view.left - dx / across,
            top: self.view.top - dy / down,
            ..self.view
        };
        clamped(view, &self.extent)
    }

    /// The view zoomed by `factor` about the pointer, along each axis that zooms.
    fn zoomed(&self, factor: f64, bounds: Bounds<Pixels>, position: Point<Pixels>) -> SpaceView {
        let (at_x, at_y) = self.units_at(bounds, position);
        let mut view = self.view;
        if let Some([low, high]) = self.zoom.x {
            view.zoom_x = (view.zoom_x * factor).clamp(f64::from(low), f64::from(high));
            view.left = at_x - (at_x - view.left) * self.view.zoom_x / view.zoom_x;
        }
        if let Some([low, high]) = self.zoom.y {
            view.zoom_y = (view.zoom_y * factor).clamp(f64::from(low), f64::from(high));
            view.top = at_y - (at_y - view.top) * self.view.zoom_y / view.zoom_y;
        }
        clamped(view, &self.extent)
    }

    fn scrolls(&self) -> bool {
        self.extent.x.is_some()
            || self.extent.y.is_some()
            || self.zoom.x.is_some()
            || self.zoom.y.is_some()
    }
}

/// Moves the spaces `view` follows to it, on their axes, as its own scroll moves them.
pub(super) fn move_leaders(
    spaces: &mut HashMap<String, SpaceView>,
    view: &SpaceView,
    follow: &tree::Follow,
) {
    if let Some(leader) = follow.x.as_ref().and_then(|key| spaces.get_mut(key)) {
        (leader.left, leader.zoom_x) = (view.left, view.zoom_x);
    }
    if let Some(leader) = follow.y.as_ref().and_then(|key| spaces.get_mut(key)) {
        (leader.top, leader.zoom_y) = (view.top, view.zoom_y);
    }
}

/// How far in from its edge, in points, a range `show` scrolls to or a page `turns` to sits.
const KEPT_IN: f64 = 24.0;

/// `view` scrolled for `show` when it changed, and across for the value of `turns`, `at`, when
/// it left what shows since the last draw, which a scroll by hand away from it is not. Once it
/// is laid out: before, it knows not what shows.
fn kept_in_view(
    mut view: SpaceView,
    show: Option<tree::Ranges>,
    at: Option<f64>,
    extent: &tree::Ranges,
) -> SpaceView {
    if view.width <= 0.0 {
        return view;
    }
    let (across, down) = view.scale();
    let (wide, tall) = (view.width / across, view.height / down);
    if show != view.shown {
        if let Some(show) = show {
            if let Some(range) = show.x {
                view.left = showing(view.left, wide, range, KEPT_IN / across);
            }
            if let Some(range) = show.y {
                view.top = showing(view.top, tall, range, KEPT_IN / down);
            }
        }
        view.shown = show;
    }
    if let (Some(at), Some(before)) = (at, view.turns_at) {
        let shows = |value: f64| value >= view.left && value <= view.left + wide;
        if shows(before) && !shows(at) {
            view.left = at - KEPT_IN / across;
        }
    }
    view.turns_at = at;
    clamped(view, extent)
}

/// The edge of an axis that shows `visible` units from `edge`, moved the least so `range` shows
/// with `margin` units beside it: not at all when it shows, and to its start when it is wider.
fn showing(edge: f64, visible: f64, [low, high]: [f32; 2], margin: f64) -> f64 {
    let (low, high) = (f64::from(low), f64::from(high));
    if low >= edge && high <= edge + visible {
        return edge;
    }
    if high - low > visible {
        return low - margin;
    }
    let margin = margin.min((visible - (high - low)) / 2.0);
    match low < edge {
        true => low - margin,
        false => high + margin - visible,
    }
}

/// `view` with the scroll and zoom of the spaces it follows, on their axes.
fn followed(
    mut view: SpaceView,
    follow: &tree::Follow,
    spaces: &HashMap<String, SpaceView>,
) -> SpaceView {
    if let Some(leader) = follow.x.as_ref().and_then(|key| spaces.get(key)) {
        (view.left, view.zoom_x) = (leader.left, leader.zoom_x);
    }
    if let Some(leader) = follow.y.as_ref().and_then(|key| spaces.get(key)) {
        (view.top, view.zoom_y) = (leader.top, leader.zoom_y);
    }
    view
}

/// `view` zoomed and scrolled so `fit` spans its width, as far as `zoom` goes.
fn fitted(mut view: SpaceView, fit: tree::Fit, zoom: Option<[f32; 2]>) -> SpaceView {
    let [from, to] = fit.x.map(f64::from);
    let room = (view.width - f64::from(fit.before + fit.after)).max(1.0);
    let wanted = room / ((to - from).max(f64::EPSILON) * view.unit_x);
    view.zoom_x = match zoom {
        Some([low, high]) => wanted.clamp(f64::from(low), f64::from(high)),
        None => wanted,
    };
    view.left = from - f64::from(fit.before) / view.scale().0;
    view.fitted = true;
    view
}

/// `view` scrolled inside `extent`: an axis without one shows from 0.
fn clamped(mut view: SpaceView, extent: &tree::Ranges) -> SpaceView {
    let (across, down) = view.scale();
    let hold = |edge: f64, range: Option<[f32; 2]>, shown: f64| match range {
        Some([low, high]) => edge.min(f64::from(high) - shown).max(f64::from(low)),
        None => 0.0,
    };
    view.left = hold(view.left, extent.x, view.width / across);
    view.top = hold(view.top, extent.y, view.height / down);
    view
}

/// Hears the pointer on a space: presses, drags, hovers, the wheel and pinches.
fn listen(
    frame: &Rc<SpaceFrame>,
    card: &WeakEntity<TypeScriptCard>,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    painted: Rc<Vec<Painted>>,
    window: &mut Window,
) {
    window.on_mouse_event({
        let (frame, card, painted, hitbox) =
            (frame.clone(), card.clone(), painted.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !heard(event.button) || !hitbox.is_hovered(window)
            {
                return;
            }
            let target = found(&frame, &painted, event.position);
            if target.is_none() && !frame.hears.any() && frame.drag.is_none() {
                return;
            }
            card.update(cx, |card, cx| {
                if card
                    .live
                    .read(cx)
                    .info(card.card)
                    .is_some_and(|info| info.keys)
                {
                    window.focus(&card.focus, cx);
                }
                card.press(&frame, target, bounds, event, cx);
            })
            .ok();
            cx.stop_propagation();
        }
    });
    window.on_mouse_event({
        let (frame, card, painted, hitbox) =
            (frame.clone(), card.clone(), painted.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window);
            card.update(cx, |card, cx| {
                if event.dragging() {
                    card.drag_to(&frame, event.position, event.modifiers, cx);
                    return;
                }
                if let Some(drag) = frame.drag.as_ref().filter(|drag| drag.turns()) {
                    card.point(&frame.key, drag, over, cx);
                }
                let hovered = found(&frame, &painted, event.position)
                    .filter(|_| over)
                    .and_then(|index| frame.pointed.get(index).cloned().flatten())
                    .map(|pointer| (frame.key.clone(), pointer.node));
                let hovered_here = card
                    .hovered
                    .as_ref()
                    .is_some_and(|(space, _)| *space == frame.key);
                if hovered.is_some() || hovered_here {
                    card.hover(hovered, cx);
                }
            })
            .ok();
        }
    });
    window.on_mouse_event({
        let (frame, card, hitbox) = (frame.clone(), card.clone(), hitbox.clone());
        move |event: &MouseUpEvent, phase, window, cx| {
            // Only the left button clicks and drags.
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let over = hitbox.is_hovered(window).then(|| {
                found(&frame, &painted, event.position)
                    .and_then(|index| Some(frame.pointed.get(index)?.as_ref()?.node))
                    .unwrap_or(frame.node)
            });
            (card.update(cx, |card, cx| card.pointer_up(&frame, over, cx))).ok();
        }
    });
    if !frame.scrolls() {
        return;
    }
    window.on_mouse_event({
        let (frame, card, hitbox) = (frame.clone(), card.clone(), hitbox.clone());
        move |event: &ScrollEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let view = frame_now(&frame, &card, bounds, cx).scrolled(event, bounds);
            card.update(cx, |card, cx| {
                card.moved_space(&frame.key, view, &frame.follow, cx)
            })
            .ok();
            cx.stop_propagation();
        }
    });
    let (frame, card) = (frame.clone(), card.clone());
    window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
            return;
        }
        let factor = f64::from(1.0 + event.delta);
        let view = frame_now(&frame, &card, bounds, cx).zoomed(factor, bounds, event.position);
        card.update(cx, |card, cx| {
            card.moved_space(&frame.key, view, &frame.follow, cx)
        })
        .ok();
        cx.stop_propagation();
    });
}

/// The frame with its view as the card has it now: several wheel events may come before the
/// next draw.
fn frame_now(
    frame: &SpaceFrame,
    card: &WeakEntity<TypeScriptCard>,
    bounds: Bounds<Pixels>,
    cx: &App,
) -> SpaceFrame {
    let mut view = (card.upgrade())
        .and_then(|card| {
            let spaces = card.read(cx).spaces.borrow();
            let view = spaces.get(&frame.key).copied()?;
            Some(followed(view, &frame.follow, &spaces))
        })
        .unwrap_or(frame.view);
    view.width = f64::from(f32::from(bounds.size.width));
    view.height = f64::from(f32::from(bounds.size.height));
    SpaceFrame {
        key: frame.key.clone(),
        node: frame.node,
        view,
        placed: Vec::new(),
        pointed: Vec::new(),
        background: None,
        radius: 0.0,
        hears: Hears::default(),
        drag: None,
        cursor: None,
        extent: frame.extent,
        zoom: frame.zoom,
        follow: frame.follow.clone(),
    }
}

#[cfg(test)]
mod tests {
    use gpui::size;

    use super::*;

    fn frame(view: SpaceView, extent: tree::Ranges, zoom: tree::Ranges) -> SpaceFrame {
        SpaceFrame {
            key: "roll".into(),
            node: 1,
            view,
            placed: Vec::new(),
            pointed: Vec::new(),
            background: None,
            radius: 0.0,
            hears: Hears::default(),
            drag: None,
            cursor: None,
            extent,
            zoom,
            follow: tree::Follow::default(),
        }
    }

    #[test]
    fn a_space_finds_the_units_under_the_pointer_and_zooms_about_them() {
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(400.), px(200.)));
        let view = SpaceView {
            left: 960.0,
            unit_x: 0.1,
            unit_y: 10.0,
            width: 400.0,
            height: 200.0,
            ..SpaceView::default()
        };
        let extent = tree::Ranges {
            x: Some([0.0, 38_400.0]),
            y: Some([0.0, 128.0]),
        };
        let zoom = tree::Ranges {
            x: Some([0.25, 8.0]),
            y: None,
        };
        let frame = frame(view, extent, zoom);
        let pointer = point(px(300.), px(150.));
        assert_eq!(frame.units_at(bounds, pointer), (2960.0, 10.0));
        let zoomed = frame.zoomed(2.0, bounds, pointer);
        assert_eq!(zoomed.zoom_x, 2.0);
        assert_eq!(zoomed.zoom_y, 1.0);
        let after = SpaceFrame {
            view: zoomed,
            ..frame
        };
        assert_eq!(after.units_at(bounds, pointer), (2960.0, 10.0));
    }

    #[test]
    fn a_space_scrolls_inside_its_extent_and_not_along_an_axis_without_one() {
        let view = SpaceView {
            left: 100.0,
            top: 3.0,
            width: 400.0,
            height: 200.0,
            ..SpaceView::default()
        };
        let held = clamped(
            view,
            &tree::Ranges {
                x: Some([0.0, 450.0]),
                y: None,
            },
        );
        assert_eq!((held.left, held.top), (50.0, 0.0));
    }
}
