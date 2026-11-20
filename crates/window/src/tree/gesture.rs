//! What a pointer does to an element: a press, a drag, a paint or a right press. Rune presses
//! an element also when a drag ends on it, and once more on each press of a double click: here
//! a release is a press or the end of a drag, never both, and each press says which click it is.

use std::rc::Rc;

use rune::{Cursor, El, Phase, PointerEvent};
use serde::Serialize;
use serde_json::json;
use sound_core::InstanceId;
use sound_typescript::live::Num;

use crate::app::{State, plays};
use crate::lives::Hand;

use super::semantics::Press;
use super::{Bound, BoxNode, DragSpec, Opens, Scope};

/// A press or the end of a drag, as `Pointer` of `sdk/view.ts`: where it began and how far it
/// went, in the units of the plane around the element, else in points of the element.
#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct Pointer {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    pub(crate) clicks: u32,
    pub(crate) right: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) cmd: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) over: Option<String>,
}

impl DragSpec {
    /// Where a drag by `moved` units ends: `from` moved lands on the grid unless `free`, then
    /// the move stays in its bounds. An axis it does not move along stays.
    pub(crate) fn landed(&self, moved: [f32; 2], from: [f32; 2], free: bool) -> [f32; 2] {
        let axis = |along: Option<Bound>, moved: f32, from: f32, step: f32| {
            let snapped = match step > 0.0 && !free {
                true => ((from + moved) / step).round() * step - from,
                false => moved,
            };
            match along {
                None => 0.0,
                Some(Bound::Free) => snapped,
                Some(Bound::Within(low, high)) => snapped.clamp(low, high),
            }
        };
        [
            axis(self.x, moved[0], from[0], self.snap[0]),
            axis(self.y, moved[1], from[1], self.snap[1]),
        ]
    }

    /// The pointer over it, by the axes it moves along.
    fn cursor(&self) -> Cursor {
        match (self.x.is_some(), self.y.is_some()) {
            (true, true) => Cursor::Grab,
            (true, false) => Cursor::ResizeColumn,
            _ => Cursor::ResizeRow,
        }
    }
}

/// How far the pointer travels before a press is a drag, in points: a drag never also presses.
const DRAG_SLOP: f32 = 3.0;
/// How much of the pointer's travel a drag moves with Shift held.
const FINE: f32 = 0.1;

/// A press on an element that may become a drag.
pub(crate) struct Gesture {
    pub(super) key: String,
    drag: Option<Dragging>,
    /// Where the pointer was from the press, and how far it moved the element, in points: with
    /// Shift held, a tenth as far.
    last: [f32; 2],
    moved: [f32; 2],
    /// It went past the slop: a drag, and no press.
    far: bool,
}

/// What a drag began with.
struct Dragging {
    /// What `live.drag` follows, its key in the window.
    id: String,
    spec: Rc<DragSpec>,
    /// Points per unit of the plane around it, each axis.
    per: [f32; 2],
    /// The point that lands on the grid, and where the pointer began, in units.
    from: [f32; 2],
    start: [f32; 2],
}

impl Gesture {
    /// The drag an element follows now, once past the slop: its id in the window.
    pub(crate) fn dragging(&self) -> Option<&str> {
        (self.drag.as_ref())
            .filter(|_| self.far)
            .map(|drag| drag.id.as_str())
    }

    fn follow(&mut self, d: &rune::Drag, fine: bool) {
        let by = if fine { FINE } else { 1.0 };
        self.moved[0] += (d.dx - self.last[0]) * by;
        self.moved[1] += (d.dy - self.last[1]) * by;
        self.last = [d.dx, d.dy];
        self.far |= d.dx.abs().max(d.dy.abs()) > DRAG_SLOP;
    }

    /// Where its drag lands now, in units, and the hand `live.drag` follows.
    fn hand(&self, drag: &Dragging, mods: rune::Mods, active: bool) -> Hand {
        let moved = [self.moved[0] / drag.per[0], self.moved[1] / drag.per[1]];
        Hand {
            moved: drag.spec.landed(moved, drag.from, mods.cmd),
            start: drag.start,
            active,
            alt: mods.alt,
        }
    }
}

/// The held press ends without a release, as a cancelled one ends it.
pub(crate) fn cancel_gesture(s: &mut State) {
    let Some(gesture) = s.gesture.take() else {
        return;
    };
    s.drops.set_over(None);
    if let Some(drag) = gesture.drag.filter(|_| gesture.far) {
        drag_cancelled(s, &drag.id);
    }
}

/// The drag of `id` ends without a drop: the sound from before it, and the hand back.
fn drag_cancelled(s: &mut State, id: &str) {
    plays::cancelled(s);
    s.lives.move_hand(s, id, Hand::default());
}

