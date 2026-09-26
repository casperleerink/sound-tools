//! Device card: one device of a rack, 192 pt tall. A 32 pt header, a body of two rows of 72 pt
//! and 16 pt under it. The body is a display at the left, 8 pt of air, then columns of cells,
//! 16 pt from each side of the card. So a card is 32 pt plus its display plus 8 plus 56 per
//! column wide. Expanding it shows the columns it hides to the right of a hairline: the card
//! gets wider and never taller.
//!
//! The header: the title at 16 pt from the left, which is the picker of the slot in a rack, and
//! at the right, 8 pt from the edge, icons in 24 pt targets 4 pt apart: expand, power and close,
//! each only when the owner gives it. Power off draws the glyph and the title muted and the body
//! at 40 %.
//!
//! The card knows no device. It shows what it is given and reports clicks; whether it is
//! expanded is the owner's interface state and whether it is on the owner's record.
//!
//! In a rack the view of the device draws the whole card, because it owns what the body shows,
//! and the rack gives it a [`CardFrame`]: the title, which is the picker of the slot, and the
//! power and close icons of an effect. Whether an effect is on is saved on its slot, which is
//! the rack's, so the frame reads it when the card draws.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, AnyView, App, ClickEvent, Div, ElementId, Entity, Hsla, MouseButton, SharedString,
    Stateful, StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::cell::{CELL_WIDTH, ROW_HEIGHT, VALUE_LINE};
use crate::components::icon::Icon;
use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;

pub const CARD_HEIGHT: f32 = 192.;
/// A card with no display and no cells: a plugin, or a slot with nothing to show.
pub const PLAIN_CARD_WIDTH: f32 = 200.;
/// The value line of the second row of cells, from the top of the body: where a card puts its
/// quiet line, such as `CLAP · <maker>`.
pub const BODY_VALUE_LINE: f32 = ROW_HEIGHT + VALUE_LINE;
pub const HEADER_HEIGHT: f32 = 32.;
/// From the sides of the card to its body, and from its left edge to the title.
pub const CARD_PADDING: f32 = 16.;
/// Between the display and the first column of cells.
pub const DISPLAY_GAP: f32 = 8.;
const ICON_TARGET: f32 = 24.;
const ICON_GLYPH: f32 = 12.;
const ICON_GAP: f32 = 4.;
/// The border of a card, inside its width and height: its header and body start this far in.
pub const BORDER: f32 = 1.;
/// The border is inside the width, so the padding is one point less than the room it makes.
const INSIDE: f32 = CARD_PADDING - BORDER;
/// Air on each side of the hairline before the hidden columns.
const HIDDEN_GAP: f32 = 8.;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
/// Whether a device is on, read when the card draws.
type IsOn = Rc<dyn Fn(&App) -> bool>;
/// What makes the header of a card a handle that drags it, see [`CardFrame::draggable`].
type Grip = Rc<dyn Fn(Stateful<Div>) -> Stateful<Div>>;

/// What a rack gives the view of the device in one of its slots, so that the view can draw the
/// whole card with [`CardFrame::card`] and add its display and cells.
#[derive(Clone)]
pub struct CardFrame {
    /// Tells this card from every other in the rack, so that two cards with controls of one
    /// name keep a drag and a focus each, and names its icons for tests: `<id>-close`.
    id: SharedString,
    /// The picker of the slot: a ghost trigger, which brings 8 pt of padding of its own.
    title: AnyView,
    power: Option<(IsOn, Rc<dyn Fn(&mut Window, &mut App)>)>,
    close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    grip: Option<Grip>,
}

