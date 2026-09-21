//! Fit tempo: the project's grid follows a freely played take.
//!
//! One record, `state/fit-tempo.json`, holds what an algorithm cannot know: which take, which
//! moment of it is the first downbeat, whether the grid runs at half, normal or double the
//! beat that was found, and how steady the tempo should be. Everything else is computed from
//! it, every time it changes, by a function that gives the same bytes for the same inputs:
//!
//! - the tempo map in `project.json`, one step per beat,
//! - the ticks of the notes and the pedal of the take's clip.
//!
//! Neither is saved twice and neither is edited by hand. They are written by a **derive**, the
//! core's way for a record to decide state of its own, so a change of the fit record, from the
//! window or from a file an agent wrote, is one group, one engine batch and one undo step
//! together with the tempo map and the clip.
//!
//! This extension knows no track and no MIDI device. It reads a take through the note contract
//! crate ([`sound_notes::RawTake`]), which the MIDI extension writes, and it writes a
//! [`sound_notes::Clip`], which the arrangement plays. `README.md` in this crate is the guide
//! and `agent-doc.md` is what an agent reads.

mod beats;
mod grid;

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, Assets, Changes, Derived, Instance, InstanceId, Place, Project, ProjectError,
    Registry, RegistryError, State, TimeSignature, Was,
};
use sound_notes::{Clip, RawTake};

pub use beats::{CHORD_US, MIN_ONSETS, Onset, SNAP_US, find_beats, onsets};
pub use grid::{BeatRate, FIT_SAMPLE_RATE, FitError, Fitted, fit};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "fit-tempo";

/// Where the fit of a project lives. One fit per project, so it has one name.
pub const DEFAULT_FIT: &str = "fit-tempo";

/// The label of the undo step that fits the tempo to a take.
pub const FIT_LABEL: &str = "Fit tempo";

/// The label of the undo step of a steadiness change.
pub const STEADINESS_LABEL: &str = "Change steadiness";

/// The inputs of a fit. Everything a deterministic algorithm cannot work out by itself.
///
/// The results are not here: the tempo map is in `project.json` and the notes are in the clip,
/// where everything already reads them. Change a field and both are made again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitState {
    /// The take the grid follows: the name of a file under `assets/takes/`, without `.json`.
    /// The clip that names the same take in its own `take` field is the one that is rewritten.
    pub take: String,
    /// Which moment of the take is the first downbeat, in microseconds from the start of the
    /// recording, the same unit every time in the take file uses. The beat nearest to it
    /// becomes beat 1 of a bar. 0 means the first beat that was found.
    #[serde(default)]
    pub first_downbeat_us: u64,
    /// `half`, `normal` or `double`: how many beats the grid has for each beat that was found.
    #[serde(default)]
    pub beat: BeatRate,
    /// 0 for the tempo as it was played, 1 for one steady tempo. In between the beats move
    /// towards even spacing. The notes keep their ticks whatever it is, so the touch inside
    /// each beat stays and going back to 0 gives the fitted map again, byte for byte.
    #[serde(default)]
    pub steadiness: f32,
}

impl FitState {
    /// What decides where the beats are, which is everything but the steadiness. When none of
    /// it changed, the clip is left exactly as it is.
    fn grid_inputs(&self) -> (&str, u64, BeatRate) {
        (&self.take, self.first_downbeat_us, self.beat)
    }

    /// The fit of a take, as the window makes it: the first beat is the first downbeat, the
    /// grid runs at the beat that was found, and the tempo follows the playing.
    pub fn new(take: impl Into<String>) -> Self {
        Self {
            take: take.into(),
            first_downbeat_us: 0,
            beat: BeatRate::Normal,
            steadiness: 0.0,
        }
    }
}

impl State for FitState {
    const TOOL: &'static str = "fit-tempo";
    /// The fit is about the whole project, so it sits at the top of `state/` and only there,
    /// under one name. A project has one fit by construction: a second record does not load
    /// and the problem names the one path. Two of them would write over each other's tempo
    /// map with no way to say which won.
    const PLACE: Place = Place::Only(DEFAULT_FIT);

