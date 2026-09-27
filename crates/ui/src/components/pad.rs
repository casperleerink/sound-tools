//! Pad: one pad of a drum pad, 72 x 32 pt with 6 pt corners, the fill and border of a clip and
//! its name in 12 pt medium, 8 pt in. Selected, it has the `gray-950` border of a selected
//! clip. While it sounds it is green, at 24 % that fades with the sound: sound is moving. A
//! sample pad carries a 12 pt waveform glyph at its right, and a pad whose file is missing a
//! peach warning glyph instead. A pad with no glyph is synthesized. A long name ends in an
//! ellipsis. Where a file from the Finder would land, it has the 2 pt lavender ring of a drop
//! target.
//!
//! Controlled: the owner gives every state and hears a press and a drop of files. A pad is no
//! tab stop: the grid it sits in is one, and the owner moves the selection with the arrows.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, ElementId, ExternalPaths, FontWeight, MouseButton, SharedString, StyleRefinement, Window,
    div, prelude::*, px,
};

use crate::components::icon::Icon;
use crate::theme::ActiveTheme;

pub const PAD_WIDTH: f32 = 72.;
pub const PAD_HEIGHT: f32 = 32.;
/// Between two pads of a grid.
pub const PAD_GAP: f32 = 4.;
/// The fill of a sounding pad at full level.
pub const SOUNDING_OPACITY: f32 = 0.24;
const GLYPH: f32 = 12.;
/// From the edge of the pad to its name and its glyph.
const INSET: f32 = 8.;

/// What the glyph at the right of a pad says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadGlyph {
    /// The pad plays a sample.
    Sample,
    /// The pad plays a sample whose file is not there, so it is silent.
    Missing,
}

type PressHandler = Rc<dyn Fn(&mut Window, &mut App)>;
type DropHandler = Rc<dyn Fn(Vec<PathBuf>, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Pad {
    id: ElementId,
    name: SharedString,
    selected: bool,
    sounding: f32,
    glyph: Option<PadGlyph>,
    drop_target: bool,
    on_press: Option<PressHandler>,
    on_drop_files: Option<DropHandler>,
}

impl Pad {
    pub fn new(id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            selected: false,
            sounding: 0.,
            glyph: None,
            drop_target: false,
            on_press: None,
            on_drop_files: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// How loud the pad sounds now, from 0 (silent) to 1 (its hit).
    pub fn sounding(mut self, level: f32) -> Self {
        self.sounding = level;
        self
    }

    pub fn glyph(mut self, glyph: Option<PadGlyph>) -> Self {
        self.glyph = glyph;
        self
    }

    /// Shows the ring of a drop target, whatever is dragged. A pad that takes files shows it
    /// by itself while files are over it, see [`Self::on_drop_files`].
    pub fn drop_target(mut self, drop_target: bool) -> Self {
        self.drop_target = drop_target;
        self
    }

    /// A press of the left button on the pad.
    pub fn on_press(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }

    /// Files from the Finder let go of on the pad. While they are over it, it has the ring.
    pub fn on_drop_files(
        mut self,
        f: impl Fn(Vec<PathBuf>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drop_files = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Pad {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, border, selected, text) = (
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.gray_950,
        );
        let sounding = theme
            .green
            .opacity(SOUNDING_OPACITY * self.sounding.clamp(0., 1.));
        let (ring, warning) = (theme.lavender, theme.peach);
        // The ring is 2 pt where the border is 1, so the padding gives up a point for it and
        // the name stays where it is.
        let ring_style = move |style: StyleRefinement| {
            style
                .border_2()
                .border_color(ring)
                .pl(px(INSET - 2.))
                .pr(px(INSET - 2.))
        };
        let glyph = self.glyph.map(|glyph| match glyph {
            PadGlyph::Sample => Icon::new("audio-lines").size(GLYPH).color(text),
            PadGlyph::Missing => Icon::new("triangle-alert").size(GLYPH).color(warning),
        });
        // For tests, which find a pad by its id: `pad-<id>`. Nothing in a normal build.
        let selector = self.id.clone();
        let pad = div()
            .id(self.id)
            .debug_selector(move || format!("pad-{selector}"))
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .w(px(PAD_WIDTH))
            .h(px(PAD_HEIGHT))
            .pl(px(INSET - 1.))
            .pr(px(INSET - 1.))
            .rounded(px(6.))
            .bg(fill)
            .border_1()
            .border_color(if self.selected { selected } else { border })
            .text_size(px(12.))
            .line_height(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            // The sound, over the fill and under the name.
            .when(self.sounding > 0., |pad| {
                pad.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(px(5.))
                        .bg(sounding),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.name),
            )
            .children(glyph.map(|glyph| div().relative().child(glyph)))
            .when(self.drop_target, |pad| {
                pad.border_2()
                    .border_color(ring)
                    .pl(px(INSET - 2.))
                    .pr(px(INSET - 2.))
            });
        let pad = match self.on_drop_files {
            Some(on_drop) => pad
                .drag_over::<ExternalPaths>(move |style, _, _, _| ring_style(style))
                .on_drop(move |paths: &ExternalPaths, window, cx| {
                    on_drop(paths.paths().to_vec(), window, cx)
                }),
            None => pad,
        };
        match self.on_press {
            Some(on_press) => pad
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| on_press(window, cx)),
            None => pad,
        }
    }
}
