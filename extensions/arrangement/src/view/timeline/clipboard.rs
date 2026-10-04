//! Copy, paste, duplicate, delete and nudge of the selected clips.

use gpui::Context;
use sound_core::{Changes, InstanceId, ProjectError, Ticks};

use super::Timeline;
use crate::clip_moves::{change_lanes, range_of};
use crate::view::clipboard::{Copied, CopiedClips};
use crate::view::layout::shifted;
use crate::view::plural;
use crate::{
    AnyClip, Carried, ClipMove, TrackKind, TrackState, add_audio_clips, add_clips, automation,
    move_clips, travel_in, unnumbered,
};

impl Timeline {
    /// What the selected clips are for the clipboard: each with its row, its name and the
    /// automation under it on the timeline.
    fn copied(&mut self, cx: &mut Context<Self>) -> Option<CopiedClips> {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let travel = travel_in(project);
        let clips = self
            .selected_states(cx)
            .into_iter()
            .filter_map(|(id, clip)| {
                let track = id.parent()?;
                let row = self.row_of(&track)?;
                let state = project.state(&project.resolve::<TrackState>(&track)?)?;
                let lanes = Carried::under(&track, state, range_of(project, &clip), &travel);
                Some((row, id.name().to_string(), clip, lanes))
            });
        CopiedClips::new(clips.collect::<Vec<_>>())
    }

    /// Cmd-c. What was copied, when there was something.
    pub(super) fn copy(&mut self, cx: &mut Context<Self>) -> Option<CopiedClips> {
        let copied = self.copied(cx)?;
        *self.clipboard.borrow_mut() = Some(Copied::Clips(copied.clone()));
        Some(copied)
    }

