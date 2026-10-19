//! The arrangement wiring: the one place outside the registration in `lib.rs` that names the
//! arrangement. The arrangement is the home of the product, so the app may know it, one way:
//! what it must not know, such as the default instrument or the tempo fit, is handed in from
//! here. Every other extension the runtime names only where it registers it.

pub mod recording;
pub mod transport;

use anyhow::{Result, bail};
use arrangement::{ArrangementState, Colour, TrackKind, TrackState};
use gpui::AppContext as _;
use instrument::SynthState;
use plugin_host::Plugins;
use sound_core::{Changes, Engine, Instance, Project, ProjectError, Registry, State};
use sound_ui::{Views, extension_is_enabled};

/// Registers the arrangement's tools, as every bundled extension registers its own.
pub(crate) fn register(registry: &mut Registry) -> Result<()> {
    arrangement::register(registry)?;
    Ok(())
}

/// Registers the arrangement's views, with what it must not know: the default instrument and
/// the tempo fit.
pub(crate) fn register_views(views: &mut Views) {
    arrangement::view::register(views, add_track_of_kind, FIT_TEMPO);
}

/// The arrangement's transport in the title row while it is the main view, and where live notes
/// go then: the selected track, or the first one, see [`recording::live_notes_inputs`].
pub(crate) const CHROME: Chrome = Chrome {
    tool: ArrangementState::TOOL,
    make: |session, live, _, cx| cx.new(|cx| TransportPill::new(session, live, cx)).into(),
    live_notes: recording::live_notes_inputs,
};

/// A new project with one arrangement and no track, see [`crate::Template`].
pub(crate) const TEMPLATE: crate::Template = crate::Template {
    name: "Arrangement",
    files: &[],
    create: arrangement::create_default_project,
};

use crate::render_into;
use crate::window::Chrome;
use transport::TransportPill;

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

/// [`add_track`], [`add_audio_track`] or a return: what the add track button of the
/// arrangement calls. The arrangement knows no instrument, so the runtime hands it this.
pub fn add_track_of_kind(
    project: &mut Project,
    arrangement: &Instance<ArrangementState>,
    kind: TrackKind,
) -> Result<(), ProjectError> {
    match kind {
        TrackKind::Instrument => add_track(project, arrangement),
        TrackKind::Audio => add_audio_track(project, arrangement),
        TrackKind::Return => {
            let mut changes = Changes::new();
            arrangement::add_return(project, &mut changes, arrangement.id())?;
            project.commit("Add return", changes)
        }
    }
}
