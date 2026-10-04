//! Builds the scene of a paint from the project: the rows, the clips, the lanes, and the
//! ghosts of a drag or a drop.

use std::collections::BTreeMap;
use std::ops::Range;

use gpui::{App, SharedString};
use sound_core::{InstanceId, Project, Ticks};
use sound_media::Cached;
use sound_notes::Clip;
use sound_ui::components::audio_clip::ClipHandle;
use sound_ui::{ActiveTheme, LiveBody, LiveSound};

use super::Timeline;
use super::scene::{
    AudioShape, Body, ClipShape, Ghosts, LanePoint, LaneShape, PointKey, Scene, Sound, TempoMark,
    TrackRow, lanes_label,
};
use super::state::{
    ClipDragKind, DropTarget, Edge, EdgeDrag, Held, LaneDragKind, LaneGhost, MoveDrag,
};
use crate::view::clips::{gain_label, time_label};
use crate::view::layout::{LANE_HEIGHT, Rect, Rows, Viewport, ordered};
use crate::view::paint::accent;
use crate::view::track_lanes;
use crate::{AudioClip, AutomationLane, TrackKind, TrackState, shown_end, travel_in};

impl Timeline {
    /// Everything to paint into a timeline area of this size, read from the project now.
    pub(super) fn scene(&self, width: f32, height: f32, cx: &App) -> Scene {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let tempo_map = &project.project_file().tempo_map;
        let time_signatures = tempo_map.time_signatures();
        // Clamped again for this size: the window may have grown since the last scroll.
        let viewport = self.clamped(self.viewport, width, height, cx);
        let visible_ticks = viewport.visible_ticks(width);
        let renaming = self.rename.as_ref().map(|rename| rename.track.id());

        let changes = tempo_map.tempo_changes().iter();
        let tempo = changes
            .filter(|change| change.tick > Ticks(0))
            .map(|change| TempoMark {
                tick: change.tick,
                x: viewport.x_of(change.tick),
                text: change.bpm.to_string().into(),
                selected: self.selected_tempo == Some(change.tick),
            })
            // A label reaches right of its tick, so one that starts just left of the area
            // still shows its end.
            .filter(|mark| (-TEMPO_LABEL_ROOM..width).contains(&mark.x))
            .collect();
        let marquee = match &self.held {
            Held::Marquee(marquee) => Some(marquee),
            _ => None,
        };
        let marquee = marquee.map(|marquee| {
            let (left, right) = ordered(marquee.from.0, marquee.to.0);
            let (top, bottom) = ordered(marquee.from.1, marquee.to.1);
            let x = viewport.x_of(left);
            Rect {
                x,
                y: (top - viewport.scroll_y) as f32,
                width: viewport.x_of(right) - x,
                height: (bottom - top) as f32,
            }
        });

        let layout = self.rows(cx);
        let lane_ghosts = self.lane_ghosts();
        let mut scene = Scene {
            viewport,
            clips: Vec::new(),
            rows: Vec::new(),
            bars: viewport.ruler_bars(time_signatures, width),
            tempo,
            tempo_zones: Vec::new(),
            marquee,
            ghosts: self.ghosts(&viewport, &layout, project),
            assets: project.assets().clone(),
            lanes: Vec::new(),
            hint: None,
            layout,
        };
        let layout = scene.layout.clone();
        let recording = self.recording.read(cx);
        let visible =
            |start: Ticks, end: Ticks| start < visible_ticks.end && end > visible_ticks.start;
        for index in viewport.visible_tracks(&layout, height) {
            let Some(track) = self.order.get(index) else {
                break;
            };
            // Gone since the order was read: the render after its event leaves it out.
            let Some(state) = project.state(track) else {
                continue;
            };
            let accent = accent(state.colour, theme);
            let expanded = layout.lanes(index).is_some();
            scene.rows.push(TrackRow {
                y: viewport.y_of(&layout, index),
                name: state.name.clone().into(),
                accent,
                kind: state.kind,
                selected: self.selected_track.as_ref() == Some(track.id()),
                muted: state.mute,
                renaming: renaming == Some(track.id()),
                armed: state.kind == TrackKind::Audio && recording.is_armed(track.id()),
                lifted: matches!(
                    &self.held,
                    Held::Track(drag) if drag.moving && drag.track == *track.id()
                ),
                expanded,
                automated: !state.automation.is_empty(),
                lanes_label: lanes_label(state.automation.len()),
            });
            if expanded {
                let count = state.automation.len();
                let tops = (0..count).map(|lane| viewport.y_at(layout.lane_top(index, lane)));
                let area = (width, height);
                let shapes = self.lane_shapes(track.id(), state, &viewport, area, tops, cx);
                scene.lanes.extend(shapes);
            }
            let shape = |id: &InstanceId, rect: Rect, body: Body| ClipShape {
                id: id.clone(),
                rect,
                body,
                accent,
                selected: self.clips.contains(id),
                muted: state.mute,
                carries: !expanded && lane_ghosts.iter().any(|ghost| ghost.clip == *id),
            };
            // The order of `clips()`, by start and then by id, for the few that are visible:
            // it decides which of two overlapping clips is on top.
            let mut notes: Vec<_> = project
                .children::<Clip>(track.id())
                .filter(|(_, clip)| visible(clip.start, clip.end()))
                .collect();
            notes.sort_by(|(a, a_clip), (b, b_clip)| {
                (a_clip.start, a.id()).cmp(&(b_clip.start, b.id()))
            });
            for (clip, state) in notes {
                let rect = viewport.clip_rect(&layout, index, state.start, state.end());
                let body = Body::Notes(viewport.miniature(state, rect).collect());
                scene.clips.push(shape(clip.id(), rect, body));
            }
            // Audio clips in the order they are heard: the one on top of an overlap is the one
            // that plays, and the one a press takes.
            let mut audio: Vec<_> = project
                .children::<AudioClip>(track.id())
                .map(|(clip, state)| (clip, state, shown_end(project, state)))
                .filter(|(_, clip, end)| visible(clip.start, *end))
                .collect();
            audio.sort_by(|(a, a_clip, _), (b, b_clip, _)| {
                (a_clip.layer, a_clip.start, a.id()).cmp(&(b_clip.layer, b_clip.start, b.id()))
            });
            for (clip, state, end) in audio {
                let rect = viewport.clip_rect(&layout, index, state.start, end);
                let body = self.audio_shape(clip.id(), state, rect, &viewport, width, project);
                scene
                    .clips
                    .push(shape(clip.id(), rect, Body::Audio(Box::new(body))));
            }
        }
        // The hint goes in the top left corner of the clip under the pointer, while the drag
        // takes automation, as the value of a fade shows in its clip: what it covers there is
        // what the pointer holds.
        let grabbed = self.held.clip_drag().and_then(ClipDragKind::grabbed);
        let grabbed = grabbed.filter(|_| !lane_ghosts.is_empty());
        let under = grabbed.and_then(|id| scene.clips.iter().find(|shape| shape.id == *id));
        scene.hint = under.map(|shape| (shape.rect.x + 4., shape.rect.y + 4.));
        scene
    }

