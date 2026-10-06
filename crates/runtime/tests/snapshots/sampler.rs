//! The Sampler in the window, in the states of `docs/mockups/sampler.png`:
//!
//! - `sampler-playing.png`: the piece with a Kalimba track whose instrument is a Sampler of a
//!   kalimba-like file, its panel open, playing: the envelope over the waveform, and the green
//!   line where the last note is in the file.
//! - `sampler-expanded.png`: the same card expanded: Start, End, Reverse, Tune, Attack, Decay
//!   and Sustain.
//! - `sampler-drop-replace.png`: a file from the Finder over its display: `Drop to replace the
//!   file` in the lavender ring.
//! - `sampler-instruments.png`: its Instrument select open: the file, then the library by
//!   category with the size of each download.
//! - `sampler-library.png`: a Sampler of a library instrument this machine does not have:
//!   its name, library and licence over `Download · 73 MB on disk`.
//! - `sampler-empty.png`: a new Sampler: `Drop an audio file here` over `Choose file`.
//! - `sampler-drop.png`: a file over the empty display: `Drop to load the file`.
//! - `sampler-missing.png`: a Sampler whose file is not in the project.

use anyhow::{Context as _, Result};
use arrangement::{Colour, TrackState};
use gpui::{
    Entity, ExternalPaths, FileDropEvent, HeadlessAppContext, PlatformInput, Point, point, px,
};
use sampler::view::SamplerView;
use sampler::{LibraryId, SamplerState};
use sound_core::{Changes, Instance, Project, Ticks};
use sound_media::AudioAsset;
use sound_notes::Pitch;

use super::audio::write_wav;
use super::{BAR, Opened, clip, note, piece};

/// Something like a kalimba: a tine of 523 Hz with a bright start, dying away over a second.
fn kalimba(time: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let tine =
        (tau * 523.25 * time).sin() + 0.35 * (tau * 1_570. * time).sin() * (-9. * time).exp();
    let knock = (tau * 2_900. * time).sin() * (-60. * time).exp() * 0.6;
    ((tine + knock) * (-2.6 * time).exp() * 0.75).clamp(-1., 1.)
}

/// A Kalimba track at the end of the piece, whose Sampler is `sampler`, playing a phrase.
fn add_kalimba(project: &mut Project, sampler: SamplerState) -> Result<Instance<TrackState>> {
    let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
    let mut changes = Changes::new();
    let track = arrangement::add_track(
        project,
        &mut changes,
        arrangement.id(),
        "Kalimba",
        Colour::Teal,
        sampler,
    )?;
    let notes = [
        (0, 480, 72),
        (480, 480, 76),
        (960, 960, 79),
        (1920, 1920, 74),
    ];
    let notes = notes
        .iter()
        .map(|(start, length, pitch)| note(*start, *length, *pitch))
        .collect::<Result<Vec<_>>>()?;
    changes.create(track.id().child("phrase")?, clip(4, 4, notes)?);
    project.commit("Add kalimba", changes)?;
    Ok(track)
}

/// The kalimba of the mockup: its file trimmed a little at each end, a short attack, and a
/// decay to 55 %.
fn kalimba_sampler() -> Result<SamplerState> {
    Ok(SamplerState {
        sample: Some(AudioAsset::new("kalimba.wav")?),
        sfz: None,
        library: None,
        root: Pitch::new(72)?,
        tune: 0.,
        start_seconds: 0.012,
        end_seconds: Some(1.18),
        reverse: false,
        attack_seconds: 0.002,
        decay_seconds: 0.4,
        sustain: 0.55,
        release_seconds: 0.3,
        velocity_to_volume: 0.5,
        gain_db: 0.,
    })
}

/// Opens the panel of the last track, whose instrument is the Sampler, and gives its card.
fn open_panel(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<Entity<SamplerView>> {
    let view = opened.arrangement_view(cx)?;
    let track = cx.update(|cx| {
        let project = opened.session.read(cx).project();
        let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
        let tracks = project.children::<TrackState>(arrangement.id());
        let last = tracks.max_by_key(|(_, state)| state.order);
        last.map(|(track, _)| track).context("no track")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        view.update(cx, |view, cx| view.open_track_panel(track, window, cx));
    })?;
    cx.run_until_parked();
    cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned().context("no panel")?;
        let card = panel.read(cx).device_views().next().flatten().cloned();
        let card = card.context("no card")?.downcast::<SamplerView>().ok();
        card.context("not a Sampler")
    })
}

/// The middle of the display of the card of the first slot, in the window of 1470 x 920: the
/// panel starts under the title row of 48 and the arrangement of 656, its card 12 lower, the
/// body 32 under the top of the card; the rack 16 right of the header column of 176, the display
/// 16 into the card.
const DISPLAY_MIDDLE: (f32, f32) = (176. + 16. + 16. + 312. / 2., 48. + 656. + 12. + 32. + 59.);

