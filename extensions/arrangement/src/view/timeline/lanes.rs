//! The automation lanes of a track: showing them, the select that adds one, and the press and
//! drag that add, move, erase and delete points.

use gpui::{
    App, Context, Entity, Modifiers, MouseDownEvent, SharedString, Subscription, div, prelude::*,
    px,
};
use sound_core::{Changes, InstanceId, Ticks};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::{Devices, DragEdit};

use super::Timeline;
use super::scene::{PointKey, Scene};
use super::state::{After, Held, LaneDrag, LaneDragKind};
use crate::automation::{devices, free_lanes};
use crate::view::lanes::{DRAG_THRESHOLD, erase_range};
use crate::view::layout::{
    ADD_LANE_HEIGHT, HEADER_INSET, HEADER_WIDTH, Part, RULER_HEIGHT, shifted,
};
use crate::view::track_lanes::{self, LANE_BOX};
use crate::{AutomationLane, AutomationValue, TrackState, travel_in};

/// The undo steps of the points of an automation lane.
const ADD_POINT_LABEL: &str = "Add automation point";
const MOVE_POINT_LABEL: &str = "Move automation point";
const DELETE_POINT_LABEL: &str = "Delete automation point";
const ERASE_LABEL: &str = "Erase automation";

/// The select under the lanes of a track that adds one: a number of the track or of one of its
/// devices that has none yet.
pub(super) struct LaneMenu {
    menu: Entity<DropdownMenu>,
    /// What it offered the last time it was filled, so it is filled again only when that
    /// changed.
    offered: Vec<SharedString>,
    _subscriptions: [Subscription; 2],
}

/// What the select that adds a lane says.
const ADD_LANE: &str = "Add lane";
/// The select that adds a lane is as wide as the shape of a selected header, and this tall.
const ADD_LANE_BUTTON_HEIGHT: f32 = 24.;

impl Timeline {
    /// The select that adds a lane to a track, while the track shows its lanes.
    pub fn lane_menu(&self, track: &InstanceId) -> Option<&Entity<DropdownMenu>> {
        self.lane_menus.get(track).map(|lane_menu| &lane_menu.menu)
    }

    /// Whether a track shows its automation lanes.
    pub fn shows_lanes(&self, track: &InstanceId) -> bool {
        self.expanded.contains(track)
    }

    /// Shows the automation lanes of a track under it, with the select that adds one, or
    /// folds them away. Interface state: not saved, no undo step.
    pub fn show_lanes(&mut self, track: &InstanceId, shown: bool, cx: &mut Context<Self>) {
        if !shown {
            self.expanded.remove(track);
            self.lane_menus.remove(track);
            // A point that does not show is not selected: delete would take it unseen.
            if self
                .selected_point
                .as_ref()
                .is_some_and(|key| key.track == *track)
            {
                self.selected_point = None;
            }
            cx.notify();
            return;
        }
        if !self.expanded.insert(track.clone()) {
            return;
        }
        let menu = cx.new(|cx| {
            DropdownMenu::new(ADD_LANE, Vec::new(), cx)
                .searchable("Search", cx)
                .debug_name(format!("add-lane-{}", track.name()))
                .trigger(Trigger::Select)
                .trigger_width(HEADER_WIDTH - 2. * HEADER_INSET)
                .width(240.)
                .max_height(320.)
        });
        let picked = cx.subscribe(&menu, {
            let track = track.clone();
            move |timeline, _, picked: &MenuPicked, cx| timeline.add_lane(&track, &picked.0, cx)
        });
        // What it offers is read again when it opens: a device may have come since.
        let opened = cx.observe(&menu, {
            let track = track.clone();
            move |timeline, menu, cx| {
                if menu.read(cx).is_open() {
                    timeline.fill_lane_menu(&track, cx);
                }
            }
        });
        self.lane_menus.insert(
            track.clone(),
            LaneMenu {
                menu,
                offered: Vec::new(),
                _subscriptions: [picked, opened],
            },
        );
        self.fill_lane_menu(track, cx);
        cx.notify();
    }

