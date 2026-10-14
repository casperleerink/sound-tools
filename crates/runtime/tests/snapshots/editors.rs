//! Clips in editors of the project, see `shared/editors.rs`:
//!
//! - `editors-drums.png`: a beat in the drum grid: a named row per sound, 20 pt high, and the
//!   hits as squares, one selected.
//! - `editors-scale.png`: a tune in C major: only the rows of the scale, the Cs named, and an
//!   F# on an extra row of its own, shaded as outside the clip and named.
//! - `editors-menu.png`: the menu of that clip, its Editor submenu open on Scale, and the row
//!   of its scale choice.

use anyhow::{Context as _, Result};
use gpui::HeadlessAppContext;
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::{Clip, EditorName, Editors, Length, Note, Pitch, Velocity};

use super::clip_editors::{drums, scale};
use super::{BAR, Opened};

const BEAT: &str = "arrangement/track-1/beat";
const TUNE: &str = "arrangement/track-1/tune";

fn note(start: u64, pitch: u8, velocity: u8) -> Result<Note> {
    Ok(Note::new(
        Ticks(start),
        Length::new(Ticks(240))?,
        Pitch::new(pitch)?,
        Velocity::new(velocity)?,
    ))
}

fn clip_in(editor: &str, start: u64, notes: Vec<Note>) -> Result<Clip> {
    let mut clip = Clip::new(Ticks(start), Length::new(Ticks(2 * BAR))?, notes);
    clip.editor = Some(EditorName::new(editor)?);
    Ok(clip)
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let opened = Opened::new(cx, |project| {
        let editors = project.provided::<Editors>().context("no editors")?;
        let refused = editors
            .set_project(vec![drums(), scale(&["major", "minor"])])
            .refused;
        anyhow::ensure!(refused.is_empty(), "refused: {refused:?}");
        let mut beat = Vec::new();
        for bar in 0..2 {
            for eighth in 0..8_u64 {
                let start = bar * BAR + eighth * 480;
                let hat = if eighth == 7 { 46 } else { 42 };
                beat.push(note(start, hat, if eighth % 2 == 0 { 100 } else { 70 })?);
                match eighth {
                    0 | 5 => beat.push(note(start, 36, 120)?),
                    2 | 6 => beat.push(note(start, 38, 110)?),
                    _ => {}
                }
            }
        }
        let tune = [
            (0, 60),
            (480, 64),
            (960, 67),
            (1440, 66),
            (1920, 72),
            (2880, 71),
        ];
        let tune: Vec<Note> = tune
            .into_iter()
            .map(|(start, pitch)| note(start, pitch, 100))
            .collect::<Result<_>>()?;
        let mut changes = Changes::new();
        changes.create(InstanceId::new(BEAT)?, clip_in("drums", 0, beat)?);
        changes.create(InstanceId::new(TUNE)?, clip_in("scale", 3 * BAR, tune)?);
        project.commit("Add clips", changes)?;
        Ok(())
    })?;
    let editor = opened.open_editor(&InstanceId::new(BEAT)?, cx)?;
    cx.update(|cx| editor.update(cx, |editor, cx| editor.select_notes(&[3], cx)));
    cx.run_until_parked();
    save(cx, &opened, "editors-drums")?;
    opened.open_editor(&InstanceId::new(TUNE)?, cx)?;
    save(cx, &opened, "editors-scale")?;
    let timeline = opened.timeline_view(cx)?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        timeline.update(cx, |timeline, cx| timeline.open_clip_menu(window, cx))
    })?;
    cx.run_until_parked();
    for key in ["down", "right"] {
        opened.key(key, cx)?;
    }
    save(cx, &opened, "editors-menu")?;
    Ok(())
}