impl CardFrame {
    pub fn new(id: impl Into<SharedString>, title: impl Into<AnyView>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            power: None,
            close: None,
            grip: None,
        }
    }

    /// Makes the header of the card a handle that drags it, as a rack reorders its cards: a
    /// press on the header that moves carries `value` to wherever the rack takes a drop of it,
    /// with `preview` under the pointer. A press that does not move is still a click on the
    /// title or an icon.
    pub fn draggable<T: Clone + 'static, W: Render>(
        mut self,
        value: T,
        preview: impl Fn(&T, &mut Window, &mut App) -> Entity<W> + 'static,
    ) -> Self {
        let preview = Rc::new(preview);
        self.grip = Some(Rc::new(move |header: Stateful<Div>| {
            let preview = preview.clone();
            header.on_drag(value.clone(), move |value, _, window, cx| {
                preview(value, window, cx)
            })
        }));
        self
    }

    /// The power icon, which bypasses the device. `is_on` is read every time the card draws,
    /// because the rack keeps whether a slot is on and the view of the device does not.
    pub fn power(
        mut self,
        is_on: impl Fn(&App) -> bool + 'static,
        on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.power = Some((Rc::new(is_on), Rc::new(on_toggle)));
        self
    }

    /// The close icon, which takes the device out of the rack.
    pub fn close(mut self, on_close: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.close = Some(Rc::new(on_close));
        self
    }

    /// A card with the id, the title, and the power and close icons of this frame. The view
    /// adds the rest.
    pub fn card(&self) -> DeviceCard {
        // The trigger brings its own padding, so it moves left by that much and its text lands
        // where a card title is.
        let title = div().flex().min_w_0().ml(px(-8.)).child(self.title.clone());
        let mut card = DeviceCard::new(ElementId::Name(self.id.clone()), title);
        card.grip = self.grip.clone();
        if let Some((is_on, toggle)) = self.power.clone() {
            card.power = Some((is_on, Box::new(move |_, window, cx| toggle(window, cx))));
        }
        match self.close.clone() {
            Some(close) => card.close(move |_, window, cx| close(window, cx)),
            None => card,
        }
    }
}

/// A column of a card: one cell in each of the two rows. Either may be empty.
#[derive(IntoElement, Default)]
pub struct Column {
    top: Option<AnyElement>,
    bottom: Option<AnyElement>,
}

impl Column {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn top(mut self, cell: impl IntoElement) -> Self {
        self.top = Some(cell.into_any_element());
        self
    }

    pub fn bottom(mut self, cell: impl IntoElement) -> Self {
        self.bottom = Some(cell.into_any_element());
        self
    }
}

impl RenderOnce for Column {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let row = |cell: Option<AnyElement>| div().h(px(ROW_HEIGHT)).children(cell);
        div()
            .flex_none()
            .w(px(CELL_WIDTH))
            .flex()
            .flex_col()
            .child(row(self.top))
            .child(row(self.bottom))
    }
}

/// An icon of the header, with the focus it keeps in element state.
struct IconState {
    focus_handle: gpui::FocusHandle,
    keyboard_focus: KeyboardFocus,
}

#[derive(IntoElement)]
pub struct DeviceCard {
    base: Div,
    id: ElementId,
    title: AnyElement,
    expand: Option<(bool, ClickHandler)>,
    power: Option<(IsOn, ClickHandler)>,
    close: Option<ClickHandler>,
    grip: Option<Grip>,
    display: Option<AnyElement>,
    columns: Vec<Column>,
    hidden: Vec<Column>,
    children: Vec<AnyElement>,
}

impl DeviceCard {
    /// `title` is what the header says, at 16 pt from the left. A picker whose trigger brings
    /// its own padding moves itself left by that much.
    pub fn new(id: impl Into<ElementId>, title: impl IntoElement) -> Self {
        Self {
            base: div(),
            id: id.into(),
            title: title.into_any_element(),
            expand: None,
            power: None,
            close: None,
            grip: None,
            display: None,
            columns: Vec::new(),
            hidden: Vec::new(),
            children: Vec::new(),
        }
    }

    /// The expand icon, and whether the card shows its hidden columns.
    pub fn expand(
        mut self,
        expanded: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.expand = Some((expanded, Box::new(on_click)));
        self
    }

    /// The power icon, and whether the device is on.
    pub fn power(
        mut self,
        on: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.power = Some((Rc::new(move |_| on), Box::new(on_click)));
        self
    }

    /// The close icon, which takes the device off.
    pub fn close(
        mut self,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.close = Some(Box::new(on_click));
        self
    }

    /// The display at the left of the body.
    pub fn display(mut self, display: impl IntoElement) -> Self {
        self.display = Some(display.into_any_element());
        self
    }

    pub fn column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }

    /// A column that shows only when the card is expanded.
    pub fn hidden_column(mut self, column: Column) -> Self {
        self.hidden.push(column);
        self
    }
}

