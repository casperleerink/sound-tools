//! Where things are in the arrangement view. Pure math, no GPUI: painting, hit testing and
//! gestures all go through these functions, so they cannot disagree about a position.
//!
//! The coordinates are those of the timeline area: `x` 0 is its left edge, right of the
//! track headers, and `y` 0 is its top edge, below the ruler. Sizes follow the 8 px grid.

use std::ops::Range;

use sound_core::{TICKS_PER_QUARTER, Ticks, TimeSignature};
use sound_notes::Clip;

pub const HEADER_WIDTH: f32 = 176.0;
pub const RULER_HEIGHT: f32 = 32.0;
pub const TRACK_HEIGHT: f32 = 64.0;
/// Tick 0 sits this far into the timeline area, so the start of the piece, its bar number and
/// the playhead at rest are clear of the track headers.
pub const LEAD_IN: f32 = 8.0;
/// The gap between a clip and the edge of its track row.
pub const CLIP_INSET: f32 = 4.0;
/// Scroll room below the last track, so that the floating transport never hides it.
pub const BOTTOM_ROOM: f32 = 96.0;
/// Scroll room after the last clip.
pub const END_ROOM_BARS: u64 = 16;
/// Everything snaps to a sixteenth note. Fixed for the first milestone.
pub const SNAP: Ticks = Ticks(TICKS_PER_QUARTER / 4);

const MIN_PIXELS_PER_QUARTER: f64 = 1.0;
const MAX_PIXELS_PER_QUARTER: f64 = 384.0;
/// Bar numbers in the ruler are at least this far apart.
const MIN_LABEL_SPACING: f64 = 64.0;
/// Below this width a clip shows no notes.
const MIN_MINIATURE_WIDTH: f32 = 8.0;
/// A miniature shows at least this many semitones, so two close pitches do not fill the clip.
const MIN_MINIATURE_SEMITONES: f32 = 12.0;

/// The nearest multiple of [`SNAP`].
pub fn snap(tick: Ticks) -> Ticks {
    Ticks((tick.0 + SNAP.0 / 2) / SNAP.0 * SNAP.0)
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// How much there is to scroll over: the end of the last clip and the number of tracks.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Extent {
    pub end: Ticks,
    pub tracks: usize,
}

/// Zoom and scroll: interface state, never saved. Scroll is in pixels from tick 0 and from
/// the first track. They are `f64`, because a long piece zoomed in is millions of pixels wide.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Viewport {
    pub pixels_per_quarter: f64,
    pub scroll_x: f64,
    pub scroll_y: f64,
}

impl Default for Viewport {
    /// A bar of 4/4 is 96 px wide.
    fn default() -> Self {
        Self {
            pixels_per_quarter: 24.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }
}

impl Viewport {
    fn pixels_per_tick(&self) -> f64 {
        self.pixels_per_quarter / TICKS_PER_QUARTER as f64
    }

    pub fn x_of(&self, tick: Ticks) -> f32 {
        (tick.0 as f64 * self.pixels_per_tick() - self.scroll_x) as f32 + LEAD_IN
    }

    /// The tick at `x`, not snapped. Left of tick 0 is tick 0.
    pub fn tick_at(&self, x: f32) -> Ticks {
        let tick = (f64::from(x - LEAD_IN) + self.scroll_x) / self.pixels_per_tick();
        Ticks(tick.round().max(0.0) as u64)
    }

    /// The top of a track row.
    pub fn y_of(&self, track: usize) -> f32 {
        (track as f64 * f64::from(TRACK_HEIGHT) - self.scroll_y) as f32
    }

    /// The index of the track row at `y`. `None` above the first and below the last.
    pub fn track_at(&self, y: f32, tracks: usize) -> Option<usize> {
        let row = ((f64::from(y) + self.scroll_y) / f64::from(TRACK_HEIGHT)).floor();
        (row >= 0.0 && (row as usize) < tracks).then_some(row as usize)
    }

