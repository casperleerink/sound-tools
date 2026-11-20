//! Files dropped from the system on an element that has `onDrop`.

use rune::{Color, El, Live};

use crate::app::State;

use super::gesture::units;
use super::semantics::Press;
use super::{BoxNode, Scope};

/// `e` taking the files dropped on it, in the units of the plane around it, as a press. Any
/// file: what it holds, not its name, says whether it is a sound or notes. Its `hover` look
/// shows while files are over it.
pub(super) fn target(scope: &Scope, mut e: El, b: &BoxNode, press: &Press) -> El {
    if !b.drop {
        return e;
    }
    let theme = &scope.state.theme;
    let key = scope.key(&press.element);
    if let Some(over) = b.hover.as_deref().map(|colour| theme.color(colour)) {
        let rest = b
            .fill
            .as_deref()
            .map_or(Color::TRANSPARENT, |c| theme.color(c));
        // Live, so a memo Rune does not build again shows it too.
        let look = Live::new(match scope.state.drops.over(&key) {
            true => over,
            false => rest,
        });
        e = e.live_fill(&look);
        e = e.on_drop_hover(move |s: &mut State, on: bool, _: &mut rune::Tx| {
            look.set(if on { over } else { rest });
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