    /// Where the clips of a drag put the automation they take along, as its last mouse move
    /// wrote it. Empty while no drag of clips takes any.
    fn lane_ghosts(&self) -> &[LaneGhost] {
        match self.held.clip_drag() {
            Some(ClipDragKind::Move(MoveDrag { ghosts, .. })) => ghosts,
            _ => &[],
        }
    }

    /// Whether the drag of clips going on takes automation along: when its hint shows, with
    /// the ghosts of the lines, or the mark on a clip whose lanes are folded away.
    pub fn automation_moves(&self) -> bool {
        !self.lane_ghosts().is_empty()
    }

    /// The tracks as they were when a drag of clips opened its gesture, while it goes on.
    fn tracks_before_drag(&self) -> Option<&BTreeMap<InstanceId, TrackState>> {
        match self.held.clip_drag() {
            Some(ClipDragKind::Move(MoveDrag { tracks, .. })) if !tracks.is_empty() => Some(tracks),
            _ => None,
        }
    }

    /// The lanes of `track`, whose record is `state`, whose tops in the timeline area are
    /// `tops`, as they show in an area of `(width, height)`: each with its name and its line,
    /// and while clips are dragged, where each lands with the lane it carries, with the line
    /// that was there before. Only the lanes that show.
    fn lane_shapes(
        &self,
        track: &InstanceId,
        state: &TrackState,
        viewport: &Viewport,
        (width, height): (f32, f32),
        tops: impl Iterator<Item = f32>,
        cx: &App,
    ) -> Vec<LaneShape> {
        let project = self.session.read(cx).project();
        let travel = travel_in(project);
        let accent = accent(state.colour, cx.theme());
        let visible = viewport.visible_ticks(width);
        let before = self.tracks_before_drag();
        let before = before.and_then(|tracks| tracks.get(track));
        let ghosts = self.lane_ghosts().iter();
        let ghosts: Vec<&LaneGhost> = ghosts.filter(|ghost| ghost.track == *track).collect();
        let lanes = state.automation.iter().zip(tops);
        let lanes = lanes.filter(|(_, y)| *y < height && *y + LANE_HEIGHT > 0.);
        let lanes = lanes.map(|(lane, y)| {
            let range = lane.number(track, state, &travel);
            let range = range.map(|number| number.range);
            let line = |lane: &AutomationLane, ticks: Range<Ticks>| {
                let line = range.map(|range| track_lanes::line(viewport, lane, range, ticks));
                line.unwrap_or_default()
            };
            let was = before.and_then(|before| {
                let mut lanes = before.automation.iter();
                lanes.find(|was| was.same_number(lane))
            });
            let ghosts = ghosts.iter().filter(|ghost| {
                let mut carried = ghost.lanes.iter();
                carried.any(|carried| carried.same_number(lane))
            });
            let ghosts = ghosts.map(|ghost| {
                let across = viewport.x_of(ghost.range.start)..viewport.x_of(ghost.range.end);
                let replaced = was.map(|was| line(was, ghost.range.clone()));
                (across, replaced.unwrap_or_default())
            });
            let name = match &lane.device {
                Some(device) => {
                    let number = self.number_name(track, device, &lane.parameter, cx);
                    format!("{} · {number}", self.device_name(track, device, cx))
                }
                None => track_lanes::lane_name(&lane.parameter),
            };
            let is = |key: &Option<PointKey>, tick| {
                key.as_ref()
                    .is_some_and(|key| key.is_in(track, lane) && key.tick == tick)
            };
            // A lane may have many points and the timeline shows a few bars of it.
            let from = lane
                .points
                .partition_point(|point| point.tick < visible.start);
            let to = lane
                .points
                .partition_point(|point| point.tick < visible.end);
            let shown = lane.points.get(from..to).unwrap_or_default().iter();
            let dragging = matches!(
                &self.held,
                Held::Lane(drag) if matches!(drag.kind, LaneDragKind::Point { moving: true, .. })
            );
            let points = range.map(|range| {
                let points = shown.map(|point| {
                    let (x, y) = track_lanes::place(viewport, range, point);
                    let hovered = is(&self.hovered_point, point.tick);
                    let selected = is(&self.selected_point, point.tick);
                    let readout = (hovered || selected && dragging).then(|| {
                        let device = lane.device.as_deref();
                        track_lanes::readout(device, &lane.parameter, point.value.0).into()
                    });
                    LanePoint {
                        x,
                        y,
                        hovered,
                        selected,
                        readout,
                    }
                });
                points.collect()
            });
            LaneShape {
                y,
                name: name.into(),
                accent,
                muted: state.mute,
                line: line(lane, visible.clone()),
                ghosts: ghosts.collect(),
                points: points.unwrap_or_default(),
            }
        });
        lanes.collect()
    }

