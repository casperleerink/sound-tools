//! The scene of one paint: the rows, clips, lanes and ruler marks on screen, the hit tests on
//! them, and the code that paints them. Nothing here touches the `Timeline`.

use std::ops::Range;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, FontWeight, Hsla, PathBuilder, Pixels, Point,
    SharedString, TextAlign, TextRun, Window, fill, point, px, quad, size,
};
use sound_core::{Assets, InstanceId, Ticks};
use sound_media::{AudioAsset, TakeOverview};
use sound_ui::components::audio_clip::{
    AudioClipLook, ClipHandle, ClipHandles, Columns, paint_audio_clip,
};
use sound_ui::{ActiveTheme, Waveforms, typography};

use super::loop_bar::{LoopBar, paint_loop_bar};
use super::ruler::{
    MarkZones, RulerMark, SectionBand, label_left, paint_ghost, paint_section_bands,
    paint_section_names, paint_time_signature_marks,
};
use crate::AutomationLane;
use crate::view::gesture::{Zone, zone_at};
use crate::view::layout::{
    DOT_LEFT, HEADER_INSET, HEADER_WIDTH, LANE_HEIGHT, NAME_LEFT, NAME_MIDDLE, RULER_HEIGHT, Rect,
    Rows, RulerBar, TRACK_HEIGHT, Viewport,
};
use crate::view::paint::{Fit, paint_ruler, paint_text, paint_track_label, placed, text_width};

pub(super) struct TrackRow {
    pub(super) y: f32,
    pub(super) name: SharedString,
    pub(super) accent: Hsla,
    pub(super) selected: bool,
    /// A track that does not sound, muted or left out by a solo, has its name, dot, clips and
    /// lanes at 40 %.
    pub(super) silent: bool,
    /// The name is being edited: the field of the timeline shows it, not the paint.
    pub(super) renaming: bool,
    /// An armed audio track shows the level of its input in its header, and its name is
    /// shorter.
    pub(super) armed: bool,
    /// Its header is being dragged: the ring of a drag shows where it lands.
    pub(super) lifted: bool,
}

/// An automation lane under a track, as one paint shows it.
pub(super) struct LaneShape {
    /// The top of the lane in the timeline area.
    pub(super) y: f32,
    /// The device whose number the lane moves, `None` for the track's own volume and pan.
    pub(super) device: Option<SharedString>,
    /// The number in plain words: `Cutoff`.
    pub(super) number: SharedString,
    pub(super) accent: Hsla,
    pub(super) silent: bool,
    /// The line in the lane, from the left edge to the right one. Empty for a number the
    /// project does not know.
    pub(super) line: Vec<(f32, f32)>,
    /// While clips are dragged: where across each lands with the line it takes along, and the
    /// line that was there before, faded.
    pub(super) ghosts: Vec<(Range<f32>, Vec<(f32, f32)>)>,
    /// The dots of the points that show.
    pub(super) points: Vec<LanePoint>,
}

/// The dot of a point of an automation lane, in the lane.
pub(super) struct LanePoint {
    pub(super) x: f32,
    pub(super) y: f32,
    /// Under the pointer or selected: the dot is bigger, and a selected one has a ring.
    pub(super) hovered: bool,
    pub(super) selected: bool,
    /// Its value with its unit, beside the dot, while it is under the pointer or dragged.
    pub(super) readout: Option<SharedString>,
}

/// A point of an automation lane: the track, the number of the lane and the tick of the point.
/// What is selected and what is under the pointer. Interface state: not saved, no undo step.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PointKey {
    pub(super) track: InstanceId,
    pub(super) device: Option<String>,
    pub(super) parameter: String,
    pub(super) tick: Ticks,
}

impl PointKey {
    pub(super) fn of(track: &InstanceId, lane: &AutomationLane, tick: Ticks) -> Self {
        Self {
            track: track.clone(),
            device: lane.device.clone(),
            parameter: lane.parameter.clone(),
            tick,
        }
    }

    /// Whether it is a point of `lane` of `track`.
    pub(super) fn is_in(&self, track: &InstanceId, lane: &AutomationLane) -> bool {
        self.track == *track && self.device == lane.device && self.parameter == lane.parameter
    }
}

/// What a clip shows: the notes of a note clip, or the waveform of an audio clip.
pub(super) enum Body {
    Notes(Vec<Rect>),
    Audio(Box<AudioShape>),
}

