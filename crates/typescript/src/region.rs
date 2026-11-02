//! The `region`: a child of a parent of the project that plays a stretch of an audio file at its
//! place in the song, out of its output `audio`.

use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    Assets, BehaviourContext, BehaviourError, InstanceId, JsonTool, JsonToolDoc, Project, State,
    Ticks, ToolSchema,
};
use sound_dsp::amplitude;
use sound_media::{
    AUDIO_FOLDER, Audio, AudioAsset, Keep, Loader, Periods, Region, Stamp, WarpPoint,
};

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
    /// Seconds of the file pinned to ticks after its start, to play it longer or shorter at its
    /// own pitch. It stops at the last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warp: Vec<WarpPoint>,
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
            group: crate::BLOCKS,
            when: "An audio file at a place in the song: an audio clip or a take".to_string(),
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
    let mut before = WarpPoint {
        ticks: Ticks(0),
        seconds: record.file_start_seconds,
    };
    for (index, point) in record.warp.iter().enumerate() {
        let (ticks, seconds) = match index {
            0 => (
                "the start of the region".to_string(),
                "file_start_seconds".to_string(),
            ),
            _ => (
                format!("warp[{}]", index - 1),
                format!("warp[{}]", index - 1),
            ),
        };
        if point.ticks <= before.ticks {
            return Err(format!(
                "state.warp[{index}].ticks must be more than {}, at {ticks}: they count from the start of the region and go forward, not {}",
                before.ticks.0, point.ticks.0
            ));
        }
        if !(point.seconds.is_finite() && point.seconds > before.seconds) {
            return Err(format!(
                "state.warp[{index}].seconds must be more than {}, at {seconds}: they go forward in the file, not {}",
                before.seconds, point.seconds
            ));
        }
        before = *point;
    }
    if !record.warp.is_empty() && record.file_end_seconds.is_some() {
        return Err(
            "state.file_end_seconds must be left out with warp points: the region stops at its last point"
                .to_string(),
        );
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

/// Where it stops playing in the song. `None` while the file is not there.
fn end(project: &Project, state: &Value) -> Option<Ticks> {
    let region = region(&record(state).ok()?);
    let file = sound_media::info(project.assets(), &region.asset).ok()?;
    Some(region.end(&file, project.clock()))
}

/// The region a record plays.
pub(crate) fn region(record: &Record) -> Region {
    let gain = match record.gain_db <= SILENT_DB {
        true => 0.0,
        false => amplitude(record.gain_db),
    };
    Region {
        asset: record.asset.clone(),
        start: record.start,
        file_start_seconds: record.file_start_seconds,
        file_end_seconds: record.file_end_seconds,
        gain,
        fade_in_ms: record.fade_in_ms,
        fade_out_ms: record.fade_out_ms,
        warp: record.warp.clone(),
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
    let (_, problems) = sound_media::snapshot([(what, region, None)], context.assets(), rate);
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

/// The periods of the file of a region with warp points, read on a thread of their own in the
/// window, as its file is.
static PERIODS: Loader<PathBuf, Periods> = Loader::new("region periods", Keep::WhileUsed);

/// The periods of `asset`, or `None` while they are read for the region `waiter`, which runs
/// again with its parent then, or when its file does not play, which it says itself.
pub(crate) fn periods(
    assets: &Assets,
    asset: &AudioAsset,
    waiter: &InstanceId,
) -> Option<Arc<Periods>> {
    let path = assets.path(asset.asset_name());
    let read = {
        let (assets, asset, path) = (assets.clone(), asset.clone(), path.clone());
        move || {
            let audio = sound_media::load(&assets, &asset).map_err(|error| error.to_string());
            (
                audio.map(|audio| Periods::of(&audio)),
                vec![Stamp::of(&path)],
            )
        }
    };
    PERIODS.get(path, (assets, waiter), read).ok()?
}

/// The regions of the project of `assets` whose file or periods were read since the last call,
/// to run their behaviour again.
pub(crate) fn take_read(assets: &Assets) -> Vec<InstanceId> {
    let mut read = FILES.take_done(assets);
    read.extend(PERIODS.take_done(assets));
    read
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn refusal(warp: Value, file_end_seconds: Option<f64>) -> String {
        let state = json!({
            "asset": "voice.wav",
            "file_start_seconds": 0.5,
            "file_end_seconds": file_end_seconds,
            "warp": warp,
        });
        record(&state).err().unwrap_or_default()
    }

    #[test]
    fn warp_points_that_do_not_go_forward_are_refused_with_what_to_change() {
        let refusals = [
            (
                json!([{"ticks": 0, "seconds": 1.0}]),
                "state.warp[0].ticks must be more than 0, at the start of the region",
            ),
            (
                json!([{"ticks": 960, "seconds": 0.5}]),
                "state.warp[0].seconds must be more than 0.5, at file_start_seconds",
            ),
            (
                json!([{"ticks": 960, "seconds": 1.0}, {"ticks": 960, "seconds": 2.0}]),
                "state.warp[1].ticks must be more than 960, at warp[0]",
            ),
            (
                json!([{"ticks": 960, "seconds": 1.0}, {"ticks": 1920, "seconds": 0.9}]),
                "state.warp[1].seconds must be more than 1, at warp[0]",
            ),
        ];
        for (warp, expected) in refusals {
            let refusal = refusal(warp, None);
            assert!(refusal.starts_with(expected), "{refusal}");
        }
        let warp = json!([{"ticks": 960, "seconds": 1.0}, {"ticks": 1920, "seconds": 1.5}]);
        assert_eq!(refusal(warp.clone(), None), "");
        let refusal = refusal(warp, Some(2.0));
        assert!(refusal.starts_with("state.file_end_seconds must be left out"));
    }

    #[test]
    fn a_record_without_warp_points_is_written_as_it_was() {
        let state = json!({
            "asset": "voice.wav",
            "start": 3840,
            "file_start_seconds": 0.5,
            "file_end_seconds": null,
            "gain_db": 0.0,
            "fade_in_ms": 10.0,
            "fade_out_ms": 0.0,
        });
        let written = serde_json::to_value(record(&state).ok()).ok();
        assert_eq!(written, Some(state));
    }
}
