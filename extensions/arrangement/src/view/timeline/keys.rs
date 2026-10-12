//! The keys of the timeline: the clip keys, the track keys, and deleting and renaming a track.

use gpui::{
    App, Context, Focusable, KeyDownEvent, Modifiers, PromptLevel, Window, div, prelude::*, px,
};
use sound_core::{Changes, Instance};
use sound_notes::Clip;
use sound_ui::components::text_input::{InputSize, TextInput};

use super::state::{GAIN_LABEL, Rename};
use super::{Timeline, TimelineEvent, is_clip_tool};
use crate::view::clips::{GAIN_KEY_STEP_DB, gain_moved};
use crate::view::gesture::nudged_track;
use crate::view::layout::{HEADER_WIDTH, NAME_LEFT, NAME_MIDDLE, RULER_HEIGHT};
use crate::{AudioClip, TrackKind, TrackState, remove_return};

impl Timeline {
    /// Alt-up and alt-down: the gain of every selected audio clip by a decibel, as one undo
    /// step. Note clips have none. Whether there was one to change.
    fn step_gains(&mut self, step: f32, cx: &mut Context<Self>) -> bool {
        let project = self.session.read(cx).project();
        let clips: Vec<_> = self
            .clips
            .iter()
            .filter_map(|id| {
                let clip = project.resolve::<AudioClip>(id)?;
                let state = project.state(&clip)?.clone();
                Some((clip, state))
            })
            .collect();
        if clips.is_empty() {
            return false;
        }
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                for (clip, state) in clips {
                    let gain_db = gain_moved(state.gain_db, step);
                    changes.set(&clip, AudioClip { gain_db, ..state });
                }
                project.commit(GAIN_LABEL, changes)
            })
        });
        true
    }

    /// The keys of the focused timeline. Whether the key was one of them.
    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Keys that bubble up from a control inside, such as the name field of a track or the
        // snap setting, are that control's.
        if !self.focus_handle.is_focused(window) {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        let menu_key = match key {
            "menu" => !modifiers.modified(),
            "f10" => modifiers == Modifiers::shift(),
            _ => false,
        };
        if menu_key && !self.dragging() {
            return self.open_clip_menu(window, cx);
        }
        // Alt-up and alt-down: the gain of the selected audio clips, and with no clip selected,
        // the place of the selected track.
        if modifiers == Modifiers::alt() && !self.dragging() {
            let project = self.session.read(cx).project();
            let primary = self.clips.primary();
            let clip_selected = primary.is_some_and(|clip| is_clip_tool(project, clip));
            return match (key, clip_selected) {
                ("up", true) => self.step_gains(GAIN_KEY_STEP_DB, cx),
                ("down", true) => self.step_gains(-GAIN_KEY_STEP_DB, cx),
                ("up", false) => self.nudge_track(-1, cx),
                ("down", false) => self.nudge_track(1, cx),
                _ => false,
            };
        }
        let command = modifiers == Modifiers::secondary_key();
        if modifiers.modified() && !command {
            return false;
        }
        if key == "escape" && !command {
            if self.cancel_drag(cx) {
                return true;
            }
            // Then a selected tempo change or point lets go, and only then the panel below
            // closes.
            if self.selected_tempo.is_some() {
                self.select_tempo(None, cx);
                return true;
            }
            if self.selected_point.is_some() {
                self.select_point(None, cx);
                return true;
            }
            return false;
        }
        // The mouse has the clips: a key would fight the next mouse move.
        if self.dragging() {
            return false;
        }
        if command {
            return self.on_command(key, cx);
        }
        if key == "t" {
            // While playing, the playhead is between two frames of any grid: the change goes
            // to the nearest step. Stopped, it is where a click on the ruler put it.
            let playhead = *self.playhead.read(cx);
            let tick = match playhead.playing {
                true => self.grid(cx).snap(playhead.tick),
                false => playhead.tick,
            };
            self.add_tempo_change(tick, cx);
            return true;
        }
        if let Some(tick) = self.selected_tempo
            && matches!(key, "backspace" | "delete")
        {
            self.remove_tempo_change(tick, cx);
            return true;
        }
        if matches!(key, "backspace" | "delete") && self.delete_point(cx) {
            return true;
        }
        let project = self.session.read(cx).project();
        let primary = self.clips.primary().cloned();
        let Some(clip) = primary.filter(|clip| is_clip_tool(project, clip)) else {
            return self.on_track_key(key, window, cx);
        };
        match key {
            "enter" => self.open(&clip, cx),
            "backspace" | "delete" => self.delete_clips("Delete clip", "Delete clips", None, cx),
            "left" => self.nudge_in_time(false, cx),
            "right" => self.nudge_in_time(true, cx),
            "up" => self.nudge_to_track(-1, cx),
            "down" => self.nudge_to_track(1, cx),
            _ => return false,
        }
        true
    }

    /// The keys with cmd: select all, copy, cut, paste, duplicate, and cmd-down, which opens
    /// what is selected as enter does in the Finder.
    fn on_command(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "a" => self.select_all(cx),
            "c" => {
                self.copy(cx);
            }
            "x" => self.cut(cx),
            "v" => self.paste(cx),
            "d" => self.duplicate(cx),
            "down" => {
                let project = self.session.read(cx).project();
                let primary = self.clips.primary().cloned();
                if let Some(clip) = primary.filter(|clip| is_clip_tool(project, clip)) {
                    self.open(&clip, cx);
                } else {
                    let project = self.session.read(cx).project();
                    let selected = self.selected_track.as_ref();
                    let track = selected.and_then(|track| project.resolve::<TrackState>(track));
                    let Some(track) = track else {
                        return false;
                    };
                    cx.emit(TimelineEvent::OpenTrack(track));
                }
            }
            _ => return false,
        }
        true
    }

    /// The keys of the selected track, which it gets while no clip is selected: up and down
    /// select the track above or below, enter edits its name, backspace and delete delete it,
    /// and `a` shows its automation lanes or folds them away, as the toggle in its header does.
    fn on_track_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.refresh_order(cx);
        let selected = self.selected_track.as_ref();
        let Some(current) = selected.and_then(|id| self.row_of(id)) else {
            return false;
        };
        if key == "a" {
            let Some(track) = self.order.get(current).map(|track| track.id().clone()) else {
                return false;
            };
            let shown = self.shows_lanes(&track);
            self.show_lanes(&track, !shown, cx);
            return true;
        }
        if matches!(key, "backspace" | "delete") {
            let Some(track) = self.order.get(current).cloned() else {
                return false;
            };
            self.delete_track(track, window, cx);
            return true;
        }
        let next = match key {
            "enter" => current,
            "up" => nudged_track(current, self.order.len(), -1),
            "down" => nudged_track(current, self.order.len(), 1),
            _ => return false,
        };
        let Some(track) = self.order.get(next).cloned() else {
            return false;
        };
        if key == "enter" {
            self.start_rename(track, window, cx);
        } else {
            self.select_track(Some(track.id().clone()), cx);
        }
        true
    }

    /// Deletes a track with its clips, devices and automation as one undo step, once the composer
    /// says yes. The keys go to the track when no clip is selected, so without the question one
    /// backspace too many after deleting a clip would take the whole track. A return takes the
    /// sends into it along.
    pub(in crate::view) fn delete_track(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = self.session.read(cx).project();
        let Some(state) = project.state(&track) else {
            return;
        };
        let is_return = state.kind == TrackKind::Return;
        let (message, detail, label) = match is_return {
            true => (
                format!("Delete the return \"{}\"?", state.name),
                "Its effects, automation and the sends into it go with it. Undo brings it back.",
                "Delete return",
            ),
            false => (
                format!("Delete the track \"{}\"?", state.name),
                "Its clips, devices and automation go with it. Undo brings it back.",
                "Delete track",
            ),
        };
        let buttons = ["Delete", "Cancel"];
        let answer = window.prompt(PromptLevel::Warning, &message, Some(detail), &buttons, cx);
        let session = self.session.clone();
        cx.spawn(async move |_, cx| {
            if answer.await != Ok(0) {
                return;
            }
            session.update(cx, |session, cx| {
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    match is_return {
                        true => remove_return(project, &mut changes, &track),
                        false => changes.delete(track.id()),
                    }
                    project.commit(label, changes)
                })
            });
        })
        .detach();
    }

    /// Opens the name field of a track in its header, with the name selected.
    pub fn start_rename(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self
            .session
            .read(cx)
            .project()
            .state(&track)
            .map(|state| state.name.clone())
        else {
            return;
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx).size(InputSize::Sm);
            input.set_text(name, cx);
            input.select_all_text(cx);
            input
        });
        let this = cx.weak_entity();
        input.update(cx, |input, _| {
            let submit = this.clone();
            input.set_on_submit(move |text, window, cx| {
                if let Some(timeline) = submit.upgrade() {
                    let text = text.to_string();
                    timeline.update(cx, |timeline, cx| {
                        timeline.finish_rename(Some(text), window, cx)
                    });
                }
            });
            input.set_on_cancel(move |_, window, cx| {
                if let Some(timeline) = this.upgrade() {
                    timeline.update(cx, |timeline, cx| timeline.finish_rename(None, window, cx));
                }
            });
        });
        let focus = input.focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |timeline, window, cx| {
            let text = timeline
                .rename
                .as_ref()
                .map(|rename| rename.input.read(cx).text().to_string());
            timeline.finish_rename(text, window, cx);
        });
        let focus_before = window.focused(cx);
        window.focus(&focus, cx);
        self.rename = Some(Rename {
            track,
            input,
            focus,
            focus_before,
            _blur: blur,
        });
        cx.notify();
    }

    /// Enter or a click elsewhere gives the name, one undo step; escape gives nothing. A name of
    /// only spaces is no name, and the track keeps the one it had. Enter and escape give the
    /// focus back to where it was, so the keys of a return work on after its rename.
    fn finish_rename(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        if rename.focus.is_focused(window) {
            let back = rename
                .focus_before
                .unwrap_or_else(|| self.focus_handle.clone());
            window.focus(&back, cx);
        }
        let project = self.session.read(cx).project();
        let state = project.state(&rename.track).cloned();
        let name = name.map(|name| name.trim().to_string());
        if let (Some(mut state), Some(name)) = (state, name)
            && !name.is_empty()
            && name != state.name
        {
            state.name = name;
            let track = rename.track;
            self.session.update(cx, |session, cx| {
                session.edit(cx, |project| {
                    let mut changes = Changes::new();
                    changes.set(&track, state);
                    project.commit("Rename track", changes)
                })
            });
        }
        cx.notify();
    }

    /// Cmd-a: every clip of the arrangement.
    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let project = self.session.read(cx).project();
        let mut clips = Vec::new();
        for track in &self.order {
            let notes = project.children::<Clip>(track.id());
            clips.extend(notes.map(|(clip, _)| clip.id().clone()));
            let audio = project.children::<AudioClip>(track.id());
            clips.extend(audio.map(|(clip, _)| clip.id().clone()));
        }
        let primary = self.clips.primary().cloned();
        self.set_clips(clips, primary, cx);
    }

    /// The name field over the header of the track being renamed, where the name is painted.
    pub(super) fn rename_field(&self, cx: &App) -> Option<gpui::AnyElement> {
        let rename = self.rename.as_ref()?;
        let row = self.row_of(rename.track.id())?;
        let rows = self.rows(cx);
        let top = self.painted.get().y_of(&rows, row);
        let top = RULER_HEIGHT + top + NAME_MIDDLE - RENAME_HEIGHT / 2.;
        Some(
            div()
                .debug_selector(|| "rename-track".to_string())
                .absolute()
                .top(px(top))
                .left(px(RENAME_LEFT))
                .w(px(HEADER_WIDTH - RENAME_LEFT - 12.))
                .occlude()
                .child(rename.input.clone())
                .into_any_element(),
        )
    }
}

/// The name field of a renamed track: a small text input where the name is painted.
pub(in crate::view) const RENAME_HEIGHT: f32 = 28.;
/// Its left edge, so that its text starts where the painted name does.
pub(in crate::view) const RENAME_LEFT: f32 = NAME_LEFT - 8.;