    /// The ticks a timeline area of this width shows. A clip is visible when it overlaps.
    pub fn visible_ticks(&self, width: f32) -> Range<Ticks> {
        let end = (f64::from(width - LEAD_IN) + self.scroll_x) / self.pixels_per_tick();
        self.tick_at(0.0)..Ticks(end.ceil().max(0.0) as u64)
    }

    /// The track rows a timeline area of this height shows, whole or in part.
    pub fn visible_tracks(&self, height: f32, tracks: usize) -> Range<usize> {
        let row_height = f64::from(TRACK_HEIGHT);
        let first = (self.scroll_y / row_height).floor().max(0.0) as usize;
        let last = ((self.scroll_y + f64::from(height)) / row_height)
            .ceil()
            .max(0.0) as usize;
        first.min(tracks)..last.min(tracks)
    }

    /// Zooms by `factor` and keeps the tick under `anchor_x` where it is.
    pub fn zoomed(&self, factor: f64, anchor_x: f32) -> Self {
        let pixels_per_quarter = (self.pixels_per_quarter * factor)
            .clamp(MIN_PIXELS_PER_QUARTER, MAX_PIXELS_PER_QUARTER);
        let anchor = f64::from(anchor_x - LEAD_IN);
        let scale = pixels_per_quarter / self.pixels_per_quarter;
        Self {
            pixels_per_quarter,
            scroll_x: (self.scroll_x + anchor) * scale - anchor,
            ..*self
        }
    }

    /// Moves the content by a scroll delta: a positive delta moves it right and down.
    pub fn scrolled(&self, delta_x: f32, delta_y: f32) -> Self {
        Self {
            scroll_x: self.scroll_x - f64::from(delta_x),
            scroll_y: self.scroll_y - f64::from(delta_y),
            ..*self
        }
    }

    /// Keeps the scroll inside the content for a timeline area of this size. The view paints
    /// with the clamped viewport, so a shrinking project or a growing window never shows a
    /// position that the next scroll would jump away from.
    pub fn clamped(
        &self,
        extent: Extent,
        time_signature: TimeSignature,
        width: f32,
        height: f32,
    ) -> Self {
        let end_room = END_ROOM_BARS * time_signature.ticks_per_bar();
        let content_width =
            (extent.end.0 + end_room) as f64 * self.pixels_per_tick() + f64::from(LEAD_IN);
        let content_height =
            extent.tracks as f64 * f64::from(TRACK_HEIGHT) + f64::from(BOTTOM_ROOM);
        Self {
            scroll_x: self
                .scroll_x
                .clamp(0.0, (content_width - f64::from(width)).max(0.0)),
            scroll_y: self
                .scroll_y
                .clamp(0.0, (content_height - f64::from(height)).max(0.0)),
            ..*self
        }
    }

    /// Where a clip is drawn on its track row. It may reach outside the timeline area.
    pub fn clip_rect(&self, track: usize, clip: &Clip) -> Rect {
        let x = self.x_of(clip.start);
        Rect {
            x,
            y: self.y_of(track) + CLIP_INSET,
            // At least a pixel, so a short clip far zoomed out is still there.
            width: (self.x_of(clip.end()) - x).max(1.0),
            height: TRACK_HEIGHT - 2.0 * CLIP_INSET,
        }
    }

    /// The bars that get a mark in the ruler, as `(bar number from 1, x)`. When bars get
    /// narrow only every 2nd, 4th, 8th and so on is marked, so the numbers never crowd. The
    /// first one is at or left of the left edge: its number scrolls out, it does not vanish.
    pub fn ruler_bars(
        &self,
        time_signature: TimeSignature,
        width: f32,
    ) -> impl Iterator<Item = (u64, f32)> + use<> {
        let ticks_per_bar = time_signature.ticks_per_bar();
        let pixels_per_bar = ticks_per_bar as f64 * self.pixels_per_tick();
        let step =
            ((MIN_LABEL_SPACING / pixels_per_bar).ceil().max(1.0) as u64).next_power_of_two();
        let visible = self.visible_ticks(width);
        let first = visible.start.0 / ticks_per_bar / step * step;
        let viewport = *self;
        (first..)
            .step_by(step as usize)
            .map(move |bar| (bar + 1, viewport.x_of(Ticks(bar * ticks_per_bar))))
            .take_while(move |(_, x)| *x < width)
    }

