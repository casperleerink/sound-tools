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