/// What the pointer does to the element `b`, keyed `key` in the window: press, drag, paint,
/// right press, and how the pointer looks over it.
pub(super) fn pointer(scope: &Scope, mut e: El, b: &BoxNode, key: &str, press: &Press) -> El {
    let (menu, plane) = (scope.menu.clone(), scope.plane.clone());
    if b.press || b.opens_menu() {
        e = right_press(e, press, plane.clone(), menu);
    }
    let (paints, drag) = (b.enter, b.drag.clone().map(Rc::new));
    if !(b.press || paints || drag.is_some()) {
        return e;
    }
    e = e.cursor(match &drag {
        Some(_) if b.plane.is_some() => Cursor::Crosshair,
        Some(drag) => drag.cursor(),
        None => Cursor::Pointer,
    });
    let from = scope.placed.clone();
    let (press, key) = (press.clone(), key.to_string());
    let id = (drag.as_ref()).map(|drag| scope.key(drag.id.as_deref().unwrap_or(&press.element)));
    let moves_element = (drag.as_ref()).is_some_and(|drag| drag.plays.is_none());
    e.on_drag(
        move |s: &mut State, d: &mut rune::Drag, tx: &mut rune::Tx| {
            let mods = tx.mods();
            match d.phase {
                Phase::Down if paints => s.paint(&press.instance, &press.element),
                Phase::Down => {
                    let start = d.window_point();
                    let dragging = drag.clone().zip(id.clone()).map(|(spec, id)| {
                        let per = (plane.as_deref()).map_or([1.0; 2], |plane| s.planes.per(plane));
                        let at = s.lives.value_of(s, &press.instance, from.as_ref());
                        Dragging {
                            from: spec.from.or(at).unwrap_or_default(),
                            start: units(s, plane.as_deref(), start, [d.start_x, d.start_y]),
                            id,
                            spec,
                            per,
                        }
                    });
                    if let Some(plays) = (drag.as_ref()).and_then(|drag| drag.plays.as_ref()) {
                        let values = played(s, &press.instance, plays);
                        plays::grab(s, &press.instance, &values);
                    }
                    s.gesture = Some(Gesture {
                        key: key.clone(),
                        drag: dragging,
                        last: [0.0; 2],
                        moved: [0.0; 2],
                        far: false,
                    });
                }
                Phase::Move if paints => {
                    let [x, y] = d.window_point();
                    tx.element_at(x, y, |s: &mut State, picked, _: &mut rune::Tx| {
                        if let Some(picked) = picked {
                            s.paint_over(picked.path());
                        }
                    });
                }
                Phase::Move => {
                    let Some(gesture) = s.gesture.as_mut().filter(|gesture| gesture.key == key)
                    else {
                        return;
                    };
                    gesture.follow(d, mods.shift);
                    // An element dragged to another, not a control the sound follows.
                    if gesture.far && moves_element {
                        let (press, plane) = (press.clone(), plane.clone());
                        let [x, y] = d.window_point();
                        tx.element_at(x, y, move |s: &mut State, picked, _: &mut rune::Tx| {
                            s.drag_over(&press, plane.as_deref(), [x, y], picked.as_ref());
                        });
                    }
                    if let (true, Some(drag)) = (gesture.far, &gesture.drag) {
                        let hand = gesture.hand(drag, mods, true);
                        let (id, spec) = (drag.id.clone(), drag.spec.clone());
                        s.lives.move_hand(s, &id, hand);
                        if !plays_moved(s, &press.instance, spec.plays.as_ref()) {
                            s.lives.move_hand(s, &id, Hand::default());
                        }
                        tx.live_only();
                    }
                }
                // A press that paints is no press too.
                Phase::Up if paints => {
                    s.dragged = Some(key.clone());
                    s.painted(d.cancelled());
                }
                Phase::Up => {
                    let Some(gesture) = s.gesture.take().filter(|gesture| gesture.key == key)
                    else {
                        return;
                    };
                    s.drops.set_over(None);
                    // Rune presses it after this: the release is this press, or the end of a drag.
                    s.dragged = Some(key.clone());
                    let Some(drag) = gesture.drag.as_ref().filter(|_| gesture.far) else {
                        // Escape or the window losing the keys lets go of it.
                        if !gesture.far && !d.cancelled() && (press.pressed || press.opens) {
                            let start = [d.start_x, d.start_y];
                            let window = [d.window_point()[0] - d.dx, d.window_point()[1] - d.dy];
                            let [x, y] = units(s, plane.as_deref(), window, start);
                            let pointer = Pointer {
                                x,
                                y,
                                clicks: d.clicks,
                                ..Pointer::of(mods)
                            };
                            s.press(&press, "press", Some(json!(pointer)));
                        }
                        return;
                    };
                    let id = drag.id.clone();
                    if d.cancelled() {
                        drag_cancelled(s, &id);
                        return;
                    }
                    if !plays::let_go(s) {
                        s.lives.move_hand(s, &id, Hand::default());
                        return;
                    }
                    let hand = gesture.hand(drag, mods, false);
                    s.lives.move_hand(s, &id, hand);
                    let pointer = Pointer {
                        x: drag.start[0],
                        y: drag.start[1],
                        dx: hand.moved[0],
                        dy: hand.moved[1],
                        clicks: d.clicks,
                        ..Pointer::of(mods)
                    };
                    let (press, plane) = (press.clone(), plane.clone());
                    let [x, y] = d.window_point();
                    tx.element_at(x, y, move |s: &mut State, picked, _: &mut rune::Tx| {
                        let over = s.over(&press, plane.as_deref(), [x, y], picked.as_ref());
                        let pointer = Pointer { over, ..pointer };
                        s.pointed(
                            &press.instance,
                            &press.element,
                            "drag",
                            Some(json!(pointer)),
                        );
                    });
                }
            }
        },
    )
}