    /// The notes of a clip as small bars inside `rect`, the rect of [`Self::clip_rect`].
    /// Pitches spread over the height. Nothing for a clip too narrow to read.
    pub fn miniature<'a>(
        &self,
        clip: &'a Clip,
        rect: Rect,
    ) -> impl Iterator<Item = Rect> + use<'a> {
        let pitches = clip.notes.iter().map(|note| note.pitch.number());
        let lowest = pitches.clone().min().unwrap_or(0);
        let highest = pitches.max().unwrap_or(0);
        let range = f32::from(highest - lowest) + 1.0;
        let shown = range.max(MIN_MINIATURE_SEMITONES);
        let inner_height = rect.height - 4.0 * CLIP_INSET;
        let row_height = inner_height / shown;
        let note_height = row_height.clamp(2.0, 4.0);
        // The pitch range sits in the middle of the clip.
        let bottom = rect.y + rect.height - 2.0 * CLIP_INSET - (shown - range) / 2.0 * row_height;
        let viewport = *self;
        let readable = rect.width >= MIN_MINIATURE_WIDTH;
        clip.placed_notes()
            .filter(move |_| readable)
            .map(move |note| {
                let x = viewport.x_of(note.start);
                let above_lowest = f32::from(note.pitch.number() - lowest);
                Rect {
                    x,
                    y: bottom - (above_lowest + 0.5) * row_height - note_height / 2.0,
                    // A gap of a pixel between a note and the next one on the same pitch.
                    width: (viewport.x_of(note.end) - x - 1.0).max(1.0),
                    height: note_height,
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use sound_core::{Ticks, TimeSignature};
    use sound_notes::{Clip, Length, Note, Pitch, Velocity};

    use super::*;

    const BAR: u64 = 4 * TICKS_PER_QUARTER;

    fn four_four() -> TimeSignature {
        TimeSignature::new(4, 4).unwrap()
    }

    fn clip(start: u64, length: u64, notes: &[(u64, u64, u8)]) -> Clip {
        Clip {
            start: Ticks(start),
            length: Length::new(Ticks(length)).unwrap(),
            notes: notes
                .iter()
                .map(|&(start, length, pitch)| Note {
                    start: Ticks(start),
                    length: Length::new(Ticks(length)).unwrap(),
                    pitch: Pitch::new(pitch).unwrap(),
                    velocity: Velocity::new(100).unwrap(),
                })
                .collect(),
        }
    }

    #[test]
    fn ticks_and_pixels_convert_both_ways() {
        let viewport = Viewport {
            scroll_x: 96.0,
            ..Viewport::default()
        };
        // One bar is scrolled away, so bar 2 starts at the left edge.
        assert_eq!(viewport.x_of(Ticks(BAR)), LEAD_IN);
        assert_eq!(viewport.x_of(Ticks(2 * BAR)), LEAD_IN + 96.0);
        assert_eq!(viewport.x_of(Ticks(0)), LEAD_IN - 96.0);
        assert_eq!(viewport.tick_at(LEAD_IN + 96.0), Ticks(2 * BAR));
        assert_eq!(Viewport::default().x_of(Ticks(0)), LEAD_IN);
        assert_eq!(viewport.tick_at(-500.0), Ticks(0));
        for tick in [0, 1, 239, 240, 3840, 1_000_003] {
            let zoomed_in = Viewport {
                pixels_per_quarter: 384.0,
                ..viewport
            };
            assert_eq!(zoomed_in.tick_at(zoomed_in.x_of(Ticks(tick))), Ticks(tick));
        }
    }

    #[test]
    fn positions_stay_exact_far_into_a_long_piece() {
        // 400 bars in at full zoom is about 600,000 px from the start.
        let tick = Ticks(400 * BAR + 240);
        let viewport = Viewport {
            pixels_per_quarter: 384.0,
            scroll_x: 400.0 * 4.0 * 384.0,
            scroll_y: 0.0,
        };
        assert_eq!(viewport.x_of(tick), LEAD_IN + 96.0);
        assert_eq!(viewport.tick_at(LEAD_IN + 96.0), tick);
    }

    #[test]
    fn snap_goes_to_the_nearest_sixteenth() {
        assert_eq!(snap(Ticks(0)), Ticks(0));
        assert_eq!(snap(Ticks(119)), Ticks(0));
        assert_eq!(snap(Ticks(120)), Ticks(240));
        assert_eq!(snap(Ticks(359)), Ticks(240));
        assert_eq!(snap(Ticks(3840 + 130)), Ticks(3840 + 240));
    }

    #[test]
    fn tracks_and_rows_convert_both_ways() {
        let viewport = Viewport {
            scroll_y: 96.0,
            ..Viewport::default()
        };
        assert_eq!(viewport.y_of(0), -96.0);
        assert_eq!(viewport.y_of(2), 32.0);
        assert_eq!(viewport.track_at(0.0, 10), Some(1));
        assert_eq!(viewport.track_at(31.9, 10), Some(1));
        assert_eq!(viewport.track_at(32.0, 10), Some(2));
        assert_eq!(viewport.track_at(32.0, 2), None);
        assert_eq!(Viewport::default().track_at(-1.0, 10), None);
    }

    #[test]
    fn the_visible_range_is_what_overlaps_the_area() {
        let viewport = Viewport {
            scroll_x: 48.0,
            scroll_y: 96.0,
            ..Viewport::default()
        };
        let visible = viewport.visible_ticks(LEAD_IN + 96.0);
        assert_eq!(visible.end, Ticks(BAR + BAR / 2));
        // The lead-in shows the 8 px before it too.
        assert_eq!(visible.start, Ticks(BAR / 2 - BAR / 12));
        // Rows 1 and 2 in part, rows 3 and 4 whole: 96 px into 64 px rows, 200 px high.
        assert_eq!(viewport.visible_tracks(200.0, 100), 1..5);
        assert_eq!(viewport.visible_tracks(200.0, 3), 1..3);
        assert_eq!(viewport.visible_tracks(200.0, 0), 0..0);
        assert_eq!(Viewport::default().visible_tracks(64.0, 100), 0..1);
    }

    #[test]
    fn zoom_keeps_the_tick_under_the_pointer() {
        let viewport = Viewport {
            scroll_x: 500.0,
            ..Viewport::default()
        };
        let under_pointer = viewport.tick_at(300.0);
        let zoomed = viewport.zoomed(1.5, 300.0);
        assert_eq!(zoomed.pixels_per_quarter, 36.0);
        assert_eq!(zoomed.tick_at(300.0), under_pointer);

        // The limits hold, and hitting one does not move the content.
        let far_out = viewport.zoomed(0.000_1, 300.0);
        assert_eq!(far_out.pixels_per_quarter, 1.0);
        assert_eq!(far_out.zoomed(0.5, 300.0), far_out);
        assert_eq!(viewport.zoomed(1_000.0, 0.0).pixels_per_quarter, 384.0);
    }

    #[test]
    fn scroll_stays_inside_the_content() {
        let extent = Extent {
            end: Ticks(8 * BAR),
            tracks: 10,
        };
        let far = Viewport::default().scrolled(-100_000.0, -100_000.0);
        let clamped = far.clamped(extent, four_four(), 960.0, 320.0);
        // The lead-in, 8 bars and 16 bars of room are 2312 px. 10 rows and the bottom room
        // are 736 px.
        assert_eq!(clamped.scroll_x, 2312.0 - 960.0);
        assert_eq!(clamped.scroll_y, 736.0 - 320.0);

        let before_start = Viewport::default().scrolled(50.0, 50.0);
        assert_eq!(
            before_start.clamped(extent, four_four(), 960.0, 320.0),
            Viewport::default()
        );

        // Content smaller than the area does not scroll.
        let small = Extent {
            end: Ticks(0),
            tracks: 1,
        };
        assert_eq!(
            far.clamped(small, four_four(), 1600.0, 800.0),
            Viewport::default()
        );
    }

    #[test]
    fn a_clip_sits_inside_its_row() {
        let viewport = Viewport {
            scroll_y: 32.0,
            ..Viewport::default()
        };
        let rect = viewport.clip_rect(2, &clip(BAR, 2 * BAR, &[]));
        assert_eq!(
            rect,
            Rect {
                x: LEAD_IN + 96.0,
                y: 100.0,
                width: 192.0,
                height: 56.0
            }
        );
        assert!(rect.contains(LEAD_IN + 96.0, 100.0));
        assert!(!rect.contains(LEAD_IN + 288.0, 100.0));
        assert!(!rect.contains(LEAD_IN + 96.0, 99.0));

        let far_out = Viewport {
            pixels_per_quarter: 1.0,
            ..Viewport::default()
        };
        assert_eq!(far_out.clip_rect(0, &clip(0, 120, &[])).width, 1.0);
    }

    #[test]
    fn the_ruler_marks_bars_and_thins_them_out_when_narrow() {
        let bars: Vec<_> = Viewport::default().ruler_bars(four_four(), 300.0).collect();
        assert_eq!(bars, [(1, 8.0), (2, 104.0), (3, 200.0), (4, 296.0)]);

        let scrolled = Viewport {
            scroll_x: 100.0,
            ..Viewport::default()
        };
        let bars: Vec<_> = scrolled.ruler_bars(four_four(), 200.0).collect();
        assert_eq!(bars, [(1, -92.0), (2, 4.0), (3, 100.0), (4, 196.0)]);

        // 12 px bars: every 8th bar is 96 px apart, every 4th would be 48 px.
        let narrow = Viewport {
            pixels_per_quarter: 3.0,
            ..Viewport::default()
        };
        let bars: Vec<_> = narrow.ruler_bars(four_four(), 208.0).collect();
        assert_eq!(bars, [(1, 8.0), (9, 104.0), (17, 200.0)]);

        let waltz = TimeSignature::new(3, 4).unwrap();
        let bars: Vec<_> = Viewport::default().ruler_bars(waltz, 160.0).collect();
        assert_eq!(bars, [(1, 8.0), (2, 80.0), (3, 152.0)]);
    }

    #[test]
    fn a_miniature_places_notes_by_time_and_pitch() {
        let viewport = Viewport::default();
        let clip = clip(BAR, BAR, &[(0, 960, 60), (960, 960, 72), (2880, 9600, 60)]);
        let rect = viewport.clip_rect(0, &clip);
        let notes: Vec<_> = viewport.miniature(&clip, rect).collect();
        assert_eq!(notes.len(), 3);

        assert_eq!(notes[0].x, LEAD_IN + 96.0);
        assert_eq!(notes[0].width, 23.0);
        assert_eq!(notes[1].x, LEAD_IN + 120.0);
        // Higher is further up, and the same pitch is on the same line.
        assert!(notes[1].y < notes[0].y);
        assert_eq!(notes[2].y, notes[0].y);
        // A note that is longer than its clip ends with the clip.
        assert_eq!(notes[2].x + notes[2].width, rect.x + rect.width - 1.0);
        for note in &notes {
            assert!(note.y >= rect.y + 2.0 * CLIP_INSET - 0.01);
            assert!(note.y + note.height <= rect.y + rect.height - 2.0 * CLIP_INSET + 0.01);
        }

        // One pitch sits in the middle.
        let single = self::clip(0, BAR, &[(0, 960, 64)]);
        let rect = viewport.clip_rect(0, &single);
        let note = viewport.miniature(&single, rect).next().unwrap();
        assert_eq!(note.y + note.height / 2.0, rect.y + rect.height / 2.0);

        // Too narrow to read: no notes.
        let far_out = Viewport {
            pixels_per_quarter: 1.0,
            ..Viewport::default()
        };
        let rect = far_out.clip_rect(0, &clip);
        assert_eq!(far_out.miniature(&clip, rect).count(), 0);
    }
}