    fn validate(&self) -> Result<(), String> {
        if !Clip::is_valid_take_name(&self.take) {
            return Err(format!(
                "take must be the name of a file under assets/takes/ without `.json`: lowercase letters, digits, `-` and `_`, not {:?}",
                self.take
            ));
        }
        if !(0.0..=1.0).contains(&self.steadiness) {
            return Err(format!(
                "steadiness must be from 0 to 1, not {}",
                self.steadiness
            ));
        }
        Ok(())
    }
}

/// The doc an agent opens to correct a fit.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "fit-tempo",
    when: "The grid does not sit where the music does, or you fit the tempo to a recorded take",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the tool and its doc. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    let fits = Fits::default();
    let summaries = fits.clone();
    registry
        .tool::<FitState>(EXTENSION)?
        .derive(move |project, fit, was, derived| fits.derive(project, fit, was, derived))
        .summary(move |project, fit| summaries.summary(project, fit));
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// The fit of a project, or `None` when it has none. It is one record at one id, so this is a
/// lookup and not a walk: the transport asks for it after every project event.
pub fn fit_of(project: &Project) -> Option<Instance<FitState>> {
    project.resolve::<FitState>(&fit_id())
}

/// The one id a fit lives at. The name is fixed, so this cannot fail.
fn fit_id() -> InstanceId {
    InstanceId::new(DEFAULT_FIT).unwrap_or_else(|_| unreachable!("a fixed valid name"))
}

/// The clips that name `take`, in id order. Normally one: the clip the recording made.
fn clips_of_take<'a>(project: &'a Project, take: &str) -> Vec<(Instance<Clip>, &'a Clip)> {
    let instances: Vec<&InstanceId> = project
        .instances()
        .filter(|(_, tool)| *tool == Clip::TOOL)
        .map(|(id, _)| id)
        .collect();
    instances
        .into_iter()
        .filter_map(|id| {
            let clip = project.resolve::<Clip>(id)?;
            let state = project.state(&clip)?;
            (state.take.as_deref() == Some(take)).then_some((clip, state))
        })
        .collect()
}

/// Makes the project's grid follow the take of `clip`, as part of a group of changes.
///
/// It replaces the fit the project has, because one project has one fit. The tempo map and the
/// clip follow in the same group, through the derive, so the whole thing is one undo step.
pub fn fit_take(project: &Project, changes: &mut Changes, clip: &Clip) -> Result<(), ProjectError> {
    let take = clip
        .take
        .clone()
        .ok_or_else(|| ProjectError::InvalidState {
            id: InstanceId::new(DEFAULT_FIT).unwrap_or_else(|_| unreachable!("a fixed name")),
            message: "this clip was not recorded, so there is no take to fit the tempo to"
                .to_string(),
        })?;
    let id = match fit_of(project) {
        Some(fit) => fit.id().clone(),
        None => InstanceId::new(DEFAULT_FIT)?,
    };
    changes.create(id, FitState::new(take));
    Ok(())
}

/// Sets the steadiness of the project's fit, as part of a group of changes. `None` when the
/// project has no fit.
pub fn set_steadiness(project: &Project, changes: &mut Changes, steadiness: f32) -> Option<()> {
    let fit = fit_of(project)?;
    let mut state = project.state(&fit)?.clone();
    state.steadiness = steadiness.clamp(0.0, 1.0);
    changes.set(&fit, state);
    Some(())
}

/// What a take and a set of inputs fit to, kept between runs.
///
/// A steadiness drag publishes sixty times a second and each publish runs the derive, so
/// reading and searching a ten-minute take every time would be minutes of work a second. The
/// take is parsed once per name and the grid is computed once per set of inputs; steadiness is
/// not one of them, because it only moves beats that are already known.
#[derive(Clone, Default)]
struct Fits {
    cache: Rc<RefCell<Cache>>,
}

/// Everything that decides where the beats are. Steadiness is not in it.
type GridKey = (String, u64, BeatRate, TimeSignature);

#[derive(Default)]
struct Cache {
    take: Option<(String, Rc<RawTake>)>,
    grid: Option<(GridKey, Rc<Result<Fitted, String>>)>,
}

