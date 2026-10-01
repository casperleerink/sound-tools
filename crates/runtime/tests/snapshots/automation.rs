//! The automation lanes under a track:
//!
//! - `automation-lanes.png`: the bass of the piece with a Filter, its lanes shown: a volume
//!   that fades in from silence and out again, and a sweep of the cutoff up and back down.
//! - `automation-add-lane.png`: the same with the select under the lanes open, which offers
//!   the pan and the numbers of the Filter.
//! - `automation-drag.png`: the second bass clip dragged two bars on, the button still down:
//!   the line it takes along over a light band where it lands, the line it replaces faded,
//!   and the hint under the clip.
//! - `automation-drag-folded.png`: the same drag with the lanes folded away, where the clip
//!   carries a small mark instead.

use anyhow::{Context as _, Result};
use arrangement::view::layout::{ADD_LANE_HEIGHT, HEADER_WIDTH, RULER_HEIGHT, TRACK_HEIGHT};
use arrangement::{AutomationLane, AutomationValue, TrackState};
use gpui::{HeadlessAppContext, Pixels, Point, point, px};
use sound_core::{Changes, InstanceId, Ticks};

use super::{BAR, Opened, add_filter, piece};

const BASS: &str = "arrangement/bass";

fn lane(device: Option<&str>, parameter: &str, points: &[(u64, f32)]) -> AutomationLane {
    AutomationLane {
        device: device.map(str::to_string),
        parameter: parameter.to_string(),
        points: points
            .iter()
            .map(|&(tick, value)| sound_notes::Point {
                tick: Ticks(tick),
                value: AutomationValue(value),
            })
            .collect(),
    }
}

pub fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let bass = InstanceId::new(BASS)?;
    let opened = Opened::new(cx, |project| {
        piece(project)?;
        add_filter(project, "bass")?;
        let track = project.resolve::<TrackState>(&bass).context("no bass")?;
        let mut state = project.state(&track).cloned().context("a track")?;
        let fade = [
            (0, f32::NEG_INFINITY),
            (2 * BAR, 0.),
            (10 * BAR, 0.),
            (12 * BAR - 1, -24.),
        ];
        let sweep = [
            (4 * BAR, 200.),
            (8 * BAR, 8000.),
            (9 * BAR, 8000.),
            (11 * BAR, 400.),
        ];
        state.automation = vec![
            lane(None, "gain_db", &fade),
            lane(Some("filter"), "cutoff_hz", &sweep),
        ];
        let mut changes = Changes::new();
        changes.set(&track, state);
        project.commit("Automate", changes)?;
        anyhow::ensure!(project.problems().is_empty(), "{:?}", project.problems());
        Ok(())
    })?;
    let timeline = opened.timeline_view(cx)?;
    let show = |shown: bool, cx: &mut HeadlessAppContext| {
        cx.update(|cx| timeline.update(cx, |timeline, cx| timeline.show_lanes(&bass, shown, cx)));
        cx.run_until_parked();
    };
    show(true, cx);
    save(cx, &opened, "automation-lanes")?;

    // The select in the row under the two lanes of the bass, at the left of its words.
    let menu = cx.update(|cx| {
        let timeline = timeline.read(cx);
        let (viewport, rows) = (timeline.viewport(), timeline.rows(cx));
        let top = viewport.y_at(rows.lane_top(1, 2));
        point(px(56.), px(48. + RULER_HEIGHT + top + ADD_LANE_HEIGHT / 2.))
    });
    opened.drag(menu, point(px(0.), px(0.)), 0, cx)?;
    save(cx, &opened, "automation-add-lane")?;
    opened.key("escape", cx)?;

    // The second bass clip, bars 5 to 8, two bars on.
    let at = |tick: u64, cx: &mut HeadlessAppContext| -> Point<Pixels> {
        cx.update(|cx| {
            let timeline = timeline.read(cx);
            let (viewport, rows) = (timeline.viewport(), timeline.rows(cx));
            point(
                px(HEADER_WIDTH + viewport.x_of(Ticks(tick))),
                px(48. + RULER_HEIGHT + viewport.y_of(&rows, 1) + TRACK_HEIGHT / 2.),
            )
        })
    };
    let (from, to) = (at(5 * BAR, cx), at(7 * BAR, cx));
    opened.press_and_move(from, to, cx)?;
    save(cx, &opened, "automation-drag")?;
    opened.release(to, cx)?;

    show(false, cx);
    let (from, to) = (at(7 * BAR, cx), at(5 * BAR, cx));
    opened.press_and_move(from, to, cx)?;
    save(cx, &opened, "automation-drag-folded")?;
    opened.release(to, cx)?;
    Ok(())
}
