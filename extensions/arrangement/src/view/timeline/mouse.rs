//! The mouse on the timeline: what a press takes, the ruler and its tempo changes, the
//! marquee, the end of a drag, hover and the cursor.

use std::collections::{BTreeMap, BTreeSet};

use gpui::{Bounds, Context, CursorStyle, MouseDownEvent, Pixels, Point, Window};
use sound_core::{Changes, InstanceId, Project, Ticks};
use sound_media::{AudioAsset, Cached, Info};
use sound_notes::Clip;
use sound_ui::DragEdit;
use sound_ui::components::audio_clip::ClipHandle;

use super::scene::{Grip, LANES_TOGGLE_RIGHT, Scene};
use super::state::{
    ClipDrag, ClipDragKind, Edge, EdgeDrag, GainDrag, Held, LaneDragKind, Marquee, MoveDrag,
    MovedClip, OnRelease, ResizeDrag,
};
use super::{Timeline, TimelineEvent};
use crate::clip_moves::range_of;
use crate::view::gesture::{Zone, new_clip};
use crate::view::layout::{HEADER_WIDTH, LANES_MIDDLE, Part, RULER_HEIGHT, ordered};
use crate::{AnyClip, AudioClip, TrackKind, TrackState, add_clip, shown_end};

impl Timeline {
    /// The position of a mouse event in the coordinates of [`layout`].
    pub(super) fn timeline_position(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - bounds.left()) - HEADER_WIDTH,
            f32::from(position.y - bounds.top()) - RULER_HEIGHT,
        )
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        (x, y): (f32, f32),
        scene: &Scene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A press whose release never came: end what it held, or its gesture stays open.
        if self.dragging() {
            self.end_drag(cx);
        }
        let double = event.click_count == 2;
        // Shift and cmd add to the selection or take out of it, as in the Finder.
        let (shift, cmd) = (event.modifiers.shift, event.modifiers.platform);
        let adds = shift || cmd;
        let part = scene.viewport.part_at(&scene.layout, y);
        if x < 0.0 || y < 0.0 || !matches!(part, Some((_, Part::Lane(_)))) {
            self.select_point(None, cx);
        }
        if x < 0.0 {
            // The corner above the headers holds the snap setting, which takes its own clicks.
            if y < 0.0 {
                return;
            }
            // The header of a track, not of its lanes. Its second line is the toggle of its
            // lanes; elsewhere it is selected, and a double click edits its name.
            let Some((row, Part::Track)) = part else {
                return;
            };
            let in_row = y - scene.viewport.y_of(&scene.layout, row);
            if in_row >= LANES_MIDDLE - 8. && x + HEADER_WIDTH < LANES_TOGGLE_RIGHT {
                if let Some(track) = self.order.get(row).map(|track| track.id().clone()) {
                    let shown = self.shows_lanes(&track);
                    self.show_lanes(&track, !shown, cx);
                }
                return;
            }
            if let Some(track) = self.order.get(row).cloned() {
                self.select_track(Some(track.id().clone()), cx);
                cx.emit(TimelineEvent::OpenTrack(track.clone()));
                if double {
                    self.start_rename(track, window, cx);
                } else {
                    self.start_track_drag(track.id(), y, scene.layout.clone());
                }
            }
            return;
        }
        if y < 0.0 {
            self.on_ruler(x, double, scene, cx);
            return;
        }
        match part {
            Some((row, Part::Lane(lane))) => return self.press_lane(row, lane, event, x, y, cx),
            Some((_, Part::AddLane)) => return,
            Some((_, Part::Track)) | None => {}
        }
        let Some((shape, grip)) = scene.zone_at(x, y) else {
            if double {
                self.add_clip_at(x, y, scene, cx);
            } else {
                self.start_marquee(x, y, adds, cx);
            }
            return;
        };
        let id = shape.id.clone();
        if shift {
            self.toggle_clip(id, cx);
            return;
        }
        if double {
            self.select_clip(Some(id.clone()), cx);
            self.open(&id, cx);
            return;
        }
        let grab = self.painted.get().tick_at(x);
        let kind = match grip {
            Grip::Zone(Zone::Body) => self.start_move(&id, cmd, cx),
            Grip::Zone(zone @ (Zone::LeftEdge | Zone::RightEdge)) => {
                let edge = match zone {
                    Zone::LeftEdge => Edge::Left,
                    _ => Edge::Right,
                };
                self.start_edge(&id, edge, cmd, cx)
            }
            Grip::Handle(handle) => self.start_handle(&id, handle, y, cx),
        };
        let Some(kind) = kind else {
            return;
        };
        let several = matches!(&kind, ClipDragKind::Move(moved) if moved.clips.len() > 1);
        let on_release = match (cmd, several) {
            (true, _) => Some(OnRelease::Toggle(id)),
            (false, true) => Some(OnRelease::SelectAlone(id)),
            (false, false) => None,
        };
        let drag = ClipDrag {
            grab,
            edit: DragEdit::default(),
            on_release,
        };
        self.held = Held::Clips(drag, kind);
    }

    /// Opens what a clip is edited in: the note editor of a note clip, the track panel of the
    /// track of an audio clip, whose Clip card shows it.
    pub(super) fn open(&mut self, clip: &InstanceId, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        if let Some(notes) = project.resolve::<Clip>(clip) {
            cx.emit(TimelineEvent::OpenEditor(notes));
            return;
        }
        let track = clip
            .parent()
            .and_then(|track| project.resolve::<TrackState>(&track));
        if let Some(track) = track {
            cx.emit(TimelineEvent::OpenTrack(track));
        }
    }

    /// A press on an edge: a resize of a note clip, a trim of an audio clip.
    fn start_edge(
        &mut self,
        id: &InstanceId,
        edge: Edge,
        cmd: bool,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        // A cmd press changes the selection when it comes up, or with the first move.
        if !cmd {
            self.select_clip(Some(id.clone()), cx);
        }
        let project = self.session.read(cx).project();
        if let Some(clip) = project.resolve::<Clip>(id) {
            let state = project.state(&clip)?.clone();
            return Some(ClipDragKind::Resize(ResizeDrag {
                clip,
                edge,
                origin: state.clone(),
                written: state,
                delta: 0,
            }));
        }
        let clip = project.resolve::<AudioClip>(id)?;
        let origin = project.state(&clip)?.clone();
        // A clip whose file is missing has nothing to trim.
        let file = known_file(project, &origin.asset)?;
        Some(ClipDragKind::Trim(EdgeDrag {
            clip,
            edge,
            origin,
            file,
        }))
    }

    /// A press on a handle of an audio clip: a fade or the gain, from where it is.
    fn start_handle(
        &mut self,
        id: &InstanceId,
        handle: ClipHandle,
        y: f32,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        if !self.clips.contains(id) {
            self.select_clip(Some(id.clone()), cx);
        }
        let project = self.session.read(cx).project();
        let clip = project.resolve::<AudioClip>(id)?;
        let origin = project.state(&clip)?.clone();
        let file = known_file(project, &origin.asset)?;
        Some(match handle {
            ClipHandle::FadeIn => ClipDragKind::Fade(EdgeDrag {
                clip,
                edge: Edge::Left,
                origin,
                file,
            }),
            ClipHandle::FadeOut => ClipDragKind::Fade(EdgeDrag {
                clip,
                edge: Edge::Right,
                origin,
                file,
            }),
            ClipHandle::Gain => ClipDragKind::Gain(GainDrag {
                clip,
                from_db: origin.gain_db,
                from_y: y,
                fine: false,
            }),
        })
    }

    /// Shift-click and cmd-click: the clip in or out of the selection.
    fn toggle_clip(&mut self, clip: InstanceId, cx: &mut Context<Self>) {
        let mut clips = self.clips.clone();
        clips.toggle(clip);
        let primary = clips.primary().cloned();
        self.set_clips(clips.iter().cloned().collect::<Vec<_>>(), primary, cx);
    }

    /// A press on the body of a clip: a move of it, or of every selected clip when it is one of
    /// them. With cmd held (`keeps`) the selection stays as it is until the first move, and the
    /// clip moves with it. A selected clip that the project no longer has is left out. `None`
    /// when the pressed clip is gone.
    fn start_move(
        &mut self,
        pressed: &InstanceId,
        keeps: bool,
        cx: &mut Context<Self>,
    ) -> Option<ClipDragKind> {
        self.refresh_order(cx);
        let selected = self.clips.contains(pressed);
        let mut ids: Vec<InstanceId> = match (selected, keeps) {
            (false, false) => vec![pressed.clone()],
            _ => self.clips.iter().cloned().collect(),
        };
        if !selected && keeps {
            ids.push(pressed.clone());
        }
        if !keeps {
            self.set_clips(ids.clone(), Some(pressed.clone()), cx);
        }
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for id in ids {
            let Some(clip) = AnyClip::read(project, &id) else {
                continue;
            };
            let Some(row) = id.parent().and_then(|track| self.row_of(&track)) else {
                continue;
            };
            clips.push(MovedClip {
                clip: id.clone(),
                home: id,
                kind: clip.kind(),
                row,
                range: range_of(project, &clip),
            });
        }
        let grabbed = clips.iter().position(|moved| moved.clip == *pressed)?;
        let grab_row = clips.get(grabbed)?.row;
        Some(ClipDragKind::Move(MoveDrag {
            clips,
            grab_row,
            grabbed,
            rows: 0,
            tracks: BTreeMap::new(),
            lanes_written: BTreeSet::new(),
            ghosts: Vec::new(),
        }))
    }

    /// A press in the ruler: on a tempo mark it selects the tempo change and moves the playhead
    /// onto it, so the tempo of the transport is its tempo and a drag there edits it. Anywhere
    /// else it moves the playhead to the grid, and a double click adds a tempo change there.
    fn on_ruler(&mut self, x: f32, double: bool, scene: &Scene, cx: &mut Context<Self>) {
        if let Some(tick) = scene.tempo_at(x) {
            self.select_tempo(Some(tick), cx);
            self.seek(tick, cx);
            return;
        }
        let tick = self.grid(cx).snap(scene.viewport.tick_at(x));
        if double {
            self.add_tempo_change(tick, cx);
        } else {
            self.select_tempo(None, cx);
        }
        self.seek(tick, cx);
    }

    fn seek(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, _| session.engine().seek(tick));
    }

    /// Adds a tempo change at `tick` with the tempo that plays there, and selects it. A change
    /// that is there already is selected.
    pub(super) fn add_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let Some(tempo_map) = project.project_file().tempo_map.with_change_at(tick) else {
                    return Ok(());
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit("Add tempo change", changes)
            })
        });
        let project = self.session.read(cx).project();
        if project.project_file().tempo_map.change_at(tick).tick == tick && tick > Ticks(0) {
            self.select_tempo(Some(tick), cx);
        }
    }

    pub(super) fn remove_tempo_change(&mut self, tick: Ticks, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let map = &project.project_file().tempo_map;
                let Some(tempo_map) = map.without_change_at(tick) else {
                    return Ok(());
                };
                let mut changes = Changes::new();
                changes.set_tempo_map(tempo_map);
                project.commit("Remove tempo change", changes)
            })
        });
        self.select_tempo(None, cx);
    }

    /// A double click on empty track space: a clip of one bar in the cell under the pointer.
    fn add_clip_at(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let row = scene.viewport.track_at(&scene.layout, y);
        let Some(track) = row.and_then(|row| self.order.get(row)).cloned() else {
            return;
        };
        // An audio track plays audio clips only, which come from a file and not from a click.
        let project = self.session.read(cx).project();
        if project
            .state(&track)
            .is_some_and(|state| state.kind == TrackKind::Audio)
        {
            return;
        }
        let clip = new_clip(scene.viewport.tick_at(x), &self.grid(cx));
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let added = add_clip(project, &mut changes, &track, "clip", clip)?;
                project.commit("Add clip", changes)?;
                Ok(added)
            })
        });
        if let Some(added) = added {
            self.select_clip(Some(added.id().clone()), cx);
        }
    }

    /// A press on empty track space begins a rectangle that selects what it touches. Without
    /// shift or cmd it starts from nothing selected.
    fn start_marquee(&mut self, x: f32, y: f32, adds: bool, cx: &mut Context<Self>) {
        let at_press = (
            self.clips.iter().cloned().collect(),
            self.clips.primary().cloned(),
        );
        if !adds {
            self.select_clip(None, cx);
        }
        self.select_tempo(None, cx);
        let viewport = self.painted.get();
        let corner = (viewport.tick_at(x), f64::from(y) + viewport.scroll_y);
        self.held = Held::Marquee(Marquee {
            from: corner,
            to: corner,
            before: self.clips.iter().cloned().collect(),
            at_press,
        });
    }

    /// One mouse move of the rectangle: the clips it touches and what was selected before.
    pub(super) fn marquee_to(
        &mut self,
        marquee: &mut Marquee,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        self.refresh_order(cx);
        let viewport = self.painted.get();
        let layout = self.rows(cx);
        marquee.to = (viewport.tick_at(x), viewport.content_y(y));
        let (left, right) = ordered(marquee.from.0, marquee.to.0);
        let rows = layout.between(marquee.from.1, marquee.to.1);
        let project = self.session.read(cx).project();
        let mut selected = marquee.before.clone();
        for track in self.order.get(rows).unwrap_or_default() {
            let notes = project.children::<Clip>(track.id());
            let notes = notes.filter(|(_, clip)| clip.start <= right && clip.end() > left);
            selected.extend(notes.map(|(clip, _)| clip.id().clone()));
            let audio = project.children::<AudioClip>(track.id());
            let audio =
                audio.filter(|(_, clip)| clip.start <= right && shown_end(project, clip) > left);
            selected.extend(audio.map(|(clip, _)| clip.id().clone()));
        }
        let primary = self.clips.primary().cloned();
        self.set_clips(selected, primary, cx);
        cx.notify();
    }

    /// Mouse up, or a clip went away under the drag: the gesture becomes one undo step. A press
    /// that did not move changes the selection as the click it was, see [`OnRelease`].
    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        let mut held = std::mem::take(&mut self.held);
        let finished = held
            .edit()
            .is_some_and(|edit| edit.finish(&self.session, cx));
        if !finished && let Held::Clips(drag, _) = held {
            match drag.on_release {
                Some(OnRelease::SelectAlone(pressed)) => self.select_clip(Some(pressed), cx),
                Some(OnRelease::Toggle(pressed)) => self.toggle_clip(pressed, cx),
                None => {}
            }
        }
        cx.notify();
    }

    /// Escape: the clips or the track go back to where they were at mouse down. Whether there
    /// was a drag.
    pub(super) fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let mut held = std::mem::take(&mut self.held);
        let begun = held
            .edit()
            .is_some_and(|edit| edit.cancel(&self.session, cx));
        match held {
            Held::Nothing => return false,
            Held::Marquee(marquee) => {
                let (clips, primary) = marquee.at_press;
                self.set_clips(clips, primary, cx);
            }
            Held::Track(_) => {}
            // The point goes back, or away when the press added it: it is not selected any more.
            Held::Lane(_) => self.selected_point = None,
            Held::Clips(_, kind) => {
                if begun && let ClipDragKind::Move(MoveDrag { clips, grabbed, .. }) = kind {
                    let homes: Vec<_> = clips.into_iter().map(|moved| moved.home).collect();
                    let primary = homes.get(grabbed).cloned();
                    self.set_clips(homes, primary, cx);
                }
            }
        }
        cx.notify();
        true
    }

    /// The cursor says what a drag from here does, and the audio clip under the pointer shows
    /// its handles.
    pub(super) fn hover(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        let inside = x >= 0.0 && y >= 0.0;
        let zone = scene.zone_at(x, y).filter(|_| inside);
        let cursor = zone.and_then(|(_, grip)| match grip {
            Grip::Zone(Zone::Body) => None,
            Grip::Zone(Zone::LeftEdge | Zone::RightEdge) => Some(CursorStyle::ResizeLeftRight),
            Grip::Handle(ClipHandle::FadeIn | ClipHandle::FadeOut) => {
                Some(CursorStyle::ResizeLeftRight)
            }
            Grip::Handle(ClipHandle::Gain) => Some(CursorStyle::ResizeUpDown),
        });
        let hovered = zone.map(|(shape, _)| shape.id.clone());
        // In a lane, a press on a dot takes the point, and anywhere else adds one.
        let in_lane = matches!(
            scene.viewport.part_at(&scene.layout, y),
            Some((_, Part::Lane(_)))
        );
        let point = self.point_at(x, y, scene, cx).filter(|_| inside);
        let cursor = match (inside && in_lane, &point) {
            (true, Some(_)) => Some(CursorStyle::PointingHand),
            (true, None) => Some(CursorStyle::Crosshair),
            (false, _) => cursor,
        };
        if self.hover_cursor != cursor || self.hovered != hovered || self.hovered_point != point {
            (self.hover_cursor, self.hovered) = (cursor, hovered);
            self.hovered_point = point;
            cx.notify();
        }
    }

    /// The pointer left the timeline: no clip shows its handles for it any more.
    pub(super) fn unhover(&mut self, cx: &mut Context<Self>) {
        let left = self.hovered_point.take().is_some();
        if self.hovered.take().is_some() || self.hover_cursor.take().is_some() || left {
            cx.notify();
        }
    }

    pub(super) fn cursor(&self) -> Option<CursorStyle> {
        match &self.held {
            Held::Track(drag) if drag.moving => Some(CursorStyle::ClosedHand),
            Held::Lane(drag) => Some(match drag.kind {
                LaneDragKind::Erase { .. } => CursorStyle::Crosshair,
                LaneDragKind::Point { .. } => CursorStyle::PointingHand,
            }),
            Held::Clips(_, kind) => kind.cursor(),
            Held::Nothing | Held::Marquee(_) | Held::Track(_) => self.hover_cursor,
        }
    }
}

/// not read the disk. `None` for a file that is missing, does not play, or is not known yet.
fn known_file(project: &Project, asset: &AudioAsset) -> Option<Info> {
    match sound_media::cached(project.assets(), asset) {
        Cached::Plays(file) => Some(file),
        Cached::DoesNotPlay(_) | Cached::Missing | Cached::Unknown => None,
    }
}