/// Where the waveform of an audio shape comes from.
pub(super) enum Sound {
    /// The file of a clip, whose overview is asked for while painting.
    File(AudioAsset),
    /// A take while it records: what its file holds so far, once it is lined up.
    Take(Option<TakeOverview>),
}

/// An audio clip as one paint shows it. The waveform needs the overview of its file, which is
/// asked for while painting, so here are the times of the file each column of the clip covers.
pub(super) struct AudioShape {
    pub(super) sound: Sound,
    /// The first column on screen, and the time in the file at each column edge from there.
    pub(super) first: f32,
    pub(super) edges: Vec<f64>,
    pub(super) gain: f32,
    pub(super) fade_in: f32,
    pub(super) fade_out: f32,
    /// The part of the file past the edge that is dragged: its first column and column edges.
    pub(super) hidden: Option<(f32, Vec<f64>)>,
    /// The value of a fade or of the gain while it is dragged.
    pub(super) label: Option<(SharedString, ClipHandle)>,
    pub(super) missing: Option<SharedString>,
    /// The pointer is on it or it is selected: its handles show and can be pressed.
    pub(super) handles: bool,
}

/// A clip as it is on screen, in the coordinates of [`layout`].
pub struct ClipShape {
    pub id: InstanceId,
    pub rect: Rect,
    pub(super) body: Body,
    pub(super) accent: Hsla,
    pub(super) selected: bool,
    pub(super) silent: bool,
    /// It is dragged with automation that goes along, on a track whose lanes are folded away.
    pub(super) carries: bool,
}

impl ClipShape {
    /// The handle of an audio clip at a place, when its handles show.
    fn handle_at(&self, x: f32, y: f32) -> Option<ClipHandle> {
        let Body::Audio(audio) = &self.body else {
            return None;
        };
        let Rect {
            x: left,
            y: top,
            width,
            ..
        } = self.rect;
        let handles = ClipHandles::of(left, top, width, audio.fade_in, audio.fade_out)?;
        handles.at(x, y)
    }
}

/// A tempo change after tick 0, in the ruler. The one at tick 0 shows in the transport.
pub(super) struct TempoMark {
    pub(super) tick: Ticks,
    pub(super) x: f32,
    pub(super) text: SharedString,
    pub(super) selected: bool,
}

/// Where a drop of files would go: the ghost of each clip it makes, and whether it makes a
/// track under the last one.
pub(super) struct Ghosts {
    pub(super) clips: Vec<(Rect, SharedString)>,
    pub(super) new_track: Option<f32>,
}

/// What one paint shows: only the visible rows, clips and bars. Later clips are on top.
pub struct Scene {
    pub viewport: Viewport,
    pub clips: Vec<ClipShape>,
    pub(super) rows: Vec<TrackRow>,
    pub(super) bars: Vec<RulerBar>,
    pub(super) tempo: Vec<TempoMark>,
    pub(super) sections: Vec<SectionBand>,
    /// The bars on screen where a time signature change starts, each with whether it is
    /// selected.
    pub(super) time_signature_marks: Vec<(u64, bool)>,
    /// Where each mark is in the ruler, across: filled by the paint, which measures the
    /// labels, and hit by a press.
    pub(super) mark_zones: MarkZones,
    /// Where the row of the ruler menu under the pointer or the keys would add its mark.
    pub(super) ghost: Option<f32>,
    pub(super) loop_bar: LoopBar,
    /// The rectangle of a drag on empty space.
    pub(super) marquee: Option<Rect>,
    /// Where dropped files would go.
    pub(super) ghosts: Option<Ghosts>,
    /// The folder of the files the audio clips name, for their waveforms.
    pub(super) assets: Assets,
    /// How tall each track row is, with the lanes it shows.
    pub layout: Rows,
    /// The automation lanes that show.
    pub(super) lanes: Vec<LaneShape>,
    /// Where the hint of a drag of clips that takes automation along goes: under the clip under
    /// the pointer.
    pub(super) hint: Option<(f32, f32)>,
}

/// What a press on a clip took: its body, an edge, or one of the handles of an audio clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Grip {
    Zone(Zone),
    Handle(ClipHandle),
}

