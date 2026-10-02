//! The Drum pad in the window, as drawn in `docs/mockups/drum-pad.png` and
//! `drop-file.png`:
//!
//! - `drums-playing.png`: the piece with a Drums track playing a beat, its panel open with the
//!   Drum pad and a Compressor. Kick and Hat are green as they sound, Hat is selected and the
//!   knobs are its own, and pad 48 is a sample pad, `shaker`, with its waveform glyph.
//! - `drums-expanded.png`: the same card expanded: the Sound of the Hat and its Choke.
//! - `drums-sound-list.png`: the Sound list open: the sounds of the kit, then `Choose file…`.
//! - `drums-drop.png`: a file from the Finder over Tom 6, which has the lavender ring.
//! - `drums-missing.png`: the file of the shaker gone: its pad has the warning glyph.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use arrangement::Colour;
use compressor::CompressorState;
use drum_pad::view::DrumPadView;
use drum_pad::{DrumPadState, Source};
use gpui::{Entity, ExternalPaths, FileDropEvent, HeadlessAppContext, PlatformInput, point, px};
use sound_core::{Changes, Project, Ticks};
use sound_media::AudioAsset;
use sound_notes::{Clip, Length, Note, Pitch, Velocity};

use super::audio::write_wav;
use super::{BAR, Opened, add_compressor, main_arrangement, piece};

/// Where the centre of a pad is in the window: the card starts at 192 pt across, after the
/// mixer strip and 16 pt, and its body under its border and its header, at 749 pt down. The
/// bottom row is pads 0 to 3.
fn pad_centre(pad: usize) -> gpui::Point<gpui::Pixels> {
    let (row, column) = (pad / 4, pad % 4);
    point(
        px(192. + 1. + 15. + 76. * column as f32 + 36.),
        px(749. + 36. * (3 - row) as f32 + 16.),
    )
}

/// One bar of a beat: kick on 1 and 3 and the "and" of 3, snare on 2 and 4, hat on the eighths,
/// the shaker on the offbeats.
fn beat_bar(bar: u64) -> Result<Vec<Note>> {
    let mut hits = vec![
        (0, 36, 120),
        (1920, 36, 110),
        (2400, 36, 90),
        (960, 38, 112),
        (2880, 38, 112),
    ];
    for eighth in 0..8_u64 {
        hits.push((eighth * 480, 42, if eighth % 2 == 0 { 100 } else { 70 }));
        if eighth % 2 == 1 {
            hits.push((eighth * 480, 48, 80));
        }
    }
    hits.into_iter()
        .map(|(start, pitch, velocity)| {
            Ok(Note {
                start: Ticks(bar * BAR + start),
                length: Length::new(Ticks(240))?,
                pitch: Pitch::new(pitch)?,
                velocity: Velocity::new(velocity)?,
            })
        })
        .collect()
}

/// The piece and a Drums track that plays a beat for twelve bars through a Compressor, with a
/// shaker in pad 48.
fn drums_piece(project: &mut Project) -> Result<()> {
    piece(project)?;
    let audio = project.root().join("assets/audio");
    write_wav(&audio.join("shaker.wav"), 0.4, |time| {
        let noise = ((time * 48_000.).floor() * 12.9898).sin() * 43_758.545;
        (noise.fract() * 2. - 1.) * (-time * 18.).exp() * 0.5
    })?;
    let mut drums = DrumPadState::default();
    drums.pads[12].load_sample(AudioAsset::new("shaker.wav")?, 0.4);
    let arrangement = main_arrangement(project).context("no arrangement")?;
    let mut changes = Changes::new();
    let track = arrangement::add_track(
        project,
        &mut changes,
        arrangement.id(),
        "Drums",
        Colour::Sky,
        drums,
    )?;
    for (index, start_bar) in [0, 4, 8].into_iter().enumerate() {
        let mut notes = Vec::new();
        for bar in 0..4 {
            notes.extend(beat_bar(bar)?);
        }
        let clip = Clip::new(Ticks(start_bar * BAR), Length::new(Ticks(4 * BAR))?, notes);
        changes.create(track.id().child(&format!("beat-{index}"))?, clip);
    }
    project.commit("Add drums", changes)?;
    let sound = CompressorState {
        threshold_db: -18.,
        ratio: 4.,
        ..CompressorState::default()
    };
    add_compressor(project, "drums", sound)
}

