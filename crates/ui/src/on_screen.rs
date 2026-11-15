//! What a window shows, by name, for tests and `sound-tools --look`: [`look`] draws one whole
//! frame, during which each named part writes down where it is. Other frames write nothing.

use std::cell::RefCell;
use std::fmt::Write as _;

use gpui::{
    AbsoluteLength, App, Bounds, ParentElement, Pixels, Point, SharedString, Styled, Window,
    canvas, point, px, rems,
};

thread_local! {
    static LOOKING: RefCell<Option<Vec<Seen>>> = const { RefCell::new(None) };
}

/// One part of the window as the last look saw it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    /// `<kind>-<key>`, such as `knob-cutoff` or `shape-clip-t01/clip`. Empty for a text with no
    /// name of its own.
    pub name: SharedString,
    pub text: Option<SharedString>,
    /// Such as `on` for a toggle, or `disabled`.
    pub state: Option<SharedString>,
    /// What of it shows: cut by the parts that hold it, empty when scrolled away.
    pub bounds: Bounds<Pixels>,
    /// All of it, what shows or not.
    pub whole: Bounds<Pixels>,
    /// For a space: where its units are in the window.
    pub units: Option<Units>,
}

impl Seen {
    pub fn named(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn state(mut self, state: impl Into<SharedString>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Placed at `whole`, of which `shown` shows.
    pub fn at(mut self, whole: Bounds<Pixels>, shown: Bounds<Pixels>) -> Self {
        self.bounds = whole.intersect(&shown);
        self.whole = whole;
        self
    }

    /// Whether a point or more of it is cut off: a text a little taller than its box is not.
    pub fn is_cut(&self) -> bool {
        let (whole, shown) = (self.whole.size, self.bounds.size);
        whole.width - shown.width >= px(1.) || whole.height - shown.height >= px(1.)
    }

    pub fn shows(&self) -> bool {
        !self.bounds.is_empty()
    }

    /// The middle of what shows of it, where a click lands.
    pub fn middle(&self) -> Point<Pixels> {
        match self.shows() {
            true => self.bounds.center(),
            false => self.whole.center(),
        }
    }
}

impl From<String> for Seen {
    fn from(name: String) -> Self {
        Self::named(name)
    }
}

impl From<SharedString> for Seen {
    fn from(name: SharedString) -> Self {
        Self::named(name)
    }
}

impl From<&'static str> for Seen {
    fn from(name: &'static str) -> Self {
        Self::named(name)
    }
}

/// The units of a space: what point of the window a place in them is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Units {
    /// The window's point of the space's top left corner.
    pub origin: Point<Pixels>,
    /// The units at that corner.
    pub left: f64,
    pub top: f64,
    /// Points per unit.
    pub across: f64,
    pub down: f64,
}

impl Units {
    pub fn point(&self, (x, y): (f64, f64)) -> Point<Pixels> {
        self.origin
            + point(
                px(((x - self.left) * self.across) as f32),
                px(((y - self.top) * self.down) as f32),
            )
    }

    pub fn at(&self, position: Point<Pixels>) -> (f64, f64) {
        let offset = position - self.origin;
        (
            self.left + f64::from(f32::from(offset.x)) / self.across,
            self.top + f64::from(f32::from(offset.y)) / self.down,
        )
    }
}

/// Whether a look is drawing: only then is anything written down.
pub fn looking() -> bool {
    LOOKING.with_borrow(Option::is_some)
}

/// Writes `seen` down, while a look is drawing.
pub fn record(seen: Seen) {
    LOOKING.with_borrow_mut(|looking| {
        if let Some(looking) = looking {
            looking.push(seen);
        }
    });
}

pub trait Named: ParentElement + Styled + Sized {
    /// Writes this part down as `seen` says, while a look draws. Takes a closure, so a normal
    /// frame formats no name. Call it after the border is set: its place has the border in it.
    fn seen<S: Into<Seen>>(mut self, seen: impl FnOnce() -> S) -> Self {
        if !looking() {
            return self;
        }
        let seen = seen().into();
        // Laid out over its parent and out over its border, so it has the parent's place and
        // changes no layout.
        let border = self.style().border_widths.clone();
        let out = |width: Option<AbsoluteLength>| match width {
            Some(AbsoluteLength::Pixels(width)) => AbsoluteLength::Pixels(-width),
            Some(AbsoluteLength::Rems(width)) => AbsoluteLength::Rems(rems(-width.0)),
            None => AbsoluteLength::Pixels(px(0.)),
        };
        let marker = canvas(
            move |bounds, window, _| record(seen.at(bounds, window.content_mask().bounds)),
            |_, (), _, _| {},
        )
        .absolute()
        .top(out(border.top))
        .right(out(border.right))
        .bottom(out(border.bottom))
        .left(out(border.left));
        self.child(marker)
    }
}

impl<E: ParentElement + Styled> Named for E {}

