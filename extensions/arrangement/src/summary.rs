//! The arrangement in a project summary: what plays where, without opening every clip.

use sound_core::{Instance, Project};

use crate::{ArrangementState, INSTRUMENT, clips, tracks};

/// One line per track and per clip. Positions are `bar:beat:tick`, the end is where the clip
/// stops, so a clip over bars 5 to 8 reads `5:1:000 to 9:1:000`.
pub(crate) fn of_arrangement(
    project: &Project,
    arrangement: &Instance<ArrangementState>,
) -> String {
    let time_signature = project.project_file().tempo_map.time_signature();
    let tracks = tracks(project, arrangement.id());
    let mut lines = vec![format!(
        "arrangement `{}`: {} {}. Positions are bar:beat:tick, a clip runs up to its end position",
        arrangement.id(),
        tracks.len(),
        if tracks.len() == 1 { "track" } else { "tracks" },
    )];
    for (track, state) in tracks {
        let instrument = track.id().child(INSTRUMENT).ok();
        let instrument = instrument.and_then(|id| project.tool_of(&id));
        lines.push(format!(
            "  track `{}` {:?}: colour {}, order {}, {}",
            track.id(),
            state.name,
            state.colour.name(),
            state.order,
            match instrument {
                Some(tool) => format!("instrument {tool}"),
                None => format!("no instrument, so it is silent: add {INSTRUMENT}.json"),
            },
        ));
        let clips = clips(project, track.id());
        if clips.is_empty() {
            lines.push("    no clips".to_string());
        }
        for (clip, state) in clips {
            let pitches = state.notes.iter().map(|note| note.pitch.number());
            let pitches = match (pitches.clone().min(), pitches.max()) {
                (Some(lowest), Some(highest)) => format!(", pitch {lowest} to {highest}"),
                _ => String::new(),
            };
            lines.push(format!(
                "    clip `{}`: {} to {}, ticks {} to {}, {} notes{pitches}",
                clip.id(),
                time_signature.bar_beat_of(state.start),
                time_signature.bar_beat_of(state.end()),
                state.start.0,
                state.end().0,
                state.notes.len(),
            ));
        }
    }
    lines.join("\n")
}
