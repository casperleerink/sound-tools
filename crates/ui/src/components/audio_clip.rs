//! Clip waveform: an audio clip on the timeline. The clip shape of a note clip, 6 pt corners,
//! `alpha/5` fill and `alpha/10` border, `gray-950` when selected, with the waveform of its file
//! in the track colour at 85 %, mirrored about the middle, at most 22 pt each way. It is drawn as
//! it sounds: scaled by the clip gain and faded, so a fade is seen in the waveform.
//!
//! With the pointer on it, or selected, it shows three handles at its top, 7 pt down: a fade
//! handle at each top corner and the gain handle, hollow, in the middle. A fade is then a 1.5 pt
//! `gray-950` line from the bottom corner to its handle, with `gray-50` at 50 % outside it. While
//! a fade or the gain is dragged its value shows on a label: `Fade in 420 ms`, `-6 dB`. While an
//! edge is dragged, the part of the file the clip hides shows past that edge at 25 %.
//!
//! This is a painter, not an element: the timeline paints every clip on one canvas and hit
//! tests them itself, with [`ClipHandles`] for where the handles are. The gallery paints it on a
//! canvas of its own.

use gpui::{
    App, BorderStyle, Bounds, Hsla, PathBuilder, Pixels, Point, SharedString, TextAlign, TextRun,
    TruncateFrom, Window, fill, point, px, quad, size,
};

use crate::components::paint;
use crate::theme::ActiveTheme;
use crate::typography;

/// The centre of the handles, this far below the top of the clip.
pub const HANDLE_DOWN: f32 = 7.;
/// The dot of a handle.
pub const HANDLE_SIZE: f32 = 10.;
/// The target of a handle is larger than its dot: a trackpad is not a mouse.
pub const HANDLE_TARGET: f32 = 18.;
/// A clip narrower than this shows no handles: they would cover each other and the edges.
pub const MIN_HANDLES_WIDTH: f32 = 3. * HANDLE_TARGET;
/// The most the waveform reaches from the middle, each way.
pub const WAVEFORM_REACH: f32 = 22.;
const HANDLE_RING: f32 = 1.5;
const FADE_LINE: f32 = 1.5;
/// The waveform of the track colour, and the part of the file a trim hides.
const WAVEFORM_OPACITY: f32 = 0.85;
const HIDDEN_OPACITY: f32 = 0.25;
/// A clip on a muted track, as any clip there.
const MUTED_OPACITY: f32 = 0.4;
const LABEL_HEIGHT: f32 = 20.;

/// One of the three handles of an audio clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipHandle {
    FadeIn,
    FadeOut,
    Gain,
}

/// Where the three handles of a clip are, in the coordinates the owner gives: the clip's left
/// edge and top, its width, and how wide each fade is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipHandles {
    pub fade_in: (f32, f32),
    pub fade_out: (f32, f32),
    pub gain: (f32, f32),
}

impl ClipHandles {
    /// The fade handles are at the end of each fade, so at the corners while there is none,
    /// and the gain handle in the middle. `None` for a clip too narrow for handles.
    pub fn of(left: f32, top: f32, width: f32, fade_in: f32, fade_out: f32) -> Option<Self> {
        if width < MIN_HANDLES_WIDTH {
            return None;
        }
        let y = top + HANDLE_DOWN;
        // A handle stays inside the clip, where its target can be pressed.
        let inside = |x: f32| x.clamp(left + HANDLE_SIZE / 2., left + width - HANDLE_SIZE / 2.);
        Some(Self {
            fade_in: (inside(left + fade_in), y),
            fade_out: (inside(left + width - fade_out), y),
            gain: (left + width / 2., y),
        })
    }

    /// The centre of one handle.
    pub fn of_handle(&self, handle: ClipHandle) -> (f32, f32) {
        match handle {
            ClipHandle::FadeIn => self.fade_in,
            ClipHandle::FadeOut => self.fade_out,
            ClipHandle::Gain => self.gain,
        }
    }

    /// The handle whose target is at a place, the gain first, since it sits between the others.
    pub fn at(&self, x: f32, y: f32) -> Option<ClipHandle> {
        let near = |(centre_x, centre_y): (f32, f32)| {
            (x - centre_x).abs() <= HANDLE_TARGET / 2. && (y - centre_y).abs() <= HANDLE_TARGET / 2.
        };
        [ClipHandle::Gain, ClipHandle::FadeIn, ClipHandle::FadeOut]
            .into_iter()
            .find(|handle| near(self.of_handle(*handle)))
    }
}

/// A stretch of the waveform: the peak of each column of one point, from `left` on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Columns {
    pub left: Pixels,
    pub peaks: Vec<f32>,
}

