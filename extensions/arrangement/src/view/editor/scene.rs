//! The scene of one paint: the rows, notes, lane and marquee on screen, read from the
//! project, and the code that paints them.

use gpui::{
    App, BorderStyle, Bounds, ContentMask, FontWeight, Hsla, PathBuilder, Pixels, Point,
    SharedString, Window, fill, point, px, quad, size,
};
use std::rc::Rc;

use sound_notes::Note;
use sound_ui::ActiveTheme;

use super::NoteEditor;
use crate::TrackState;
use crate::view::lanes::Shown;
use crate::view::layout::{HEADER_WIDTH, NAME_LEFT, RULER_HEIGHT, Rect, RulerBar, Viewport};
use crate::view::paint::{Fit, accent, paint_ruler, paint_text, paint_track_label, placed};
use crate::view::roll::{KEYS_WIDTH, VELOCITY_HEIGHT, clamped, shown_notes, velocity_bar};
use crate::view::rows::Rows;

impl NoteEditor {
    /// Everything to paint into a panel of this size, read from the project now. `None` when
    /// the clip is gone: the view that holds the editor closes it after the same event.
    pub(super) fn scene(&self, bounds: Bounds<Pixels>, cx: &App) -> Option<RollScene> {
        let project = self.session.read(cx).project();
        let clip = project.state(&self.clip)?;
        let track = self.clip.id().parent();
        let track = track.and_then(|track| project.resolve::<TrackState>(&track));
        let track = track.and_then(|track| project.state(&track));
        let theme = cx.theme();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        let width = f32::from(bounds.size.width) - HEADER_WIDTH;
        let height = f32::from(bounds.size.height) - RULER_HEIGHT - VELOCITY_HEIGHT;
        let rows = self.rows_of(clip);
        let viewport = clamped(&self.viewport, &rows, clip, time_signatures, width, height);
        // Only what shows: a long clip has many notes and the editor shows a few bars of it.
        let ticks = viewport.visible_ticks(width);
        let shown_rows = rows.visible(&viewport, height);
        let in_time = |note: &&Note| {
            let start = clip.start + note.start;
            start < ticks.end && clip.start + note.end() > ticks.start
        };
        let selected = |note: &Note| self.selection.contains(note);
        let rows_shown = shown_rows.clone().filter_map(|index| {
            let look = rows.look(index)?;
            Some(SceneRow {
                y: rows.top_of(&viewport, index),
                shade: look.shade,
                extra: look.extra,
                label: look
                    .label
                    .map(|label| SharedString::from(label.into_owned())),
            })
        });
        Some(RollScene {
            viewport,
            width,
            height,
            row_height: rows.height(),
            rows_shown: rows_shown.collect(),
            track_name: track
                .map(|track| track.name.clone())
                .unwrap_or_default()
                .into(),
            accent: track.map_or(theme.blue, |track| accent(track.colour, theme)),
            bars: viewport.ruler_bars(time_signatures, width),
            beats: viewport.beat_lines(time_signatures, width),
            clip_start: viewport.x_of(clip.start).clamp(0.0, width),
            clip_end: viewport.x_of(clip.end()).clamp(0.0, width),
            notes: shown_notes(&viewport, &rows, clip, width, shown_rows)
                .map(|(note, rect)| (rect, selected(note)))
                .collect(),
            velocities: match self.shown {
                Shown::Velocity => clip
                    .notes
                    .iter()
                    .filter(in_time)
                    .map(|note| (velocity_bar(&viewport, clip, note), selected(note)))
                    .collect(),
                Shown::Lane(_) => Vec::new(),
            },
            line: match self.shown {
                Shown::Velocity => Vec::new(),
                Shown::Lane(lane) => lane.line(&viewport, clip, ticks.clone()),
            },
            middle: match self.shown {
                Shown::Velocity => None,
                Shown::Lane(lane) => lane.middle(),
            },
            marquee: self.marquee_rect(&viewport),
            rows: Rc::new(rows),
        })
    }
}

