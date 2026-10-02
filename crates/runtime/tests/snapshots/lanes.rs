//! The expression lanes of the note editor:
//!
//! - `editor-bend.png`: the melody with a drawn bend: a scoop into the first note, a slide up
//!   a tone and back, and a wobble, the switch of the lane on Bend and the middle line at 0.
//! - `editor-mod.png`: the same clip with a mod wheel that rises over two bars, on Mod.

use anyhow::{Context as _, Result};
use arrangement::view::lanes::{Lane, Shown};
use gpui::HeadlessAppContext;
use sound_core::{Changes, InstanceId, Ticks};
use sound_notes::{Amount, Bend, Clip, Point};

use super::{BAR, Opened, piece};

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let melody =
        InstanceId::new("arrangement/a-melody-with-a-name-too-long-for-its-header/clip-000")?;
    let opened = Opened::new(cx, |project| {
        piece(project)?;
        let instance = project.resolve::<Clip>(&melody).context("no melody")?;
        let mut clip = project.state(&instance).cloned().context("a clip")?;
        let bend = |tick: u64, value: i64| Point {
            tick: Ticks(tick),
            value: Bend::nearest(value),
        };
        clip.bend = vec![
            bend(0, -4096),
            bend(240, 0),
            bend(1920, 0),
            bend(2400, 8191),
        ];
        clip.bend.push(bend(3360, 8191));
        clip.bend.push(bend(3840, 0));
        // A wobble through the second bar.
        for step in 1..16 {
            let wobble = if step % 2 == 0 { 1200 } else { -1200 };
            clip.bend.push(bend(BAR + step * 240, wobble));
        }
        clip.bend.push(bend(2 * BAR, 0));
        let amount = |tick: u64, value: u8| Point {
            tick: Ticks(tick),
            value: Amount::nearest(i64::from(value)),
        };
        clip.mod_wheel = vec![amount(0, 0), amount(2 * BAR, 0), amount(4 * BAR - 1, 110)];
        let mut changes = Changes::new();
        changes.set(&instance, clip);
        project.commit("Draw lanes", changes)?;
        Ok(())
    })?;
    let editor = opened.open_editor(&melody, cx)?;
    cx.update(|cx| editor.update(cx, |editor, cx| editor.show(Shown::Lane(Lane::Bend), cx)));
    cx.run_until_parked();
    save(cx, &opened, "editor-bend")?;
    let mod_wheel = Shown::Lane(Lane::ModWheel);
    cx.update(|cx| editor.update(cx, |editor, cx| editor.show(mod_wheel, cx)));
    cx.run_until_parked();
    save(cx, &opened, "editor-mod")?;
    Ok(())
}
