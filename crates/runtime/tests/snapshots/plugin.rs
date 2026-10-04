//! The card of a plugin with parameters pinned on it, in the window:
//!
//! - `track-panel-plugin-pins.png`: the CLAP test plugin with `Cutoff` and `Level` as knobs,
//!   `Wave` as a dropdown and `Bright` as a toggle, each with the plugin's own text.
//! - `track-panel-plugin-parameters.png`: the same with the list of its parameters open, the
//!   pinned ones checked.
//! - `track-panel-plugin-problems.png`: a pin the plugin has no parameter for and one whose
//!   value is out of range, next to one that plays.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use gpui::HeadlessAppContext;
use plugin_host::view::PluginView;
use plugin_host::{Pin, PluginFormat, PluginRecord};
use sound_core::{Changes, InstanceId, Project};

use super::Opened;

/// The instrument of `Track 1` is the CLAP test plugin with these pins.
fn with_pins(project: &mut Project, pins: &[(u32, &str, f64)]) -> Result<()> {
    let slot = InstanceId::new("arrangement/track-1/instrument")?;
    let mut changes = Changes::new();
    changes.delete(&slot);
    project.commit("Remove synth", changes)?;
    let mut record = PluginRecord::new(PluginFormat::Clap, test_clap_plugin::PLUGIN_ID, "tone")
        .context("a plugin record")?;
    record.parameters = pins
        .iter()
        .map(|&(id, name, value)| {
            let name = name.to_string();
            (id, Pin { name, value })
        })
        .collect::<BTreeMap<_, _>>();
    let mut changes = Changes::new();
    changes.create(slot, record);
    project.commit("Choose a plugin", changes)?;
    Ok(())
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let opened = Opened::new(cx, |project| {
        with_pins(
            project,
            &[
                (test_clap_plugin::CUTOFF, "Cutoff", 2400.),
                (test_clap_plugin::WAVE, "Wave", 1.),
                (test_clap_plugin::LEVEL, "Level", 0.8),
                (test_clap_plugin::BRIGHT, "Bright", 1.),
            ],
        )
    })?;
    opened.click_track_header(0., cx)?;
    save(cx, &opened, "track-panel-plugin-pins")?;
    let view = opened.arrangement_view(cx)?;
    let menu = cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let card = panel.read(cx).device_views().next().flatten().cloned();
        let card = card.context("the plugin has no card")?;
        let card = card
            .downcast::<PluginView>()
            .map_err(|_| anyhow::anyhow!("the card is not a plugin's"))?;
        anyhow::Ok(card.read(cx).parameters_menu().clone())
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        menu.update(cx, |menu, cx| menu.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "track-panel-plugin-parameters")?;
    drop(opened);

    let opened = Opened::new(cx, |project| {
        with_pins(
            project,
            &[
                (test_clap_plugin::CUTOFF, "Cutoff", 2400.),
                (test_clap_plugin::WAVE, "Wave", 7.),
                (77, "Drive", 0.5),
            ],
        )
    })?;
    opened.click_track_header(0., cx)?;
    save(cx, &opened, "track-panel-plugin-problems")?;
    Ok(())
}