/// What one paint of the editor shows, in the coordinates of [`crate::view::roll`].
pub(super) struct RollScene {
    pub(super) viewport: Viewport,
    pub(super) width: f32,
    /// The height of the pitch rows, without the lane.
    height: f32,
    row_height: f32,
    /// The rows that show.
    rows_shown: Vec<SceneRow>,
    /// Every row, for the mouse.
    pub(super) rows: Rc<Rows>,
    track_name: SharedString,
    accent: Hsla,
    bars: Vec<RulerBar>,
    beats: Vec<f32>,
    /// The part of the note area that is inside the clip.
    clip_start: f32,
    clip_end: f32,
    /// The visible notes, and whether each is selected.
    notes: Vec<(Rect, bool)>,
    /// The bars of the lane, in its coordinates, and whether the note of each is selected.
    velocities: Vec<(Rect, bool)>,
    /// The line of the expression lane that shows, in its coordinates.
    line: Vec<(f32, f32)>,
    /// The height of the rest of that lane when it is not the bottom: the middle of the bend.
    middle: Option<f32>,
    /// The rectangle of a drag on empty space.
    marquee: Option<Rect>,
}

struct SceneRow {
    /// Its top.
    y: f32,
    shade: bool,
    extra: bool,
    label: Option<SharedString>,
}