fn card(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<Entity<DrumPadView>> {
    let view = opened.arrangement_view(cx)?;
    cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned().context("no panel")?;
        let card = panel.read(cx).device_views().next().flatten().cloned();
        let card = card.context("the Drum pad has no card")?;
        card.downcast::<DrumPadView>()
            .map_err(|_| anyhow::anyhow!("the first card is not the Drum pad"))
    })
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let mut opened = Opened::new(cx, drums_piece)?;
    opened.click_track_header(3., cx)?;
    let card = card(&opened, cx)?;
    cx.update(|cx| card.update(cx, |card, cx| card.select(6, cx)));
    // Into the first beat of bar 6: the kick and the hat of it are sounding.
    opened.play_from(Ticks(5 * BAR), cx)?;
    for _ in 0..4 {
        opened.advance(960, cx);
    }
    cx.update(|cx| card.update(cx, |card, cx| card.read_levels(cx)));
    cx.run_until_parked();
    let sounding = cx.update(|cx| card.read(cx).sounding());
    anyhow::ensure!(sounding[0] > 0. && sounding[6] > 0., "{sounding:?}");
    save(cx, &opened, "drums-playing")?;

    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    cx.run_until_parked();
    save(cx, &opened, "drums-expanded")?;

    let list = cx.update(|cx| card.read(cx).sound_list().clone());
    cx.update_window(opened.window.into(), |_, window, cx| {
        list.update(cx, |list, cx| list.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "drums-sound-list")?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        list.update(cx, |list, cx| list.close(window, cx));
    })?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(false, cx)));
    drop(opened);

    // A file from the Finder over Tom 6, not let go of.
    let opened = Opened::new(cx, drums_piece)?;
    opened.click_track_header(3., cx)?;
    let folder = tempfile::tempdir()?;
    let file: PathBuf = folder.path().join("tom-deep.wav");
    write_wav(&file, 0.8, |time| {
        (time * 90. * std::f64::consts::TAU).sin() * (-time * 4.).exp()
    })?;
    let over = pad_centre(14);
    let entered = FileDropEvent::Entered {
        position: over - point(px(30.), px(0.)),
        paths: ExternalPaths(vec![file].into()),
    };
    opened.mouse(PlatformInput::FileDrop(entered), cx)?;
    for step in [20., 10., 0.] {
        let pending = FileDropEvent::Pending {
            position: over - point(px(step), px(0.)),
        };
        opened.mouse(PlatformInput::FileDrop(pending), cx)?;
    }
    save(cx, &opened, "drums-drop")?;
    opened.mouse(PlatformInput::FileDrop(FileDropEvent::Exited), cx)?;
    drop(opened);

    // The shaker gone from the folder.
    let opened = Opened::new(cx, |project| {
        drums_piece(project)?;
        std::fs::remove_file(project.root().join("assets/audio/shaker.wav"))?;
        // Written again, so the behaviour looks for the file and finds it gone.
        let drums = project
            .resolve::<DrumPadState>(&sound_core::InstanceId::new(
                "arrangement/drums/instrument",
            )?)
            .context("no drums")?;
        let mut state = project.state(&drums).context("no drums")?.clone();
        state.pads[12].volume_db = -1.0;
        anyhow::ensure!(matches!(state.pads[12].source, Source::Sample(_)));
        let mut changes = Changes::new();
        changes.set(&drums, state);
        project.commit("Touch", changes)?;
        Ok(())
    })?;
    opened.click_track_header(3., cx)?;
    save(cx, &opened, "drums-missing")?;
    Ok(())
}
