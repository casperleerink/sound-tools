//! Fields, icons, shapes and the keys of the views.

use super::*;

const ICONS: &str = r#"import { h, input, tool } from "./sdk";
export const icons = tool({
  name: "icons", title: "Icons", when: "Never", doc: "Icons.", state: {}, sound: () => input,
  view: () => (
    <row gap={2}>
      <icon name="plus" />
      {/* @ts-expect-error: not yet a file */}
      <icon name="sparkle" size={24} color="sound" />
    </row>
  ),
});
"#;

fn icon_problems(state: &State) -> Vec<String> {
    let problems = state.project.problems().into_iter();
    problems
        .filter(|problem| problem.path.starts_with("extensions/ui/icons/"))
        .map(|problem| format!("{}: {}", problem.path, problem.message))
        .collect()
}

#[test]
fn an_icon_without_a_file_is_a_problem_until_the_file_comes_and_the_code_loads() {
    let (_scratch, folder) = with_tools(&[("icons", ICONS)]);
    let mut state = opened(&folder);
    assert_eq!(
        icon_problems(&state),
        [
            "extensions/ui/icons/sparkle.svg: a view draws the icon \"sparkle\", which has no file here"
        ]
    );
    let icons = state.project.root().join("extensions/ui/icons");
    std::fs::copy(icons.join("plus.svg"), icons.join("sparkle.svg")).unwrap();
    state.tools.as_mut().unwrap().code_changed();
    frame(&mut state, 0.1);
    state.settle();
    frame(&mut state, 0.2);
    assert_eq!(icon_problems(&state), Vec::<String>::new());
    let names =
        std::fs::read_to_string(state.project.root().join("extensions/sdk/icons.ts")).unwrap();
    assert!(names.contains("\"sparkle\""), "{names}");
}

const GROWS: &str = r#"import { h, input, live, tool } from "./sdk";
export const grows = tool({
  name: "grows", title: "Grows", when: "Never", doc: "Grows.", state: {}, sound: () => input,
  view: () => (
    <box w={200} h={40}>
      <shapes key="grows" w={live.elapsed.times(60).min(180)} h={40} units={[4, 1]} lines={[{ color: "text", width: 1, points: [0, 1, 2, 0, 2, 0, 4, 1], fill: "text/10" }]} />
    </box>
  ),
});
"#;

#[test]
fn a_shape_with_a_live_width_grows_every_frame_and_runs_no_view() {
    let (_scratch, folder) = with_tools(&[("grows", GROWS)]);
    let mut window = laid_out(opened(&folder));
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    drawn_frame(&mut window, 0.5);
    let early = width(&window, "t00/chain/grows:grows");
    drawn_frame(&mut window, 1.5);
    let later = width(&window, "t00/chain/grows:grows");
    assert!(early > 0.0 && later > early, "{early} {later}");
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
}