impl Scene {
    /// The clip on top at a position in the timeline area.
    pub fn clip_at(&self, x: f32, y: f32) -> Option<&ClipShape> {
        self.clips
            .iter()
            .rev()
            .find(|shape| shape.rect.contains(x, y))
    }

    /// The clip on top at a position, with the part of it that is there: a handle of an audio
    /// clip, its body or an edge.
    pub fn zone_at(&self, x: f32, y: f32) -> Option<(&ClipShape, Grip)> {
        let shape = self.clip_at(x, y)?;
        let grip = match shape.handle_at(x, y) {
            Some(handle) => Grip::Handle(handle),
            None => Grip::Zone(zone_at(shape.rect, x)),
        };
        Some((shape, grip))
    }

    /// The mark at `x` in the ruler, the one painted last when they overlap.
    pub(super) fn mark_at(&self, x: f32) -> Option<RulerMark> {
        let mut zones = self.mark_zones.iter().rev();
        zones
            .find(|(_, across)| across.contains(&x))
            .map(|(mark, _)| *mark)
    }
}

/// The hint near a drag of clips that takes automation along.
const AUTOMATION_HINT: &str = "Automation moves · shift to leave it";
/// Where the meter of the input of an armed track starts in its header, on the line of its
/// name: 45 pt, ending where the solo toggle under it ends.
pub(in crate::view) const ARMED_METER_LEFT: f32 = HEADER_WIDTH - HEADER_INSET - 4. - 45.;
/// The meter is the master meter of the transport, 45 x 8.
pub(in crate::view) const ARMED_METER_HEIGHT: f32 = 8.;
/// What the header of the track a drop would make says.
const NEW_AUDIO_TRACK: &str = "New audio track";

/// Paints the takes of [`Timeline::take_shapes`] over a timeline painted at `bounds`, inside
/// its area right of the headers and under the ruler.
pub(in crate::view) fn paint_takes(
    shapes: &[ClipShape],
    bounds: Bounds<Pixels>,
    assets: &Assets,
    window: &mut Window,
    cx: &mut App,
) {
    let timeline = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(
            bounds.size.width - px(HEADER_WIDTH),
            bounds.size.height - px(RULER_HEIGHT),
        ),
    );
    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for shape in shapes {
            let body = placed(shape.rect, timeline.origin);
            match &shape.body {
                Body::Audio(audio) => {
                    let look = audio_look(shape, audio, body, timeline.origin, assets, cx);
                    paint_audio_clip(&look, window, cx);
                }
                Body::Notes(notes) => {
                    paint_live_notes(shape, notes, body, timeline.origin, window, cx);
                }
            }
        }
    });
}

/// A MIDI take while it records: opaque and with a red border, as an audio take is, and the
/// notes played so far.
fn paint_live_notes(
    shape: &ClipShape,
    notes: &[Rect],
    body: Bounds<Pixels>,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &App,
) {
    let theme = cx.theme();
    let dim = if shape.silent { 0.4 } else { 1. };
    let radius = px(6.).min(body.size.width / 2.);
    let (solid, clear) = (BorderStyle::Solid, Hsla::transparent_black());
    let (window_fill, clip_fill, border) = (
        theme.gray_100,
        theme.alpha_at(0.05).opacity(dim),
        theme.red.opacity(dim),
    );
    window.paint_quad(quad(body, radius, window_fill, px(0.), clear, solid));
    window.paint_quad(quad(body, radius, clip_fill, px(1.), border, solid));
    for note in notes {
        window.paint_quad(fill(placed(*note, origin), shape.accent.opacity(dim)));
    }
}

