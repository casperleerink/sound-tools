//! The card of another instance on a page, which the page draws from its last frame while the
//! card did not change: a playhead, a meter or an edit of the page draws the page again, not
//! every card on it.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, Context, Entity, Pixels, Size, StyleRefinement, Window, canvas, div,
    prelude::*, px,
};

#[cfg(feature = "test-support")]
static RENDERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times an embedded card was drawn, for a test of what a page draws again.
#[cfg(feature = "test-support")]
pub fn embedded_card_renders() -> u64 {
    RENDERS.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) struct EmbeddedCard {
    pub(crate) view: AnyView,
    /// Whether its view was notified since it last drew, see [`CardFrame::on_notify`].
    ///
    /// [`CardFrame::on_notify`]: sound_ui::components::device_card::CardFrame::on_notify
    changed: Rc<Cell<bool>>,
    /// Its size as it last drew itself. The window lays out a view that it draws from its last
    /// frame at a size given before it draws.
    size: Rc<Cell<Option<Size<Pixels>>>>,
}

impl EmbeddedCard {
    pub(crate) fn new(view: AnyView, changed: Rc<Cell<bool>>) -> Self {
        Self {
            view,
            changed,
            size: Rc::default(),
        }
    }
}

/// The card `card` as a page draws it: from its last frame at its last size, or, the first time
/// and when it changed, laid out by what it holds, as a column added makes it wider.
pub(crate) fn draw(card: &Entity<EmbeddedCard>, cx: &App) -> AnyElement {
    let held = card.read(cx);
    match held.size.get().filter(|_| !held.changed.get()) {
        Some(size) => (card.clone())
            .cached(
                StyleRefinement::default()
                    .flex_none()
                    .w(size.width)
                    .h(size.height),
            )
            .into_any_element(),
        None => card.clone().into_any_element(),
    }
}

impl Render for EmbeddedCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "test-support")]
        RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.changed.set(false);
        let size = self.size.clone();
        // Its own size, however large the box the page gave it: a card that changed while its
        // view was not notified, as when a view inside it was, lays the page out again on the
        // next frame. Less than a pixel is the rounding of where it lies.
        let measures = canvas(
            move |bounds, window, _| {
                let last = size.replace(Some(bounds.size));
                if last.is_some_and(|last| {
                    (last.width - bounds.size.width).abs() >= px(1.)
                        || (last.height - bounds.size.height).abs() >= px(1.)
                }) {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        );
        div().flex().items_start().child(
            div()
                .flex_none()
                .relative()
                .child(self.view.clone())
                .child(measures.absolute().size_full()),
        )
    }
}