/// Draws a whole frame of `window` and gives what it showed, in the order it was painted: the
/// last of a name is the one on top.
pub fn look(window: &mut Window, cx: &mut App) -> Vec<Seen> {
    LOOKING.set(Some(Vec::new()));
    // A cached view draws only when it is told it changed.
    window.refresh();
    window.draw(cx).clear(cx);
    // The next frame is drawn as normal, to show what this one drew.
    window.refresh();
    LOOKING.take().unwrap_or_default()
}

/// The part named `name` that was drawn last, so on top: of those that show, if any does. A
/// part with no such name is found by its text, such as a label of a space with no key. One
/// scrolled out of view is drawn with nothing that shows.
pub fn find<'a>(seen: &'a [Seen], name: &str) -> Option<&'a Seen> {
    let named = |seen: &&Seen| seen.name == name;
    let said = |seen: &&Seen| seen.text.as_ref().is_some_and(|text| text == name);
    let on_top = |by: &dyn Fn(&&Seen) -> bool| {
        let mut found = seen.iter().rev().filter(|seen| by(seen));
        let last = found.clone().next();
        found.find(|seen| seen.shows()).or(last)
    };
    on_top(&named).or_else(|| on_top(&said))
}

/// One line per part that shows, top to bottom: its name, its text and state, where it is, and
/// what is wrong with how it shows, such as a text over another.
pub fn listing(seen: &[Seen]) -> String {
    let mut shown: Vec<&Seen> = (seen.iter()).filter(|seen| seen.shows()).collect();
    let key = |seen: &Seen| (f32::from(seen.bounds.top()), f32::from(seen.bounds.left()));
    shown.sort_by(|a, b| {
        let (a, b) = (key(a), key(b));
        a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
    });
    let mut out = String::new();
    for (index, each) in shown.iter().enumerate() {
        let Bounds { origin, size } = each.bounds;
        let mut line = String::new();
        if !each.name.is_empty() {
            line.push_str(&each.name);
            line.push(' ');
        }
        if let Some(text) = &each.text {
            write!(line, "{text:?} ").ok();
        }
        if let Some(state) = &each.state {
            write!(line, "[{state}] ").ok();
        }
        let number = |pixels: Pixels| f32::from(pixels).round();
        write!(
            line,
            "at {},{} size {}x{}",
            number(origin.x),
            number(origin.y),
            number(size.width),
            number(size.height)
        )
        .ok();
        if each.is_cut() {
            line.push_str(" cut");
        }
        if let Some(units) = &each.units {
            let (left, top) = units.at(each.bounds.origin);
            let (right, bottom) = units.at(each.bounds.bottom_right());
            write!(
                line,
                " units x {left:.4}..{right:.4} y {top:.4}..{bottom:.4}"
            )
            .ok();
        }
        // Two plain texts over each other are a layout gone wrong; a tooltip or a menu over
        // a text is not.
        let plain = |seen: &Seen| seen.name.is_empty() && seen.text.is_some();
        if plain(each) {
            // A div's text inside another div's is a part of it.
            let inside = |a: &Bounds<Pixels>, b: &Bounds<Pixels>| a.intersect(b) == *a;
            let over = (shown.iter().enumerate())
                .filter(|(other, seen)| *other != index && plain(seen))
                .find(|(_, seen)| {
                    let (a, b) = (&seen.bounds, &each.bounds);
                    a.intersects(b) && !inside(a, b) && !inside(b, a)
                });
            if let Some((_, other)) = over {
                write!(line, " overlaps {:?}", other.text.as_deref().unwrap_or("")).ok();
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn text(text: &str, (x, y, width, height): (f32, f32, f32, f32)) -> Seen {
        let whole = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
        Seen::named("").text(text.to_string()).at(whole, whole)
    }

    #[test]
    fn the_listing_tells_texts_that_cross_and_cut_texts_from_ones_inside_others() {
        let mut cut = text("Long name", (0., 100., 80., 18.));
        cut.bounds.size.width = px(60.);
        let seen = [
            text("Bar 1", (0., 0., 40., 16.)),
            text("Bar 2", (30., 0., 40., 16.)),
            text("Track", (0., 40., 100., 20.)),
            text("T", (10., 42., 10., 16.)),
            Seen::named("tooltip").text("Play").at(
                Bounds::new(point(px(5.), px(5.)), size(px(50.), px(20.))),
                Bounds::new(point(px(5.), px(5.)), size(px(50.), px(20.))),
            ),
            cut,
        ];
        let listing = listing(&seen);
        let line = |start: &str| {
            listing
                .lines()
                .find(|line| line.starts_with(start))
                .unwrap()
        };
        assert!(
            line("\"Bar 1\"").ends_with("overlaps \"Bar 2\""),
            "{listing}"
        );
        assert!(!line("\"Track\"").contains("overlaps"), "{listing}");
        assert!(!line("tooltip").contains("overlaps"), "{listing}");
        assert!(line("\"Long name\"").contains(" cut"), "{listing}");
        assert!(!line("\"Bar 2\"").contains(" cut"), "{listing}");
    }
}
