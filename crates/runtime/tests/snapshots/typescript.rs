//! A page of a tool of the project, drawn by Bun:
//!
//! - `typescript-page.png`: the knob, steps, meter and pad of the SDK, made of shapes, beside
//!   a small piano roll whose notes and labels are shapes of a space.

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui::{AppContext, HeadlessAppContext, px, size};
use runtime::window::{DeviceAccess, Shell};
use runtime::{OFFLINE, views};
use sound_core::Engine;
use sound_ui::Session;

use super::{WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::plugin_hosts::test_plugin_host;

const DEMO: &str = r##"import { h, knob, Knob, Label, Line, Meter, Pad, pattern, Rect, Space, Steps, live, tool } from "./sdk";

const notes = [[0, 60], [480, 64], [960, 67], [1440, 72], [1920, 67], [2400, 64], [2880, 62], [3360, 60]];

tool({
  name: "demo",
  title: "Demo",
  when: "A snapshot",
  doc: "Every element of the SDK.",
  kind: "source",
  state: {
    cutoff: knob({ min: 100, max: 8000, default: 1200, unit: "hz" }),
    level: knob({ min: -30, max: 6, default: -6, unit: "db" }),
    steps: pattern({ length: 8, min: 0, max: 1, default: 0 }),
  },
  controls: { bend: live({ min: 0, max: 1, default: 0.3 }), drive: live({ min: 0, max: 1, default: 0.7 }) },
  sound: () => 0,
  page: ({ now }) =>
    h("div", { style: { direction: "row", gap: 24 } },
      h("div", { style: { gap: 16 } },
        h("div", { style: { direction: "row", gap: 8 } },
          h(Knob, { path: "cutoff", label: "Cutoff", min: 100, max: 8000, default: 1200, unit: "hz" }),
          h(Knob, { path: "level", label: "Level", min: -30, max: 6, default: -6, unit: "db" }),
          h(Meter, { watch: "level", label: "Out" }),
        ),
        h(Steps, { path: "steps" }),
        h(Pad, { x: "bend", y: "drive", size: 120 }),
      ),
      h(Space, { key: "roll", width: 640, height: 320, background: "#121317", radius: 6, unit: { x: 0.15, y: 12 }, extent: { x: [0, 3840], y: [0, 36] } },
        Array.from({ length: 4 }, (_, beat) => h(Line, { from: [beat * 960, 0], to: [beat * 960, 36], color: "#292c34", width: 1 })),
        notes.map(([start, pitch], index) => h(Rect, {
          key: String(index), x: start, y: 84 - pitch, width: 440, height: 0.85, radius: 3,
          color: index === 2 ? "#e9ebef" : "#a9b1ff", hover: "#e9ebef", cursor: "grab",
        })),
        notes.map(([start, pitch], index) => h(Label, { x: start + 30, y: 84 - pitch + 0.05, text: String(pitch), size: 9, color: "#121317" })),
        h(Line, { from: [now.tick.plus(1200), 0], to: [now.tick.plus(1200), 36], color: "#7ee0a0", width: 2 }),
      ),
    ),
});
"##;

pub(crate) fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &super::Opened, &str) -> Result<()>,
) -> Result<()> {
    if !sound_typescript::has_bun() {
        println!("Bun is not installed: no snapshot of a page of a tool of the project");
        return Ok(());
    }
    let folder = tempfile::tempdir()?;
    let root = folder.path().join("Night Study");
    let write = |relative: &str, contents: &str| -> Result<()> {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().context("a parent folder")?)?;
        std::fs::write(path, contents)?;
        Ok(())
    };
    write(
        "project.json",
        r#"{"format": 1, "extensions": [],
        "tempo_map": {"time_signature": "4/4", "tempo_changes": [{"tick": 0, "bpm": 120.0}]},
        "connections": []}"#,
    )?;
    write(
        "state/demo.json",
        r#"{"tool": "demo", "state": {"steps": [1, 0, 0, 1, 0, 1, 0, 0], "cutoff": 2400}}"#,
    )?;
    write("extensions/demo.ts", DEMO)?;
    cx.allow_parking();
    let (control, engine) = Engine::new(OFFLINE);
    let plugins = test_plugin_host(&root);
    let (project, extensions) = runtime::open_with_extensions(&root, control, plugins.clone())?;
    let extensions = extensions.context("the tools of the project")?;
    let plugins = plugins.downgrade();
    let session = cx.update(|cx| cx.new(|cx| Session::new(project, cx)));
    let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
        let session = session.clone();
        let (mut views, mut devices) = views(plugins.clone());
        sound_typescript::start_window(extensions, &session, &mut views, &mut devices, cx);
        cx.new(|cx| {
            let name = "MacBook Pro Speakers".into();
            Shell::with_device(
                session,
                (views, devices),
                name,
                DeviceAccess::default(),
                window,
                cx,
            )
        })
    })?;
    // Bun answers in real time, on a thread of its own.
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let opened = super::Opened {
        _folder: folder,
        engine,
        plugins,
        session,
        window,
    };
    save(cx, &opened, "typescript-page")
}
