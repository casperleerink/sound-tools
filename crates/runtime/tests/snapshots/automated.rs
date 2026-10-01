//! Controls that automation lanes move, in the window:
//!
//! - `track-panel-automated.png`: the bass with the synth and a Filter after it, playing
//!   halfway up a lane that sweeps the cutoff, with a lane that holds the volume of the track.
//!   The cutoff knob, the handle on the display and the volume show what the lanes play, the
//!   knob and the volume with the mark of an automated control.

use anyhow::{Context as _, Result};
use arrangement::{AutomationLane, AutomationValue, TrackState};
use filter::FilterState;
use gpui::HeadlessAppContext;
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::Point;

use super::{BAR, Opened, piece};

fn lane(device: Option<&str>, parameter: &str, points: &[(u64, f32)]) -> AutomationLane {
    let points = points.iter().map(|&(tick, value)| Point {
        tick: Ticks(tick),
        value: AutomationValue(value),
    });
    AutomationLane {
        device: device.map(str::to_string),
        parameter: parameter.to_string(),
        points: points.collect(),
    }
}

pub fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let mut opened = Opened::new(cx, |project| {
        piece(project)?;
        let id = InstanceId::new("arrangement/bass")?;
        let track = project
            .resolve::<TrackState>(&id)
            .context("the track is not there")?;
        let mut changes = Changes::new();
        let slot = arrangement::add_effect(project, &mut changes, &track, "Filter")?;
        let filter = slot.name().to_string();
        changes.create(slot, FilterState::default());
        project.commit("Add Filter", changes)?;
        let mut state = project.state(&track).context("no track record")?.clone();
        let sweep = [(0, 200.), (4 * BAR, 8_000.)];
        state.automation = vec![
            lane(Some(&filter), "cutoff_hz", &sweep),
            lane(None, "gain_db", &[(0, -6.)]),
        ];
        let mut changes = Changes::new();
        changes.set(&track, state);
        project.commit("Automate", changes)?;
        anyhow::ensure!(project.problems().is_empty(), "{:?}", project.problems());
        Ok(())
    })?;
    opened.play_from(Ticks(2 * BAR), cx)?;
    opened.click_track_header(1., cx)?;
    opened.listen(0.2, cx)?;
    save(cx, &opened, "track-panel-automated")?;
    Ok(())
}
