//! The loop in the ruler: a bar from its start to its end, dragged by its body or an edge on
//! the grid. A click on it turns it on or off.

use std::ops::Range;

use gpui::{App, Bounds, ContentMask, Context, Modifiers, Pixels, Window, fill, point, px, size};
use sound_core::Ticks;
use sound_ui::{ActiveTheme, Loop};

use super::Timeline;
use super::scene::Scene;
use super::state::{After, Held};
use crate::view::gesture::{Zone, zone_at};
use crate::view::layout::{RULER_HEIGHT, Rect, shifted};

/// The bar sits at the bottom of the ruler, over the bar marks, under the numbers.
const BAR_TOP: f32 = RULER_HEIGHT - 10.;
const BAR_HEIGHT: f32 = 8.;
/// A press this far above the bar still takes it, for a trackpad.
const BAR_REACH: f32 = 4.;

/// The loop as one paint shows it.
pub(super) struct LoopBar {
    pub(super) x: Range<f32>,
    pub(super) on: bool,
}

impl LoopBar {
    fn rect(&self) -> Rect {
        Rect {
            x: self.x.start,
            y: BAR_TOP,
            width: self.x.end - self.x.start,
            height: BAR_HEIGHT,
        }
    }
}

/// A press on the loop, from mouse down to mouse up. Every move starts from `origin`.
pub(super) struct LoopDrag {
    pub(super) zone: Zone,
    pub(super) origin: Loop,
    /// The tick under the pointer at mouse down.
    pub(super) grab: Ticks,
    /// A press that moved less than a grid step is a click: it turns the loop on or off.
    pub(super) moved: bool,
}

impl Scene {
    /// The part of the loop at `x` and `y` in the ruler, from its top.
    pub(super) fn loop_zone_at(&self, x: f32, y: f32) -> Option<Zone> {
        let rect = self.loop_bar.rect();
        let across = rect.x <= x && x < rect.x + rect.width;
        let down = (BAR_TOP - BAR_REACH..RULER_HEIGHT).contains(&y);
        (across && down).then(|| zone_at(rect, x))
    }
}

impl Timeline {
    pub(super) fn start_loop_drag(&mut self, zone: Zone, x: f32, cx: &mut Context<Self>) {
        self.held = Held::Loop(LoopDrag {
            zone,
            origin: self.session.read(cx).looped().read(cx).clone(),
            grab: self.painted.get().tick_at(x),
            moved: false,
        });
    }

    /// One mouse move: the body moves the loop, an edge moves that end. An end never passes
    /// the other. Cmd held is no snap.
    pub(super) fn drag_loop(
        &mut self,
        drag: &mut LoopDrag,
        x: f32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> After {
        let grid = match modifiers.secondary() {
            true => self.grid(cx).free(),
            false => self.grid(cx),
        };
        let Range { start, end } = drag.origin.range;
        let anchor = match drag.zone {
            Zone::RightEdge => end,
            Zone::Body | Zone::LeftEdge => start,
        };
        let delta = grid.delta(anchor, drag.grab, self.painted.get().tick_at(x));
        let range = match drag.zone {
            Zone::Body => {
                let delta = delta.max(-(start.0 as i64));
                shifted(start, delta)..shifted(end, delta)
            }
            Zone::LeftEdge => shifted(start, delta)..end,
            Zone::RightEdge => start..shifted(end, delta),
        };
        // An end dragged past the other one in a single move is still no click.
        drag.moved |= delta != 0;
        if range.is_empty() {
            return After::Keep;
        }
        let looped = Loop {
            range,
            on: drag.origin.on,
        };
        self.session
            .update(cx, |session, cx| session.set_loop(looped, cx));
        After::Keep
    }

    /// Mouse up: a click turns the loop on or off.
    pub(super) fn end_loop_drag(&mut self, drag: LoopDrag, cx: &mut Context<Self>) {
        if !drag.moved {
            let looped = Loop {
                on: !drag.origin.on,
                ..drag.origin
            };
            self.session
                .update(cx, |session, cx| session.set_loop(looped, cx));
        }
    }

    /// Escape: the loop goes back to where it was at mouse down.
    pub(super) fn cancel_loop_drag(&mut self, drag: LoopDrag, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.set_loop(drag.origin, cx));
    }
}

/// Muted while off, bright while on, as the click in the transport is.
pub(super) fn paint_loop_bar(
    bar: &LoopBar,
    ruler: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = cx.theme();
    let color = match bar.on {
        true => theme.gray_900,
        false => theme.alpha_at(0.15),
    };
    let rect = bar.rect();
    let bounds = Bounds::new(
        ruler.origin + point(px(rect.x.round()), px(rect.y)),
        size(px(rect.width.round().max(1.)), px(rect.height)),
    );
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        window.paint_quad(fill(bounds, color).corner_radii(px(2.)));
    });
}
