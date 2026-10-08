//! The arrangement in a project summary: what plays where, without opening every clip.

use sound_core::{Instance, Project};

use crate::{
    ArrangementState, INSTRUMENT, TrackKind, TrackState, audio_clip_end, audio_clips, clips, tracks,
};

/// One line per track and per clip. Positions are `bar:beat:tick`, the end is where the clip
/// stops, so a clip over bars 5 to 8 reads `5:1:000 to 9:1:000`.
pub(crate) fn of_arrangement(
    project: &Project,
    arrangement: &Instance<ArrangementState>,
) -> String {
    let time_signatures = project.project_file().tempo_map.time_signatures();
    let tracks = tracks(project, arrangement.id());
    let mut lines = vec![format!(
        "arrangement `{}`: {}. Positions are bar:beat:tick, a clip runs up to its end position",
        arrangement.id(),
        counted(tracks.len(), "track"),
    )];
    for (track, state) in tracks {
        if state.kind == TrackKind::Audio {
            audio_track(project, &track, state, &mut lines);
            continue;
        }
        let instrument = track.id().child(INSTRUMENT).ok();
        let instrument = instrument.and_then(|id| project.tool_of(&id));
        // Each effect with its tool, so an agent sees that a record it wrote loaded.
        let effects: Vec<String> = (state.effects.iter())
            .map(|slot| {
                let tool = track.id().child(&slot.name).ok();
                match tool.and_then(|id| project.tool_of(&id)) {
                    Some(tool) => format!("{} ({tool})", slot.name),
                    None => format!("{} (not loaded)", slot.name),
                }
            })
            .collect();
        let effects = match effects.is_empty() {
            true => String::new(),
            false => format!(", effects {}", effects.join(", ")),
        };
        lines.push(format!(
            "  track `{}` {:?}: colour {}, order {}, {}{effects}",
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
                "    clip `{}`: {} to {}, ticks {} to {}, {}{pitches}",
                clip.id(),
                time_signatures.bar_beat_of(state.start),
                time_signatures.bar_beat_of(state.end()),
                state.start.0,
                state.end().0,
                counted(state.notes.len(), "note"),
            ));
        }
    }
    lines.join("\n")
}

/// An audio track and its clips: the file of each, where it plays in the piece and which part
/// of the file it plays.
fn audio_track(
    project: &Project,
    track: &Instance<TrackState>,
    state: &TrackState,
    lines: &mut Vec<String>,
) {
    let time_signatures = project.project_file().tempo_map.time_signatures();
    lines.push(format!(
        "  track `{}` {:?}: colour {}, order {}, audio track",
        track.id(),
        state.name,
        state.colour.name(),
        state.order,
    ));
    let clips = audio_clips(project, track.id());
    if clips.is_empty() {
        lines.push("    no clips".to_string());
    }
    for (clip, state) in clips {
        let file = match sound_media::info(project.assets(), &state.asset) {
            Ok(audio) => {
                let (from, to) = state.file_frames(&audio);
                let rate = f64::from(audio.sample_rate);
                format!(
                    "{} from {:.3} s to {:.3} s of {:.3} s",
                    state.asset,
                    from as f64 / rate,
                    to as f64 / rate,
                    audio.seconds()
                )
            }
            Err(error) => format!("{} is silent: {error}", state.asset),
        };
        let end = audio_clip_end(project, state);
        lines.push(format!(
            "    audio clip `{}`: {} to {}, ticks {} to {}, {file}, gain {} dB, fades {} ms and {} ms, layer {}",
            clip.id(),
            time_signatures.bar_beat_of(state.start),
            time_signatures.bar_beat_of(end),
            state.start.0,
            end.0,
            state.gain_db,
            state.fade_in_ms,
            state.fade_out_ms,
            state.layer,
        ));
    }
}

fn counted(count: usize, thing: &str) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {thing}{plural}")
}
