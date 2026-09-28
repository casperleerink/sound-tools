//! Where things are in the arrangement view. Pure math, no GPUI: painting, hit testing and
//! gestures all go through these functions, so they cannot disagree about a position.
//!
//! The coordinates are those of the timeline area: `x` 0 is its left edge, right of the
//! track headers, and `y` 0 is its top edge, below the ruler. Sizes follow the 8 px grid.

use std::ops::Range;

use sound_core::{Bar, TICKS_PER_QUARTER, Ticks, TimeSignature, TimeSignatures};
use sound_notes::Clip;

pub const HEADER_WIDTH: f32 = 176.0;
pub const RULER_HEIGHT: f32 = 32.0;
pub const TRACK_HEIGHT: f32 = 64.0;
/// The row under the last track that holds the add track button. The scroll reaches it.
pub const ADD_ROW_HEIGHT: f32 = 40.0;
/// Tick 0 sits this far into the timeline area, so the start of the piece, its bar number and
/// the playhead at rest are clear of the track headers.
pub const LEAD_IN: f32 = 8.0;
/// The gap between a clip and the edge of its track row.
pub const CLIP_INSET: f32 = 4.0;
/// Scroll room after the last clip.
pub const END_ROOM_BARS: u64 = 16;
const MIN_PIXELS_PER_QUARTER: f64 = 1.0;
const MAX_PIXELS_PER_QUARTER: f64 = 384.0;
/// Bar numbers in the ruler are at least this far apart.
const MIN_LABEL_SPACING: f64 = 64.0;
/// Beat lines show only when beats are at least this far apart.
const MIN_BEAT_SPACING: f64 = 24.0;
/// Below this width a clip shows no notes.
const MIN_MINIATURE_WIDTH: f32 = 8.0;
/// A miniature shows at least this many semitones, so two close pitches do not fill the clip.
const MIN_MINIATURE_SEMITONES: f32 = 12.0;

/// A tick moved by a signed delta. It stops at tick 0.
pub fn shifted(tick: Ticks, delta: i64) -> Ticks {
    Ticks(tick.0.saturating_add_signed(delta))
}

/// The track rows between two heights from the top of the first track, both rows included:
/// what a rectangle drawn over the tracks touches. Only rows that exist.
pub fn rows_between(a: f64, b: f64, tracks: usize) -> Range<usize> {
    let row = |y: f64| (y / f64::from(TRACK_HEIGHT)).floor().max(0.0) as usize;
    let (top, bottom) = if a <= b { (a, b) } else { (b, a) };
    row(top).min(tracks)..(row(bottom) + 1).min(tracks)
}

/// A bar with a mark and a number in the ruler, see [`Viewport::ruler_bars`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RulerBar {
    pub number: u64,
    pub x: f32,
    /// The time signature, on a bar where one starts.
    pub signature: Option<TimeSignature>,
}

impl RulerBar {
    /// The number, and the time signature where one starts: `9  7/8`.
    pub fn label(&self) -> String {
        match self.signature {
            Some(signature) => format!("{}  {signature}", self.number),
            None => self.number.to_string(),
        }
    }
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

    /// The row of a drag: above the first track is the first, below the last is the last.
    /// `None` without tracks.
    pub fn nearest_track(&self, y: f32, tracks: usize) -> Option<usize> {
        let row = ((f64::from(y) + self.scroll_y) / f64::from(TRACK_HEIGHT)).floor();
        let last = tracks.checked_sub(1)?;
        Some((row.max(0.0) as usize).min(last))
    }

    /// The ticks a timeline area of this width shows. A clip is visible when it overlaps.
    pub fn visible_ticks(&self, width: f32) -> Range<Ticks> {
        let end = (f64::from(width - LEAD_IN) + self.scroll_x) / self.pixels_per_tick();
        self.tick_at(0.0)..Ticks(end.ceil().max(0.0) as u64)
    }

    /// Whether a timeline area of this width shows `tick`. The playhead line uses the same
    /// bounds, so this is exactly "the playhead is on screen".
    pub fn shows(&self, tick: Ticks, width: f32) -> bool {
        let x = self.x_of(tick);
        x >= 0.0 && x < width
    }

