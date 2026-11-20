//! What its feeds and facts show: its file, where it plays in it, and its library instrument.

use schemars::JsonSchema;
use serde::Serialize;
use sound_core::feeds::FeedContext;
use sound_media::{Cached, FileFacts};

use crate::SamplerState;
use crate::library::{self, CATALOG, Entry, Status};

/// Columns of a wave a feed gives unless the binding asks for another number.
const POINTS: usize = 96;

/// See the feed `wave`.
pub(crate) fn wave(state: &SamplerState, context: &FeedContext<'_>) -> Vec<f32> {
    let Some(asset) = &state.sample else {
        return Vec::new();
    };
    let Cached::Plays(info) = sound_media::cached(context.assets(), asset) else {
        return Vec::new();
    };
    let columns = context.points_or(POINTS);
    let edges: Vec<f64> = context.across([0.0, info.seconds()], columns + 1).collect();
    (context.wave(asset.asset_name(), &edges)).unwrap_or_default()
}

/// See the feed `envelope`.
pub(crate) fn envelope(state: &SamplerState, context: &FeedContext<'_>) -> Vec<f32> {
    let Some(asset) = &state.sample else {
        return Vec::new();
    };
    let Cached::Plays(info) = sound_media::cached(context.assets(), asset) else {
        return Vec::new();
    };
    let length = info.seconds();
    context.curve([0.0, length], POINTS, |seconds| {
        envelope_at(state, f64::from(seconds), length)
    })
}

/// The level of a held note at `seconds` of a file of `length`, as the card draws it: silent
/// before the start, straight up to full over the attack, straight down to the sustain over the
/// decay, and held there to the end.
fn envelope_at(state: &SamplerState, seconds: f64, length: f64) -> f32 {
    let start = state.start_seconds;
    let end = state.end_seconds.unwrap_or(length);
    let (attack, decay) = (
        f64::from(state.attack_seconds),
        f64::from(state.decay_seconds),
    );
    let sustain = state.sustain.clamp(0.0, 1.0);
    let into = seconds - start;
    if into < 0.0 || seconds > end.max(start + attack + decay) {
        return 0.0;
    }
    if into < attack {
        return (into / attack) as f32;
    }
    if into < attack + decay {
        let part = ((into - attack) / decay) as f32;
        return 1.0 + (sustain - 1.0) * part;
    }
    sustain
}

/// What its card knows that is no record.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Facts {
    /// The file of `sample`; null without one.
    file: Option<FileFacts>,
    /// The name of the SFZ instrument of `sfz`; null without one.
    sfz: Option<String>,
    /// The library instrument of `library`; null without one.
    instrument: Option<Instrument>,
    /// Every instrument of the library, for a picker.
    library: Vec<Offer>,
}

#[derive(Serialize, JsonSchema)]
struct Instrument {
    name: &'static str,
    /// Its licence, short: `CC0`, `CC BY 4.0`, and what a CC BY licence asks to show.
    license: &'static str,
    attribution: Option<&'static str>,
    state: Download,
    /// What is here of its download, and all of it, in bytes.
    downloaded: u64,
    download: u64,
    /// Why its last download failed.
    error: Option<String>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Download {
    Here,
    Missing,
    Downloading,
    Failed,
    /// This computer keeps no library.
    NoLibrary,
}

#[derive(Serialize, JsonSchema)]
struct Offer {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    /// What it takes on disk, such as `85 MB`.
    size: String,
    here: bool,
}

impl Facts {
    pub(crate) fn of(state: &SamplerState, context: &sound_core::facts::FactsContext<'_>) -> Self {
        let file = (state.sample.as_ref()).map(|asset| FileFacts::of(context.assets(), asset));
        let sfz = state.sfz.as_ref().map(|path| {
            let path = path.to_string();
            let file = path.rsplit('/').next().unwrap_or(&path);
            file.strip_suffix(".sfz").unwrap_or(file).to_string()
        });
        let instrument = (state.library.as_ref()).map(|id| instrument(id.entry()));
        let library = (CATALOG.iter())
            .map(|entry| Offer {
                id: entry.id,
                name: entry.name,
                category: entry.category.name(),
                size: library::size_text(entry.disk_bytes),
                here: library::status(entry) == Status::Here,
            })
            .collect();
        Self {
            file,
            sfz,
            instrument,
            library,
        }
    }
}

fn instrument(entry: &'static Entry) -> Instrument {
    let (state, downloaded, error) = match library::status(entry) {
        Status::Here => (Download::Here, entry.download_bytes, None),
        Status::Missing => (Download::Missing, 0, None),
        Status::Downloading { bytes } => (Download::Downloading, bytes, None),
        Status::Failed(error) => (Download::Failed, 0, Some(error)),
        Status::NoLibrary => (Download::NoLibrary, 0, None),
    };
    Instrument {
        name: entry.name,
        license: entry.library.license,
        attribution: entry.library.attribution,
        state,
        downloaded,
        download: entry.download_bytes,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handles of the card sit on these corners: the top of the attack and the sustain after
    /// the decay, both from the start line.
    #[test]
    fn the_envelope_rises_from_the_start_and_holds_the_sustain_to_the_end() {
        let state = SamplerState {
            start_seconds: 0.1,
            end_seconds: Some(1.0),
            attack_seconds: 0.1,
            decay_seconds: 0.2,
            sustain: 0.5,
            ..SamplerState::default()
        };
        let at = |seconds| envelope_at(&state, seconds, 2.0);
        assert_eq!(at(0.05), 0.0);
        assert!((at(0.15) - 0.5).abs() < 1e-6);
        assert!((at(0.2) - 1.0).abs() < 1e-6);
        assert!((at(0.3) - 0.75).abs() < 1e-6);
        assert_eq!(at(0.8), 0.5);
        assert_eq!(at(1.5), 0.0);
    }
}
