//! The `region`: a child of a parent of the project that plays a stretch of an audio file at its
//! place in the song, out of its output `audio`.

use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    Assets, BehaviourContext, BehaviourError, Frames, InstanceId, JsonTool, JsonToolDoc, Project,
    State, Ticks, ToolSchema,
};
use sound_media::{AUDIO_FOLDER, Audio, AudioAsset, Keep, Loader, Region, Stamp};

pub(crate) const NAME: &str = "region";
/// What its parent's view reads of how long it plays, in seconds.
const SECONDS: &str = "seconds";

/// The loudest a region plays over its file, in decibels.
pub(crate) const MAX_GAIN_DB: f32 = 24.0;
/// What a region at this gain or below plays: silence.
const SILENT_DB: f32 = -96.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    /// The file, under `assets/audio/`, such as `"take-1.wav"`.
    pub(crate) asset: AudioAsset,
    /// Where it starts in the song, in ticks: 3840 is a bar of 4/4.
    #[serde(default)]
    pub(crate) start: Ticks,
    /// Where in the file it starts playing.
    #[serde(default)]
    pub(crate) file_start_seconds: f64,
    /// Where in the file it stops. The end of the file when left out.
    #[serde(default)]
    pub(crate) file_end_seconds: Option<f64>,
    /// How much louder or quieter it plays than its file, -96 (silence) to 24.
    #[serde(default)]
    pub(crate) gain_db: f32,
    /// A straight line from silence at its start to its level.
    #[serde(default)]
    pub(crate) fade_in_ms: f32,
    /// A straight line from its level to silence at its end.
    #[serde(default)]
    pub(crate) fade_out_ms: f32,
}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project plays an audio file at a place in the song: an audio clip or a take"
                .to_string(),
            markdown: include_str!("region.md").to_string(),
        }),
        behaviour: Box::new(apply),
        // A file that arrives plays a region that waited for it.
        asset_folders: vec![AUDIO_FOLDER],
        owns_children: false,
        end: Some(Box::new(end)),
    }
}

/// The record of the region, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema::of::<Record>(&[])
}

/// The record of `state`, or what is wrong with it, by its path.
pub(crate) fn record(state: &Value) -> Result<Record, String> {
    let record: Record = sound_core::parse_state(state)?;
    let seconds = |field: &str, value: f64| match value.is_finite() && value >= 0.0 {
        true => Ok(()),
        false => Err(format!(
            "state.{field} must be a number of seconds, 0 or more, not {value}"
        )),
    };
    seconds("file_start_seconds", record.file_start_seconds)?;
    if let Some(end) = record.file_end_seconds {
        seconds("file_end_seconds", end)?;
        if end <= record.file_start_seconds {
            return Err(format!(
                "state.file_end_seconds must be after file_start_seconds {}, not {end}",
                record.file_start_seconds
            ));
        }
    }
    if !(SILENT_DB..=MAX_GAIN_DB).contains(&record.gain_db) {
        return Err(format!(
            "state.gain_db must be from {SILENT_DB} to {MAX_GAIN_DB}, not {}",
            record.gain_db
        ));
    }
    for (field, value) in [
        ("fade_in_ms", record.fade_in_ms),
        ("fade_out_ms", record.fade_out_ms),
    ] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(format!(
                "state.{field} must be a number of milliseconds, 0 or more, not {value}"
            ));
        }
    }
    Ok(record)
}

/// Where it stops playing in the song: its start, then its stretch of the file at the file's
/// speed. `None` while the file is not there.
fn end(project: &Project, state: &Value) -> Option<Ticks> {
    let region = region(&record(state).ok()?);
    let clock = project.clock();
    let file = sound_media::info(project.assets(), &region.asset).ok()?;
    let length = region.length(&file, clock.sample_rate());
    Some(clock.tick_at(Frames(clock.frame_of(region.start).0 + length)))
}

/// The region a record plays.
pub(crate) fn region(record: &Record) -> Region {
    let gain = match record.gain_db <= SILENT_DB {
        true => 0.0,
        false => 10_f32.powf(record.gain_db / 20.0),
    };
    Region {
        asset: record.asset.clone(),
        start: record.start,
        file_start_seconds: record.file_start_seconds,
        file_end_seconds: record.file_end_seconds,
        gain,
        fade_in_ms: record.fade_in_ms,
        fade_out_ms: record.fade_out_ms,
    }
}

/// What a region says of itself: how long it plays, and a file that is not there or does not
/// play. Its parent plays it, with the other regions that play into the same place, see
/// `tools::region_players`; until its file is read it is silent there.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let region = region(&record(state).map_err(BehaviourError::Other)?);
    let rate = context.prepare_config().sample_rate;
    // How long it plays, for a view to draw it: seconds of its file, so no tempo moves them.
    let seconds = sound_media::info(context.assets(), &region.asset)
        .map(|file| region.length(&file, rate) as f64 / f64::from(rate.max(1)));
    context.watch(SECONDS).set(seconds.unwrap_or(0.0) as f32);
    // A long file is read on a thread of its own: silent until it is there, then this and its
    // parent run again.
    let _audio = match sound_media::in_memory(context.assets(), &region.asset) {
        Some(audio) => Some(audio),
        None => match read(context.assets(), &region.asset, context.id()) {
            Ok(None) => return Ok(()),
            Ok(Some(audio)) => Some(Arc::clone(&*audio)),
            // The snapshot says why.
            Err(_) => None,
        },
    };
    let what = format!("the region {:?}", context.id().name());
    let (_, problems) = sound_media::snapshot([(what, region)], context.assets(), rate);
    for problem in problems {
        context.problem(problem);
    }
    Ok(())
}

/// The files regions play, read on a thread of their own in the window.
static FILES: Loader<PathBuf, Arc<Audio>> = Loader::new("region", Keep::WhileUsed);

/// The file `asset`, or `None` while it is read for the region `waiter`. The loader keeps an
/// `Arc` of what it read; a read gives the shared file itself.
#[allow(clippy::redundant_allocation)]
fn read(
    assets: &Assets,
    asset: &AudioAsset,
    waiter: &InstanceId,
) -> Result<Option<Arc<Arc<Audio>>>, String> {
    let path = assets.path(asset.asset_name());
    let read = {
        let (assets, asset, path) = (assets.clone(), asset.clone(), path.clone());
        move || {
            let audio = sound_media::load(&assets, &asset).map_err(|error| error.to_string());
            (audio, vec![Stamp::of(&path)])
        }
    };
    FILES.get(path, (assets, waiter), read)
}

/// The file of a region its parent plays, or `None` while it is read for the region `waiter`,
/// which runs again with its parent then. The read the region asked for is let go of when
/// nothing held it by the next frame, as when its parent did not play it yet: Bun had not
/// connected it.
pub(crate) fn audio(
    assets: &Assets,
    asset: &AudioAsset,
    waiter: &InstanceId,
) -> Option<Arc<Audio>> {
    sound_media::in_memory(assets, asset)
        .or_else(|| Some(Arc::clone(&*read(assets, asset, waiter).ok()??)))
}

/// The regions of the project of `assets` whose file was read since the last call, to run
/// their behaviour again.
pub(crate) fn take_read(assets: &Assets) -> Vec<InstanceId> {
    FILES.take_done(assets)
}
