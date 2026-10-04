//! Audio files dropped from the Finder: where they would go and the clips they make.

use std::path::PathBuf;

use gpui::{Context, prelude::*};
use sound_core::{Changes, InstanceId, ProjectError};

use super::Timeline;
use super::state::{DropTarget, Incoming};
use crate::view::plural;
use crate::{AudioClip, Colour, TrackKind, TrackState, add_audio_clips, add_audio_track, tracks};

impl Timeline {
    /// Where files dropped at a place of the timeline area would go: an audio track under the
    /// pointer, or a new one under the last track, from the snap step under the pointer. Over an
    /// instrument track, the ruler or the headers, nowhere.
    pub fn drop_target_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> Option<DropTarget> {
        self.refresh_order(cx);
        if x < 0. || y < 0. {
            return None;
        }
        let viewport = self.painted.get();
        let tick = self.grid(cx).floor(viewport.tick_at(x));
        let rows = self.rows(cx);
        let Some(row) = viewport.track_at(&rows, y) else {
            let below = y >= viewport.y_at(rows.height());
            return below.then_some(DropTarget::NewTrack(tick));
        };
        let track = self.order.get(row)?;
        let project = self.session.read(cx).project();
        let audio = project.state(track)?.kind == TrackKind::Audio;
        audio.then(|| DropTarget::Track(track.id().clone(), tick))
    }

    /// Files from the Finder are dragged over a place of the timeline area: the ghosts of the
    /// clips they would make follow the pointer. What each file is, for how long its ghost is,
    /// is read on a background thread the first time.
    pub fn drag_files_over(&mut self, paths: Vec<PathBuf>, x: f32, y: f32, cx: &mut Context<Self>) {
        let target = self.drop_target_at(x, y, cx);
        let known = self
            .incoming
            .as_ref()
            .is_some_and(|incoming| incoming.paths == paths);
        if !known {
            let count = paths.len();
            self.incoming = Some(Incoming {
                paths: paths.clone(),
                files: vec![None; count],
                target: None,
            });
            // The header of each file only, on a background thread.
            let reading = cx.background_spawn(async move {
                let read = |path: &PathBuf| sound_media::probe(path).ok();
                paths.iter().map(read).collect::<Vec<_>>()
            });
            cx.spawn(async move |timeline, cx| {
                let files = reading.await;
                timeline
                    .update(cx, |timeline, cx| {
                        if let Some(incoming) = &mut timeline.incoming
                            && incoming.files.len() == files.len()
                        {
                            incoming.files = files;
                            cx.notify();
                        }
                    })
                    .ok();
            })
            .detach();
        }
        if let Some(incoming) = &mut self.incoming
            && incoming.target != target
        {
            incoming.target = target;
            cx.notify();
        }
    }

    /// The drag of files left the timeline, or ended.
    pub fn forget_files(&mut self, cx: &mut Context<Self>) {
        self.dragged_paths.borrow_mut().clear();
        if self.incoming.take().is_some() {
            cx.notify();
        }
    }

    /// Where the files dragged over the timeline would go now.
    pub fn incoming_target(&self) -> Option<&DropTarget> {
        self.incoming.as_ref()?.target.as_ref()
    }

    /// Files dropped from the Finder: each is copied into `assets/audio/` on a background
    /// thread, then all become clips one after another on one track, from the target on, as
    /// one undo step. A file that is no audio this app plays is left out, and the notice says
    /// why. Under the last track the drop makes a new audio track named after the first file.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, target: DropTarget, cx: &mut Context<Self>) {
        let assets = self.session.read(cx).project().assets().clone();
        let importing = cx.background_spawn(async move {
            paths
                .iter()
                .map(|path| {
                    let name = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned());
                    (name.unwrap_or_default(), sound_media::import(&assets, path))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |timeline, cx| {
            let imported = importing.await;
            timeline
                .update(cx, |timeline, cx| {
                    timeline.add_dropped(imported, target, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The clips of files that were copied in, as one undo step, selected.
    fn add_dropped(
        &mut self,
        imported: Vec<(
            String,
            Result<sound_media::Imported, sound_media::MediaError>,
        )>,
        target: DropTarget,
        cx: &mut Context<Self>,
    ) {
        let mut files = Vec::new();
        for (name, result) in imported {
            match result {
                Ok(asset) => files.push((name, asset)),
                Err(error) => {
                    let session = self.session.clone();
                    session.update(cx, |session, cx| session.report(error, cx));
                }
            }
        }
        let Some((first_name, _)) = files.first() else {
            return;
        };
        let first_name = first_name.clone();
        let arrangement = self.arrangement.clone();
        let label = plural(files.len(), "Add audio clip", "Add audio clips");
        let added = self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                let (track, start) = match target {
                    DropTarget::Track(track, start) => {
                        let missing = || ProjectError::MissingInstance(track.clone());
                        (
                            project.resolve::<TrackState>(&track).ok_or_else(missing)?,
                            start,
                        )
                    }
                    DropTarget::NewTrack(start) => {
                        let count = tracks(project, arrangement.id()).len();
                        let colour = Colour::ALL[count % Colour::ALL.len()];
                        let track = add_audio_track(
                            project,
                            &mut changes,
                            arrangement.id(),
                            &first_name,
                            colour,
                        )?;
                        (track, start)
                    }
                };
                // One after another: each starts where the one before it ends.
                // The files are in memory here, so this reads nothing, and the track that plays
                // them reads nothing either while they are held.
                let clock = project.clock();
                let mut at = start;
                let mut clips = Vec::new();
                for (_, imported) in &files {
                    let clip = AudioClip::new(imported.asset.clone(), at);
                    at = clip.end(Some(&imported.audio.info()), clock);
                    clips.push((imported.asset.asset_name().name().to_string(), clip));
                }
                let clips = clips
                    .iter()
                    .map(|(name, clip)| (&track, name.as_str(), clip.clone()));
                let added = add_audio_clips(project, &mut changes, clips)?;
                project.commit(label, changes)?;
                Ok(added)
            })
        });
        if let Some(added) = added {
            let ids: Vec<InstanceId> = added.iter().map(|clip| clip.id().clone()).collect();
            let primary = ids.first().cloned();
            self.set_clips(ids, primary, cx);
        }
    }
}