pub(super) fn paint_roll(
    scene: &RollScene,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = cx.theme();
    let (hairline, beat_line, black_row, outside, white_key, black_key, label, selection) = (
        theme.alpha_at(0.05),
        theme.alpha_at(0.025),
        theme.alpha_at(0.02),
        theme.gray_50.opacity(0.5),
        theme.alpha_at(0.10),
        theme.alpha_at(0.03),
        theme.gray_700,
        theme.gray_950,
    );
    let (marquee_fill, marquee_border) = (theme.alpha_at(0.05), theme.alpha_at(0.20));
    let RollScene {
        width,
        height,
        row_height,
        ..
    } = *scene;
    let ruler = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(0.)),
        size(px(width), px(RULER_HEIGHT)),
    );
    let area = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(px(width), px(height)),
    );
    let lane = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT + height)),
        size(px(width), px(VELOCITY_HEIGHT)),
    );
    let keys = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH - KEYS_WIDTH), px(RULER_HEIGHT)),
        size(px(KEYS_WIDTH), px(height)),
    );
    let names = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT)),
        size(px(HEADER_WIDTH - KEYS_WIDTH), px(height)),
    );
    let row = |shown: &SceneRow, width: f32, height: f32| Rect {
        x: 0.0,
        y: shown.y,
        width,
        height,
    };
    let upright = |x: f32, top: Point<Pixels>, tall: f32| {
        Bounds::new(top + point(px(x.round()), px(0.)), size(px(1.), px(tall)))
    };
    // Notes live inside the clip. What is outside is a shade darker, in the rows and the lane.
    let veils = [(0.0, scene.clip_start), (scene.clip_end, width)];
    let paint_veils = |top: Point<Pixels>, tall: f32, window: &mut Window| {
        for (left, right) in veils {
            if right > left {
                let veil = Rect {
                    x: left,
                    y: 0.0,
                    width: right - left,
                    height: tall,
                };
                window.paint_quad(fill(placed(veil, top), outside));
            }
        }
    };

    paint_ruler(&scene.bars, ruler, window, cx);
    // The track of the clip, where the track headers are above.
    let name_width = HEADER_WIDTH - NAME_LEFT - 40.;
    let name = scene.track_name.clone();
    let top = bounds.origin;
    let label_size = (RULER_HEIGHT / 2., name_width);
    paint_track_label(name, scene.accent, top, label_size, false, window, cx);

    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
        // A row no row of the grid has is shaded as what is outside the clip.
        for shown in &scene.rows_shown {
            let color = match (shown.extra, shown.shade) {
                (true, _) => outside,
                (false, true) => black_row,
                (false, false) => continue,
            };
            let tint = placed(row(shown, width, row_height), area.origin);
            window.paint_quad(fill(tint, color));
        }
        for x in &scene.beats {
            window.paint_quad(fill(upright(*x, area.origin, height), beat_line));
        }
        for bar in &scene.bars {
            window.paint_quad(fill(upright(bar.x, area.origin, height), hairline));
        }
        paint_veils(area.origin, height, window);
        // A selected note is filled with the text colour, which is the lightest there is, and
        // keeps its accent as the outline. An outline alone on a pastel fill was hard to see.
        for (rect, selected) in &scene.notes {
            let body = placed(*rect, area.origin);
            let fill_color = if *selected { selection } else { scene.accent };
            let radius = px(3.).min(body.size.width / 2.);
            let solid = BorderStyle::Solid;
            window.paint_quad(quad(body, radius, fill_color, px(1.), scene.accent, solid));
        }
        if let Some(rect) = scene.marquee {
            let solid = BorderStyle::Solid;
            let body = placed(rect, area.origin);
            window.paint_quad(quad(
                body,
                px(2.),
                marquee_fill,
                px(1.),
                marquee_border,
                solid,
            ));
        }
    });

    // The lane: a bar at the start of each note in the track colour at 70 %, the selected ones
    // in the text colour and on top.
    window.with_content_mask(Some(ContentMask { bounds: lane }), |window| {
        for bar in &scene.bars {
            window.paint_quad(fill(upright(bar.x, lane.origin, VELOCITY_HEIGHT), hairline));
        }
        paint_veils(lane.origin, VELOCITY_HEIGHT, window);
        let bar_color = scene.accent.opacity(0.7);
        for selected in [false, true] {
            let bars = scene.velocities.iter().filter(|(_, is)| *is == selected);
            for (rect, _) in bars {
                let color = if selected { selection } else { bar_color };
                window.paint_quad(fill(placed(*rect, lane.origin), color));
            }
        }
        // An expression lane: its rest when that is the middle, and its line in the track
        // colour, as a note is.
        if let Some(middle) = scene.middle {
            let rest = Bounds::new(
                lane.origin + point(px(0.), px(middle.round())),
                size(px(width), px(1.)),
            );
            window.paint_quad(fill(rest, hairline));
        }
        let mut path = PathBuilder::stroke(px(1.5));
        for (index, (x, y)) in scene.line.iter().enumerate() {
            let at = lane.origin + point(px(*x), px(*y));
            match index {
                0 => path.move_to(at),
                _ => path.line_to(at),
            }
        }
        // A path that does not tessellate paints nothing, which is all there is to do about it.
        if scene.line.len() > 1
            && let Ok(path) = path.build()
        {
            window.paint_path(path, scene.accent);
        }
    });

    window.with_content_mask(Some(ContentMask { bounds: keys }), |window| {
        for shown in &scene.rows_shown {
            let color = if shown.shade { black_key } else { white_key };
            let key = row(shown, KEYS_WIDTH - 1.0, row_height - 1.0);
            window.paint_quad(fill(placed(key, keys.origin), color));
        }
    });
    // The names of the rows, left of the keys. A name may reach over the rows next to it.
    window.with_content_mask(Some(ContentMask { bounds: names }), |window| {
        for shown in &scene.rows_shown {
            let Some(name) = shown.label.clone() else {
                continue;
            };
            let middle = shown.y + row_height / 2.0;
            let origin =
                names.origin + point(names.size.width - px(8.), px((middle - 9.0).round()));
            let (weight, fit) = (FontWeight::NORMAL, Fit::AlignRight);
            paint_text(name, origin, 12., weight, label, fit, window, cx);
        }
    });

    // One hairline above the panel, one under the ruler, one above the lane, one beside the
    // keys.
    let lines = [
        Bounds::new(bounds.origin, size(bounds.size.width, px(1.))),
        Bounds::new(
            bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
            size(bounds.size.width, px(1.)),
        ),
        Bounds::new(
            bounds.origin + point(px(0.), px(RULER_HEIGHT + height)),
            size(bounds.size.width, px(1.)),
        ),
        Bounds::new(
            bounds.origin + point(px(HEADER_WIDTH - 1.), px(0.)),
            size(px(1.), bounds.size.height),
        ),
    ];
    for line in lines {
        window.paint_quad(fill(line, hairline));
    }
}
