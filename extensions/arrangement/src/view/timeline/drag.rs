//! The mouse moves of a drag: clips moved, resized, trimmed, faded and turned up or down, and
//! a track dragged to another row.

use gpui::{Context, Modifiers, Window};
use sound_core::{Changes, Instance, InstanceId, Project, Ticks};
use sound_ui::DragEdit;

use super::Timeline;
use super::state::{
    After, ClipDrag, ClipDragKind, Edge, EdgeDrag, FADE_IN_LABEL, FADE_OUT_LABEL, GAIN_LABEL,
    GainDrag, Held, LaneDragKind, LaneGhost, MOVE_TRACK_LABEL, MoveDrag, ResizeDrag,
    TRACK_DRAG_THRESHOLD, TrackDrag,
};
use crate::clip_moves::{lane_moves, move_records, range_of, track_states};
use crate::view::clips::{
    GAIN_DB, GAIN_TRAVEL, fade_in, fade_out, fitted, gain_moved, trimmed_left, trimmed_right,
};
use crate::view::gesture::{nudged_track, resized_left, resized_right};
use crate::view::layout::{Rows, shifted};
use crate::view::plural;
use crate::view::snap::Grid;
use crate::{
    AnyClip, AudioClip, ClipMove, Moved, TrackKind, TrackState, automation, move_track, moved,
    track_orders, travel_in,
};

impl Timeline {
    /// One mouse move of what the mouse holds. It is taken out for the move and goes back in
    /// [`Self::settle`] alone, so no handler can lose it: each says how it goes on.
    pub(super) fn drag_held(
        &mut self,
        x: f32,
        y: f32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let mut held = std::mem::take(&mut self.held);
        let after = match &mut held {
            Held::Nothing => After::Keep,
            Held::Clips(drag, kind) => self.drag_to(drag, kind, (x, y), modifiers, cx),
            Held::Marquee(marquee) => {
                self.marquee_to(marquee, x, y, cx);
                After::Keep
            }
            Held::Track(drag) => self.drag_track(drag, y, cx),
            Held::Lane(drag) => self.drag_lane(drag, x, y, modifiers, cx),
        };
        self.settle(held, after, cx);
    }

    /// The mouse holds `held` again, or it ends when the handler said so.
    pub(super) fn settle(&mut self, held: Held, after: After, cx: &mut Context<Self>) {
        self.held = held;
        match after {
            After::Keep => {}
            After::End => self.end_drag(cx),
        }
    }

