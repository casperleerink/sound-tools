//! A view's nodes as Rune elements.

use std::cell::Cell;
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use rune::{Align, El, Phase, ScrollPos, ScrollSpec, col, el, memo, row, text};
use sound_core::InstanceId;
use sound_typescript::live::Num;
use sound_typescript::window::Drawn;

use crate::app::State;
use crate::lives::{Lives, Shown, Shows};

use super::field::{kept, typed};
use super::semantics::{delegates_enter, semantics};
use super::shapes::drawing;
use super::{BoxNode, Node, Pad, Place, Scope, Scroll, Size, TextNode, live_place, plane};

/// The name of every binding the views shown now make, of their memos too, whether Rune builds
/// them again or not: what [`crate::lives::Lives::keep`] keeps.
pub(crate) fn bound(state: &State) -> HashSet<String> {
    let mut named = HashSet::new();
    for id in &state.shown {
        if let Some(node) = state.root(id) {
            let prefix = state.prefix_of(id);
            walk(&Scope::new(state, id, &prefix), &node, &mut |shows| {
                named.insert(Lives::name(id, &shows));
            });
        }
    }
    named
}

/// The keys of the elements of `node` and its memos that a paint enters, see [`BoxNode::enter`].
pub(crate) fn enterable(state: &State, id: &InstanceId, node: &Node, keys: &mut Vec<String>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            keys.extend(b.key.clone().filter(|_| b.enter));
            for child in &b.children {
                enterable(state, id, child, keys);
            }
        }
        Node::Memo { key } => {
            if let Some((_, inner)) = state.memo_of(id, key) {
                enterable(state, id, &inner, keys);
            }
        }
        Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Icon(_) | Node::Child { .. } => {}
    }
}

/// The keys of the boxes of `node` and its memos.
pub(crate) fn keyed(state: &State, id: &InstanceId, node: &Node, keys: &mut Vec<String>) {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            keys.extend(b.key.clone());
            for child in &b.children {
                keyed(state, id, child, keys);
            }
        }
        Node::Memo { key } => {
            if let Some((_, inner)) = state.memo_of(id, key) {
                keyed(state, id, &inner, keys);
            }
        }
        Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Icon(_) | Node::Child { .. } => {}
    }
}

/// Gives `bind` everything `node` binds, and its memos.
fn walk(scope: &Scope, node: &Node, bind: &mut impl FnMut(Shows)) {
    let live: Vec<Shows> = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            let plane = b.plane.as_ref().zip(b.key.as_deref());
            let inside = Scope {
                plane: plane.map(|(_, key)| scope.key(key)).or(scope.plane.clone()),
                parent: match plane {
                    Some(_) => None,
                    None => b.parent_of_children(scope.parent.as_ref()),
                },
                ..scope.clone()
            };
            for child in &b.children {
                walk(&inside, child, bind);
            }
            let placed = b
                .place
                .as_ref()
                .and_then(|place| scope.placed_shows(place, b.size(), b.inside()));
            (placed.into_iter())
                .chain(live_place(b.at.as_ref(), b.size(), b.opacity.as_ref()))
                .collect()
        }
        Node::Text(t) => {
            if let Some(value) = &t.live {
                bind(t.shows(value));
            }
            live_place(t.at.as_ref(), (None, None), t.opacity.as_ref())
        }
        Node::Shapes(d) => {
            if let Some(trim) = d
                .trim
                .as_ref()
                .filter(|trim| trim.iter().any(|t| t.fixed().is_none()))
            {
                bind(Shows::Trim(trim.clone()));
            }
            if let Some(listed) = super::shapes::listed(d, scope.plane.as_deref()) {
                bind(Shows::Lists(listed));
            }
            let moving = super::shapes::moving(d);
            if !moving.is_empty() {
                bind(Shows::Drawn(moving));
            }
            let placed = (d.place.as_ref())
                .and_then(|place| scope.placed_shows(place, (Some(&d.w), Some(&d.h)), d.inside()));
            let size = d.place.is_none().then_some((Some(&d.w), Some(&d.h)));
            (placed.into_iter())
                .chain(live_place(
                    d.at.as_ref(),
                    size.unwrap_or_default(),
                    d.opacity.as_ref(),
                ))
                .collect()
        }
        Node::Memo { key } => {
            if let Some(inner) = scope.memo(key) {
                walk(scope, &inner, bind);
            }
            return;
        }
        Node::Child { .. } | Node::Field(_) | Node::Icon(_) => return,
    };
    live.into_iter().for_each(bind);
}

