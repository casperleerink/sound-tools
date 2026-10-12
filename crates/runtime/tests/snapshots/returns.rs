//! Returns and sends in the window:
//!
//! - `returns-collapsed.png`: the piece with two returns, Space and Echo, their section folded
//!   above the master row.
//! - `returns-sends.png`: the same with the panel of the bass, which sends to both: the Sends
//!   card at the end of its rack.
//! - `returns-expanded.png`: the section open and Space clicked: its row selected, and its
//!   panel with the reverb, no solo, and `Pre fader` where an audio track has its input.

use anyhow::{Context as _, Result};
use arrangement::{Colour, TrackState};
use gpui::{HeadlessAppContext, point, px};
use sound_core::{Changes, InstanceId, Project};

use super::{HEADER_WIDTH, Opened, add_delay, add_reverb, piece};

/// The window in points, and the master row at its bottom.
const WINDOW_HEIGHT: f32 = 920.;
const MASTER_ROW_HEIGHT: f32 = arrangement::view::MASTER_ROW_HEIGHT;

fn with_returns(project: &mut Project) -> Result<()> {
    piece(project)?;
    let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
    // One commit each: a return goes after the returns the project has.
    for (name, colour) in [("Space", Colour::Teal), ("Echo", Colour::Mauve)] {
        let mut changes = Changes::new();
        arrangement::add_return(project, &mut changes, arrangement.id(), name, colour)?;
        project.commit("Add return", changes)?;
    }
    add_reverb(project, "space")?;
    add_delay(project, "echo")?;
    let bass = InstanceId::new("arrangement/bass")?;
    let bass = project.resolve::<TrackState>(&bass).context("no bass")?;
    let mut state = project.state(&bass).context("no bass")?.clone();
    state.set_send("space", -12.);
    state.set_send("echo", -18.);
    let mut changes = Changes::new();
    changes.set(&bass, state);
    project.commit("Send", changes)?;
    Ok(())
}

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let opened = Opened::new(cx, with_returns)?;
    // The add track button follows the size the timeline painted last, which the first frame
    // does not know yet.
    cx.capture_screenshot(opened.window.into())?;
    cx.update_window(opened.window.into(), |_, window, _| window.refresh())?;
    save(cx, &opened, "returns-collapsed")?;
    opened.click_track_header(1., cx)?;
    save(cx, &opened, "returns-sends")?;
    drop(opened);

    let opened = Opened::new(cx, with_returns)?;
    let view = opened.arrangement_view(cx)?;
    let returns = cx.update(|cx| view.read(cx).returns().clone());
    cx.update(|cx| returns.update(cx, |returns, cx| returns.set_expanded(true, cx)));
    cx.run_until_parked();
    // The first of two rows of returns, under the row that folds them.
    let rows = 2. * arrangement::view::returns::RETURN_ROW_HEIGHT;
    let top = WINDOW_HEIGHT - MASTER_ROW_HEIGHT - rows;
    let space = point(
        px(HEADER_WIDTH / 2.),
        px(top + arrangement::view::returns::RETURN_ROW_HEIGHT / 2.),
    );
    opened.drag(space, point(px(0.), px(0.)), 0, cx)?;
    let open = cx.update(|cx| view.read(cx).track_panel().is_some());
    anyhow::ensure!(open, "the panel of the return did not open");
    save(cx, &opened, "returns-expanded")?;
    Ok(())
}
