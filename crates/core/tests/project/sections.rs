//! Sections in `project.json`: from a file, from the window, and refused when invalid.

use std::num::NonZeroU64;

use sound_core::{Changes, Section, SectionName, Sections};

use crate::tools::{Harness, project_file};

fn bar(number: u64) -> NonZeroU64 {
    NonZeroU64::new(number).unwrap()
}

fn name(text: &str) -> SectionName {
    SectionName::new(text).unwrap()
}

fn with_sections(sections: &str) -> String {
    project_file("").replace(
        r#""connections": []"#,
        &format!(r#""sections": {sections}, "connections": []"#),
    )
}

fn names_and_bars(harness: &Harness) -> Vec<(String, u64)> {
    let sections = harness.project.project_file().sections.as_slice().iter();
    let pairs = sections.map(|section| (section.name.as_str().to_string(), section.bar.get()));
    pairs.collect()
}

#[test]
fn sections_from_a_file_apply_live_and_a_window_edit_saves_and_undoes() {
    let mut harness = Harness::new();
    let written = with_sections(r#"[{"bar": 1, "name": "Intro"}, {"bar": 9, "name": "Chorus"}]"#);
    harness.write_and_apply("project.json", &written);
    assert_eq!(harness.project.problems(), []);
    assert_eq!(
        names_and_bars(&harness),
        [("Intro".into(), 1), ("Chorus".into(), 9)]
    );

    let sections = &harness.project.project_file().sections;
    let moved = sections.with_section_moved(bar(9), bar(17)).unwrap();
    let mut changes = Changes::new();
    changes.set_sections(moved);
    harness.project.commit("Move section", changes).unwrap();
    assert!(
        harness
            .read("project.json")
            .contains(r#"{"bar": 17, "name": "Chorus"}"#),
        "{}",
        harness.read("project.json")
    );

    harness.project.undo().unwrap();
    assert_eq!(
        names_and_bars(&harness),
        [("Intro".into(), 1), ("Chorus".into(), 9)]
    );
    assert!(harness.read("project.json").contains(r#""bar": 9"#));
}

#[test]
fn invalid_sections_do_not_load() {
    let cases = [
        (
            r#"[{"bar": 9, "name": "A"}, {"bar": 1, "name": "B"}]"#,
            "see bar 1",
        ),
        (
            r#"[{"bar": 9, "name": "A"}, {"bar": 9, "name": "B"}]"#,
            "see bar 9",
        ),
        (r#"[{"bar": 1, "name": " "}]"#, "needs a name"),
        (r#"[{"bar": 0, "name": "A"}]"#, "sections"),
        (r#"[{"tick": 0, "name": "A"}]"#, "sections"),
    ];
    for (sections, expected) in cases {
        let mut harness = Harness::new();
        assert_eq!(
            harness.write_and_apply("project.json", &with_sections(sections)),
            0
        );
        let problem = harness.problem_at("project.json").unwrap();
        assert!(problem.contains(expected), "{sections}: {problem}");
        assert_eq!(harness.project.project_file().sections, Sections::default());
    }
}

#[test]
fn section_edits_keep_the_list_sorted_and_refuse_a_taken_bar() {
    let section = |number, text| Section {
        bar: bar(number),
        name: name(text),
    };
    let sections = Sections::new(vec![section(1, "Intro"), section(9, "Verse")]).unwrap();

    let added = sections.with_section_at(bar(5), name("Build")).unwrap();
    assert_eq!(
        added.as_slice(),
        [
            section(1, "Intro"),
            section(5, "Build"),
            section(9, "Verse")
        ]
    );
    assert_eq!(sections.with_section_at(bar(9), name("Again")), None);

    let moved = added.with_section_moved(bar(1), bar(13)).unwrap();
    assert_eq!(
        moved.as_slice(),
        [
            section(5, "Build"),
            section(9, "Verse"),
            section(13, "Intro")
        ]
    );
    assert_eq!(added.with_section_moved(bar(1), bar(9)), None);
    assert_eq!(added.with_section_moved(bar(2), bar(3)), None);

    let renamed = sections.with_name_at(bar(9), name("Chorus")).unwrap();
    assert_eq!(renamed.as_slice()[1], section(9, "Chorus"));
    assert_eq!(sections.with_name_at(bar(2), name("Chorus")), None);

    assert_eq!(
        added.without_section_at(bar(5)),
        Some(sections.clone()),
        "removing what was added gives the list before"
    );
    assert_eq!(sections.without_section_at(bar(5)), None);
}