/// A right press, the menu key or Shift-F10 on `e`, or on what is inside that takes none:
/// its press hears it, then the menu around it opens at the pointer.
fn right_press(e: El, press: &Press, plane: Option<String>, menu: Option<String>) -> El {
    let press = press.clone();
    e.on_secondary_press_local(move |s: &mut State, p: PointerEvent, tx: &mut rune::Tx| {
        let window = [p.window_x, p.window_y];
        let [x, y] = units(s, plane.as_deref(), window, [p.x, p.y]);
        let pointer = Pointer {
            x,
            y,
            right: true,
            ..Pointer::of(tx.mods())
        };
        if press.pressed {
            s.press(&press, "press", Some(json!(pointer)));
        }
        if let Some(opener) = &menu {
            s.menu_at(opener, Some(window));
        }
    })
}

impl Pointer {
    /// A pointer with these keys held: the rest is told after.
    pub(crate) fn of(mods: rune::Mods) -> Self {
        Self {
            shift: mods.shift,
            alt: mods.alt,
            cmd: mods.cmd,
            clicks: 1,
            ..Self::default()
        }
    }
}

/// Where the window point `window` is in the units of `plane`, else `local`, points in the
/// element.
pub(super) fn units(
    state: &State,
    plane: Option<&str>,
    window: [f32; 2],
    local: [f32; 2],
) -> [f32; 2] {
    plane.map_or(local, |plane| state.planes.units(plane, window))
}

/// A move of a drag that `plays`: its values as they are now, after the hand moved. Whether
/// the drag goes on.
fn plays_moved(state: &mut State, instance: &InstanceId, plays: Option<&Plays>) -> bool {
    let Some(plays) = plays else {
        return true;
    };
    let values = played(state, instance, plays);
    let values: Vec<(&str, f64)> = values.iter().map(|(path, value)| (*path, *value)).collect();
    plays::moved(state, instance, &values)
}

/// What a drag plays now, by path.
fn played<'a>(state: &State, instance: &InstanceId, plays: &'a Plays) -> Vec<(&'a str, f64)> {
    (plays.iter())
        .map(|(path, value)| {
            (
                path.as_str(),
                f64::from(state.lives.value(state, instance, value)),
            )
        })
        .collect()
}

/// The numbers of a record a drag plays, by path, and what they play.
type Plays = std::collections::HashMap<String, Num>;

impl BoxNode {
    /// Whether a right press on it, or on anything inside, opens a menu of its own.
    pub(crate) fn opens_menu(&self) -> bool {
        self.overlay_of(Opens::Menu).is_some()
    }
}

impl crate::lives::Lives {
    /// The point `at` of the view of `instance` is now, if there is one.
    pub(crate) fn value_of(
        &self,
        state: &State,
        instance: &InstanceId,
        at: Option<&[Num; 2]>,
    ) -> Option<[f32; 2]> {
        at.map(|at| {
            at.each_ref()
                .map(|value| self.value(state, instance, value))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: serde_json::Value) -> DragSpec {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_drag_lands_its_point_on_the_grid_then_stays_in_its_bounds() {
        let clip = spec(json!({ "x": [-480, 100000], "snap": [240, 1] }));
        assert_eq!(clip.landed([130.0, 7.0], [480.0, 0.0], false), [240.0, 0.0]);
        assert_eq!(clip.landed([130.0, 0.0], [480.0, 0.0], true), [130.0, 0.0]);
        assert_eq!(
            clip.landed([-1000.0, 0.0], [480.0, 0.0], false),
            [-480.0, 0.0]
        );
        let swapped = spec(json!({ "y": [4, -4] }));
        assert_eq!(swapped.landed([3.0, 10.0], [0.0; 2], false), [0.0, 4.0]);
    }
}
