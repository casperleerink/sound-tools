//! The card of another instance on a page, drawn over the page where the page made room for it.
//! What moves on the card, such as its meter or a knob, then draws the card alone: in GPUI a
//! notified view also renders every view it is in, and the page is the heaviest one.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, Entity, FocusHandle, Pixels, Point, Size,
    WeakEntity, Window, canvas, div, prelude::*,
};

use super::TypeScriptCard;
use super::input::hearing_keys;

/// Where the page made room for a card, and where it cuts it off, as by a div that scrolls.
#[derive(Clone, Copy)]
struct Room {
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
}

/// What the page and the card it draws over it share.
pub(super) struct Placing {
    pub(super) view: Entity<Placed>,
    /// `None` while the page shows no room for it, as when it is scrolled out of view.
    room: Rc<Cell<Option<Room>>>,
    /// The size of the card when it last drew, which its room takes.
    size: Rc<Cell<Option<Size<Pixels>>>>,
}

impl Placing {
    pub(super) fn new(
        card: AnyView,
        origin: Rc<Cell<Point<Pixels>>>,
        window: &mut Window,
        cx: &mut Context<TypeScriptCard>,
    ) -> Self {
        let (room, size) = (Rc::default(), Rc::default());
        // The keys of the page that its controls hand on, held while the focus is in it.
        let focus = cx.focus_handle();
        cx.on_focus_out(&focus, window, |page, _, _, cx| page.let_go_of_keys(cx))
            .detach();
        let page = cx.entity();
        let view = cx.new(|cx| {
            // The page drew again: its room may have moved.
            cx.observe(&page, |_, _, cx| cx.notify()).detach();
            Placed {
                card,
                focus,
                room: Rc::clone(&room),
                size: Rc::clone(&size),
                origin,
                page: page.downgrade(),
            }
        });
        Self { view, room, size }
    }

    /// The page draws in a new frame: what it does not make room for again shows nowhere.
    pub(super) fn forget_room(&self) {
        self.room.set(None);
    }

    /// What the page draws for the card: room of its size, or, while its size is not known,
    /// the card itself, which measures it.
    pub(super) fn room(&self, card: &AnyView, page: &WeakEntity<TypeScriptCard>) -> AnyElement {
        match self.size.get() {
            Some(size) => {
                let room = Rc::clone(&self.room);
                let measure = canvas(
                    move |bounds, window, _| {
                        let mask = window.content_mask().bounds;
                        room.set(Some(Room { bounds, mask }));
                    },
                    |_, _, _, _| {},
                );
                (div().flex_none().w(size.width).h(size.height))
                    .child(measure.size_full())
                    .into_any_element()
            }
            None => (div().flex().flex_none().relative())
                .child(card.clone())
                .child(measured(&self.size, page).absolute().size_full())
                .into_any_element(),
        }
    }
}

/// Writes the size it is laid out at into `size`, and draws the page again when it changed, for
/// the room to take it.
fn measured(
    size: &Rc<Cell<Option<Size<Pixels>>>>,
    page: &WeakEntity<TypeScriptCard>,
) -> impl IntoElement + Styled {
    let (size, page) = (Rc::clone(size), page.clone());
    canvas(
        move |bounds, _, cx: &mut App| {
            if size.replace(Some(bounds.size)) != Some(bounds.size) {
                cx.defer(move |cx| {
                    page.update(cx, |_, cx| cx.notify()).ok();
                });
            }
        },
        |_, _, _, _| {},
    )
}

/// A card the page draws over itself. It is cached in the page, so only its own changes and
/// the page's draw it again.
pub(crate) struct Placed {
    card: AnyView,
    focus: FocusHandle,
    room: Rc<Cell<Option<Room>>>,
    size: Rc<Cell<Option<Size<Pixels>>>>,
    /// Where the page is in the window.
    origin: Rc<Cell<Point<Pixels>>>,
    page: WeakEntity<TypeScriptCard>,
}

impl Render for Placed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = div().size_full();
        let (Some(Room { bounds, mask }), Some(size)) = (self.room.get(), self.size.get()) else {
            return layer;
        };
        let origin = self.origin.get();
        // A card that grows lies over what follows it until the next frame of the page.
        let card = (div().flex_none().relative())
            .child(self.card.clone())
            .child(measured(&self.size, &self.page).absolute().size_full());
        let at = div()
            .absolute()
            .left(bounds.origin.x - mask.origin.x)
            .top(bounds.origin.y - mask.origin.y)
            .w(size.width)
            .h(size.height)
            .flex()
            .items_start()
            .child(card);
        // The keys the card's controls do not keep reach the page, as they did inside it.
        let keys = (self.page.upgrade()).and_then(|page| page.read(cx).keys_of_page(cx));
        let at = match keys {
            Some(context) => hearing_keys(at, &self.focus, context, &self.page),
            None => at,
        };
        let cut = div()
            .absolute()
            .left(mask.origin.x - origin.x)
            .top(mask.origin.y - origin.y)
            .w(mask.size.width)
            .h(mask.size.height)
            .overflow_hidden();
        layer.child(cut.child(at))
    }
}