    /// One mouse move of a drag: the clips become what the pointer says, through the gesture
    /// of the session, so sound and every other view follow. Cmd held is no snap. Shift held
    /// moves the gain ten times finer. Alt held leaves the automation where it is when a clip
    /// moves.
    fn drag_to(
        &mut self,
        drag: &mut ClipDrag,
        kind: &mut ClipDragKind,
        (x, y): (f32, f32),
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> After {
        self.refresh_order(cx);
        let grid = match modifiers.platform {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        match kind {
            ClipDragKind::Move(moving) => {
                self.drag_move(drag, moving, (x, y), grid, modifiers.alt, cx)
            }
            ClipDragKind::Resize(resize) => self.drag_resize(drag, resize, x, grid, cx),
            ClipDragKind::Trim(trim) => self.drag_trim(drag, trim, x, grid, cx),
            ClipDragKind::Fade(fade) => self.drag_fade(drag, fade, x, cx),
            ClipDragKind::Gain(gain) => self.drag_gain(drag, gain, y, modifiers.shift, cx),
        }
    }

    /// Alt pressed or let go during a move of clips, or shift or cmd during a move of a point:
    /// the move again where the pointer is, so the automation goes along or stays, or the point
    /// takes its axis or the grid, at once and not at the next mouse move.
    pub(super) fn modifiers_changed(
        &mut self,
        modifiers: Modifiers,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.painted_bounds.get();
        let (x, y) = Self::timeline_position(bounds, window.mouse_position());
        let again = match &self.held {
            Held::Clips(_, kind) => matches!(kind, ClipDragKind::Move(_)),
            Held::Lane(drag) => matches!(drag.kind, LaneDragKind::Point { moving: true, .. }),
            Held::Nothing | Held::Marquee(_) | Held::Track(_) => false,
        };
        if again {
            self.drag_held(x, y, modifiers, cx);
        }
    }

    /// Whether the mouse has something: clips, a rectangle or a track. Keys then wait, as they
    /// would fight the next mouse move.
    pub(super) fn dragging(&self) -> bool {
        !matches!(self.held, Held::Nothing)
    }

    /// A press on a track header, which may become a drag of the track.
    pub(super) fn start_track_drag(&mut self, track: &InstanceId, y: f32, rows: Rows) {
        // The tracks are read at the first move, see `drag_track`.
        self.held = Held::Track(TrackDrag {
            track: track.clone(),
            rows,
            origin: Vec::new(),
            from: 0,
            at: 0,
            press: f64::from(y) + self.painted.get().scroll_y,
            moving: false,
            edit: DragEdit::default(),
        });
    }

    /// One mouse move of a track drag: the track goes to the row under the pointer, above the
    /// first the first and below the last the last. The master is not a track and stays last.
    fn drag_track(&mut self, drag: &mut TrackDrag, y: f32, cx: &mut Context<Self>) -> After {
        let viewport = self.painted.get();
        let moved = (f64::from(y) + viewport.scroll_y - drag.press).abs();
        if !drag.moving && moved < TRACK_DRAG_THRESHOLD {
            return After::Keep;
        }
        if !drag.moving {
            drag.moving = true;
            cx.notify();
        }
        let project = self.session.read(cx).project();
        let tracks = drag.origin.len();
        match drag.edit.is_open() {
            // Until the gesture opens, undo is free and an agent may write: the drag starts
            // from the tracks as they are now. Once it opens, it owns the orders.
            false => drag.origin = track_orders(project, self.arrangement.id()),
            // A track deleted from outside leaves the rows, and the others close up.
            true => drag
                .origin
                .retain(|(track, _)| project.state(track).is_some()),
        }
        let Some(from) = drag
            .origin
            .iter()
            .position(|(row, _)| *row.id() == drag.track)
        else {
            return After::End;
        };
        if !drag.edit.is_open() {
            (drag.from, drag.at) = (from, from);
        } else if drag.origin.len() != tracks {
            // The rows moved under the drag: publish again, wherever the pointer is.
            (drag.from, drag.at) = (from, usize::MAX);
        }
        let to = viewport
            .nearest_track(&drag.rows, y)
            .unwrap_or(drag.from)
            .min(drag.origin.len().saturating_sub(1));
        if to == drag.at {
            return After::Keep;
        }
        drag.edit
            .publish(&self.session, MOVE_TRACK_LABEL, cx, |project, edit| {
                let mut changes = Changes::new();
                move_track(project, &mut changes, &drag.origin, drag.from, to);
                project.publish(edit, changes)
            });
        drag.at = to;
        After::Keep
    }

    /// Alt-up and alt-down: the selected track one place up or down, one undo step. Whether a
    /// track is selected.
    pub(super) fn nudge_track(&mut self, step: i64, cx: &mut Context<Self>) -> bool {
        let Some(track) = self.selected_track.clone() else {
            return false;
        };
        let arrangement = self.arrangement.id().clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let tracks = track_orders(project, &arrangement);
                let Some(from) = tracks.iter().position(|(row, _)| *row.id() == track) else {
                    return Ok(());
                };
                let to = nudged_track(from, tracks.len(), step);
                let mut changes = Changes::new();
                move_track(project, &mut changes, &tracks, from, to);
                project.commit(MOVE_TRACK_LABEL, changes)
            })
        });
        true
    }

    /// The kind of the track on a row.
    fn kind_of_row(&self, row: usize, project: &Project) -> Option<TrackKind> {
        let track = self.order.get(row)?;
        project.state(track).map(|state| state.kind)
    }

    /// Whether every clip, by its row and its kind, lands on a track of its own kind when
    /// moved by `rows`.
    pub(super) fn fits(
        &self,
        clips: impl IntoIterator<Item = (usize, TrackKind)>,
        rows: i64,
        project: &Project,
    ) -> bool {
        clips.into_iter().all(|(row, kind)| {
            let row = row.checked_add_signed(rows as isize);
            row.and_then(|row| self.kind_of_row(row, project)) == Some(kind)
        })
    }

    /// A move of the selected clips: all by the same distance in time and in track rows. The
    /// earliest stops at tick 0 and the outer ones at the first and the last track, and the
    /// others keep their distance to them. A note clip goes on instrument tracks only and an
    /// audio clip on audio tracks only: over a track one of them cannot go on, they stay on
    /// the rows where they last could. Each clip takes the automation under it along, and
    /// with `alone` it leaves it where it is.
    fn drag_move(
        &mut self,
        drag: &mut ClipDrag,
        moving: &mut MoveDrag,
        (x, y): (f32, f32),
        grid: Grid,
        alone: bool,
        cx: &mut Context<Self>,
    ) -> After {
        let MoveDrag {
            clips,
            grab_row,
            grabbed,
            rows: last_rows,
            tracks,
            lanes_written,
            ghosts,
        } = moving;
        let label = plural(clips.len(), "Move clip", "Move clips");
        let project = self.session.read(cx).project();
        // A clip deleted from outside is left out of the move. When it is the one under the
        // pointer, the drag ends: the delete was the last write.
        let Some(grabbed_id) = clips.get(*grabbed).map(|moved| moved.clip.clone()) else {
            return After::End;
        };
        let lives: Vec<Option<AnyClip>> = clips
            .iter()
            .map(|moved| AnyClip::read(project, &moved.clip))
            .collect();
        let mut lives = lives.into_iter();
        clips.retain(|_| lives.next().flatten().is_some());
        let Some(index) = clips.iter().position(|moved| moved.clip == grabbed_id) else {
            return After::End;
        };
        *grabbed = index;
        let lives: Vec<AnyClip> = clips
            .iter()
            .filter_map(|moved| AnyClip::read(project, &moved.clip))
            .collect();
        // Once it moves, the drag owns the starts. Before that, an undo under the press may
        // have moved a clip.
        if !drag.edit.is_open() {
            for (moved, live) in clips.iter_mut().zip(&lives) {
                moved.range = range_of(project, live);
            }
            *tracks = track_states(project, self.arrangement.id());
        }
        let viewport = self.painted.get();
        let earliest = clips
            .iter()
            .map(|moved| moved.range.start.0)
            .min()
            .unwrap_or(0);
        // The clip under the pointer lands on the grid, and the others move with it.
        let anchor = clips
            .get(index)
            .map_or(Ticks(earliest), |moved| moved.range.start);
        let delta = grid.delta(anchor, drag.grab, viewport.tick_at(x));
        let delta = delta.max(-(earliest as i64));
        let rows = self.order.len();
        let (top, bottom) = (
            clips.iter().map(|moved| moved.row).min().unwrap_or(0),
            clips.iter().map(|moved| moved.row).max().unwrap_or(0),
        );
        let layout = self.rows(cx);
        let under_pointer = viewport.nearest_track(&layout, y).unwrap_or(*grab_row);
        let row_delta = (under_pointer as i64 - *grab_row as i64)
            .clamp(-(top as i64), rows.saturating_sub(1 + bottom) as i64);
        let placed = clips.iter().map(|moved| (moved.row, moved.kind));
        if self.fits(placed, row_delta, project) {
            *last_rows = row_delta;
        }
        let row_delta = *last_rows;
        // Each clip with the automation it would take along, one for one.
        let mut moves = Vec::new();
        for (moved, live) in clips.iter().zip(lives) {
            let row = moved.row.saturating_add_signed(row_delta as isize);
            let Some(to) = self.order.get(row).cloned() else {
                return After::Keep;
            };
            let next = live.with_start(shifted(moved.range.start, delta));
            moves.push(ClipMove {
                clip: moved.clip.clone(),
                home: moved.home.clone(),
                was: moved.range.clone(),
                to,
                next,
            });
        }
        let steps = lane_moves(&moves);
        // A clip without a track has no step, and the ghosts below need one for each.
        if steps.len() != moves.len() {
            return After::Keep;
        }
        let taken = match alone {
            true => Moved::default(),
            false => moved(tracks, &steps, &travel_in(project)),
        };
        let mut lanes = taken.lanes;
        // A track the drag wrote before and leaves now goes back to how it was.
        for track in lanes_written.iter() {
            if let Some(state) = tracks.get(track) {
                let automation = || state.automation.clone();
                lanes.entry(track.clone()).or_insert_with(automation);
            }
        }
        lanes_written.extend(lanes.keys().cloned());
        let same_lanes = lanes.iter().all(|(track, automation)| {
            let track = project.resolve::<TrackState>(track);
            let state = track.and_then(|track| project.state(&track));
            state.is_none_or(|state| state.automation == *automation)
        });
        let unchanged = same_lanes
            && moves.iter().all(|step| {
                step.clip.parent().as_ref() == Some(step.to.id())
                    && AnyClip::read(project, &step.clip).map(|live| live.start())
                        == Some(step.next.start())
            });
        if unchanged {
            return After::Keep;
        }
        let moved = drag
            .edit
            .publish(&self.session, label, cx, |project, edit| {
                let mut changes = Changes::new();
                let moved = move_records(project, &mut changes, moves)?;
                automation::write(project, &mut changes, lanes);
                project.publish(edit, changes)?;
                Ok(moved)
            });
        if let Some(moved) = moved {
            for (clip, now) in clips.iter_mut().zip(moved) {
                clip.clip = now;
            }
        }
        // What each clip takes along, for the ghosts and the hint.
        let carried = clips.iter().zip(steps).zip(taken.carried);
        let carried = carried.filter(|(_, lanes)| !lanes.is_empty());
        *ghosts = carried
            .map(|((clip, step), lanes)| LaneGhost {
                clip: clip.clip.clone(),
                range: step.start..step.start + step.range.end.saturating_sub(step.range.start),
                track: step.to,
                lanes,
            })
            .collect();
        let selected: Vec<_> = clips.iter().map(|moved| moved.clip.clone()).collect();
        let primary = selected.get(*grabbed).cloned();
        self.set_clips(selected, primary, cx);
        After::Keep
    }

    /// Publishes one mouse move of a drag of one audio clip into the gesture of the session,
    /// which opens with the first move that changes something.
    fn publish_audio(
        &mut self,
        drag: &mut ClipDrag,
        label: &str,
        clip: Instance<AudioClip>,
        next: AudioClip,
        cx: &mut Context<Self>,
    ) -> After {
        let project = self.session.read(cx).project();
        if project.state(&clip) == Some(&next) {
            return After::Keep;
        }
        drag.edit
            .publish(&self.session, label, cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&clip, next);
                project.publish(edit, changes)
            });
        cx.notify();
        After::Keep
    }

    /// A move of an edge of an audio clip: the part of its file that plays. The left edge keeps
    /// the sound where it is in time.
    fn drag_trim(
        &mut self,
        drag: &mut ClipDrag,
        trim: &mut EdgeDrag,
        x: f32,
        grid: Grid,
        cx: &mut Context<Self>,
    ) -> After {
        let EdgeDrag {
            clip,
            edge,
            origin,
            file,
        } = trim;
        let project = self.session.read(cx).project();
        let Some(live) = project.state(clip).cloned() else {
            return After::End;
        };
        // An undo between mouse down and the first change may have changed the clip.
        if !drag.edit.is_open() {
            *origin = live.clone();
        }
        let clock = project.clock();
        let anchor = match edge {
            Edge::Left => origin.start,
            Edge::Right => origin.end(Some(file), clock),
        };
        let delta = grid.delta(anchor, drag.grab, self.painted.get().tick_at(x));
        let unit = grid.unit_at(anchor);
        let next = match edge {
            Edge::Left => {
                let trimmed = trimmed_left(origin, file, clock, delta, unit);
                AudioClip {
                    start: trimmed.start,
                    file_start_seconds: trimmed.file_start_seconds,
                    ..live
                }
            }
            Edge::Right => {
                let trimmed = trimmed_right(origin, file, clock, delta, unit);
                AudioClip {
                    file_end_seconds: trimmed.file_end_seconds,
                    ..live
                }
            }
        };
        // The fades of the live clip, inside what it plays now.
        let next = fitted(next, file);
        let clip = clip.clone();
        self.publish_audio(drag, "Trim clip", clip, next, cx)
    }

    /// A move of a fade handle: the fade grows by the time the pointer went, in the time of the
    /// clip. No snap: a fade is a time and not a place on the grid.
    fn drag_fade(
        &mut self,
        drag: &mut ClipDrag,
        fade: &mut EdgeDrag,
        x: f32,
        cx: &mut Context<Self>,
    ) -> After {
        let EdgeDrag {
            clip,
            edge,
            origin,
            file,
        } = fade;
        let project = self.session.read(cx).project();
        let Some(live) = project.state(clip).cloned() else {
            return After::End;
        };
        // An undo between mouse down and the first change may have changed the fades.
        if !drag.edit.is_open() {
            *origin = live.clone();
        }
        let clock = project.clock();
        let went = clock.seconds_of(self.painted.get().tick_at(x)) - clock.seconds_of(drag.grab);
        let went = (went * 1000.) as f32;
        let next = match edge {
            Edge::Left => AudioClip {
                fade_in_ms: fade_in(&live, file, origin.fade_in_ms + went),
                ..live
            },
            Edge::Right => AudioClip {
                fade_out_ms: fade_out(&live, file, origin.fade_out_ms - went),
                ..live
            },
        };
        let label = match edge {
            Edge::Left => FADE_IN_LABEL,
            Edge::Right => FADE_OUT_LABEL,
        };
        let clip = clip.clone();
        self.publish_audio(drag, label, clip, next, cx)
    }

    /// A move of the gain handle: up is louder, 200 pt for the whole range as on a knob, and
    /// ten times finer with shift.
    fn drag_gain(
        &mut self,
        drag: &mut ClipDrag,
        gain: &mut GainDrag,
        y: f32,
        fine: bool,
        cx: &mut Context<Self>,
    ) -> After {
        let GainDrag {
            clip,
            from_db,
            from_y,
            fine: was_fine,
        } = gain;
        let project = self.session.read(cx).project();
        let Some(live) = project.state(clip).cloned() else {
            return After::End;
        };
        if fine != *was_fine {
            (*from_db, *from_y, *was_fine) = (live.gain_db, y, fine);
        }
        // An undo between mouse down and the first change may have changed the gain.
        if !drag.edit.is_open() {
            *from_db = live.gain_db;
        }
        let (bottom, top) = GAIN_DB;
        let speed = if fine { 0.1 } else { 1. };
        let db = (*from_y - y) * (top - bottom) / GAIN_TRAVEL * speed;
        // No move up or down leaves the gain as it is, also a `-inf` under the range.
        let gain_db = match db == 0. {
            true => live.gain_db,
            false => gain_moved(*from_db, db),
        };
        let next = AudioClip { gain_db, ..live };
        let clip = clip.clone();
        self.publish_audio(drag, GAIN_LABEL, clip, next, cx)
    }

    /// A move of an edge of one clip. It goes on from the live clip when that is not what the
    /// drag wrote last.
    fn drag_resize(
        &mut self,
        drag: &mut ClipDrag,
        resize: &mut ResizeDrag,
        x: f32,
        grid: Grid,
        cx: &mut Context<Self>,
    ) -> After {
        let label = "Resize clip";
        let ResizeDrag {
            clip,
            edge,
            origin,
            written,
            delta,
        } = resize;
        let project = self.session.read(cx).project();
        let Some(live) = project.state(clip).cloned() else {
            return After::End;
        };
        let pointer = self.painted.get().tick_at(x);
        // Something else wrote the clip. The drag goes on from that clip, and the grab moves
        // by what the drag had done to its edge, so the pointer still means the same distance.
        let rebased = live != *written;
        if rebased {
            let ticks = |ticks: Ticks| ticks.0 as i64;
            let done = match edge {
                Edge::Left => ticks(written.start) - ticks(origin.start),
                Edge::Right => ticks(written.length.ticks()) - ticks(origin.length.ticks()),
            };
            drag.grab = shifted(drag.grab, done);
            (*origin, *written) = (live.clone(), live);
        }
        let anchor = match edge {
            Edge::Left => origin.start,
            Edge::Right => origin.end(),
        };
        let next_delta = grid.delta(anchor, drag.grab, pointer);
        if !rebased && next_delta == *delta {
            return After::Keep;
        }
        *delta = next_delta;
        let next = match edge {
            Edge::Left => resized_left(origin, next_delta, grid.unit_at(anchor)),
            Edge::Right => resized_right(origin, next_delta, grid.unit_at(anchor)),
        };
        if next == *written {
            return After::Keep;
        }
        let first = !drag.edit.is_open();
        let (instance, wrote) = (clip.clone(), next.clone());
        let instance_id = clip.id().clone();
        let published = drag
            .edit
            .publish(&self.session, label, cx, |project, edit| {
                let mut changes = Changes::new();
                changes.set(&instance, next);
                project.publish(edit, changes)
            });
        if published.is_some() {
            *written = wrote;
        }
        // A cmd press left the selection alone until now: what is resized is selected.
        if first {
            self.select_clip(Some(instance_id), cx);
        }
        After::Keep
    }
}
