//! The toggles on the second line of each track header: the lanes at the left, mute and solo
//! at the right. The arm toggle of an audio track sits left of mute, from the recording
//! overlay.

use gpui::{App, Context, Div, SharedString, Window, div, prelude::*, px};
use sound_core::{Changes, Instance};
use sound_ui::ActiveTheme;
use sound_ui::components::toggle::{self, Toggle};

use super::Timeline;
use crate::TrackState;
use crate::view::layout::{HEADER_INSET, HEADER_WIDTH, RULER_HEIGHT, TOGGLES_MIDDLE};

/// The tooltip of the toggle of the lanes of a track.
const AUTOMATION: &str = "Automation";
/// The first toggle starts 4 pt inside the shape of a selected header and the last ends 4 pt
/// inside it, 4 pt apart.
const LANES_LEFT: f32 = HEADER_INSET + 4.;
const SOLO_LEFT: f32 = HEADER_WIDTH - HEADER_INSET - 4. - toggle::LETTER_WIDTH;
const MUTE_LEFT: f32 = SOLO_LEFT - 4. - toggle::LETTER_WIDTH;
pub(in crate::view) const ARM_LEFT: f32 = MUTE_LEFT - 4. - toggle::HEIGHT;

/// What a mute or solo toggle sets, and its undo steps when it turns off and on.
type Switch = (fn(&mut TrackState, bool), [&'static str; 2]);
const MUTE: Switch = (|state, on| state.mute = on, ["Unmute track", "Mute track"]);
const SOLO: Switch = (|state, on| state.solo = on, ["Unsolo track", "Solo track"]);

impl Timeline {
    /// Small, so a press on the rest of the header selects the track. Out of the tab order: a
    /// click gives the focus to the timeline, as a click on the rest of the header does, so its
    /// keys work after it, and `a` reaches the lanes.
    pub(super) fn header_toggles(&self, cx: &Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let (peach, yellow) = (cx.theme().peach, cx.theme().yellow);
        let rows = self.rows(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(self.viewport, width, height, cx);
        let mut toggles = Vec::new();
        // Every track, and the column clips them: the height of the last paint may be old.
        for (index, track) in self.order.iter().enumerate() {
            let Some(state) = project.state(track) else {
                continue;
            };
            let name = |kind: &str| SharedString::from(format!("{kind}-{}", track.id().name()));
            let shown = self.shows_lanes(track.id());
            let lanes = Toggle::icon(name("lanes"), "automation", AUTOMATION, shown)
                .count(state.automation.len())
                .tab_stop(false)
                .on_change(on_toggle(track, cx, |timeline, track, shown, cx| {
                    // Released: there is nothing left to show.
                    timeline.show_lanes(track.id(), shown, cx);
                }));
            let switch = |kind, letter, on, (set, labels): Switch| {
                Toggle::new(name(kind), letter, on)
                    .tab_stop(false)
                    .on_change(on_toggle(track, cx, move |timeline, track, on, cx| {
                        let label = labels[usize::from(on)];
                        timeline.set_track(track, label, |state| set(state, on), cx);
                    }))
            };
            let mute = switch("mute", "M", state.mute, MUTE).color(peach);
            let solo = switch("solo", "S", state.solo, SOLO).color(yellow);
            let top = viewport.y_of(&rows, index) + TOGGLES_MIDDLE - toggle::HEIGHT / 2.;
            // A toggle takes its own clicks: the header under it is not pressed.
            let at = |left: f32| div().absolute().top(px(top)).left(px(left)).occlude();
            toggles.extend([
                at(LANES_LEFT).child(lanes),
                at(MUTE_LEFT).child(mute),
                at(SOLO_LEFT).child(solo),
            ]);
        }
        header_column().overflow_hidden().children(toggles)
    }

    /// One undo step that changes the record of a track.
    fn set_track(
        &mut self,
        track: &Instance<TrackState>,
        label: &str,
        change: impl FnOnce(&mut TrackState),
        cx: &mut Context<Self>,
    ) {
        let Some(mut state) = self.session.read(cx).project().state(track).cloned() else {
            return;
        };
        change(&mut state);
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(track, state);
                project.commit(label, changes)
            })
        });
    }
}

/// A change of a toggle in the header of `track`. The timeline takes the focus first.
fn on_toggle(
    track: &Instance<TrackState>,
    cx: &Context<Timeline>,
    f: impl Fn(&mut Timeline, &Instance<TrackState>, bool, &mut Context<Timeline>) + 'static,
) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    let (track, timeline) = (track.clone(), cx.weak_entity());
    move |on, window, cx| {
        timeline
            .update(cx, |timeline, cx| {
                timeline.take_focus_from_press(window, cx);
                f(timeline, &track, on, cx);
            })
            .ok();
    }
}

/// The header column under the ruler, where the controls over the painted headers go.
pub(super) fn header_column() -> Div {
    div()
        .absolute()
        .top(px(RULER_HEIGHT))
        .bottom_0()
        .left_0()
        .w(px(HEADER_WIDTH - 1.))
}
