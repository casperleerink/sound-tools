//! Files dropped from the system on an element that has `onDrop`.

use rune::{Color, El};

use crate::app::State;

use super::gesture::units;
use super::semantics::Press;
use super::{BoxNode, Scope};

/// `e` taking the files dropped on it, in the units of the plane around it, as a press. Any
/// file: what it holds, not its name, says whether it is a sound or notes. Its `hover` look
/// shows while files are over it, see [`over`].
pub(super) fn target(scope: &Scope, mut e: El, b: &BoxNode, press: &Press) -> El {
    if !b.drop {
        return e;
    }
    let key = scope.key(&press.element);
    if b.hover.is_some() {
        e = e.on_drop_hover(move |s: &mut State, on: bool, _: &mut rune::Tx| {
            s.drops.hover(&key, on);
        });
    }
    let (press, plane) = (press.clone(), scope.plane.clone());
    e.on_drop_files(
        move |s: &mut State, files: &[std::path::PathBuf], tx: &mut rune::Tx| {
            let at = tx.drop_point().map_or([0.0; 2], |at| {
                units(
                    s,
                    plane.as_deref(),
                    [at.window_x, at.window_y],
                    [at.x, at.y],
                )
            });
            s.drop_files(&press.instance, &press.element, files.to_vec(), at);
        },
    )
}

/// `e` showing its `hover` look also while files or a dragged element are over it, which no
/// pointer hover shows.
pub(super) fn over(scope: &Scope, e: El, b: &BoxNode) -> El {
    let (Some(over), Some(key)) = (b.hover.as_deref(), b.key.as_deref()) else {
        return e;
    };
    let theme = &scope.state.theme;
    let rest = (b.fill.as_deref()).map_or(Color::TRANSPARENT, |c| theme.color(c));
    let look = scope
        .state
        .drops
        .look(&scope.key(key), rest, theme.color(over));
    e.live_fill(&look)
}