    /// The viewport that brings `tick` into a timeline area of this width, with the playhead
    /// at the left edge, so the view pages forward. Unchanged when the tick is already shown.
    ///
    /// The whole rule for following the playhead is this plus [`Self::shows`]: while playing,
    /// page forward once the playhead has passed the right edge; on a jump, bring it back in.
    pub fn following(&self, tick: Ticks, width: f32) -> Self {
        if self.shows(tick, width) {
            return *self;
        }
        Self {
            scroll_x: tick.0 as f64 * self.pixels_per_tick(),
            ..*self
        }
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
    /// position that the next scroll would jump away from. The content is the tracks and the
    /// row of the add track button under them.
    pub fn clamped(
        &self,
        extent: Extent,
        time_signatures: &TimeSignatures,
        width: f32,
        height: f32,
    ) -> Self {
        let rows = extent.tracks as f64 * f64::from(TRACK_HEIGHT);
        let content_height = rows + f64::from(ADD_ROW_HEIGHT);
        self.clamped_to(extent.end, content_height, time_signatures, width, height)
    }

    /// The same for any content below the ruler: `end` with the room after it across, and
    /// `content_height` down. Nothing floats over the content, so it needs no room below.
    pub fn clamped_to(
        &self,
        end: Ticks,
        content_height: f64,
        time_signatures: &TimeSignatures,
        width: f32,
        height: f32,
    ) -> Self {
        let end_room = END_ROOM_BARS * time_signatures.bar_at(end).length().0;
        let content_width = (end.0 + end_room) as f64 * self.pixels_per_tick() + f64::from(LEAD_IN);
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

    /// Where a clip from `start` to `end` is drawn on its track row. It may reach outside the
    /// timeline area.
    pub fn clip_rect(&self, track: usize, start: Ticks, end: Ticks) -> Rect {
        let x = self.x_of(start);
        Rect {
            x,
            y: self.y_of(track) + CLIP_INSET,
            // At least a pixel, so a short clip far zoomed out is still there.
            width: (self.x_of(end) - x).max(1.0),
            height: TRACK_HEIGHT - 2.0 * CLIP_INSET,
        }
    }

    /// The bars that get a mark in the ruler. When bars get narrow only every 2nd, 4th, 8th and
    /// so on of a time signature is marked, counted from where it starts, and a mark never
    /// comes closer than [`MIN_LABEL_SPACING`] to the one before it, so the numbers never crowd.
    /// A mark shows the time signature of its bar when one started since the mark before.
    ///
    /// The marks are counted from bar 1, so they stay put while the view scrolls. The first one
    /// is at or left of the left edge: its number scrolls out, it does not vanish.
    pub fn ruler_bars(&self, time_signatures: &TimeSignatures, width: f32) -> Vec<RulerBar> {
        let left = self.x_of(self.visible_ticks(width).start);
        let runs: Vec<Bar> = time_signatures.changes().collect();
        let mut marks: Vec<RulerBar> = Vec::new();
        let mut last: Option<(u64, f32)> = None;
        for (index, first) in runs.iter().enumerate() {
            let end = runs.get(index + 1).map_or(u64::MAX, |next| next.number);
            let pixels_per_bar = first.length().0 as f64 * self.pixels_per_tick();
            let step =
                ((MIN_LABEL_SPACING / pixels_per_bar).ceil().max(1.0) as u64).next_power_of_two();
            let numbers = (first.number..end).step_by(step as usize);
            for bar in numbers.map_while(|number| time_signatures.bar(number)) {
                let x = self.x_of(bar.start);
                if x >= width {
                    return marks;
                }
                let room =
                    last.is_none_or(|(_, last_x)| f64::from(x - last_x) >= MIN_LABEL_SPACING);
                if !room {
                    continue;
                }
                let changed = last.is_none_or(|(number, _)| number < first.number);
                // Marks left of the one at the left edge are never seen.
                if x <= left {
                    marks.clear();
                }
                marks.push(RulerBar {
                    number: bar.number,
                    x,
                    signature: changed.then_some(bar.signature),
                });
                last = Some((bar.number, x));
            }
        }
        marks
    }

    /// The x of every beat that is not a bar line, for very faint lines in the note editor.
    /// Only in bars whose beats are wide enough to help.
    pub fn beat_lines(&self, time_signatures: &TimeSignatures, width: f32) -> Vec<f32> {
        let visible = self.visible_ticks(width);
        let mut lines = Vec::new();
        let bars = time_signatures.bars_from(visible.start);
        for bar in bars.take_while(|bar| bar.start < visible.end) {
            let beat = bar.signature.ticks_per_beat();
            if (beat as f64 * self.pixels_per_tick()) < MIN_BEAT_SPACING {
                continue;
            }
            let beats = (1..u64::from(bar.signature.numerator()))
                .map(|index| bar.start + Ticks(index * beat))
                .filter(|tick| visible.contains(tick));
            lines.extend(beats.map(|tick| self.x_of(tick)));
        }
        lines
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
    use std::num::NonZeroU32;

    use sound_core::{SignatureRun, Ticks, TimeSignature, TimeSignatures};

    fn time_signatures(runs: &[(&str, u32)]) -> TimeSignatures {
        let runs = runs
            .iter()
            .map(|(signature, bars)| SignatureRun {
                signature: signature.parse().unwrap(),
                bars: NonZeroU32::new(*bars).unwrap(),
            })
            .collect();
        TimeSignatures::new(runs).unwrap()
    }
    use sound_notes::{Clip, Length, Note, Pitch, Velocity};

    use super::*;

    const BAR: u64 = 4 * TICKS_PER_QUARTER;

    fn four_four() -> &'static TimeSignatures {
        static FOUR_FOUR: std::sync::LazyLock<TimeSignatures> =
            std::sync::LazyLock::new(TimeSignatures::default);
        &FOUR_FOUR
    }

    fn clip(start: u64, length: u64, notes: &[(u64, u64, u8)]) -> Clip {
        Clip::new(
            Ticks(start),
            Length::new(Ticks(length)).unwrap(),
            notes
                .iter()
                .map(|&(start, length, pitch)| Note {
                    start: Ticks(start),
                    length: Length::new(Ticks(length)).unwrap(),
                    pitch: Pitch::new(pitch).unwrap(),
                    velocity: Velocity::new(100).unwrap(),
                })
                .collect(),
        )
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
    fn a_shift_stops_at_tick_zero() {
        // What is off the grid keeps its offset, and nothing goes before tick 0.
        assert_eq!(shifted(Ticks(250), 240), Ticks(490));
        assert_eq!(shifted(Ticks(250), -240), Ticks(10));
        assert_eq!(shifted(Ticks(250), -480), Ticks(0));
        assert_eq!(shifted(Ticks(0), -240), Ticks(0));
    }

    #[test]
    fn a_rectangle_touches_the_rows_between_its_corners() {
        assert_eq!(rows_between(10.0, 10.0, 5), 0..1);
        assert_eq!(rows_between(100.0, 10.0, 5), 0..2);
        assert_eq!(rows_between(-50.0, 64.0, 5), 0..2);
        assert_eq!(rows_between(200.0, 5_000.0, 5), 3..5);
        assert_eq!(rows_between(5_000.0, 6_000.0, 5), 5..5);
        assert_eq!(rows_between(0.0, 100.0, 0), 0..0);
    }

    #[test]
    fn a_drag_outside_the_rows_stays_on_the_nearest_track() {
        let viewport = Viewport {
            scroll_y: 96.0,
            ..Viewport::default()
        };
        assert_eq!(viewport.nearest_track(-500.0, 10), Some(0));
        assert_eq!(viewport.nearest_track(0.0, 10), Some(1));
        assert_eq!(viewport.nearest_track(5_000.0, 10), Some(9));
        assert_eq!(viewport.nearest_track(0.0, 0), None);
    }

    #[test]
    fn beat_lines_show_only_when_beats_are_wide_enough() {
        let beats = Viewport::default().beat_lines(four_four(), 120.0);
        // 24 px per beat. The bar lines at 8 and 104 are not beat lines.
        assert_eq!(beats, [32.0, 56.0, 80.0]);
        let narrow = Viewport {
            pixels_per_quarter: 23.0,
            ..Viewport::default()
        };
        assert_eq!(narrow.beat_lines(four_four(), 500.0).len(), 0);

        // Each bar has the beats of its own time signature: three sixteenths in 3/16 at
        // 6 px each are too narrow, two eighths in 2/8 at 12 px too, and 4/4 at 24 px shows.
        let changing = time_signatures(&[("3/16", 1), ("2/8", 1), ("4/4", 1)]);
        let beats = Viewport::default().beat_lines(&changing, 200.0);
        // 4/4 starts at tick 1680, x 8 + 42, and goes on: the next bar line is at x 146.
        assert_eq!(beats, [74.0, 98.0, 122.0, 170.0, 194.0]);
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
        // The lead-in, 8 bars and 16 bars of room are 2312 px. 10 rows and the row of the add
        // track button are 680 px.
        assert_eq!(clamped.scroll_x, 2312.0 - 960.0);
        assert_eq!(clamped.scroll_y, 680.0 - 320.0);

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
    fn the_view_pages_forward_when_the_playhead_passes_the_right_edge() {
        // 96 px per bar, a 960 px area, tick 0 at the 8 px lead-in: bar 10 is still on screen.
        let viewport = Viewport::default();
        let width = 960.0;
        assert!(viewport.shows(Ticks(0), width));
        assert!(viewport.shows(Ticks(9 * BAR), width));
        // Bar 11 starts at 8 + 960, past the right edge.
        assert!(!viewport.shows(Ticks(10 * BAR), width));

        // Inside the view the viewport does not move at all.
        assert_eq!(viewport.following(Ticks(5 * BAR), width), viewport);
        // Past the right edge it pages: the playhead lands at the left edge.
        let paged = viewport.following(Ticks(10 * BAR), width);
        assert_eq!(paged.x_of(Ticks(10 * BAR)), LEAD_IN);
        assert_eq!(paged.pixels_per_quarter, viewport.pixels_per_quarter);
        assert!(paged.shows(Ticks(10 * BAR), width));

        // A tick left of the view comes back the same way, and tick 0 goes to the start.
        let scrolled = Viewport {
            scroll_x: 40.0 * 96.0,
            ..Viewport::default()
        };
        assert!(!scrolled.shows(Ticks(0), width));
        assert_eq!(scrolled.following(Ticks(0), width).scroll_x, 0.0);
        assert_eq!(
            scrolled
                .following(Ticks(20 * BAR), width)
                .x_of(Ticks(20 * BAR)),
            LEAD_IN
        );

        // Zoomed far in, far into a long piece, the playhead still lands at the left edge.
        let zoomed = Viewport {
            pixels_per_quarter: 384.0,
            scroll_x: 400.0 * 4.0 * 384.0,
            ..Viewport::default()
        };
        let far = Ticks(600 * BAR);
        assert_eq!(zoomed.following(far, width).x_of(far), LEAD_IN);
    }

    #[test]
    fn a_clip_sits_inside_its_row() {
        let viewport = Viewport {
            scroll_y: 32.0,
            ..Viewport::default()
        };
        let rect = viewport.clip_rect(2, Ticks(BAR), Ticks(3 * BAR));
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
        assert_eq!(far_out.clip_rect(0, Ticks(0), Ticks(120)).width, 1.0);
    }

    #[test]
    fn the_ruler_marks_bars_and_thins_them_out_when_narrow() {
        let marks = |bars: Vec<RulerBar>| -> Vec<(u64, f32)> {
            bars.iter().map(|bar| (bar.number, bar.x)).collect()
        };
        let bars = Viewport::default().ruler_bars(four_four(), 300.0);
        assert_eq!(
            marks(bars.clone()),
            [(1, 8.0), (2, 104.0), (3, 200.0), (4, 296.0)]
        );
        // The time signature shows where it starts: here only at bar 1.
        let labels: Vec<String> = bars.iter().map(RulerBar::label).collect();
        assert_eq!(labels, ["1  4/4", "2", "3", "4"]);

        let scrolled = Viewport {
            scroll_x: 100.0,
            ..Viewport::default()
        };
        let bars = scrolled.ruler_bars(four_four(), 200.0);
        assert_eq!(marks(bars), [(1, -92.0), (2, 4.0), (3, 100.0), (4, 196.0)]);

        // 12 px bars: every 8th bar is 96 px apart, every 4th would be 48 px.
        let narrow = Viewport {
            pixels_per_quarter: 3.0,
            ..Viewport::default()
        };
        let bars = narrow.ruler_bars(four_four(), 208.0);
        assert_eq!(marks(bars), [(1, 8.0), (9, 104.0), (17, 200.0)]);

        let waltz = TimeSignatures::constant(TimeSignature::new(3, 4).unwrap());
        let bars = Viewport::default().ruler_bars(&waltz, 160.0);
        assert_eq!(marks(bars), [(1, 8.0), (2, 80.0), (3, 152.0)]);
    }

    /// Each time signature thins its own marks out, counted from where it starts, and a change
    /// that has no mark of its own shows at the next mark.
    #[test]
    fn the_ruler_follows_changing_time_signatures() {
        let labels = |time_signatures: &TimeSignatures, width: f32| -> Vec<(u64, f32, String)> {
            let bars = Viewport::default().ruler_bars(time_signatures, width);
            bars.iter()
                .map(|bar| (bar.number, bar.x, bar.label()))
                .collect()
        };
        let mark = |number, x: f32, label: &str| (number, x, label.to_string());
        // 96 px per 4/4 bar, 84 per 7/8, 18 per 3/16: every 4th bar of 3/16 has a mark.
        let changing = time_signatures(&[("4/4", 2), ("7/8", 1), ("3/16", 8)]);
        assert_eq!(
            labels(&changing, 400.0),
            [
                mark(1, 8.0, "1  4/4"),
                mark(2, 104.0, "2"),
                mark(3, 200.0, "3  7/8"),
                mark(4, 284.0, "4  3/16"),
                mark(8, 356.0, "8"),
            ]
        );
        // A new time signature every bar, as in the Danse sacrale: bars 4 to 6 are too close to
        // bar 3 for a mark, so bar 7 shows the 5/16 it is in. Bar 8, where 7/8 starts, is too
        // close to bar 7, so bar 9 shows it.
        let sacrale = time_signatures(&[
            ("4/4", 2),
            ("3/16", 1),
            ("2/16", 1),
            ("3/16", 1),
            ("2/8", 1),
            ("5/16", 1),
            ("7/8", 2),
            ("4/4", 1),
        ]);
        assert_eq!(
            labels(&sacrale, 450.0),
            [
                mark(1, 8.0, "1  4/4"),
                mark(2, 104.0, "2"),
                mark(3, 200.0, "3  3/16"),
                mark(7, 272.0, "7  5/16"),
                mark(9, 386.0, "9  7/8"),
            ]
        );
    }

    #[test]
    fn a_miniature_places_notes_by_time_and_pitch() {
        let viewport = Viewport::default();
        let clip = clip(BAR, BAR, &[(0, 960, 60), (960, 960, 72), (2880, 9600, 60)]);
        let rect = viewport.clip_rect(0, clip.start, clip.end());
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
        let rect = viewport.clip_rect(0, single.start, single.end());
        let note = viewport.miniature(&single, rect).next().unwrap();
        assert_eq!(note.y + note.height / 2.0, rect.y + rect.height / 2.0);

        // Too narrow to read: no notes.
        let far_out = Viewport {
            pixels_per_quarter: 1.0,
            ..Viewport::default()
        };
        let rect = far_out.clip_rect(0, clip.start, clip.end());
        assert_eq!(far_out.miniature(&clip, rect).count(), 0);
    }
}
