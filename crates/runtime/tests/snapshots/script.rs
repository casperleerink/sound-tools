//! A Script in the window:
//!
//! - `track-panel-script.png`: the bass with the synth and a tape echo an agent wrote, four
//!   params as four knobs, one of them turned.

use anyhow::{Context as _, Result};
use arrangement::TrackState;
use gpui::HeadlessAppContext;
use script::ScriptState;
use sound_core::{Changes, InstanceId};

use super::{Opened, piece};

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let opened = Opened::new(cx, |project| {
        piece(project)?;
        let id = InstanceId::new("arrangement/bass")?;
        let track = project
            .resolve::<TrackState>(&id)
            .context("the track is not there")?;
        let mut changes = Changes::new();
        let slot = arrangement::add_effect(project, &mut changes, &track, "Script")?;
        let code = [
            "param time = 350 [1, 2000]",
            "param feedback = 0.45 [0, 0.95]",
            "param tone = 2500 [200, 12000]",
            "param blend = 0.35 [0, 1]",
            "history echo",
            "wet = delay(in + echo * feedback, time)",
            "echo = saturate(lowpass(wet, tone))",
            "out = mix(in, wet, blend)",
        ];
        let sound = ScriptState {
            name: "Tape echo".into(),
            code: code.map(String::from).to_vec(),
            values: [("feedback".to_string(), 0.7)].into(),
        };
        changes.create(slot, sound);
        project.commit("Add tape echo", changes)?;
        Ok(())
    })?;
    opened.click_track_header(1., cx)?;
    save(cx, &opened, "track-panel-script")?;
    Ok(())
}