    /// Fills the select that adds a lane to `track` with what it can add now, when that is not
    /// what it holds: every number of the track and of its devices that has no lane yet, also
    /// one a device takes only once its record says so, such as a plugin's parameter that is
    /// not pinned. One group per device, in the order of the chain.
    pub(super) fn fill_lane_menu(&mut self, track: &InstanceId, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let Some(state) = project
            .resolve::<TrackState>(track)
            .and_then(|instance| project.state(&instance))
        else {
            return;
        };
        let free = free_lanes(project, track, state);
        let item = |device: Option<&str>, field: &str, label: String| {
            MenuItem::new(track_lanes::menu_value(device, field), label).selectable(false)
        };
        let mut groups: Vec<(Option<&str>, Vec<MenuItem>)> = Vec::new();
        let own = free.iter().filter(|lane| lane.device.is_none());
        let own = own.map(|lane| {
            item(
                None,
                &lane.parameter,
                track_lanes::lane_name(&lane.parameter),
            )
        });
        groups.push((None, own.collect()));
        for device in devices(state) {
            let of_device = free
                .iter()
                .filter(|lane| lane.device.as_deref() == Some(device));
            let mut items: Vec<MenuItem> = of_device
                .map(|lane| {
                    let name = self.number_name(track, device, &lane.parameter, cx);
                    item(Some(device), &lane.parameter, name)
                })
                .collect();
            if let Ok(id) = track.child(device) {
                let latent = Devices::latent_of(project, &id, cx).into_iter();
                items.extend(
                    latent.map(|number| item(Some(device), &number.field, number.name.into())),
                );
            }
            groups.push((Some(device), items));
        }
        groups.retain(|(_, items)| !items.is_empty());
        let offered: Vec<SharedString> = groups
            .iter()
            .flat_map(|(_, items)| items.iter().map(|item| item.value.clone()))
            .collect();
        let entries = groups.into_iter().map(|(device, items)| {
            let label = match device {
                None => SharedString::from("Track"),
                Some(device) => self.device_name(track, device, cx),
            };
            MenuEntry::Group(MenuGroup::new().label(label).items(items))
        });
        let entries: Vec<MenuEntry> = entries.collect();
        let Some(lane_menu) = self.lane_menus.get_mut(track) else {
            return;
        };
        if lane_menu.offered != offered {
            lane_menu.offered = offered;
            let menu = lane_menu.menu.clone();
            menu.update(cx, |menu, cx| menu.set_entries(entries, cx));
        }
    }

    /// What a device of a track is called, as its card says: `Filter` for `filter`.
    pub(super) fn device_name(&self, track: &InstanceId, device: &str, cx: &App) -> SharedString {
        let Ok(id) = track.child(device) else {
            return device.to_string().into();
        };
        match Devices::label_of(&self.session, &id, cx) {
            Some(label) => label.name,
            None => {
                let project = self.session.read(cx).project();
                project.tool_of(&id).unwrap_or(device).to_string().into()
            }
        }
    }

    /// What a number of a device of a track is called: as the device names it, such as the
    /// name a plugin record gives a pin, or else its field in plain words, the unit left out,
    /// as `Cutoff` for `cutoff_hz`. The unit is what the knob shows. A lane header puts the
    /// name of the device before it: `Filter · Cutoff`, the device cut short when it is long.
    pub(super) fn number_name(
        &self,
        track: &InstanceId,
        device: &str,
        field: &str,
        cx: &App,
    ) -> String {
        let id = track.child(device).ok();
        let named = id.and_then(|id| Devices::number_name(&self.session, &id, field, cx));
        named.map_or_else(|| track_lanes::number_name(field), String::from)
    }

