//! The math and the text of the tempo control in the transport. Pure functions, so the drag,
//! the keys and the readout cannot disagree about a value.
//!
//! The transport shows the tempo in effect at the playhead, and an edit changes that tempo
//! change. The control keeps no copy of the tempo map: it reads the one the project has when
//! it renders and when it publishes, so a `project.json` written from outside shows at once
//! and is kept by the next move of a drag.

use sound_core::{Tempo, TempoMap, Ticks};

/// Bpm per point of a plain drag. With shift it is a tenth, as for every drag.
pub const DRAG_PER_POINT: f64 = 0.5;
/// The step a plain drag moves by, from the tempo it began on. It does not snap the result,
/// so a tempo written by hand keeps its fraction. With shift it is a tenth.
pub const DRAG_STEP: f64 = 1.0;
/// What one arrow key adds, plain and with shift.
pub const KEY_STEP: f64 = 1.0;
pub const FINE_KEY_STEP: f64 = 0.1;

/// The name of the undo step of every tempo edit.
pub const LABEL: &str = "Change tempo";

/// The same tempo map with the tempo change that starts exactly at `at` set to `bpm`, kept
/// inside the bounds of the clock so a drag past an end stops there. `None` when the map has
/// no change at that tick any more.
pub fn with_bpm(tempo_map: &TempoMap, at: Ticks, bpm: f64) -> Option<TempoMap> {
    tempo_map.with_tempo_at(at, bounded(bpm))
}

/// A tempo inside the bounds of the clock. `from_bpm` refuses only a tempo outside them, and
/// this one is inside them, so the fallback is never used.
fn bounded(bpm: f64) -> Tempo {
    Tempo::from_bpm(bpm.clamp(Tempo::MIN_BPM, Tempo::MAX_BPM)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use sound_core::TempoChange;

    use super::*;

    fn map(changes: &[(u64, f64)]) -> TempoMap {
        let changes = changes
            .iter()
            .map(|&(tick, bpm)| TempoChange {
                tick: Ticks(tick),
                bpm: Tempo::from_bpm(bpm).unwrap(),
            })
            .collect();
        TempoMap::new("4/4".parse().unwrap(), changes).unwrap()
    }

    #[test]
    fn an_edit_changes_the_tempo_change_at_its_tick_and_leaves_the_rest() {
        let before = map(&[(0, 120.0), (3840, 60.0)]);
        let after = with_bpm(&before, Ticks(3840), 140.0).unwrap();
        assert_eq!(after.time_signature(), before.time_signature());
        assert_eq!(after.tempo_changes()[0], before.tempo_changes()[0]);
        assert_eq!(after.tempo_changes()[1].tick, Ticks(3840));
        assert_eq!(after.tempo_changes()[1].bpm.bpm(), 140.0);

        // Past an end it stops there. A tick with no tempo change has nothing to set.
        let bpm_of = |map: TempoMap| map.tempo_changes()[0].bpm.bpm();
        assert_eq!(bpm_of(with_bpm(&before, Ticks(0), 5.0).unwrap()), 10.0);
        assert_eq!(
            bpm_of(with_bpm(&before, Ticks(0), 5_000.0).unwrap()),
            1000.0
        );
        assert_eq!(with_bpm(&before, Ticks(1920), 140.0), None);
    }
}