    /// The takes while they record, from where the recording began to the playhead, in the
    /// viewport painted last. `RecordingOverlay` paints them over the timeline every frame while
    /// they grow, so the timeline itself is not painted again for them.
    pub(in crate::view) fn take_shapes(&self, width: f32, cx: &App) -> Vec<ClipShape> {
        let project = self.session.read(cx).project();
        let recording = self.recording.read(cx);
        let playhead = self.playhead.read(cx).tick;
        let viewport = self.painted.get();
        let theme = cx.theme();
        let rows = self.rows(cx);
        let mut shapes = Vec::new();
        for take in recording.takes() {
            let Some(index) = self.row_of(&take.track) else {
                continue;
            };
            let Some(state) = self.order.get(index).and_then(|track| project.state(track)) else {
                continue;
            };
            let end = playhead.max(take.start + Ticks(1));
            let rect = viewport.clip_rect(&rows, index, take.start, end);
            let body = match &take.body {
                LiveBody::Audio(sound) => {
                    let sound = sound.as_ref();
                    let shape = live_shape(take.start, sound, rect, &viewport, width, project);
                    Body::Audio(Box::new(shape))
                }
                LiveBody::Notes(notes) => Body::Notes(live_notes(notes.as_ref(), rect, &viewport)),
            };
            shapes.push(ClipShape {
                id: take.track.clone(),
                rect,
                body,
                accent: accent(state.colour, theme),
                selected: false,
                muted: state.mute,
                carries: false,
            });
        }
        shapes
    }

