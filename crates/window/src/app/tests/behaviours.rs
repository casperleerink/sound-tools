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