impl Styled for DeviceCard {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// Anything else in the body, after the columns: the button of a plugin card, a quiet line.
impl ParentElement for DeviceCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// One icon of the header: a 24 pt target with a 12 pt glyph, `alpha/8` under the pointer.
fn header_icon(
    card: &ElementId,
    name: &'static str,
    glyph: &'static str,
    color: Hsla,
    on_click: ClickHandler,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = ElementId::NamedChild(Arc::new(card.clone()), SharedString::from(name));
    let state = window.use_keyed_state(id.clone(), cx, |_, cx| IconState {
        focus_handle: cx.focus_handle().tab_stop(true),
        keyboard_focus: KeyboardFocus::default(),
    });
    let focus_handle = state.read(cx).focus_handle.clone();
    let ring_shows = state
        .read(cx)
        .keyboard_focus
        .shows_ring(&focus_handle, window);
    let theme = cx.theme();
    let (hover, ring) = (theme.alpha_at(0.08), theme.lavender);
    // For tests, which find an icon by the card and its name: `<card>-power`.
    let card = card.clone();
    div()
        .id(id)
        .debug_selector(move || format!("{card}-{name}"))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(ICON_TARGET))
        .rounded(px(6.))
        .border(px(BORDER))
        .border_color(match ring_shows {
            true => ring,
            false => Hsla::transparent_black(),
        })
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .track_focus(&focus_handle)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
        })
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(Icon::new(glyph).size(ICON_GLYPH).color(color))
}

impl RenderOnce for DeviceCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border, text, hairline) = (
            theme.gray_200,
            theme.alpha_at(0.06),
            theme.gray_950,
            theme.alpha_at(0.06),
        );
        // Off is `gray-700`, the nearest grey with 3 : 1 on a card for an icon.
        let (glyph, glyph_off) = (theme.gray_950, theme.gray_700);
        let on = self.power.as_ref().is_none_or(|(is_on, _)| is_on(cx));
        let expanded = self.expand.as_ref().is_some_and(|(expanded, _)| *expanded);

        let mut icons = Vec::new();
        if let Some((expanded, on_click)) = self.expand {
            let chevron = if expanded {
                "chevron-left"
            } else {
                "chevron-right"
            };
            let icon = header_icon(&self.id, "expand", chevron, glyph, on_click, window, cx);
            icons.push(icon.into_any_element());
        }
        if let Some((_, on_click)) = self.power {
            let color = if on { glyph } else { glyph_off };
            let icon = header_icon(&self.id, "power", "power", color, on_click, window, cx);
            icons.push(icon.into_any_element());
        }
        if let Some(on_click) = self.close {
            let icon = header_icon(&self.id, "close", "x", glyph, on_click, window, cx);
            icons.push(icon.into_any_element());
        }

        // For tests, which find the header of a card by its id: `<card>-header`.
        let card = self.id.clone();
        let header = div()
            .id(ElementId::NamedChild(
                Arc::new(self.id.clone()),
                SharedString::from("header"),
            ))
            .debug_selector(move || format!("{card}-header"))
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .flex()
            .items_center()
            .pl(px(INSIDE))
            .pr(px(8. - 1.))
            .gap(px(ICON_GAP))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    // Off, the title is muted as the glyph of power is.
                    .when(!on, |title| title.opacity(0.6))
                    .child(self.title),
            )
            .children(icons);
        let header = match self.grip {
            Some(grip) => grip(header),
            None => header,
        };

        let hidden = (expanded && !self.hidden.is_empty()).then(|| {
            div()
                .flex()
                .child(div().mx(px(HIDDEN_GAP)).w(px(1.)).h_full().bg(hairline))
                .children(self.hidden)
        });
        let body = div()
            .flex_none()
            .h(px(ROW_HEIGHT * 2.))
            .flex()
            .px(px(INSIDE))
            .when(!on, |body| body.opacity(0.4))
            .children(
                self.display
                    .map(|display| div().flex_none().mr(px(DISPLAY_GAP)).child(display)),
            )
            .children(self.columns)
            .children(hidden)
            .children(self.children);

        // The id scopes what the controls in the card keep, so two cards with controls of one
        // name, such as two filters, keep a focus and a drag each.
        self.base
            .id(self.id)
            .flex_none()
            .flex()
            .flex_col()
            .h(px(CARD_HEIGHT))
            .rounded(px(10.))
            .bg(background)
            .border(px(BORDER))
            .border_color(border)
            .text_color(text)
            .child(header)
            .child(body)
    }
}