    /// What an audio clip shows: the times of its file under each column on screen, its fades
    /// in points, its handles while the pointer is on it or it is selected, and while a drag
    /// changes it, the value that drag shows or the part of the file past the edge it moves.
    fn audio_shape(
        &self,
        id: &InstanceId,
        clip: &AudioClip,
        rect: Rect,
        viewport: &Viewport,
        width: f32,
        project: &Project,
    ) -> AudioShape {
        let clock = project.clock();
        let file = sound_media::cached(project.assets(), &clip.asset);
        let start = clock.seconds_of(clip.start);
        // The time of the file at a place across: the clip plays at the speed of its file.
        let file_time =
            |x: f32| clock.seconds_of(viewport.tick_at(x)) - start + clip.file_start_seconds;
        let edges = |from: f32, to: f32| -> (f32, Vec<f64>) {
            let (from, to) = (from.max(0.).floor(), to.min(width).ceil());
            let columns = (to - from).max(0.) as usize;
            let edges = (0..=columns).map(|column| file_time(from + column as f32));
            (from, edges.collect())
        };
        let (first, edges_inside) = edges(rect.x, rect.x + rect.width);
        let x_at = |seconds: f64| viewport.x_of(clock.tick_at_seconds(seconds));
        let fade_in = x_at(start + f64::from(clip.fade_in_ms) / 1000.) - rect.x;
        let end = rect.x + rect.width;
        let end_seconds = clock.seconds_of(viewport.tick_at(end));
        let fade_out = end - x_at(end_seconds - f64::from(clip.fade_out_ms) / 1000.);

        let dragged = self.held.clip_drag();
        let dragged = dragged.filter(|kind| kind.grabbed() == Some(id));
        let (mut hidden, mut label) = (None, None);
        match dragged {
            Some(ClipDragKind::Trim(EdgeDrag { edge, file, .. })) => {
                // The whole file from where it starts on the timeline to where it ends.
                let file_start = x_at(start - clip.file_start_seconds);
                let file_end = x_at(start - clip.file_start_seconds + file.seconds());
                hidden = Some(match edge {
                    Edge::Left => edges(file_start, rect.x),
                    Edge::Right => edges(end, file_end),
                });
            }
            Some(ClipDragKind::Fade(EdgeDrag {
                edge: Edge::Left, ..
            })) => {
                let text = format!("Fade in {}", time_label(clip.fade_in_ms));
                label = Some((text.into(), ClipHandle::FadeIn));
            }
            Some(ClipDragKind::Fade(EdgeDrag {
                edge: Edge::Right, ..
            })) => {
                let text = format!("Fade out {}", time_label(clip.fade_out_ms));
                label = Some((text.into(), ClipHandle::FadeOut));
            }
            Some(ClipDragKind::Gain(_)) => {
                label = Some((gain_label(clip.gain_db).into(), ClipHandle::Gain));
            }
            Some(ClipDragKind::Move(_) | ClipDragKind::Resize(_)) | None => {}
        }
        let missing = match file {
            Cached::Missing => Some(format!("{} is missing", clip.asset)),
            Cached::DoesNotPlay(_) => Some(format!("{} does not play", clip.asset)),
            Cached::Plays(_) | Cached::Unknown => None,
        };
        AudioShape {
            sound: Sound::File(clip.asset.clone()),
            first,
            edges: edges_inside,
            gain: crate::decibels::amplitude(clip.gain_db),
            fade_in: fade_in.max(0.),
            fade_out: fade_out.max(0.),
            hidden,
            label,
            missing: missing.map(SharedString::from),
            handles: self.hovered.as_ref() == Some(id) || self.clips.contains(id),
        }
    }

