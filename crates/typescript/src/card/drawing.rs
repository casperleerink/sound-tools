//! One draw of the nodes of a card into elements.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, ElementId, Empty, Entity, FocusHandle, MouseButton, Pixels,
    Point, ScrollHandle, SharedString, Size, StyledText, WeakEntity, Window, canvas, div,
    prelude::*, px,
};
use serde_json::Value;
use sound_ui::ActiveTheme;
use sound_ui::components::icon::Icon;
use sound_ui::components::text_input::{self, TextInput};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::focus::TabStop;

use crate::feeds::Feeds;
use crate::live::{DragNumbers, Sources, SpaceView};
use crate::tools::ToolInfo;
use crate::tree::{self, DivProps, IconProps, InputProps, NodeTable, Props, TextProps};

use super::controls::{Control, Keys};
use super::input::{Hears, Hovers, hears_hover, listener};
use super::shapes::SpaceIndex;
use super::sync::Embedded;
use super::{Lifted, TypeScriptCard};

/// What one draw of the card reads.
pub(super) struct Drawing<'a> {
    /// The tool, as it last loaded.
    pub(super) info: Option<ToolInfo>,
    /// The record, with each field it leaves out at its default.
    pub(super) state: Rc<Value>,
    pub(super) watches: BTreeMap<String, f32>,
    pub(super) played: Option<BTreeMap<String, f32>>,
    pub(super) tick: f64,
    pub(super) spaces: HashMap<String, SpaceView>,
    /// The card's own, which the paint of a space writes its size in.
    pub(super) laid_out: Rc<RefCell<HashMap<String, SpaceView>>>,
    pub(super) drag: Option<(String, DragNumbers)>,
    pub(super) hovered: Option<(String, u64)>,
    pub(super) table: &'a NodeTable,
    pub(super) feeds: &'a Feeds,
    pub(super) indexes: &'a RefCell<HashMap<String, SpaceIndex>>,
    pub(super) inputs: &'a HashMap<u64, (Entity<TextInput>, String)>,
    /// The keys of the card.
    pub(super) focus: &'a FocusHandle,
    /// The size of the window, which a dialog's backdrop covers.
    pub(super) viewport: Size<Pixels>,
    pub(super) embedded: &'a HashMap<u64, Embedded>,
    pub(super) placed: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    pub(super) hovers: Rc<RefCell<Hovers>>,
    pub(super) scrolls: &'a RefCell<HashMap<Vec<usize>, ScrollHandle>>,
    pub(super) scrolled_cards: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    pub(super) button_down: bool,
    /// The scroll of the div that scrolls what it draws now, the innermost.
    pub(super) scroller: Option<ScrollHandle>,
    /// Whether it draws the title bar's div, which the body leaves out.
    pub(super) in_title_bar: bool,
    /// Whether what it draws shows out of its page, in the title bar.
    pub(super) out_of_page: bool,
    /// Whether what it draws is inside a div with a tooltip.
    pub(super) under_tooltip: bool,
    /// Whether its page takes the keys: it hears them, or shows an overlay, for escape.
    pub(super) takes_keys: bool,
    /// The keys of what the page shows in the title bar, while the page hears keys.
    pub(super) title_focus: Option<FocusHandle>,
    /// The spaces the body leaves to the layer over the page, and where it lays them out.
    pub(super) lifting: Option<(&'a HashSet<u64>, &'a Rc<RefCell<HashMap<u64, Lifted>>>)>,
    pub(super) card: WeakEntity<TypeScriptCard>,
    pub(super) cx: &'a App,
}

