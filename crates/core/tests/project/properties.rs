//! Loading is applying state from empty: any valid record applied on top of any other gives
//! the same project as loading it fresh.

use proptest::prelude::*;

use crate::tools::{Harness, dc_to_device, project_file};

/// A bank with a gain, levels and maybe an output child, as the files of its folder.
#[derive(Clone, Debug)]
struct BankFiles {
    gain: i8,
    levels: Vec<i8>,
    output_gain: Option<i8>,
}

/// Eighths are exact in `f32`, so sums do not depend on anything but the values.
fn eighths(value: i8) -> f32 {
    f32::from(value) / 8.0
}

fn bank_files() -> impl Strategy<Value = BankFiles> {
    let levels = prop::collection::vec(-8_i8..=8, 0..5);
    (-8_i8..=8, levels, prop::option::of(-8_i8..=8)).prop_map(|(gain, levels, output_gain)| {
        BankFiles {
            gain,
            levels,
            output_gain,
        }
    })
}

/// Makes the folder hold exactly these files, and says which paths changed.
fn write(harness: &Harness, files: &BankFiles) -> Vec<std::path::PathBuf> {
    let folder = harness.path("state/bank");
    if folder.exists() {
        std::fs::remove_dir_all(&folder).unwrap();
    }
    let bank = format!(
        r#"{{"tool": "test.bank", "state": {{"gain": {}}}}}"#,
        eighths(files.gain)
    );
    harness.write("state/bank/instance.json", &bank);
    for (index, level) in files.levels.iter().enumerate() {
        let record = format!(
            r#"{{"tool": "test.level", "state": {{"value": {}}}}}"#,
            eighths(*level)
        );
        harness.write(&format!("state/bank/level-{index}.json"), &record);
    }
    if let Some(gain) = files.output_gain {
        let record = format!(
            r#"{{"tool": "test.amplifier", "state": {{"gain": {}}}}}"#,
            eighths(gain)
        );
        harness.write("state/bank/output.json", &record);
    }
    vec![folder]
}

fn summary(harness: &mut Harness) -> (Vec<(String, Option<String>)>, f32) {
    let instances = harness
        .project
        .instances()
        .map(|(id, _)| (id.to_string(), harness.project.state_json(id)))
        .collect();
    (instances, harness.level())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn applying_on_top_equals_loading_from_empty(first in bank_files(), second in bank_files()) {
        let mut on_top = Harness::new();
        let paths = write(&on_top, &first);
        on_top.project.apply_outside_changes(&paths).unwrap();
        let paths = write(&on_top, &second);
        on_top.project.apply_outside_changes(&paths).unwrap();

        let fresh = Harness::new();
        write(&fresh, &second);
        let mut fresh = fresh.reopen();

        let expected_level = {
            let sum: f32 = second.levels.iter().map(|level| eighths(*level)).sum();
            sum * eighths(second.gain) * second.output_gain.map_or(1.0, eighths)
        };
        let (instances, level) = summary(&mut on_top);
        prop_assert_eq!(level, expected_level);
        prop_assert_eq!((instances, level), summary(&mut fresh));

        // Undo is the same path backwards: it gives the first project again.
        on_top.project.undo().unwrap();
        let undone = Harness::new();
        write(&undone, &first);
        let mut undone = undone.reopen();
        prop_assert_eq!(summary(&mut on_top), summary(&mut undone));
    }
}

#[test]
fn the_dc_tool_gives_the_same_level_loaded_or_applied() {
    let mut harness = Harness::new();
    harness.write_and_apply("project.json", &project_file(&dc_to_device("dc")));
    harness.write_and_apply("state/dc.json", &crate::tools::dc_record(0.25));
    harness.write_and_apply("state/dc.json", &crate::tools::dc_record(-0.5));
    assert_eq!(harness.level(), -0.5);
    let mut harness = harness.reopen();
    assert_eq!(harness.level(), -0.5);
}