/// What a device or the page whose tool's code did not load shows, where it sits.
pub(crate) fn code_failed(state: &State, id: &InstanceId) -> El {
    let what = match Some(id) == state.main() {
        true => "page",
        false => "device",
    };
    text(format!(
        "This {what}'s code has a mistake, so it shows nothing until it is mended."
    ))
    .key(format!("{}code-failed", state.prefix_of(id)))
    .font(state.theme.text("label"))
    .color(state.theme.color("text-secondary"))
    .wrap()
}

/// The view `drawn` of `instance`, with its keys after `prefix`.
pub(crate) fn view(state: &State, instance: &InstanceId, prefix: &str, drawn: &Drawn) -> El {
    match drawn {
        Drawn::View { .. } => {
            let scope = Scope::new(state, instance, prefix);
            match state.read_root(instance) {
                Some(Ok(node)) => build(&scope, &node),
                Some(Err(error)) => {
                    failed(state, &format!("its view sent what is no view: {error}"))
                }
                None => el(),
            }
        }
        Drawn::Failed(error) => failed(state, error),
        Drawn::None => el(),
        Drawn::NoCard(why) => text(why.clone())
            .font(state.theme.text("label"))
            .color(state.theme.color("text-secondary"))
            .wrap(),
    }
}

pub(crate) fn failed(state: &State, message: &str) -> El {
    text(format!("This view does not draw: {message}"))
        .font(state.theme.text("label"))
        .color(state.theme.color("peach"))
        .wrap()
}

pub(super) fn build(scope: &Scope, node: &Node) -> El {
    match node {
        Node::Row(b) => boxed(scope, row(), b),
        Node::Col(b) => boxed(scope, col(), b),
        Node::Plain(b) => boxed(scope, el(), b),
        Node::Text(t) => label(scope, t),
        Node::Shapes(shapes) => {
            let e = drawing(scope, shapes);
            match &shapes.place {
                Some(place) => {
                    let size = (Some(&shapes.w), Some(&shapes.h));
                    scope.put(e, place, size, shapes.inside())
                }
                None => e,
            }
        }
        Node::Field(field) => typed(scope, field),
        Node::Icon(icon) => super::icon::drawn(scope, icon),
        Node::Child { path } => match scope.instance.child(path) {
            Ok(child) => match scope.state.drawn(&child) {
                _ if scope.state.code_failed(&child) => code_failed(scope.state, &child),
                Some(drawn) => view(scope.state, &child, &scope.state.prefix_of(&child), drawn),
                None => el(),
            },
            Err(error) => failed(scope.state, &error.to_string()),
        },
        Node::Memo { key } => match scope.state.memo_of(scope.instance, key) {
            // A plane builds what shows of it as it scrolls and zooms, which the memo does not say.
            Some((_, node)) if holds_plane(scope, &node) => build(scope, &node),
            Some((version, node)) => {
                // Rune keys a memo's root with the memo's key: the root keeps its own.
                let root = node.key().unwrap_or(key);
                let version = (version, drawn_now(scope, &node));
                memo(scope.key(root), version, || build(scope, &node))
            }
            None => el(),
        },
    }
}

/// The live numbers of the drawings of `node` and its memos as they are now: what a memo that
/// holds them builds again for as they move.
pub(super) fn drawn_now(scope: &Scope, node: &Node) -> u64 {
    fn walk(scope: &Scope, node: &Node, into: &mut DefaultHasher) {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                b.children.iter().for_each(|child| walk(scope, child, into));
            }
            Node::Shapes(d) => {
                for num in super::shapes::moving(d) {
                    let state = scope.state;
                    state
                        .lives
                        .value(state, scope.instance, &num)
                        .to_bits()
                        .hash(into);
                }
            }
            Node::Memo { key } => {
                if let Some(inner) = scope.memo(key) {
                    walk(scope, &inner, into);
                }
            }
            _ => {}
        }
    }
    let mut hasher = DefaultHasher::new();
    walk(scope, node, &mut hasher);
    hasher.finish()
}

pub(super) fn holds_plane(scope: &Scope, node: &Node) -> bool {
    match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
            b.plane.is_some() || b.children.iter().any(|child| holds_plane(scope, child))
        }
        Node::Memo { key } => (scope.state.memo_of(scope.instance, key))
            .is_some_and(|(_, inner)| holds_plane(scope, &inner)),
        _ => false,
    }
}