impl Fits {
    /// The fit of these inputs, computed or remembered.
    fn fitted(
        &self,
        assets: &Assets,
        state: &FitState,
        time_signature: TimeSignature,
    ) -> Rc<Result<Fitted, String>> {
        let key: GridKey = (
            state.take.clone(),
            state.first_downbeat_us,
            state.beat,
            time_signature,
        );
        if let Some((cached, fitted)) = &self.cache.borrow().grid
            && *cached == key
        {
            return fitted.clone();
        }
        let take = self.take(assets, &state.take);
        let fitted = take.and_then(|take| {
            grid::fit(&take, time_signature, state.first_downbeat_us, state.beat)
                .map_err(|error| error.to_string())
        });
        let fitted = Rc::new(fitted);
        // A fit that worked is remembered; one that did not is worked out again next time.
        // What went wrong is almost always the take file, and an agent that mends it must see
        // the fit come back without touching the record that names it.
        if fitted.is_ok() {
            self.cache.borrow_mut().grid = Some((key, fitted.clone()));
        }
        fitted
    }

    /// The take of that name, parsed once.
    fn take(&self, assets: &Assets, name: &str) -> Result<Rc<RawTake>, String> {
        if let Some((cached, take)) = &self.cache.borrow().take
            && cached == name
        {
            return Ok(take.clone());
        }
        let take = RawTake::read(assets, name).map_err(|error| error.to_string())?;
        let take = Rc::new(take);
        self.cache.borrow_mut().take = Some((name.to_string(), take.clone()));
        Ok(take)
    }

    /// The derive: the tempo map, and the clip of the take when the beats moved.
    ///
    /// The clip is made again from the raw take only when an input that decides where the
    /// beats are changed: the take, the first downbeat, half, normal or double, or the
    /// project's time signature. A steadiness change writes the tempo map and nothing else, so
    /// a note moved by hand, a trimmed clip or a clip dragged somewhere else all survive it.
    fn derive(
        &self,
        project: &Project,
        fit: &Instance<FitState>,
        was: Was<'_, FitState>,
        derived: &mut Derived,
    ) {
        let Some(state) = project.state(fit) else {
            return;
        };
        let beats_moved = match was {
            // A fit that was just made, and a time signature that changed under one: the grid
            // is another grid either way.
            Was::Created | Was::Unchanged => true,
            Was::Changed(before) => before.grid_inputs() != state.grid_inputs(),
        };
        let time_signature = project.project_file().tempo_map.time_signature();
        let fitted = self.fitted(project.assets(), state, time_signature);
        let fitted = match &*fitted {
            Ok(fitted) => fitted,
            Err(message) => {
                derived.problem(message.clone());
                return;
            }
        };
        for problem in &fitted.problems {
            derived.problem(problem.clone());
        }
        let map = fitted.map_at(time_signature, state.steadiness);
        derived.changes().set_tempo_map(map);
        if !beats_moved {
            return;
        }

        let clips = clips_of_take(project, &state.take);
        let Some((clip, _)) = clips.first() else {
            derived.problem(format!(
                "no clip names the take {:?}, so there is nothing to write the notes into. The grid follows the take all the same. Put \"take\": {:?} back in the clip that was recorded from it",
                state.take, state.take
            ));
            return;
        };
        if clips.len() > 1 {
            derived.problem(format!(
                "{} clips name the take {:?}. Only {} is written; the others keep the ticks they have",
                clips.len(),
                state.take,
                clip.id()
            ));
        }
        let mut record = fitted.clip.clone();
        record.take = Some(state.take.clone());
        derived.changes().set(clip, record);
    }

    /// What `--inspect` prints about a fit.
    fn summary(&self, project: &Project, fit: &Instance<FitState>) -> String {
        let Some(state) = project.state(fit) else {
            return String::new();
        };
        let time_signature = project.project_file().tempo_map.time_signature();
        let head = format!(
            "fit `{}` take {} beat {} steadiness {:.0}%",
            fit.id(),
            state.take,
            state.beat.name(),
            state.steadiness * 100.0
        );
        match &*self.fitted(project.assets(), state, time_signature) {
            Ok(fitted) => {
                let downbeat = fitted.first_downbeat_tick(time_signature);
                format!(
                    "{head}, {} beats, first downbeat at {}",
                    fitted.beat_count(),
                    time_signature.bar_beat_of(downbeat)
                )
            }
            Err(message) => format!("{head}, not fitted: {message}"),
        }
    }
}
