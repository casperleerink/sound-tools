//! What recording shows over the timeline: the arm toggle of every audio track in its header,
//! the meter of the input of an armed one, and each take while it records, growing to the
//! playhead.
//!
//! It is a view of its own next to the timeline, as the playhead line is, and not inside it:
//! the level changes every poll and a take every frame, and neither may make the timeline,
//! a cached view with every clip in it, draw again. The timeline draws again only when a track
//! is armed or disarmed, which shortens its name, and when a take starts or ends.

use std::collections::BTreeMap;

use gpui::{
    Context, Entity, IntoElement, Render, SharedString, Window, canvas, div, prelude::*, px,
};
use sound_core::InstanceId;
use sound_ui::components::meter::Meter;
use sound_ui::components::toggle::{self, Toggle};
use sound_ui::{ActiveTheme, InputLevels, Metering, Recording, Session};

use super::layout::{HEADER_WIDTH, NAME_MIDDLE, RULER_HEIGHT, TRACK_HEIGHT};
use super::timeline::{ARM_LEFT, ARMED_METER_HEIGHT, ARMED_METER_LEFT, Timeline, paint_takes};
use crate::TrackState;

pub struct RecordingOverlay {
    session: Entity<Session>,
    timeline: Entity<Timeline>,
    recording: Entity<Recording>,
    /// The meter of the input of each armed track.
    meters: BTreeMap<InstanceId, Metering>,
}

impl RecordingOverlay {
    pub(super) fn new(
        session: Entity<Session>,
        timeline: Entity<Timeline>,
        cx: &mut Context<Self>,
    ) -> Self {
        let recording = session.read(cx).recording().clone();
        let playhead = session.read(cx).playhead().clone();
        // A scroll, a zoom or another order of the tracks moves the toggles.
        cx.observe(&timeline, |_, _, cx| cx.notify()).detach();
        cx.observe(&recording, |overlay, recording, cx| {
            let recording = recording.read(cx);
            overlay.meters.retain(|track, _| recording.is_armed(track));
            cx.notify();
        })
        .detach();
        cx.subscribe(&recording, |overlay, _, _: &InputLevels, cx| {
            if overlay.read_meters(cx) {
                cx.notify();
            }
        })
        .detach();
        // A take ends at the playhead, so it grows with every move of it.
        cx.observe(&playhead, |overlay, _, cx| {
            if !overlay.recording.read(cx).takes().is_empty() {
                cx.notify();
            }
        })
        .detach();
        Self {
            session,
            timeline,
            recording,
            meters: BTreeMap::new(),
        }
    }

    /// One reading of the input for the meter of each armed track, from the channels its record
    /// names. Whether any meter shows something else now.
    fn read_meters(&mut self, cx: &mut Context<Self>) -> bool {
        let project = self.session.read(cx).project();
        let recording = self.recording.read(cx);
        let mut changed = false;
        for track in recording.armed() {
            let state = project
                .resolve::<TrackState>(track)
                .and_then(|track| project.state(&track));
            let Some(state) = state else {
                continue;
            };
            let level = recording.level(state.input.device_channels());
            let meter = self.meters.entry(track.clone()).or_default();
            changed |= meter.read_amplitudes(level);
        }
        changed
    }
}

impl Render for RecordingOverlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let red = cx.theme().red;
        let recording = self.recording.read(cx);
        let mut controls = Vec::new();
        for (top, track) in self.timeline.read(cx).audio_rows(cx) {
            let id = track.id().clone();
            let armed = recording.is_armed(&id);
            let session = self.session.clone();
            let arm = Toggle::dot(SharedString::from(format!("arm-{}", id.name())), armed)
                .color(red)
                .on_change({
                    let id = id.clone();
                    move |on, _, cx| {
                        let recording = session.read(cx).recording().clone();
                        let id = id.clone();
                        recording.update(cx, |recording, cx| recording.set_armed(id, on, cx));
                    }
                });
            let at = |left: f32, top: f32| div().absolute().left(px(left)).top(px(top));
            // The toggle takes its own clicks: the header under it is not pressed.
            controls.push(
                at(ARM_LEFT, top + (TRACK_HEIGHT - toggle::HEIGHT) / 2.)
                    .occlude()
                    .child(arm)
                    .into_any_element(),
            );
            if let Some(meter) = self.meters.get(&id).filter(|_| armed) {
                let name = SharedString::from(format!("input-{}", id.name()));
                let meter = Meter::new(name, meter.level()).horizontal();
                // On the line of the name.
                let meter_top = top + NAME_MIDDLE - ARMED_METER_HEIGHT / 2.;
                controls.push(
                    at(ARMED_METER_LEFT, meter_top)
                        .child(meter)
                        .into_any_element(),
                );
            }
        }
        let (timeline, session) = (self.timeline.clone(), self.session.clone());
        let recording = !recording.takes().is_empty();
        let takes = canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| {
                if !recording {
                    return;
                }
                // The timeline painted before this, so its viewport is this frame's.
                let width = f32::from(bounds.size.width) - HEADER_WIDTH;
                let shapes = timeline.read(cx).take_shapes(width, cx);
                let assets = session.read(cx).project().assets().clone();
                paint_takes(&shapes, bounds, &assets, window, cx);
            },
        )
        .absolute()
        .inset_0();
        div()
            .absolute()
            .inset_0()
            .child(takes)
            // Clipped to the header column under the ruler, as the painted headers are.
            .child(
                div()
                    .absolute()
                    .top(px(RULER_HEIGHT))
                    .bottom_0()
                    .left_0()
                    .w(px(HEADER_WIDTH - 1.))
                    .overflow_hidden()
                    .children(controls),
            )
    }
}