pub(super) fn paint_scene(
    scene: &mut Scene,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = cx.theme();
    let (hairline, clip_fill, clip_border) = (
        theme.alpha_at(0.05),
        theme.alpha_at(0.05),
        theme.alpha_at(0.10),
    );
    let (selection, selected_header, lane_text) =
        (theme.gray_950, theme.alpha_at(0.05), theme.gray_800);
    let (marquee_fill, marquee_border) = (theme.alpha_at(0.05), theme.alpha_at(0.20));
    let (drop_ring, ghost_text, muted_ring, muted_text, window_fill) = (
        theme.lavender,
        theme.gray_950,
        theme.gray_800,
        theme.gray_700,
        theme.gray_100,
    );
    let headers = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT)),
        size(px(HEADER_WIDTH), bounds.size.height - px(RULER_HEIGHT)),
    );
    let ruler = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(0.)),
        size(bounds.size.width - px(HEADER_WIDTH), px(RULER_HEIGHT)),
    );
    let timeline = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(ruler.size.width, headers.size.height),
    );

    // The ruler: a short mark and a number per bar, the sections, the loop and the tempo
    // changes. No grid below it.
    let bars = &scene.bars;
    let signatures = &scene.time_signature_marks;
    let mut zones = paint_time_signature_marks(signatures, bars, ruler, window, cx);
    paint_section_bands(&scene.sections, ruler, window, cx);
    paint_ruler(bars, ruler, window, cx);
    paint_loop_bar(&scene.loop_bar, ruler, window, cx);
    let sections = paint_section_names(&scene.sections, bars, ruler, window, cx);
    let tempo = paint_tempo_marks(&scene.tempo, bars, &sections, ruler, window, cx);
    zones.extend(sections);
    zones.extend(tempo);
    scene.mark_zones = zones;

    window.with_content_mask(Some(ContentMask { bounds: headers }), |window| {
        for row in &scene.rows {
            let top = headers.origin + point(px(0.), px(row.y.round()));
            if row.selected {
                // The shape of a clip, in the same place of the row. No accent: it is a fill.
                let inside = Bounds::new(
                    top + point(px(HEADER_INSET), px(4.)),
                    size(px(HEADER_WIDTH - 2. * HEADER_INSET), px(TRACK_HEIGHT - 8.)),
                );
                window.paint_quad(quad(
                    inside,
                    px(6.),
                    selected_header,
                    px(0.),
                    selected_header,
                    BorderStyle::Solid,
                ));
            }
            // Where a dragged track lands: the ring of a drag, on the shape of a selected header.
            if row.lifted {
                let inside = Bounds::new(
                    top + point(px(HEADER_INSET), px(4.)),
                    size(px(HEADER_WIDTH - 2. * HEADER_INSET), px(TRACK_HEIGHT - 8.)),
                );
                let clear = Hsla::transparent_black();
                let solid = BorderStyle::Solid;
                window.paint_quad(quad(inside, px(6.), clear, px(2.), drop_ring, solid));
            }
            // The name of an armed track ends 8 pt before the meter of its input.
            let name_width = match row.armed {
                false => HEADER_WIDTH - NAME_LEFT - 2. * HEADER_INSET,
                true => ARMED_METER_LEFT - 8. - NAME_LEFT,
            };
            // The field over the header shows the name that is being edited.
            let name = match row.renaming {
                true => SharedString::default(),
                false => row.name.clone(),
            };
            let label_size = (NAME_MIDDLE, name_width);
            paint_track_label(name, row.accent, top, label_size, row.silent, window, cx);
        }
        // The name of each lane, where the name of its track starts: `Filter · Cutoff`. A long
        // device name, as a plugin's often is, gives way to the number, which always shows.
        for lane in &scene.lanes {
            let top = headers.origin + point(px(0.), px(lane.y.round()));
            let origin = top + point(px(NAME_LEFT), px(LANE_HEIGHT / 2. - 9.));
            let room = HEADER_WIDTH - NAME_LEFT - 16.;
            let color = lane_text.opacity(if lane.silent { 0.4 } else { 1. });
            let weight = FontWeight::NORMAL;
            let number = match lane.device {
                Some(_) => SharedString::from(format!(" · {}", lane.number)),
                None => lane.number.clone(),
            };
            let mut left = 0.;
            if let Some(device) = &lane.device {
                let fit = Fit::Truncate((room - text_width(&number, 12., weight, window)).max(0.));
                left = paint_text(device.clone(), origin, 12., weight, color, fit, window, cx);
            }
            let origin = origin + point(px(left), px(0.));
            let fit = Fit::Truncate(room - left);
            paint_text(number, origin, 12., weight, color, fit, window, cx);
        }
        // A drop under the last track makes a new audio track, whose header says so.
        if let Some(y) = scene.ghosts.as_ref().and_then(|ghosts| ghosts.new_track) {
            let top = headers.origin + point(px(0.), px(y.round()));
            let ring = Bounds::new(
                top + point(px(DOT_LEFT), px(TRACK_HEIGHT / 2. - 4.)),
                size(px(8.), px(8.)),
            );
            let clear = Hsla::transparent_black();
            window.paint_quad(quad(
                ring,
                px(4.),
                clear,
                px(1.5),
                muted_ring,
                BorderStyle::Solid,
            ));
            let origin = top + point(px(NAME_LEFT), px(TRACK_HEIGHT / 2. - 10.));
            let fit = Fit::Truncate(HEADER_WIDTH - NAME_LEFT - 16.);
            let text = SharedString::from(NEW_AUDIO_TRACK);
            paint_text(
                text,
                origin,
                14.,
                FontWeight::MEDIUM,
                muted_text,
                fit,
                window,
                cx,
            );
        }
    });

    // One hairline under the ruler and one between the headers and the timeline.
    let under_ruler = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
        size(bounds.size.width, px(1.)),
    );
    let beside_headers = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH - 1.), px(0.)),
        size(px(1.), bounds.size.height),
    );
    window.paint_quad(fill(under_ruler, hairline));
    window.paint_quad(fill(beside_headers, hairline));
    // A hairline over each lane, across the header and the timeline.
    let under_ruler_area =
        Bounds::new(headers.origin, size(bounds.size.width, headers.size.height));
    window.with_content_mask(
        Some(ContentMask {
            bounds: under_ruler_area,
        }),
        |window| {
            for lane in &scene.lanes {
                let line = Bounds::new(
                    headers.origin + point(px(0.), px(lane.y.round())),
                    size(bounds.size.width, px(1.)),
                );
                window.paint_quad(fill(line, hairline));
            }
        },
    );

    let assets = scene.assets.clone();
    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for lane in &scene.lanes {
            paint_lane(lane, timeline, window, cx);
        }
        for shape in &scene.clips {
            let body = placed(shape.rect, timeline.origin);
            let dim = if shape.silent { 0.4 } else { 1. };
            match &shape.body {
                Body::Notes(notes) => {
                    let border = if shape.selected {
                        selection.opacity(dim)
                    } else {
                        clip_border.opacity(dim)
                    };
                    let radius = px(6.).min(body.size.width / 2.);
                    let solid = BorderStyle::Solid;
                    let clip_fill = clip_fill.opacity(dim);
                    window.paint_quad(quad(body, radius, clip_fill, px(1.), border, solid));
                    for note in notes {
                        let note = placed(*note, timeline.origin);
                        window.paint_quad(fill(note, shape.accent.opacity(dim)));
                    }
                }
                Body::Audio(audio) => {
                    let look = audio_look(shape, audio, body, timeline.origin, &assets, cx);
                    paint_audio_clip(&look, window, cx);
                }
            }
            if shape.carries {
                paint_automation_mark(body, shape.accent.opacity(dim), window);
            }
        }

        if let Some(ghosts) = &scene.ghosts {
            for (rect, name) in &ghosts.clips {
                let area = placed(*rect, timeline.origin);
                let solid = BorderStyle::Solid;
                // Opaque: the clip it makes covers what it lies over, the newest on top.
                let clear = Hsla::transparent_black();
                window.paint_quad(quad(area, px(6.), window_fill, px(0.), clear, solid));
                window.paint_quad(quad(area, px(6.), clip_fill, px(2.), drop_ring, solid));
                let origin = area.origin + point(px(12.), px(8.));
                let fit = Fit::Truncate((f32::from(area.size.width) - 24.).max(0.));
                let weight = FontWeight::MEDIUM;
                paint_text(
                    name.clone(),
                    origin,
                    14.,
                    weight,
                    ghost_text,
                    fit,
                    window,
                    cx,
                );
            }
        }
        if let Some(marquee) = scene.marquee {
            let area = placed(marquee, timeline.origin);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(
                area,
                px(2.),
                marquee_fill,
                px(1.),
                marquee_border,
                solid,
            ));
        }
        if let Some((x, y)) = scene.hint {
            let origin = timeline.origin + point(px(x.max(4.).round()), px(y.round()));
            paint_hint(AUTOMATION_HINT, origin, window, cx);
        }
        // Over the clips and the lanes under it, the value of the point under the pointer.
        let readouts = scene.lanes.iter().flat_map(|lane| {
            let dots = lane.points.iter();
            dots.filter_map(move |dot| Some((lane.y, dot, dot.readout.clone()?)))
        });
        for (top, dot, readout) in readouts {
            let at = point(px((dot.x + 10.).round()), px((top + dot.y - 12.).round()));
            paint_hint(readout, timeline.origin + at, window, cx);
        }
    });
    if let Some(x) = scene.ghost {
        paint_ghost(x, (ruler, timeline), window, cx);
    }
}

