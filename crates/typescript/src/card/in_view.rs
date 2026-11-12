//! What holds the keyboard focus in a div that scrolls is scrolled into view: when the focus
//! comes to it by a key, and when it moves in what scrolls while it holds the focus, as a button
//! does when a card is added before it. A wheel moves nothing in what scrolls, so the composer's
//! scroll stays.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, FocusHandle, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, Point, ScrollHandle, WeakEntity, WeakFocusHandle, Window, point, prelude::*,
    px,
};

use crate::tree::ScrollAxis;

use super::TypeScriptCard;
use super::edges::FADE;

/// A div that scrolls, and along which axis.
pub(super) type Scroller = (ScrollHandle, ScrollAxis);

/// `child`, which may hold the focus, in the divs that scroll it, the innermost last. It is laid
/// out as `child` alone is.
pub(super) struct InView {
    pub(super) id: ElementId,
    pub(super) scrollers: Vec<Scroller>,
    /// Draws again after a scroll.
    pub(super) page: WeakEntity<TypeScriptCard>,
    pub(super) child: AnyElement,
}

struct Held {
    /// Never focused: on the element, it tells whether the focus is in it. GPUI gives a click
    /// to a div that tracks one, so no div does.
    within: FocusHandle,
    focused: Option<WeakFocusHandle>,
    from_keyboard: bool,
    /// Where it lies in what scrolls.
    place: Point<Pixels>,
}

impl InView {
    /// Where the divs scroll to so that `bounds` shows, each moved the least, if the focus came
    /// into them by a key or moved in them since the last frame.
    fn scrolls(
        &self,
        bounds: Bounds<Pixels>,
        held: &mut Held,
        window: &Window,
        cx: &App,
    ) -> Option<Vec<(ScrollHandle, Point<Pixels>)>> {
        let focused = (window.focused(cx))
            .filter(|focused| held.within.contains(focused, window))
            .map(|focused| focused.downgrade());
        let scrolled = (self.scrollers.iter()).fold(Point::default(), |sum, (scroller, _)| {
            sum + scroller.offset()
        });
        let outermost = self.scrollers.first()?.0.bounds().origin;
        let place = bounds.origin - scrolled - outermost;
        let came = focused.is_some() && focused != held.focused;
        if came {
            held.from_keyboard = window.last_input_was_keyboard();
        }
        // More than the rounding of a scroll.
        let moved = (place - held.place).magnitude() > 0.5;
        held.focused = focused;
        held.place = place;
        if held.focused.is_none() || !held.from_keyboard || !(came || moved) {
            return None;
        }
        let mut bounds = bounds;
        let scrolls = (self.scrollers.iter().rev()).map(|(scroller, axis)| {
            let offset = scroller.offset();
            let shows = showing(bounds, scroller, *axis);
            // The div outside this one sees it where this scroll leaves it.
            bounds.origin += shows - offset;
            (scroller.clone(), shows)
        });
        Some(scrolls.collect())
    }
}

/// The scroll of `scroller` that shows `bounds`, moved the least. Clear of the soft edge of an
/// end where more waits, its start first when it does not fit.
fn showing(bounds: Bounds<Pixels>, scroller: &ScrollHandle, axis: ScrollAxis) -> Point<Pixels> {
    let (room, offset, most) = (scroller.bounds(), scroller.offset(), scroller.max_offset());
    match axis {
        ScrollAxis::X => {
            let (shown, room) = ([bounds.left(), bounds.right()], [room.left(), room.right()]);
            point(along(shown, room, offset.x, most.x), offset.y)
        }
        ScrollAxis::Y => {
            let (shown, room) = ([bounds.top(), bounds.bottom()], [room.top(), room.bottom()]);
            point(offset.x, along(shown, room, offset.y, most.y))
        }
    }
}

/// [`showing`] along one axis: from where to where what shows and its room lie.
fn along(
    [start, end]: [Pixels; 2],
    [low, high]: [Pixels; 2],
    offset: Pixels,
    most: Pixels,
) -> Pixels {
    let edge = px(FADE).min((high - low - (end - start)) / 2.).max(px(0.));
    let (low, high) = (low + edge, high - edge);
    let by = if start < low || end - start > high - low {
        low - start
    } else if end > high {
        high - end
    } else {
        px(0.)
    };
    (offset + by).clamp(-most, px(0.))
}

impl IntoElement for InView {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for InView {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let scrolls = id.and_then(|id| {
            window.with_element_state(id, |held: Option<Held>, window| {
                let mut held = held.unwrap_or_else(|| Held {
                    within: cx.focus_handle(),
                    focused: None,
                    from_keyboard: false,
                    place: Point::default(),
                });
                window.set_focus_handle(&held.within, cx);
                (self.scrolls(bounds, &mut held, window, cx), held)
            })
        });
        if let Some(scrolls) = scrolls {
            // After the frame, in which every div read its scroll already. What holds the
            // focus inside this one is asked after it, so its scroll is the last and stays.
            let page = self.page.clone();
            window.defer(cx, move |_, cx| {
                let mut scrolled = false;
                for (scroller, shows) in scrolls {
                    scrolled |= scroller.offset() != shows;
                    scroller.set_offset(shows);
                }
                if scrolled {
                    page.update(cx, |_, cx| cx.notify()).ok();
                }
            });
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