/// Everything a clip shows. The owner works out the peaks of the columns on screen from the
/// overview of the file; the painter scales them by the gain and the fades.
#[derive(Clone, Debug)]
pub struct AudioClipLook {
    pub bounds: Bounds<Pixels>,
    /// The colour of the track.
    pub accent: Hsla,
    /// The columns of the waveform inside the clip. Empty while the overview is made.
    pub waveform: Columns,
    /// The gain of the clip as a factor.
    pub gain: f32,
    /// How wide each fade is, in points.
    pub fade_in: f32,
    pub fade_out: f32,
    pub selected: bool,
    /// The pointer is on it, or it is selected: the handles show.
    pub handles: bool,
    /// The part of the file the clip hides past the edge that is dragged.
    pub hidden: Option<Columns>,
    /// The value of a fade or the gain while it is dragged, next to the handle it belongs to.
    pub label: Option<(SharedString, ClipHandle)>,
    /// The file is not there: the clip says so and shows no waveform.
    pub missing: Option<SharedString>,
    pub muted: bool,
}

impl AudioClipLook {
    /// A clip at rest with no waveform yet: the owner fills in the rest.
    pub fn new(bounds: Bounds<Pixels>, accent: Hsla) -> Self {
        Self {
            bounds,
            accent,
            waveform: Columns::default(),
            gain: 1.,
            fade_in: 0.,
            fade_out: 0.,
            selected: false,
            handles: false,
            hidden: None,
            label: None,
            missing: None,
            muted: false,
        }
    }

    /// The level at a place across, as heard: the gain times the fades, which are straight lines
    /// from silence at the edges.
    fn level_at(&self, x: f32) -> f32 {
        let left = f32::from(self.bounds.left());
        let right = f32::from(self.bounds.right());
        let fade = |distance: f32, width: f32| match width > 0. {
            true => (distance / width).clamp(0., 1.),
            false => 1.,
        };
        self.gain * fade(x - left, self.fade_in).min(fade(right - x, self.fade_out))
    }
}

/// Paints the waveform columns, mirrored about `middle`.
fn paint_columns(
    columns: &Columns,
    middle: Pixels,
    color: Hsla,
    level: impl Fn(f32) -> f32,
    window: &mut Window,
) {
    for (index, peak) in columns.peaks.iter().enumerate() {
        let x = f32::from(columns.left) + index as f32;
        // At least a hairline, so a quiet part of the file still shows where it is.
        let reach = ((peak * level(x + 0.5)).min(1.) * WAVEFORM_REACH).max(0.5);
        let area = Bounds::new(
            point(px(x), middle - px(reach)),
            size(px(1.), px(reach * 2.)),
        );
        window.paint_quad(fill(area, color));
    }
}