/// A line through `corners`, from `origin`.
fn paint_polyline(
    corners: &[(f32, f32)],
    origin: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(px(width));
    for (index, (x, y)) in corners.iter().enumerate() {
        let at = origin + point(px(*x), px(*y));
        match index {
            0 => path.move_to(at),
            _ => path.line_to(at),
        }
    }
    // A path that does not tessellate paints nothing, which is all there is to do about it.
    if corners.len() > 1
        && let Ok(path) = path.build()
    {
        window.paint_path(path, color);
    }
}

/// An automation lane in the timeline area: its line in the track colour, as a note is. While
/// clips are dragged, where each lands has a light band, and the line that was there before
/// shows faded under the one it gets.
fn paint_lane(lane: &LaneShape, timeline: Bounds<Pixels>, window: &mut Window, cx: &App) {
    let selection = cx.theme().gray_950;
    let origin = timeline.origin + point(px(0.), px(lane.y));
    // An area takes no track colour: the band is the fill of a marquee.
    let band_fill = cx.theme().alpha_at(0.05);
    let opacity = if lane.silent { 0.4 } else { 1. };
    for (across, replaced) in &lane.ghosts {
        let band = Bounds::new(
            origin + point(px(across.start.round()), px(1.)),
            size(
                px((across.end - across.start).round().max(1.)),
                px(LANE_HEIGHT - 1.),
            ),
        );
        window.paint_quad(fill(band, band_fill));
        let faded = lane.accent.opacity(0.3 * opacity);
        window.with_content_mask(Some(ContentMask { bounds: band }), |window| {
            paint_polyline(replaced, origin, 1.5, faded, window);
        });
    }
    paint_polyline(
        &lane.line,
        origin,
        1.5,
        lane.accent.opacity(opacity),
        window,
    );
    // A dot on each point, bigger under the pointer, and a selected one with the ring of a
    // selected clip.
    let solid = BorderStyle::Solid;
    for dot in &lane.points {
        let radius: f32 = match dot.hovered || dot.selected {
            true => 4.5,
            false => 3.,
        };
        let bounds = Bounds::new(
            origin + point(px(dot.x - radius), px(dot.y - radius)),
            size(px(2. * radius), px(2. * radius)),
        );
        let (ring, edge) = match dot.selected {
            true => (px(1.5), selection),
            false => (px(0.), Hsla::transparent_black()),
        };
        let color = lane.accent.opacity(opacity);
        window.paint_quad(quad(bounds, px(radius), color, ring, edge, solid));
    }
}