    /// The ghosts of the clips a drop of files would make, while files are dragged over.
    fn ghosts(&self, viewport: &Viewport, rows: &Rows, project: &Project) -> Option<Ghosts> {
        let incoming = self.incoming.as_ref()?;
        let (row, start) = match incoming.target.as_ref()? {
            DropTarget::Track(track, start) => (self.row_of(track)?, *start),
            DropTarget::NewTrack(start) => (self.order.len(), *start),
        };
        let clock = project.clock();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        let mut at = start;
        let mut clips = Vec::new();
        for (index, path) in incoming.paths.iter().enumerate() {
            // The length of the file once it is known, else a bar.
            let end = match incoming.files.get(index).copied().flatten() {
                Some(file) => {
                    let frames = sound_media::engine_frames(
                        file.frames,
                        file.sample_rate,
                        clock.sample_rate(),
                    );
                    clock.tick_at(sound_core::Frames(clock.frame_of(at).0 + frames))
                }
                None => at + time_signatures.bar_at(at).length(),
            };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let rect = viewport.clip_rect(rows, row, at, end.max(at + Ticks(1)));
            clips.push((rect, SharedString::from(name.unwrap_or_default())));
            at = end;
        }
        let new_track = matches!(incoming.target, Some(DropTarget::NewTrack(_)))
            .then(|| viewport.y_of(rows, row));
        Some(Ghosts { clips, new_track })
    }
}

/// How far right of its tick a tempo label may start to still be seen when its tick is off the
/// left edge.
const TEMPO_LABEL_ROOM: f32 = 80.;

/// What an audio take shows while it records: the times of its file under each column on
/// screen, lined up where the composer heard them, and no handles.
fn live_shape(
    start: Ticks,
    sound: Option<&LiveSound>,
    rect: Rect,
    viewport: &Viewport,
    width: f32,
    project: &Project,
) -> AudioShape {
    let clock = project.clock();
    let start_seconds = sound.map_or(0., |sound| sound.start_seconds);
    let start = clock.seconds_of(start);
    let (from, to) = (
        rect.x.max(0.).floor(),
        (rect.x + rect.width).min(width).ceil(),
    );
    let columns = (to - from).max(0.) as usize;
    let edges = (0..=columns).map(|column| {
        clock.seconds_of(viewport.tick_at(from + column as f32)) - start + start_seconds
    });
    AudioShape {
        sound: Sound::Take(sound.map(|sound| sound.overview.clone())),
        first: from,
        edges: edges.collect(),
        gain: 1.,
        fade_in: 0.,
        fade_out: 0.,
        hidden: None,
        label: None,
        missing: None,
        handles: false,
    }
}

/// The notes a MIDI take shows while it records, cut at the playhead: a held key ends at the
/// last poll, which may be a little past it.
fn live_notes(notes: Option<&Clip>, rect: Rect, viewport: &Viewport) -> Vec<Rect> {
    let Some(notes) = notes else {
        return Vec::new();
    };
    let right = rect.x + rect.width;
    let notes = viewport
        .miniature(notes, rect)
        .filter(|note| note.x < right);
    notes
        .map(|note| Rect {
            width: note.width.min(right - note.x),
            ..note
        })
        .collect()
}