fn boxed(scope: &Scope, mut e: El, b: &BoxNode) -> El {
    let theme = &scope.state.theme;
    let key = b.key.as_deref().map(|key| scope.key(key));
    if let Some(key) = &key {
        e = e.key(key.clone());
    }
    e = match &b.w {
        Some(Size::Points(Num::Fixed(w))) => e.w(*w),
        Some(Size::Full(_)) => e.w_full(),
        _ => e,
    };
    e = match &b.h {
        Some(Size::Points(Num::Fixed(h))) => e.h(*h),
        Some(Size::Full(_)) => e.h_full(),
        _ => e,
    };
    if let Some(grow) = b.grow {
        // It takes the room left over and no more: without the floor, what it holds would
        // widen it and push what follows out of its row.
        e = e.grow(grow).min_w(0.0);
    }
    if b.shrink {
        e = e.shrink();
    }
    match b.pad {
        Some(Pad::All(step)) => e = e.padding(theme.space(step)),
        Some(Pad::Sides([across, down])) => e = e.px(theme.space(across)).py(theme.space(down)),
        None => {}
    }
    if let Some(step) = b.gap {
        e = e.gap(theme.space(step));
    }
    match b.align.as_deref() {
        Some("start") => e = e.items_start(),
        Some("center") => e = e.items_center(),
        Some("end") => e = e.items_end(),
        _ => {}
    }
    match b.justify.as_deref() {
        Some("center") => e = e.justify_center(),
        Some("between") => e = e.justify_between(),
        Some("end") => e = e.justify_end(),
        _ => {}
    }
    // Before `inset`, which pins it to more edges than `at` does.
    e = match &b.place {
        Some(place) => scope.put(e, place, b.size(), b.inside()),
        None => e,
    };
    e = scope.placed(e, b.at.as_ref(), b.size(), b.opacity.as_ref());
    if let (Some([left, top, right, bottom]), None) = (b.inset, &b.place) {
        e = e.pin(left, top, right, bottom);
    }
    if b.clip {
        // A rounded box cuts what it holds at its corners too.
        e = match b.radius.as_deref().and_then(|name| theme.radius(name)) {
            Some(radius) if radius > 0.0 => e.clip_rounded(radius),
            _ => e.clip_soft(0.0),
        };
    }
    if let Some(h) = b.max_h {
        e = e.max_h(h);
    }
    let mut bars = Vec::new();
    if let Some(scroll) = b.scroll {
        let thumbs = [rune::Live::new([0.0; 4]), rune::Live::new([0.0; 4])];
        // Where it is scrolled, and where it was when a thumb's drag began.
        let scrolled_at: Rc<Cell<Option<ScrollPos>>> = Rc::default();
        e = scrolled(e, scroll).on_scroll_offset({
            let (thumbs, scrolled_at) = (thumbs.clone(), scrolled_at.clone());
            move |at: ScrollPos| {
                for (i, thumb) in thumbs.iter().enumerate() {
                    thumb.set(thumb_at(&at, i));
                }
                scrolled_at.set(Some(at));
            }
        });
        let colour = theme.color("text-muted");
        let key = b.key.as_deref().map(|key| scope.key(key));
        bars.extend(thumbs.iter().enumerate().map(|(i, thumb)| {
            let bar = el()
                .absolute(0.0, 0.0)
                .live_rect(thumb)
                .radius(THUMB / 2.0)
                .fill(colour);
            match &key {
                Some(key) => thumb_drag(bar, key.clone(), i, scrolled_at.clone()),
                None => bar.inert(),
            }
        }));
    }
    if b.inert {
        e = e.inert();
    }
    if let Some(colour) = &b.fill {
        e = e.fill(theme.color(colour));
    }
    if let Some(colour) = &b.hover {
        e = super::drop::over(scope, e.hover(theme.color(colour)), b);
    }
    if b.hover_group {
        e = e.hover_group();
    }
    if b.reveal {
        e = e.group_opacity(0.0, 1.0);
    }
    if b.group {
        e = super::reach::group(scope, e, b);
    }
    if let Some(colour) = &b.border {
        e = match b.border_sides {
            Some(sides) => e.border_sides(sides, theme.color(colour)),
            None => e.border(1.0, theme.color(colour)),
        };
    }
    if let Some(radius) = b.radius.as_deref().and_then(|name| theme.radius(name)) {
        e = e.radius(radius);
    }
    if let Some(shadow) = b.shadow.as_deref().and_then(|name| theme.shadow(name)) {
        e = e.shadow(shadow.y, shadow.blur, theme.color(&shadow.color));
    }
    let plane = b.plane.as_ref().zip(key.as_deref());
    let here = Scope {
        plane: plane
            .map(|(_, key)| key.to_string())
            .or(scope.plane.clone()),
        placed: b.place.as_ref().map(Place::origin).or(scope.placed.clone()),
        menu: (b.opens_menu().then(|| key.clone()).flatten()).or(scope.menu.clone()),
        ..scope.clone()
    };
    e = semantics(&here, e, b, key.as_deref());
    if let Some((spec, key)) = plane {
        return plane::plane(&here, e, b, (key, spec), scope.plane.as_deref());
    }
    let inside = Scope {
        in_control: scope.in_control || delegates_enter(b),
        parent: b.parent_of_children(scope.parent.as_ref()),
        ..here
    };
    if inside.plane.is_some() {
        return e.children(plane::children(&inside, &b.children));
    }
    let shown = (b.children.iter()).filter(|child| !is_overlay(child) && kept(scope, child));
    e.children(shown.map(|child| build(&inside, child)))
        .children(bars)
}