    /// Cmd-v: the copied clips at the playhead, the top one on the track of the first selected
    /// clip, else on the selected track, else on the first track. One undo step.
    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let copied = match self.clipboard.borrow().as_ref() {
            Some(Copied::Clips(copied)) => copied.clone(),
            _ => return,
        };
        self.refresh_order(cx);
        let track = match self.clips.primary() {
            Some(clip) => clip.parent(),
            None => self.selected_track.clone(),
        };
        let top = track.and_then(|track| self.row_of(&track)).unwrap_or(0);
        let at = self.playhead.read(cx).tick;
        let label = plural(copied.len(), "Paste clip", "Paste clips");
        self.add_copies(&copied, at, top, label, cx);
    }

    /// Cmd-d: a copy of the selected clips right after them, on the same tracks. The clipboard
    /// keeps what it had.
    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(copied) = self.copied(cx) else {
            return;
        };
        let (start, top) = copied.origin();
        let label = plural(copied.len(), "Duplicate clip", "Duplicate clips");
        self.add_copies(&copied, start + copied.span(), top, label, cx);
    }

    /// Adds copies of clips as one undo step and selects them, each with the automation that
    /// was under it, over what the lanes had where it lands. A note clip goes on an instrument
    /// track only and an audio clip on an audio track only: a paste that would put one on the
    /// other kind is refused as a whole, and the notice says why.
    fn add_copies(
        &mut self,
        copied: &CopiedClips,
        at: Ticks,
        top: usize,
        label: &str,
        cx: &mut Context<Self>,
    ) {
        let order = self.order.clone();
        let arrangement = self.arrangement.id().clone();
        let placed = copied.placed(at, top, order.len());
        let project = self.session.read(cx).project();
        let wrong = placed.iter().find_map(|(row, name, clip, _)| {
            let track = order.get(*row)?;
            let kind = project.state(track)?.kind;
            (kind != clip.kind()).then(|| (track.id().clone(), name.to_string(), clip.kind()))
        });
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                if let Some((track, name, kind)) = wrong {
                    return Err(wrong_track(&track, &name, kind));
                }
                let mut changes = Changes::new();
                change_lanes(project, &mut changes, &arrangement, |tracks, travel| {
                    for (row, _, clip, lanes) in &placed {
                        let Some(track) = order.get(*row) else {
                            continue;
                        };
                        if let Some(state) = tracks.get_mut(track.id()) {
                            lanes.place(track.id(), state, clip.start(), travel);
                        }
                    }
                });
                let (mut notes, mut audio) = (Vec::new(), Vec::new());
                for (row, name, clip, _) in placed {
                    let Some(track) = order.get(row) else {
                        continue;
                    };
                    match clip {
                        AnyClip::Notes(clip) => notes.push((track, name, clip)),
                        // A copy of `take-2` is `take` when that is free, as for notes.
                        AnyClip::Audio(clip) => audio.push((track, unnumbered(name), clip)),
                    }
                }
                let mut added: Vec<InstanceId> = add_clips(project, &mut changes, notes)?
                    .into_iter()
                    .map(|clip| clip.id().clone())
                    .collect();
                let audio = add_audio_clips(project, &mut changes, audio)?;
                added.extend(audio.into_iter().map(|clip| clip.id().clone()));
                project.commit(label, changes)?;
                Ok(added)
            })
        });
        if let Some(ids) = added {
            let primary = ids.first().cloned();
            self.set_clips(ids, primary, cx);
        }
    }

    /// Deletes the selected clips as one undo step. A cut also takes the automation under them,
    /// `taken`, as a move does: each lane becomes a straight line where they were.
    pub(super) fn delete_clips(
        &mut self,
        one: &'static str,
        several: &'static str,
        taken: Option<&CopiedClips>,
        cx: &mut Context<Self>,
    ) {
        let selected: Vec<_> = self.clips.iter().cloned().collect();
        let label = plural(selected.len(), one, several);
        let arrangement = self.arrangement.id().clone();
        let deleted = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for clip in &selected {
                    changes.delete(clip);
                }
                if let Some(taken) = taken {
                    // A cut takes every lane, as it would along its own track.
                    let taken: Vec<_> = taken.lanes().map(|lanes| (lanes, lanes.track())).collect();
                    change_lanes(project, &mut changes, &arrangement, |tracks, travel| {
                        for (track, state) in tracks {
                            automation::clear(track, state, &taken, travel);
                        }
                    });
                }
                project.commit(label, changes)
            })
        });
        if deleted.is_some() {
            let first = self.clips.primary().cloned();
            self.deleted = first.iter().cloned().collect();
            self.deleted
                .extend(selected.into_iter().filter(|id| Some(id) != first.as_ref()));
        }
    }

    /// The arrows left and right: every selected clip by one unit of the grid, as one undo
    /// step, with the automation under it. The earliest stops at tick 0. A moved audio clip goes
    /// on top of its track.
    pub(super) fn nudge_in_time(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let selected = self.selected_states(cx);
        let earliest = selected.iter().map(|(_, clip)| clip.start().0).min();
        let earliest = earliest.unwrap_or(0);
        let delta = self.grid(cx).nudge(Ticks(earliest), forward);
        let delta = delta.max(-(earliest as i64));
        if delta == 0 || selected.is_empty() {
            return;
        }
        let project = self.session.read(cx).project();
        let moves: Vec<ClipMove> = selected
            .into_iter()
            .filter_map(|(clip, state)| {
                let track = project.resolve::<TrackState>(&clip.parent()?)?;
                let start = shifted(state.start(), delta);
                Some(ClipMove {
                    home: clip.clone(),
                    clip,
                    was: range_of(project, &state),
                    to: track,
                    next: state.with_start(start),
                })
            })
            .collect();
        let label = plural(moves.len(), "Nudge clip", "Nudge clips");
        let arrangement = self.arrangement.id().clone();
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                move_clips(project, &mut changes, &arrangement, moves)?;
                project.commit(label, changes)
            })
        });
    }

    /// The arrows up and down: every selected clip to the nearest tracks above or below where
    /// each lands on a track of its own kind, as one undo step, with the volume and the pan
    /// under it. Nothing moves when there are none before the first or the last track.
    pub(super) fn nudge_to_track(&mut self, step: i64, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for (clip, next) in self.selected_states(cx) {
            let Some(row) = clip.parent().and_then(|track| self.row_of(&track)) else {
                return;
            };
            clips.push((row, clip, next));
        }
        let tracks = self.order.len() as i64;
        let rows = (1..tracks).map(|times| step * times).find(|rows| {
            let clips = clips.iter().map(|(row, _, next)| (*row, next.kind()));
            self.fits(clips, *rows, project)
        });
        let Some(rows) = rows else {
            return;
        };
        let mut moves = Vec::new();
        for (row, clip, next) in clips {
            let row = row.saturating_add_signed(rows as isize);
            let Some(to) = self.order.get(row).cloned() else {
                return;
            };
            moves.push(ClipMove {
                home: clip.clone(),
                clip,
                was: range_of(project, &next),
                to,
                next,
            });
        }
        if moves.is_empty() {
            return;
        }
        let label = plural(moves.len(), "Nudge clip", "Nudge clips");
        let primary = self.clips.primary().cloned();
        let index = moves
            .iter()
            .position(|step| Some(&step.clip) == primary.as_ref());
        let arrangement = self.arrangement.id().clone();
        let moved = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let moved = move_clips(project, &mut changes, &arrangement, moves)?;
                project.commit(label, changes)?;
                Ok(moved)
            })
        });
        if let Some(ids) = moved {
            let primary = index.and_then(|index| ids.get(index).cloned());
            self.set_clips(ids, primary, cx);
        }
    }
}

/// Why a clip cannot go where a paste would put it.
fn wrong_track(track: &InstanceId, name: &str, kind: TrackKind) -> ProjectError {
    let message = match kind {
        TrackKind::Instrument => {
            "a note clip goes on an instrument track, and this is an audio track"
        }
        TrackKind::Audio => "an audio clip goes on an audio track, and this is an instrument track",
    };
    let id = track.child(name).unwrap_or_else(|_| track.clone());
    ProjectError::WrongPlace {
        id,
        message: message.to_string(),
    }
}
