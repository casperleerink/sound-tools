//! The arrangement wiring: the one place outside the registration in `lib.rs` that names the
//! arrangement. The arrangement is the home of the product, so the app may know it, one way:
//! what it must not know, such as the default instrument or the tempo fit, is handed in from
//! here. Every other extension the runtime names only where it registers it.

pub mod recording;
pub mod transport;

use anyhow::{Result, bail};
use arrangement::{ArrangementState, Colour, TrackKind, TrackState};
use instrument::SynthState;
use plugin_host::Plugins;
use sound_core::{Changes, Engine, Instance, InstanceId, Project, ProjectError, State, Ticks};
use sound_ui::extension_is_enabled;

pub(crate) use arrangement::create_default_project;

use crate::render_into;

/// The arrangement of the piece: the first one at the top of the project.
pub fn main_arrangement(project: &Project) -> Option<Instance<ArrangementState>> {
    let mut instances = project.instances();
    let (id, _) =
        instances.find(|(id, tool)| id.parent().is_none() && *tool == ArrangementState::TOOL)?;
    project.resolve(id)
}

/// Solos the tracks of the main arrangement that `names` names, by name or by id, and unmutes
/// them: every other track goes silent. Only in memory, so a render of a read-only project can
/// play one part alone and leave every file as it is. Call it before the render plays. Gives
/// what the plugin host reported meanwhile.
pub fn solo(
    project: &mut Project,
    engine: &mut Engine,
    plugins: &Plugins,
    names: &[String],
) -> Result<Vec<plugin_host::PluginProblem>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let Some(arrangement) = main_arrangement(project) else {
        bail!("the project has no arrangement, so it has no tracks to solo");
    };
    let tracks = arrangement::tracks(project, arrangement.id());
    let called = |(track, state): &(Instance<TrackState>, &TrackState), name: &String| {
        state.name == *name || track.id().as_str() == name || track.id().name() == name
    };
    if let Some(missing) = names
        .iter()
        .find(|name| !tracks.iter().any(|track| called(track, name)))
    {
        let all: Vec<String> = tracks
            .iter()
            .map(|(_, state)| format!("{:?}", state.name))
            .collect();
        bail!(
            "no track is called {missing:?}. The tracks: {}",
            all.join(", ")
        );
    }
    let mut changes = Changes::new();
    for track in &tracks {
        let mut state = track.1.clone();
        state.solo = names.iter().any(|name| called(track, name));
        state.mute &= !state.solo;
        changes.set(&track.0, state);
    }
    project.apply_in_memory(changes)?;
    // A track that changes its mute fades, which would fade in the start of the render. The
    // engine runs stopped until the fades are over.
    let fade = arrangement::RAMP_SECONDS * engine.sample_rate() as f32;
    render_into(project, engine, plugins, 2 * fade.ceil() as usize, |_| {
        Ok(())
    })
}

/// The name and colour of the next track: `Track <n>`, in the next colour of the palette, so
/// that tracks are easy to tell apart.
fn next_track(project: &Project, arrangement: &Instance<ArrangementState>) -> (String, Colour) {
    let count = arrangement::tracks(project, arrangement.id()).len();
    let colour = Colour::ALL[count % Colour::ALL.len()];
    (format!("Track {}", count + 1), colour)
}

/// Adds `Track <n>` with the default synth, as one undo step, in the next colour of the
/// palette. This is the one place that knows the default instrument.
pub fn add_track(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
) -> Result<(), ProjectError> {
    let (name, colour) = next_track(project, arrangement);
    let mut changes = Changes::new();
    let instrument = SynthState::default();
    arrangement::add_track(
        project,
        &mut changes,
        arrangement.id(),
        &name,
        colour,
        instrument,
    )?;
    project.commit("Add track", changes)
}

/// Adds an audio track `Track <n>`, empty, as one undo step, in the next colour of the palette.
pub fn add_audio_track(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
) -> Result<(), ProjectError> {
    let (name, colour) = next_track(project, arrangement);
    let mut changes = Changes::new();
    arrangement::add_audio_track(project, &mut changes, arrangement.id(), &name, colour)?;
    project.commit("Add audio track", changes)
}

/// What the menu of a recorded clip calls. The arrangement knows no fit, so the runtime hands it
/// this. A project made before the fit existed does not enable it; the file edit that does is
/// in the agent docs.
pub(crate) const FIT_TEMPO: arrangement::view::FitTempo = arrangement::view::FitTempo {
    unavailable: |project| {
        (!extension_is_enabled(project, fit_tempo::EXTENSION))
            .then_some("This project does not include the tempo fit.")
    },
    fit: |project, clip| {
        let mut changes = Changes::new();
        fit_tempo::fit_take(project, &mut changes, clip)?;
        project.commit(fit_tempo::FIT_LABEL, changes)
    },
};

/// [`add_track`] or [`add_audio_track`]: what the add track button of the arrangement calls.
/// The arrangement knows no instrument, so the runtime hands it this.
pub fn add_track_of_kind(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
    kind: TrackKind,
) -> Result<(), ProjectError> {
    match kind {
        TrackKind::Instrument => add_track(project, arrangement),
        TrackKind::Audio => add_audio_track(project, arrangement),
    }
}

/// The time `clips` cover together, from the first start to the last end, which is what the
/// window exports as the selection. `None` when none of them is a clip.
pub fn clips_span(project: &Project, clips: &[InstanceId]) -> Option<(Ticks, Ticks)> {
    let spans = clips.iter().filter_map(|id| {
        if let Some(clip) = project.resolve::<sound_notes::Clip>(id) {
            let clip = project.state(&clip)?;
            return Some((clip.start, clip.end()));
        }
        let clip = project.resolve::<arrangement::AudioClip>(id)?;
        let clip = project.state(&clip)?;
        Some((clip.start, arrangement::audio_clip_end(project, clip)))
    });
    spans.reduce(|(start, end), (from, to)| (start.min(from), end.max(to)))
}