/// A file dragged from the Finder over the display of the card, not dropped.
fn drag_over_display(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    let to: Point<_> = point(px(DISPLAY_MIDDLE.0), px(DISPLAY_MIDDLE.1));
    let path = std::path::PathBuf::from("/Users/you/Samples/kalimba.wav");
    let paths = ExternalPaths([path].into_iter().collect());
    let entered = FileDropEvent::Entered {
        position: to - point(px(40.), px(0.)),
        paths,
    };
    opened.mouse(PlatformInput::FileDrop(entered), cx)?;
    for step in 0..3 {
        let position = to - point(px(10. * (2 - step) as f32), px(0.));
        opened.mouse(
            PlatformInput::FileDrop(FileDropEvent::Pending { position }),
            cx,
        )?;
    }
    Ok(())
}

/// While a file is over the display its handles hide: the dots of the start line and the
/// attack peak, which reach 5 pt past the left edge of the display, leave the card there as it
/// is 8 pt further left. Measured in the pixels of the window, 2 per point.
fn handles_hide_under_the_drag(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<()> {
    let image = cx.capture_screenshot(opened.window.into())?;
    let left = DISPLAY_MIDDLE.0 - 312. / 2.;
    let top = DISPLAY_MIDDLE.1 - 59.;
    let pixel = |x: f32, y: f32| image.get_pixel((x * 2.) as u32, (y * 2.) as u32).0;
    // The foot of the start line, 8 pt above the bottom, and the attack peak, at 88 % up.
    for y in [top + 118. - 8., top + 118. * 0.12] {
        let (beside, card) = (pixel(left - 1.5, y), pixel(left - 9.5, y));
        anyhow::ensure!(
            beside == card,
            "a handle shows past the ring at {y} pt: {beside:?} where the card is {card:?}"
        );
    }
    Ok(())
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let mut opened = Opened::new(cx, |project| {
        piece(project)?;
        write_wav(
            &project.root().join("assets/audio/kalimba.wav"),
            1.4,
            kalimba,
        )?;
        add_kalimba(project, kalimba_sampler()?)?;
        Ok(())
    })?;
    let card = open_panel(&opened, cx)?;
    // Into the long G of the phrase: its note started half a bar, a second, before.
    opened.play_from(Ticks(4 * BAR + 960 + 700), cx)?;
    opened.listen(0.25, cx)?;
    super::audio::wait_for_waveforms(cx, &opened)?;
    cx.update(|cx| card.update(cx, |card, cx| card.read_position(cx)));
    cx.run_until_parked();
    let at = cx.update(|cx| card.read(cx).playing_at());
    anyhow::ensure!(at.is_some(), "no green line");
    save(cx, &opened, "sampler-playing")?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(true, cx)));
    cx.run_until_parked();
    save(cx, &opened, "sampler-expanded")?;
    cx.update(|cx| card.update(cx, |card, cx| card.set_expanded(false, cx)));
    cx.run_until_parked();
    drag_over_display(&opened, cx)?;
    save(cx, &opened, "sampler-drop-replace")?;
    handles_hide_under_the_drag(&opened, cx)?;
    opened.mouse(PlatformInput::FileDrop(FileDropEvent::Exited), cx)?;
    let list = cx.update(|cx| card.read(cx).instrument_list().clone());
    cx.update_window(opened.window.into(), |_, window, cx| {
        list.update(cx, |list, cx| list.open(window, cx));
    })?;
    cx.run_until_parked();
    save(cx, &opened, "sampler-instruments")?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        list.update(cx, |list, cx| list.close(window, cx));
    })?;
    drop(opened);

    // A library instrument this machine does not have, in an empty library: `Download`.
    let library = tempfile::tempdir()?;
    sampler::library::set_folder(library.path().to_path_buf());
    let opened = Opened::new(cx, |project| {
        piece(project)?;
        let cello = SamplerState {
            library: Some(
                LibraryId::try_from("strings/cello-section".to_string())
                    .map_err(anyhow::Error::msg)?,
            ),
            ..SamplerState::default()
        };
        add_kalimba(project, cello)?;
        Ok(())
    })?;
    open_panel(&opened, cx)?;
    save(cx, &opened, "sampler-library")?;
    drop(opened);

    let opened = Opened::new(cx, |project| {
        piece(project)?;
        add_kalimba(project, SamplerState::default())?;
        Ok(())
    })?;
    open_panel(&opened, cx)?;
    save(cx, &opened, "sampler-empty")?;
    drag_over_display(&opened, cx)?;
    save(cx, &opened, "sampler-drop")?;
    opened.mouse(PlatformInput::FileDrop(FileDropEvent::Exited), cx)?;
    drop(opened);

    let opened = Opened::new(cx, |project| {
        piece(project)?;
        add_kalimba(project, kalimba_sampler()?)?;
        Ok(())
    })?;
    open_panel(&opened, cx)?;
    save(cx, &opened, "sampler-missing")?;
    Ok(())
}
