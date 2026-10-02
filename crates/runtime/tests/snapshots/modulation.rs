//! The Modulation in the window, after the synth of the bass:
//!
//! - `modulation-chorus.png`: a new Modulation, the default chorus.
//! - `modulation-flanger-expanded.png`: a flanger at full spread, deeper and with more
//!   feedback, expanded: the spread knob.
//! - `modulation-phaser.png`: a phaser with no spread, its two lines on one another.

use anyhow::{Context as _, Result};
use arrangement::TrackState;
use gpui::{Entity, HeadlessAppContext};
use modulation::view::ModulationView;
use modulation::{Mode, ModulationState};
use sound_core::{Changes, InstanceId, Project};

use super::{Opened, piece};

/// Puts a modulation after the instrument of the bass, as `Add effect` does.
fn add_modulation(project: &mut Project, sound: ModulationState) -> Result<()> {
    let id = InstanceId::new("arrangement/bass")?;
    let track = project
        .resolve::<TrackState>(&id)
        .context("the track is not there")?;
    let mut changes = Changes::new();
    let slot = arrangement::add_effect(project, &mut changes, &track, "Modulation")?;
    changes.create(slot, sound);
    project.commit("Add Modulation", changes)?;
    Ok(())
}

/// Opens the panel of the bass and gives the card of the modulation, the second in its rack.
fn open_panel(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<Entity<ModulationView>> {
    opened.click_track_header(1., cx)?;
    let view = opened.arrangement_view(cx)?;
    cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned();
        let panel = panel.context("the track panel did not open")?;
        let card = panel.read(cx).device_views().nth(1).flatten().cloned();
        let card = card.context("the modulation has no card")?;
        card.downcast::<ModulationView>()
            .map_err(|_| anyhow::anyhow!("the second card is not the modulation"))
    })
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let sounds = [
        ("modulation-chorus", ModulationState::default(), false),
        (
            "modulation-flanger-expanded",
            ModulationState {
                mode: Mode::Flanger,
                depth: 0.8,
                feedback: 0.7,
                spread: 1.0,
                ..ModulationState::default()
            },
            true,
        ),
        (
            "modulation-phaser",
            ModulationState {
                mode: Mode::Phaser,
                rate_hz: 0.3,
                depth: 0.7,
                feedback: 0.5,
                spread: 0.0,
                ..ModulationState::default()
            },
            false,
        ),
    ];
    for (name, sound, expanded) in sounds {
        let opened = Opened::new(cx, |project| {
            piece(project)?;
            add_modulation(project, sound)
        })?;
        let card = open_panel(&opened, cx)?;
        cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(expanded, cx)));
        cx.run_until_parked();
        save(cx, &opened, name)?;
        drop(opened);
    }
    Ok(())
}
