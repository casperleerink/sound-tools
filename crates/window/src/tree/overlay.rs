//! Boxes that show over everything: menus a press opens and tips a hover shows.

use rune::{Align, El, Side, el};

use crate::app::State;

use super::build::build;
use super::semantics::delegates_enter;
use super::{Along, BoxNode, Edge, Node, Opens, Overlay, Scope, Within};

impl BoxNode {
    /// The overlay among its children that `open` opens.
    pub(crate) fn overlay_of(&self, open: Opens) -> Option<&BoxNode> {
        self.children.iter().find_map(|child| match child {
            Node::Row(b) | Node::Col(b) | Node::Plain(b)
                if b.overlay.is_some_and(|overlay| overlay.open == open) =>
            {
                Some(b)
            }
            _ => None,
        })
    }

    /// Whether a press in the overlay it is closes it, as a pick from a menu does.
    pub(crate) fn picks(&self) -> bool {
        matches!(self.role.as_deref(), Some("menu" | "listbox"))
    }
}

/// The overlays of the views shown: they draw over everything, so they are laid at the root
/// of the window, beside what opens them. A press overlay is there only while open.
pub(crate) fn overlays(state: &State) -> Vec<El> {
    let mut layer = Vec::new();
    for id in &state.shown {
        let Some(node) = state.root(id) else {
            continue;
        };
        let prefix = state.prefix_of(id);
        let scope = Scope::new(state, id, &prefix);
        overlays_in(&scope, &node, None, &mut layer);
    }
    layer
}

/// The overlays inside `node`. `owner` is the control whose Enter opens what a part of it opens:
/// the keys go back to it when that closes.
fn overlays_in(scope: &Scope, node: &Node, owner: Option<&str>, layer: &mut Vec<El>) {
    let b = match node {
        Node::Row(b) | Node::Col(b) | Node::Plain(b) => b,
        Node::Memo { key } => {
            if let Some(inner) = scope.memo(key) {
                overlays_in(scope, &inner, owner, layer);
            }
            return;
        }
        _ => return,
    };
    let own = (b.key.as_ref())
        .filter(|_| delegates_enter(b))
        .map(|key| scope.key(key));
    let owner = own.as_deref().or(owner);
    for child in &b.children {
        let (Node::Row(o) | Node::Col(o) | Node::Plain(o), Some(opener)) = (child, &b.key) else {
            overlays_in(scope, child, owner, layer);
            continue;
        };
        let Some(spec) = o.overlay else {
            overlays_in(scope, child, owner, layer);
            continue;
        };
        let opener = scope.key(opener);
        let (side, align) = spec.place();
        match spec.open {
            Opens::Press if scope.state.open.contains(&opener) => {
                let inside = Scope {
                    within: Some(Within {
                        opener: opener.clone(),
                        picks: o.picks(),
                    }),
                    in_control: false,
                    ..scope.clone()
                };
                let close = opener.clone();
                layer.push(
                    build(&inside, child)
                        .anchored(&opener, side, align, spec.gap)
                        .modal(true, move |s: &mut State| s.close_overlay(&close))
                        .focus_return(owner.unwrap_or(&opener)),
                );
                overlays_in(&inside, child, None, layer);
            }
            Opens::Menu if scope.state.open.contains(&opener) => {
                let inside = Scope {
                    within: Some(Within {
                        opener: opener.clone(),
                        picks: o.picks(),
                    }),
                    in_control: false,
                    ..scope.clone()
                };
                let close = opener.clone();
                let menu = build(&inside, child)
                    .modal(true, move |s: &mut State| s.close_overlay(&close))
                    .focus_return(opener.clone());
                let state = scope.state;
                let at = (state.menu.as_ref()).filter(|menu| menu.opener == opener);
                match at.and_then(|menu| menu.at) {
                    // At the pointer: kept in the window as any overlay is.
                    Some([x, y]) => {
                        let anchor = format!("{opener}-menu-at");
                        layer.push(el().key(anchor.clone()).absolute(x, y).w(0.0).h(0.0));
                        layer.push(menu.anchored(&anchor, Side::Bottom, Align::Start, 0.0));
                    }
                    None => layer.push(menu.anchored(&opener, side, align, spec.gap)),
                }
                overlays_in(&inside, child, None, layer);
            }
            Opens::Press | Opens::Menu => {}
            Opens::Hover => layer.push(
                el().anchored(&opener, side, align, spec.gap)
                    .hover_group_shared(opener)
                    .inert()
                    .child(build(scope, child).group_opacity(0.0, 1.0)),
            ),
        }
    }
}

impl Overlay {
    fn place(self) -> (Side, Align) {
        let side = match self.side {
            Edge::Top => Side::Top,
            Edge::Bottom => Side::Bottom,
            Edge::Left => Side::Left,
            Edge::Right => Side::Right,
        };
        let align = match self.align {
            Along::Start => Align::Start,
            Along::Center => Align::Center,
            Along::End => Align::End,
        };
        (side, align)
    }
}
