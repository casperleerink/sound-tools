pub mod arrangement;
pub mod dsp;
pub mod engine;
pub mod sample_asset;
pub mod sampler;

pub use arrangement::*;

use sound_core::{Error, Result};

pub const DEFAULT_STATE: &str = r#"{"tracks":[]}"#;

pub fn default_state() -> Result<serde_json::Value> {
    serde_json::from_str(DEFAULT_STATE).map_err(|error| Error(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_decodes() {
        let state: Arrangement = serde_json::from_str(DEFAULT_STATE).unwrap();
        assert!(state.tracks.is_empty());
    }
}