pub(super) fn is_overlay(node: &Node) -> bool {
    matches!(node, Node::Row(b) | Node::Col(b) | Node::Plain(b) if b.overlay.is_some())
}

/// `e` scrolling along `axes`, its edges fading where more waits.
pub(crate) fn scrolled(e: El, axes: Scroll) -> El {
    let e = match axes {
        Scroll::X => e.scroll_x(),
        Scroll::Y => e.scroll(ScrollSpec {
            stick_bottom: false,
        }),
        Scroll::Both => e.scroll_xy(),
    };
    e.scroll_edge_fade(FADE)
}

/// How far the edge of a scroll fades, in points.
const FADE: f32 = 24.0;
/// How thick a scroll bar's thumb is, and how far from the edge.
pub(super) const THUMB: f32 = 4.0;
pub(super) const THUMB_EDGE: f32 = 2.0;

/// `bar`, the thumb along axis `i` of the scroll `key`, scrolling it as it drags: the whole
/// of what scrolls is the length of the bar.
fn thumb_drag(bar: El, key: String, i: usize, at: Rc<Cell<Option<ScrollPos>>>) -> El {
    let from = Rc::new(Cell::new(0.0_f32));
    bar.on_drag(
        move |_: &mut State, d: &mut rune::Drag, tx: &mut rune::Tx| {
            let Some(now) = at.get() else {
                return;
            };
            match d.phase {
                Phase::Down => from.set(now.offset[i]),
                Phase::Move => {
                    let whole = now.max[i] + now.viewport[i];
                    let moved = [d.dx, d.dy][i] * whole / now.viewport[i].max(1.0);
                    let to = (from.get() + moved).clamp(0.0, now.max[i]);
                    match i {
                        0 => tx.scroll_to_x(key.clone(), to),
                        _ => tx.scroll_to_y(key.clone(), to),
                    }
                }
                Phase::Up => {}
            }
        },
    )
}

/// Where the thumb of the scroll bar along axis `i` of a scroll at `at` is, in the points of
/// what scrolls, so it stays at the edge: nothing when all of it shows.
fn thumb_at(at: &ScrollPos, i: usize) -> [f32; 4] {
    let whole = at.max[i] + at.viewport[i];
    if at.max[i] <= 0.5 || whole <= 0.0 {
        return [0.0; 4];
    }
    let along = at.offset[i] + at.offset[i] / whole * at.viewport[i];
    let long = at.viewport[i] / whole * at.viewport[i];
    let across = at.offset[1 - i] + at.viewport[1 - i] - THUMB - THUMB_EDGE;
    match i {
        0 => [along, across, long, THUMB],
        _ => [across, along, THUMB, long],
    }
}

fn label(scope: &Scope, t: &TextNode) -> El {
    let theme = &scope.state.theme;
    let lives = &scope.state.lives;
    let live =
        (t.live.as_ref()).map(|value| lives.bind(scope.state, scope.instance, t.shows(value)));
    let words = match &live {
        Some(Shown::Text(_, widest)) => widest.borrow().clone(),
        _ => t.text.clone(),
    };
    let mut e = text(words)
        .font(theme.text(t.style.as_deref().unwrap_or("body")))
        .color(theme.color(t.color.as_deref().unwrap_or("text")));
    if let Some(key) = &t.key {
        e = e.key(scope.key(key));
    }
    if let Some(w) = t.w {
        e = e.w(w);
    }
    match t.align.as_deref() {
        Some("center") => e = e.align(Align::Center),
        Some("end") => e = e.align(Align::End),
        _ => {}
    }
    if t.truncate {
        e = e.truncate().help_when_truncated(t.text.clone());
    }
    if let Some(live) = live {
        e = live.apply(e);
    }
    scope.placed(e, t.at.as_ref(), (None, None), t.opacity.as_ref())
}