/// The mark of a dragged clip whose automation goes along while its lanes are folded away: a
/// small fade in its top right corner.
fn paint_automation_mark(body: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    if body.size.width < px(40.) {
        return;
    }
    let origin = body.top_right();
    let corners = [(-28., 16.), (-20., 16.), (-10., 8.)];
    paint_polyline(&corners, origin, 1.5, color, window);
}

/// A short line of text in a box of the window colour, as a tempo label is, at `origin`.
fn paint_hint(
    text: impl Into<SharedString>,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = cx.theme();
    let (background, border, color) = (theme.gray_100, theme.alpha_at(0.10), theme.gray_900);
    let text = text.into();
    let run = TextRun {
        len: text.len(),
        font: typography::tabular(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(text, px(12.), &[run], None);
    let area = Bounds::new(origin, size(shaped.width + px(16.), px(24.)));
    let solid = BorderStyle::Solid;
    window.paint_quad(quad(area, px(6.), background, px(1.), border, solid));
    let at = origin + point(px(8.), px(4.));
    // A glyph that cannot be painted leaves a gap in a hint. Nothing else depends on it.
    if let Err(error) = shaped.paint(at, px(17.), TextAlign::Left, None, window, cx) {
        eprintln!("arrangement view: {error}");
    }
}

/// What an audio clip shows, with the peaks of its columns from the overview of its file. The
/// overview is asked for here, where there is an `App` to start one with, and is not there
/// until it is made on a background thread: the clip draws without its waveform until then.
fn audio_look(
    shape: &ClipShape,
    audio: &AudioShape,
    body: Bounds<Pixels>,
    origin: Point<Pixels>,
    assets: &Assets,
    cx: &mut App,
) -> AudioClipLook {
    let mut look = AudioClipLook::new(body, shape.accent);
    look.selected = shape.selected;
    look.muted = shape.silent;
    look.missing = audio.missing.clone();
    look.label = audio.label.clone();
    look.handles = audio.handles;
    look.gain = audio.gain;
    look.fade_in = audio.fade_in;
    look.fade_out = audio.fade_out;
    if audio.missing.is_some() {
        return look;
    }
    let overview = match &audio.sound {
        Sound::File(asset) => match Waveforms::overview(assets, asset, cx) {
            Some(overview) => Overview::File(overview),
            None => return look,
        },
        Sound::Take(take) => {
            look.recording = true;
            match take {
                Some(take) => Overview::Take(take.clone()),
                None => return look,
            }
        }
    };
    // Every column of one draw at one resolution, from one frame of the file to the next, so
    // together they cover every frame and no click falls between two of them.
    let columns = |first: f32, edges: &[f64]| {
        let peaks = |overview: &sound_media::Overview| {
            let rate = f64::from(overview.sample_rate());
            let frames: Vec<u64> = edges
                .iter()
                .map(|seconds| (seconds * rate).max(0.) as u64)
                .collect();
            overview.peaks(&frames)
        };
        Columns {
            left: origin.x + px(first),
            peaks: match &overview {
                Overview::File(overview) => peaks(overview),
                Overview::Take(take) => take.read(peaks),
            },
        }
    };
    look.waveform = columns(audio.first, &audio.edges);
    look.hidden = audio
        .hidden
        .as_ref()
        .map(|(first, edges)| columns(*first, edges));
    look
}

/// The overview a waveform is drawn from.
enum Overview {
    File(std::sync::Arc<sound_media::Overview>),
    Take(TakeOverview),
}

/// The tempo changes after tick 0 in the ruler: a line at the tick and a label, `140 bpm`, in
/// a box of the window colour that covers the bar numbers under it. On a bar line it starts
/// after the number of the bar, and after the name of a section there. The selected one has a
/// light border, as a selected clip. Gives where each label is across, for the hit test of a
/// press.
fn paint_tempo_marks(
    marks: &[TempoMark],
    bars: &[RulerBar],
    taken: &MarkZones,
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> MarkZones {
    let theme = cx.theme();
    let (line, background, border, selected_fill, selected_border, value, unit) = (
        theme.gray_700,
        theme.gray_100,
        theme.alpha_at(0.10),
        theme.alpha_at(0.10),
        theme.gray_950,
        theme.gray_950,
        theme.gray_700,
    );
    let font = typography::tabular();
    let font_size = px(12.);
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut zones = Vec::new();
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        for mark in marks {
            let x = mark.x.round();
            let text: SharedString = format!("{} bpm", mark.text).into();
            let runs = [
                run(mark.text.len(), value),
                run(text.len() - mark.text.len(), unit),
            ];
            let shaped = window
                .text_system()
                .shape_line(text, font_size, &runs, None);
            let left = label_left(x, bars, taken, window);
            let width = f32::from(shaped.width) + 12.;
            let tick_line = Bounds::new(
                ruler.origin + point(px(x), px(0.)),
                size(px(1.), px(RULER_HEIGHT)),
            );
            window.paint_quad(fill(tick_line, line));
            let label = Bounds::new(
                ruler.origin + point(px(left), px(6.)),
                size(px(width), px(20.)),
            );
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(label, px(6.), background, px(1.), border, solid));
            if mark.selected {
                let (fill, edge) = (selected_fill, selected_border);
                window.paint_quad(quad(label, px(6.), fill, px(1.), edge, solid));
            }
            let origin = ruler.origin + point(px(left + 6.), px(8.));
            // A glyph that cannot be painted leaves a gap in a label. Nothing else depends on it.
            if let Err(error) = shaped.paint(origin, px(17.), TextAlign::Left, None, window, cx) {
                eprintln!("arrangement view: {error}");
            }
            // Its line and its label, not what lies between: the number and time signature of
            // a bar there, and the name of a section, take their own presses.
            zones.push((RulerMark::Tempo(mark.tick), x - 4.0..x + 4.));
            zones.push((RulerMark::Tempo(mark.tick), left..left + width));
        }
    });
    zones
}
