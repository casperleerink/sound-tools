//! The DAW template: what it enables, and that its song renders with no arrangement.

use runtime::{DAW, PROJECT_FILE};

/// It enables what a new project would but the arrangement and the tempo fit: a test that fails
/// when an extension is added, so the template hears of it.
#[test]
fn the_daw_template_enables_every_extension_but_the_arrangement() {
    let (_, project) = (DAW.files.iter())
        .find(|(path, _)| *path == PROJECT_FILE)
        .unwrap();
    let file: serde_json::Value = serde_json::from_str(project).unwrap();
    let enabled: Vec<&str> = (file["extensions"].as_array().unwrap().iter())
        .filter_map(serde_json::Value::as_str)
        .collect();
    let mut expected = runtime::bundled_extensions().unwrap();
    expected.retain(|name| !["arrangement", "fit-tempo"].contains(name));
    assert_eq!(enabled, expected);
}
