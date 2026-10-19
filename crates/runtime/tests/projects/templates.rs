//! The DAW template: what it enables.

use runtime::PROJECT_FILE;

/// It enables every bundled extension: a test that fails when an extension is added, so the
/// template hears of it.
#[test]
fn the_daw_template_enables_every_extension() {
    let daw = runtime::templates::named("DAW").unwrap();
    let (_, project) = (daw.files.iter())
        .find(|(path, _)| *path == PROJECT_FILE)
        .unwrap();
    let file: serde_json::Value = serde_json::from_str(project).unwrap();
    let enabled: Vec<&str> = (file["extensions"].as_array().unwrap().iter())
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert_eq!(enabled, runtime::bundled_extensions().unwrap());
}