impl Drawing<'_> {
    /// A callback of an element that the card hears, as `Context::listener` makes one.
    pub(super) fn listener<E: ?Sized>(
        &self,
        hear: impl Fn(&mut TypeScriptCard, &E, &mut Window, &mut Context<TypeScriptCard>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        listener(&self.card, hear)
    }

    pub(super) fn sources(&self) -> Sources<'_> {
        Sources {
            tick: self.tick,
            watches: &self.watches,
            state: &self.state,
            info: self.info.as_ref(),
            played: self.played.as_ref(),
            drag: (self.drag.as_ref()).map(|(id, numbers)| (id.as_str(), *numbers)),
            spaces: &self.spaces,
            hovered: (self.hovered.as_ref()).and_then(|(_, node)| self.table.shape_key(*node)),
            feeds: self.feeds,
        }
    }

    /// Draws the node in `slot`. `index` tells siblings apart, so every element that needs an
    /// id gets its own.
    pub(super) fn draw(&mut self, slot: usize, index: &[usize]) -> AnyElement {
        let table = self.table;
        let Some(entry) = table.get(slot) else {
            return div().into_any_element();
        };
        let id = entry.id;
        match &entry.props {
            Props::Text(TextProps { text }) => {
                SharedString::from(text.clone().unwrap_or_default()).into_any_element()
            }
            Props::Space(space) => self.space(slot, id, space, &entry.children, index),
            // A shape outside a space, or a marker, draws nothing.
            Props::Shape(_) | Props::Marker => div().into_any_element(),
            Props::Input(_) => match self.inputs.get(&id) {
                Some((input, _)) => {
                    let width = match &entry.props {
                        Props::Input(InputProps {
                            width: Some(width), ..
                        }) => *width,
                        _ => 160.0,
                    };
                    div().w(px(width)).child(input.clone()).into_any_element()
                }
                None => div().into_any_element(),
            },
            Props::Button(props) => self.button(id, props, index),
            Props::Toggle(props) => self.toggle(id, props, index),
            Props::Slider(props) => self.slider(props, index),
            Props::Segmented(props) => self.segmented(id, props, index),
            Props::Notice(props) => self.notice(id, props, index),
            Props::Overlay(props) => self.overlay(id, props, &entry.children, index),
            Props::Icon(IconProps { name, size, color }) => {
                let color = color.map_or(self.cx.theme().gray_950, |color| color.0);
                Icon::new(name.clone().unwrap_or_default())
                    .size(size.unwrap_or(16.0))
                    .color(color)
                    .into_any_element()
            }
            Props::Card(_) => {
                // Not where something may lie over it, which it would hide.
                let placing = !self.out_of_page && !table.lain_over(slot);
                let card = match self.embedded.get(&id) {
                    Some(Embedded {
                        view: Some(view),
                        placing: Some(placed),
                        ..
                    }) if placing => placed.room(view, &self.card),
                    Some(Embedded {
                        view: Some(view), ..
                    }) => view.clone().into_any_element(),
                    // A tool with no card shows its frame with its name.
                    Some(Embedded { frame, .. }) => frame.card().into_any_element(),
                    None => div().into_any_element(),
                };
                match self.scroller.clone() {
                    Some(scroller) => self.scrolled_card(id, card, scroller),
                    None => card,
                }
            }
            Props::Div(DivProps {
                key,
                style,
                on_press,
                disabled,
                cursor,
                tooltip,
                tab_stop,
                keys,
                on_key,
                on_release,
                on_click,
                on_hover,
                follow,
                carry,
                on_drop,
                title_bar,
            }) => {
                if *title_bar == Some(true) && !std::mem::take(&mut self.in_title_bar) {
                    return div().into_any_element();
                }
                let style = style.clone().unwrap_or_default();
                let disabled = *disabled == Some(true);
                let shown: Vec<usize> = (entry.children.iter())
                    .filter(|child| {
                        !matches!(
                            table.get(**child).map(|child| &child.props),
                            Some(Props::Marker)
                        )
                    })
                    .copied()
                    .collect();
                // Rows of fixed heights that follow a space down are drawn only where they show:
                // the heads of a hundred tracks cost what the dozen on screen cost.
                let rows = (follow.as_ref().and_then(|follow| follow.y.as_ref()))
                    .and_then(|key| self.spaces.get(key))
                    .zip(style.stacks_down())
                    .and_then(|(view, gap)| {
                        let heights: Option<Vec<f32>> = (shown.iter())
                            .map(|child| match table.get(*child).map(|child| &child.props) {
                                Some(Props::Div(child)) => {
                                    child.style.as_ref().and_then(|style| style.fixed_height())
                                }
                                _ => None,
                            })
                            .collect();
                        let top = (view.top * view.scale().1) as f32;
                        Some((heights?, gap, top..top + view.height as f32))
                    });
                let scroller = (style.scroll()).map(|_| {
                    (self.scrolls.borrow_mut().entry(index.to_vec()))
                        .or_default()
                        .clone()
                });
                let outer = (scroller.clone()).map(|scroller| self.scroller.replace(scroller));
                // A text it cuts off with "…" shows whole on hover, and only then. Under a div
                // with a tooltip, GPUI would show this one in its place, even empty.
                let own_tip = !self.under_tooltip && tooltip.is_none();
                let text = match (style.truncates() && own_tip, &shown[..]) {
                    (true, [child]) => match table.get(*child).map(|child| &child.props) {
                        Some(Props::Text(TextProps { text })) => {
                            Some(SharedString::from(text.clone().unwrap_or_default()))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let mut cut_text = text.clone().map(StyledText::new);
                let cut = text.zip(cut_text.as_ref().map(|styled| styled.layout().clone()));
                let mut children: Vec<AnyElement> = Vec::new();
                let mut hidden = None::<f32>;
                let mut at = 0.0;
                let under_tooltip = std::mem::replace(&mut self.under_tooltip, !own_tip);
                for (position, child) in shown.iter().enumerate() {
                    if let Some((heights, gap, showing)) = &rows {
                        let height = heights.get(position).copied().unwrap_or_default();
                        let shows = at < showing.end && at + height > showing.start;
                        at += height + gap;
                        if !shows {
                            // Their room, gaps between them included, as one spacer.
                            hidden = Some(hidden.map_or(height, |room| room + gap + height));
                            continue;
                        }
                    }
                    if let Some(room) = hidden.take() {
                        children.push(div().flex_none().h(px(room)).into_any_element());
                    }
                    children.push(match cut_text.take() {
                        Some(styled) => styled.into_any_element(),
                        None => self.draw(*child, &[index, &[position]].concat()),
                    });
                }
                self.under_tooltip = under_tooltip;
                if let Some(room) = hidden {
                    children.push(div().flex_none().h(px(room)).into_any_element());
                }
                if *on_hover == Some(true) {
                    let card = self.card.clone();
                    let hovered = hears_hover(id, self.hovers.clone(), card);
                    children.push(hovered.into_any_element());
                }
                if let Some(outer) = outer {
                    self.scroller = outer;
                }
                let element = match follow {
                    // Its children move with the scroll of the spaces it follows, cut off at its
                    // edges: the heads of the rows of a timeline that scrolls. They lie over it,
                    // so how tall they are never makes it taller: its style says its size.
                    Some(follow) => {
                        let scrolled = |key: &Option<String>, along: fn(&SpaceView) -> f64| {
                            key.as_ref()
                                .and_then(|key| self.spaces.get(key))
                                .map_or(0.0, along) as f32
                        };
                        let dx = scrolled(&follow.x, |view| view.left * view.scale().0);
                        let dy = scrolled(&follow.y, |view| view.top * view.scale().1);
                        let inside = style
                            .lines_up(div())
                            .absolute()
                            .left(px(-dx))
                            .top(px(-dy))
                            .when(follow.x.is_none(), |inside| inside.w_full())
                            .when(follow.y.is_none(), |inside| inside.h_full())
                            .children(children);
                        (style.apply(div(), disabled).overflow_hidden()).child(inside)
                    }
                    None => style.apply(div(), disabled).children(children),
                };
                // Tests find a div by its key.
                let element = match key {
                    Some(key) => {
                        let selector = format!("div-{key}");
                        element.debug_selector(move || selector)
                    }
                    None => element,
                };
                let pointer = Hears::of(*on_press, *on_release, *on_click);
                let hears =
                    !disabled && (pointer.any() || *on_drop == Some(true) || carry.is_some());
                let element = match (disabled, cursor) {
                    (true, _) => element.cursor_not_allowed(),
                    (false, Some(cursor)) => element.cursor(cursor.style()),
                    (false, None) if pointer.any() => element.cursor_pointer(),
                    (false, None) => element,
                };
                let pressed = style.pressed().filter(|_| !disabled);
                let own =
                    (*on_key == Some(true) && !disabled).then(|| keys.clone().unwrap_or_default());
                let stateful = hears || pressed.is_some() || tooltip.is_some() || cut.is_some();
                let inner = match stateful || own.is_some() {
                    false => element.into_any_element(),
                    true => {
                        let name = element_name("div", key.as_deref(), index);
                        let element = (element.id(ElementId::Name(name.clone().into())))
                            .when_some(pressed, |element, look| {
                                // The hitbox the pressed look needs, on a div that hears nothing.
                                element
                                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
                                    .active(move |style| look.refine(style))
                            })
                            .when_some(tooltip.clone(), |element, text| {
                                element.tooltip(move |_, cx| Tooltip::new(text.clone()).view(cx))
                            })
                            .when_some(cut, |element, (text, layout)| {
                                element.tooltip(move |_, cx| match layout.text() == *text {
                                    true => cx.new(|_| Nothing).into(),
                                    false => Tooltip::new(text.clone()).view(cx),
                                })
                            });
                        let element = match hears {
                            true => self.heard_div(element, id, pointer, carry, on_drop),
                            false => element,
                        };
                        let element = match &own {
                            Some(keys) => element.on_key_down(self.own_keys(id, keys.clone())),
                            None => element,
                        };
                        let click = hears && pointer.click;
                        match *tab_stop == Some(true) && (click || own.is_some()) {
                            true => {
                                let control = Control {
                                    keys: if click { Keys::Press } else { Keys::Own },
                                    menu: (hears && pointer.press).then_some(pointer),
                                };
                                let element = self.control(element, id, control);
                                TabStop::new(format!("tab-{name}"), element)
                                    .when(click, |stop| stop.on_click(self.clicked(id)))
                                    .into_any_element()
                            }
                            false => element.into_any_element(),
                        }
                    }
                };
                if let (Some(axis), Some(scroller)) = (style.scroll(), scroller) {
                    // The box it scrolls in has its size; it is as large as its children.
                    let name = element_name("scroll", key.as_deref(), index);
                    let scrolling = style.sized(div()).id(ElementId::Name(name.into()));
                    let scrolling = style.fade(scrolling, disabled).track_scroll(&scroller);
                    let scrolling = match axis {
                        tree::ScrollAxis::X => scrolling.overflow_x_scroll(),
                        tree::ScrollAxis::Y => scrolling.overflow_y_scroll(),
                    };
                    return scrolling.child(inner).into_any_element();
                }
                inner
            }
        }
    }

    /// A card in a div that scrolls, which writes where it lies in what scrolls. Out of view it
    /// is an empty box of its size: a rack of many effects costs what the cards on screen cost.
    fn scrolled_card(&self, node: u64, card: AnyElement, scroller: ScrollHandle) -> AnyElement {
        let showing = scroller.bounds();
        // The scroll of a wheel past the end, as its div clamps it when laid out.
        let offset = (scroller.offset())
            .max(&-scroller.max_offset())
            .min(&Point::default());
        let last = self.scrolled_cards.borrow().get(&node).copied();
        let hidden = last.filter(|last| {
            !self.button_down && !showing.is_empty() && !(*last + offset).intersects(&showing)
        });
        let shown = match hidden {
            Some(last) => div()
                .w(last.size.width)
                .h(last.size.height)
                .into_any_element(),
            None => card,
        };
        let cards = self.scrolled_cards.clone();
        let writes = canvas(
            move |bounds, window, _| {
                // It moved into view since the last frame, as when the cards were reordered.
                if hidden.is_some() && bounds.intersects(&scroller.bounds()) {
                    window.request_animation_frame();
                }
                cards.borrow_mut().insert(node, bounds - scroller.offset());
            },
            |_, _, _, _| {},
        );
        (div().flex().flex_none().relative())
            .child(shown)
            .child(writes.absolute().size_full())
            .into_any_element()
    }
}

/// Whether a text field has the keys.
pub(super) fn typing(window: &Window) -> bool {
    (window.context_stack().iter()).any(|context| context.contains(text_input::KEY_CONTEXT))
}

/// An id for an element, from its key, else from where it is in the tree.
pub(super) fn element_name(kind: &str, key: Option<&str>, index: &[usize]) -> String {
    match key {
        Some(key) => key.to_string(),
        None => {
            let path: Vec<String> = index.iter().map(usize::to_string).collect();
            format!("{kind}-{}", path.join("-"))
        }
    }
}

/// The tooltip of a text that fits: none.
struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