    /// The select of the lanes of a track picked a number: a lane for it, which holds the
    /// value it plays now, so nothing sounds different yet. A number its device takes only once
    /// its record says so has its record changed with it. One undo step.
    fn add_lane(&mut self, track: &InstanceId, value: &str, cx: &mut Context<Self>) {
        let Some((device, field)) = track_lanes::from_menu_value(value) else {
            return;
        };
        let mut changes = Changes::new();
        {
            let project = self.session.read(cx).project();
            let Some(instance) = project.resolve::<TrackState>(track) else {
                return;
            };
            let Some(mut state) = project.state(&instance).cloned() else {
                return;
            };
            let mut lane = AutomationLane {
                device: device.map(str::to_string),
                parameter: field.to_string(),
                points: Vec::new(),
            };
            let number = lane.number(track, &state, &travel_in(project));
            let latent = || {
                let id = track.child(device?).ok()?;
                Devices::take_latent(project, &id, field, &mut changes, cx)
            };
            let Some(record) = number.and_then(|number| number.record).or_else(latent) else {
                return;
            };
            lane.points.push(sound_notes::Point {
                tick: Ticks(0),
                value: AutomationValue(record),
            });
            state.automation.push(lane);
            changes.set(&instance, state);
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| project.commit("Add automation", changes));
        });
    }

    /// The select that adds a lane, in the row under the lanes of each track that shows them.
    /// Clipped to the header column under the ruler, as the painted headers are, but not while
    /// one is open: GPUI clips a menu to where it was made.
    pub(super) fn lane_menu_column(&self, cx: &App) -> impl IntoElement {
        let rows = self.rows(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(self.viewport, width, height, cx);
        let mut open = false;
        let mut selects = Vec::new();
        for (row, track) in self.order.iter().enumerate() {
            let (Some(lanes), Some(lane_menu)) = (rows.lanes(row), self.lane_menus.get(track.id()))
            else {
                continue;
            };
            // Only one that shows whole: one under the ruler would paint over an open one. And
            // only while there is something left to add.
            let top = viewport.y_at(rows.lane_top(row, lanes))
                + (ADD_LANE_HEIGHT - ADD_LANE_BUTTON_HEIGHT) / 2.;
            let shows = top >= 0. && top + ADD_LANE_BUTTON_HEIGHT <= height;
            if !shows || lane_menu.offered.is_empty() {
                continue;
            }
            open |= lane_menu.menu.read(cx).is_open();
            selects.push(
                div()
                    .absolute()
                    .top(px(top))
                    .left(px(HEADER_INSET))
                    .occlude()
                    .child(lane_menu.menu.clone()),
            );
        }
        div()
            .absolute()
            .top(px(RULER_HEIGHT))
            .bottom_0()
            .left_0()
            .w(px(HEADER_WIDTH - 1.))
            .when(!open, |column| column.overflow_hidden())
            .children(selects)
    }

    /// A press in an automation lane, at `y` in the timeline area. On the dot of a point it
    /// selects the point, and a drag moves it. Anywhere else it adds a point there, on the grid
    /// unless cmd is held, selected, and a drag goes on to move it. With alt a drag erases the
    /// points it covers. A number the project does not know only erases.
    pub(super) fn press_lane(
        &mut self,
        row: usize,
        lane: usize,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(track) = self.order.get(row).cloned() else {
            return;
        };
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&track) else {
            return;
        };
        let Some(origin) = state.automation.get(lane).cloned() else {
            return;
        };
        let viewport = self.painted.get();
        let top = self.rows(cx).lane_top(row, lane);
        let in_lane = (viewport.content_y(y) - top) as f32;
        let range = origin.number(track.id(), state, &travel_in(project));
        let range = range.map(|number| number.range);
        let drag = |origin, kind, label| LaneDrag {
            track: track.clone(),
            origin,
            index: lane,
            top,
            kind,
            label,
            edit: DragEdit::default(),
        };
        let range = match (event.modifiers.alt, range) {
            (false, Some(range)) => range,
            (true, _) => {
                let kind = LaneDragKind::Erase {
                    from: viewport.tick_at(x),
                    moving: false,
                };
                self.select_point(None, cx);
                self.held = Held::Lane(drag(origin, kind, ERASE_LABEL));
                return;
            }
            (false, None) => return,
        };
        let press = (viewport.tick_at(x), in_lane);
        let point_drag = |point| LaneDragKind::Point {
            point,
            range,
            press,
            moving: false,
        };
        if let Some(point) = track_lanes::point_at(&viewport, &origin, range, (x, in_lane))
            && let Some(pressed) = origin.points.get(point)
        {
            let key = PointKey::of(track.id(), &origin, pressed.tick);
            self.select_point(Some(key), cx);
            self.held = Held::Lane(drag(origin, point_drag(point), MOVE_POINT_LABEL));
            return;
        }
        let tick = viewport.tick_at(x);
        let tick = match event.modifiers.platform {
            true => tick,
            false => self.grid(cx).snap(tick),
        };
        let value = AutomationValue(range.value(LANE_BOX.share_at(in_lane)));
        let (added, point) = track_lanes::with_point(&origin, tick, value);
        self.select_point(Some(PointKey::of(track.id(), &added, tick)), cx);
        let mut lane_drag = drag(added.clone(), point_drag(point), ADD_POINT_LABEL);
        let after = self.write_lane(&mut lane_drag, Some(added), cx);
        self.settle(Held::Lane(lane_drag), after, cx);
    }

    /// One mouse move of a drag in a lane, into the gesture of the session, which opens with
    /// the first change. A point moves on the grid unless cmd is held, and with shift only up
    /// and down or only sideways, the way the pointer went furthest.
    pub(super) fn drag_lane(
        &mut self,
        drag: &mut LaneDrag,
        x: f32,
        y: f32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> After {
        let viewport = self.painted.get();
        let grid = match modifiers.platform {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        let in_lane = (viewport.content_y(y) - drag.top) as f32;
        // An undo between mouse down and the first change took the lane the drag started from:
        // the drag ends, so it does not write that lane back.
        if !drag.edit.is_open() {
            let project = self.session.read(cx).project();
            let state = project.state(&drag.track);
            let mut lanes = state.into_iter().flat_map(|state| state.automation.iter());
            if lanes.find(|lane| lane.same_number(&drag.origin)) != Some(&drag.origin) {
                self.selected_point = None;
                cx.notify();
                return After::End;
            }
        }
        let next = match &mut drag.kind {
            LaneDragKind::Erase { from, moving } => {
                if !*moving && (x - viewport.x_of(*from)).abs() < DRAG_THRESHOLD {
                    return After::Keep;
                }
                *moving = true;
                let ticks = erase_range(&grid, *from, viewport.tick_at(x));
                track_lanes::erased(&drag.origin, &ticks)
            }
            LaneDragKind::Point {
                point,
                range,
                press,
                moving,
            } => {
                let (mut dx, mut dy) = (x - viewport.x_of(press.0), in_lane - press.1);
                if !*moving && dx.abs() < DRAG_THRESHOLD && dy.abs() < DRAG_THRESHOLD {
                    return After::Keep;
                }
                *moving = true;
                if modifiers.shift {
                    match dx.abs() >= dy.abs() {
                        true => dy = 0.,
                        false => dx = 0.,
                    }
                }
                let Some(&from) = drag.origin.points.get(*point) else {
                    return After::Keep;
                };
                let delta = match dx {
                    0. => 0,
                    _ => viewport.tick_at(x).0 as i64 - press.0.0 as i64,
                };
                let tick = match delta {
                    0 => from.tick,
                    delta => grid.snap(shifted(from.tick, delta)),
                };
                let value = match dy {
                    0. => from.value,
                    dy => {
                        let y = LANE_BOX.y_of(range.position(from.value.0)) + dy;
                        AutomationValue(range.value(LANE_BOX.share_at(y)))
                    }
                };
                let moved = track_lanes::moved_point(&drag.origin, *point, tick, value);
                let tick = moved.points.get(*point).map_or(tick, |point| point.tick);
                self.selected_point = Some(PointKey::of(drag.track.id(), &moved, tick));
                Some(moved)
            }
        };
        self.write_lane(drag, next, cx)
    }

    /// Puts `next`, the lane of a drag, in its place among the lanes of its track now, from
    /// where it was at mouse down: `None` takes it away, and a drag back puts it back there.
    /// The first change opens the gesture.
    fn write_lane(
        &mut self,
        drag: &mut LaneDrag,
        next: Option<AutomationLane>,
        cx: &mut Context<Self>,
    ) -> After {
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&drag.track) else {
            return After::End;
        };
        let mut automation = state.automation.clone();
        let found = automation
            .iter_mut()
            .find(|lane| lane.same_number(&drag.origin));
        match (found, next) {
            (Some(found), Some(lane)) => *found = lane,
            (Some(_), None) => automation.retain(|lane| !lane.same_number(&drag.origin)),
            (None, Some(lane)) => automation.insert(drag.index.min(automation.len()), lane),
            (None, None) => {}
        }
        if automation == state.automation {
            return After::Keep;
        }
        let next = TrackState {
            automation,
            ..state.clone()
        };
        let track = drag.track.clone();
        drag.edit
            .publish(&self.session, drag.label, cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&track, next);
                project.publish(edit, changes)
            });
        After::Keep
    }

    /// Selects a point of an automation lane and no clip or tempo change, so delete removes it.
    pub(super) fn select_point(&mut self, point: Option<PointKey>, cx: &mut Context<Self>) {
        if point.is_some() {
            self.select_clip(None, cx);
            self.select_tempo(None, cx);
        }
        if self.selected_point != point {
            self.selected_point = point;
            cx.notify();
        }
    }

    /// The point of a lane whose dot is at `(x, y)` in the timeline area.
    pub(super) fn point_at(&self, x: f32, y: f32, scene: &Scene, cx: &App) -> Option<PointKey> {
        let Some((row, Part::Lane(index))) = scene.viewport.part_at(&scene.layout, y) else {
            return None;
        };
        let track = self.order.get(row)?;
        let project = self.session.read(cx).project();
        let state = project.state(track)?;
        let lane = state.automation.get(index)?;
        let range = lane.number(track.id(), state, &travel_in(project))?.range;
        let top = scene.layout.lane_top(row, index);
        let in_lane = (scene.viewport.content_y(y) - top) as f32;
        let point = track_lanes::point_at(&scene.viewport, lane, range, (x, in_lane))?;
        Some(PointKey::of(track.id(), lane, lane.points.get(point)?.tick))
    }

    /// Delete with a point selected: it goes, and the lane with it when it was the last, as
    /// one undo step. Whether the point was there.
    pub(super) fn delete_point(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.selected_point.take() else {
            return false;
        };
        cx.notify();
        let project = self.session.read(cx).project();
        let Some(track) = project.resolve::<TrackState>(&key.track) else {
            return false;
        };
        let Some(state) = project.state(&track) else {
            return false;
        };
        let mut state = state.clone();
        let is_lane = |lane: &AutomationLane| key.is_in(&key.track, lane);
        let Some(lane) = state.automation.iter_mut().find(|lane| is_lane(lane)) else {
            return false;
        };
        let Some(point) = lane.points.iter().position(|point| point.tick == key.tick) else {
            return false;
        };
        match track_lanes::without_point(lane, point) {
            Some(left) => *lane = left,
            None => state.automation.retain(|lane| !is_lane(lane)),
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(&track, state);
                project.commit(DELETE_POINT_LABEL, changes)
            })
        });
        true
    }
}