/// Paints one audio clip.
pub fn paint_audio_clip(look: &AudioClipLook, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let dim = if look.muted { MUTED_OPACITY } else { 1. };
    let (fill_color, border, selection) = (
        theme.alpha_at(0.05 * dim),
        theme.alpha_at(0.10 * dim),
        theme.gray_950.opacity(dim),
    );
    let (dot, ring, shade, text_color) = (
        theme.gray_950,
        theme.gray_50,
        theme.gray_50.opacity(0.5),
        theme.gray_800.opacity(dim),
    );
    let (label_fill, label_border, label_text) =
        (theme.gray_100, theme.alpha_at(0.10), theme.gray_950);
    let bounds = look.bounds;
    let middle = bounds.center().y;

    if let Some(hidden) = &look.hidden {
        let color = look.accent.opacity(HIDDEN_OPACITY * dim);
        paint_columns(hidden, middle, color, |_| 1., window);
    }
    let radius = px(6.).min(bounds.size.width / 2.);
    let edge = if look.selected { selection } else { border };
    let solid = BorderStyle::Solid;
    window.paint_quad(quad(bounds, radius, fill_color, px(1.), edge, solid));

    if let Some(missing) = &look.missing {
        let origin = point(bounds.left() + px(12.), middle - px(8.));
        let width = f32::from(bounds.size.width) - 24.;
        paint_text(missing.clone(), origin, width, text_color, window, cx);
        return;
    }
    let color = look.accent.opacity(WAVEFORM_OPACITY * dim);
    paint_columns(&look.waveform, middle, color, |x| look.level_at(x), window);

    let handles = ClipHandles::of(
        f32::from(bounds.left()),
        f32::from(bounds.top()),
        f32::from(bounds.size.width),
        look.fade_in,
        look.fade_out,
    );
    if let Some(handles) = handles.filter(|_| look.handles || look.label.is_some()) {
        let (left, right) = (bounds.left(), bounds.right());
        let (top, bottom) = (bounds.top(), bounds.bottom());
        let at = |(x, y): (f32, f32)| point(px(x), px(y));
        // Each fade: the shade outside it, then its line from the bottom corner to its handle.
        let fades = [
            (
                look.fade_in,
                point(left, bottom),
                point(left, top),
                handles.fade_in,
            ),
            (
                look.fade_out,
                point(right, bottom),
                point(right, top),
                handles.fade_out,
            ),
        ];
        for (width, corner, top_corner, handle) in fades {
            if width <= 0. {
                continue;
            }
            let handle = at(handle);
            let mut outside = PathBuilder::fill();
            outside.move_to(corner);
            outside.line_to(top_corner);
            outside.line_to(point(handle.x, top));
            outside.line_to(handle);
            outside.close();
            if let Ok(path) = outside.build() {
                window.paint_path(path, shade);
            }
            let mut line = PathBuilder::stroke(px(FADE_LINE));
            line.move_to(corner);
            line.line_to(handle);
            if let Ok(path) = line.build() {
                window.paint_path(path, dot);
            }
        }
        for (place, hollow) in [
            (handles.fade_in, false),
            (handles.fade_out, false),
            (handles.gain, true),
        ] {
            let centre = at(place);
            let (inside, edge) = if hollow { (ring, dot) } else { (dot, ring) };
            paint::circle(window, centre, HANDLE_SIZE / 2., edge);
            paint::circle(window, centre, HANDLE_SIZE / 2. - HANDLE_RING, inside);
        }
    }

    if let Some((text, handle)) = &look.label {
        let font = typography::tabular();
        let run = TextRun {
            len: text.len(),
            font,
            color: label_text,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(text.clone(), px(12.), &[run], None);
        let width = shaped.width + px(12.);
        // Under the handle, into the clip: right of the fade in and the gain, left of the
        // fade out.
        let (x, y) = handles.map_or(
            (
                f32::from(bounds.left()) + 8.,
                f32::from(bounds.top()) + HANDLE_DOWN,
            ),
            |handles| handles.of_handle(*handle),
        );
        let left = match handle {
            ClipHandle::FadeOut => px(x - HANDLE_SIZE) - width,
            ClipHandle::FadeIn | ClipHandle::Gain => px(x + HANDLE_SIZE),
        };
        let area = Bounds::new(
            point(left, px(y + HANDLE_SIZE / 2.)),
            size(width, px(LABEL_HEIGHT)),
        );
        window.paint_quad(quad(area, px(6.), label_fill, px(1.), label_border, solid));
        let origin = area.origin + point(px(6.), px(2.));
        // A glyph that cannot be painted leaves a gap in a label. Nothing else depends on it.
        if let Err(error) = shaped.paint(origin, px(16.), TextAlign::Left, None, window, cx) {
            eprintln!("audio clip: {error}");
        }
    }
}

/// A line of 12 pt text that ends in an ellipsis at `width`, so it stays inside its clip.
fn paint_text(
    text: SharedString,
    origin: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    if width <= 0. {
        return;
    }
    let font = typography::tabular();
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut wrapper = window.text_system().line_wrapper(font, px(12.));
    let runs = [run];
    let (text, runs) = wrapper.truncate_line(text, px(width), "…", &runs, TruncateFrom::End);
    let runs = runs.into_owned();
    let shaped = window.text_system().shape_line(text, px(12.), &runs, None);
    if let Err(error) = shaped.paint(origin, px(16.), TextAlign::Left, None, window, cx) {
        eprintln!("audio clip: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fade_handles_sit_at_the_ends_of_the_fades_and_the_gain_in_the_middle() {
        let handles = ClipHandles::of(100., 20., 200., 40., 0.).unwrap();
        assert_eq!(handles.fade_in, (140., 27.));
        // No fade out: at the corner, inside the clip.
        assert_eq!(handles.fade_out, (295., 27.));
        assert_eq!(handles.gain, (200., 27.));
        assert_eq!(
            ClipHandles::of(0., 0., MIN_HANDLES_WIDTH - 1., 0., 0.),
            None
        );
    }

    #[test]
    fn the_waveform_is_drawn_as_it_sounds() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(56.)));
        let mut look = AudioClipLook::new(bounds, Hsla::default());
        look.gain = 0.5;
        look.fade_in = 20.;
        look.fade_out = 10.;
        assert_eq!(look.level_at(0.), 0.);
        assert_eq!(look.level_at(10.), 0.25);
        assert_eq!(look.level_at(50.), 0.5);
        assert_eq!(look.level_at(95.), 0.25);
        assert_eq!(look.level_at(100.), 0.);
    }
}
