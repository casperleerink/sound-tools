use gpui::TestAppContext;
use serde_json::json;

use super::{ONLY_THE_TOOL, Opened, open};
use crate::support::id;

const DISPLAY: &str = r#"import { boolean, h, number, output, tool } from "./sdk";

tool({
  name: "display",
  title: "Display",
  when: "A test",
  doc: "Shows an output after it changed.",
  state: { level: number({ min: 0, max: 1, default: 0.25 }), show: boolean({ default: false }) },
  sound: ({ level }) => {
    output("level", level);
    return 0;
  },
  view: ({ state, outputs }) => h("div", {}, () => state.show ? String(outputs.level) : "Hidden"),
});
"#;

fn set(window: &mut Opened<'_>, field: &str, value: serde_json::Value) {
    window.edit(|project| {
        let mut edit = project.begin("Change the display");
        project.update_json(&mut edit, &id("display"), |state| state[field] = value)?;
        project.finish(edit)
    });
}

#[gpui::test]
fn a_binding_that_starts_reading_a_watch_gets_its_current_value(cx: &mut TestAppContext) {
    let mut window = open(
        cx,
        &[
            ONLY_THE_TOOL[0],
            ("state/display.json", r#"{"tool":"display","state":{}}"#),
            ("extensions/display.ts", DISPLAY),
        ],
    );
    window.wait_for_bun();
    set(&mut window, "level", json!(0.75));
    window.until("the output changed", |window| {
        window.project(|project| {
            project
                .watches(&id("display"))
                .iter()
                .any(|(name, watch)| name == "level" && watch.get() == 0.75)
        })
    });
    window.wait_for_bun();
    set(&mut window, "show", json!(true));
    window.wait_for_bun();
    let shell = window.shell.clone();
    assert!(window.cx.read(|cx| {
        let page = shell.read(cx).main_view().unwrap();
        sound_typescript::page_has_text(page, "0.75", cx)
    }));
}
