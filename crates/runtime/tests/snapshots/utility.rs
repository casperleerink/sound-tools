//! The Utility in the window:
//!
//! - `track-panel-utility.png`: the bass with the synth and a Utility after it: wider, a little
//!   down and to the right, with bass mono on.
//! - `track-panel-utility-expanded.png`: the same card expanded: bass mono and the inverts.

use anyhow::{Context as _, Result};
use arrangement::TrackState;
use gpui::HeadlessAppContext;
use sound_core::{Changes, InstanceId};
use utility::UtilityState;
use utility::view::UtilityView;

use super::{Opened, piece};

pub fn snapshots(
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
        let slot = arrangement::add_effect(project, &mut changes, &track, "Utility")?;
        let sound = UtilityState {
            gain_db: -3.0,
            pan: 0.2,
            width: 1.5,
            bass_mono: true,
            ..UtilityState::default()
        };
        changes.create(slot, sound);
        project.commit("Add Utility", changes)?;
        Ok(())
    })?;
    opened.click_track_header(1., cx)?;
    save(cx, &opened, "track-panel-utility")?;
    let view = opened.arrangement_view(cx)?;
    let card = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let card = panel.read(cx).device_views().nth(1).flatten().cloned();
        let card = card.context("the utility has no card")?;
        card.downcast::<UtilityView>()
            .map_err(|_| anyhow::anyhow!("the second card is not the utility"))
    })?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    cx.run_until_parked();
    save(cx, &opened, "track-panel-utility-expanded")?;
    Ok(())
}
