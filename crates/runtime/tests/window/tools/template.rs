//! A new project from the DAW template, as a composer starts one: the song of `templates/daw`
//! opens on its own view, takes a track and a clip, plays the
//! keyboard on the track it selects and plays from space.

use gpui::TestAppContext;

use super::{Opened, open_folder};
use crate::song::BAR;

#[gpui::test]
fn a_new_daw_project_opens_its_song_and_plays(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), crate::song::daw()).unwrap();
    let mut window = open_folder(cx, folder);
    window.wait_for_bun();

    // A track from Add track.
    let add = window.until_found("div-add-track");
    window.click(add);
    window.until("the track added", |window| window.has("song/track"));
    window.after_the_runs();
    assert_eq!(window.undo_label().as_deref(), Some("Add a track"));

    // The keyboard plays the track its name selects.
    let name = window.until_found("div-name-t00");
    window.press(name);
    window.release(name);
    let session = window.session.clone();
    window.until("the track selected", |window| {
        (window.cx).read(|cx| session.read(cx).selected().map(ToString::to_string))
            == Some("song/t00".to_string())
    });
    window.play_midi(60);
    assert!(window.loudest(4_800) > 0.01);

    // A clip drawn on the first track.
    let at = window.on_lane(BAR / 2, 0);
    window.double_click(at);
    let clips = |window: &mut Opened| {
        let session = window.session.clone();
        window.cx.read(|cx| {
            (session.read(cx).project().instances())
                .filter(|(id, tool)| *tool == "timeline" && id.as_str().starts_with("song/t00/"))
                .count()
        })
    };
    window.until("the clip drawn", |window| clips(window) == 1);

    // Space plays, through the song's own keys.
    window.focus_the_window();
    window.key_down("space", false);
    window.until("playing", Opened::playing);
    assert_eq!(window.problems(), []);
}

/// A template saved with the tools only: the song keeps the master, limiter and mute lane its
/// tool requires, and no track.
#[gpui::test]
fn a_project_from_the_tools_only_of_a_daw_project_opens_its_empty_song(cx: &mut TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    runtime::templates::write(project.path(), crate::song::daw()).unwrap();
    let templates = tempfile::tempdir().unwrap();
    let opened = runtime::open_for_inspect(project.path()).unwrap();
    let keep = runtime::templates::Keep::tools_of(&opened).unwrap();
    let saved = runtime::templates::save_in(templates.path(), project.path(), "Mine", &keep);
    let folder = tempfile::tempdir().unwrap();
    saved.unwrap().write(folder.path()).unwrap();
    let mut window = open_folder(cx, folder);
    window.wait_for_bun();
    assert!(!window.has("song/t00"));

    let add = window.until_found("div-add-track");
    window.click(add);
    window.until("the track added", |window| window.has("song/track"));
    window.after_the_runs();
    assert_eq!(window.problems(), []);
}

/// What a tool requires is unknown while its record does not load, and a file that does not
/// load would be copied, so the tools alone are not saved then. A track that does not load is
/// left out anyway.
#[test]
fn the_tools_only_of_a_project_whose_required_part_did_not_load_are_refused() {
    let project = tempfile::tempdir().unwrap();
    runtime::templates::write(project.path(), crate::song::daw()).unwrap();
    std::fs::write(project.path().join("state/song/t00/mix.json"), "{").unwrap();
    let opened = runtime::open_for_inspect(project.path()).unwrap();
    runtime::templates::Keep::tools_of(&opened).unwrap();

    // Broken while the project is open: the older master plays on, the file does not load.
    let (control, _engine) = sound_core::Engine::new(runtime::OFFLINE);
    let (mut open, _plugins) = crate::plugin_hosts::open_or_create(project.path(), control);
    let master = open.root().join("state/song/master.json");
    std::fs::write(&master, "{").unwrap();
    open.apply_outside_changes(&[master]).unwrap();
    let error = runtime::templates::Keep::tools_of(&open).unwrap_err();
    assert!(error.to_string().contains("song/master"), "{error}");
    drop(open);

    let opened = runtime::open_for_inspect(project.path()).unwrap();
    let error = runtime::templates::Keep::tools_of(&opened).unwrap_err();
    assert!(error.to_string().contains("song/master"), "{error}");

    // A song whose own record is gone leaves its folder of parts.
    let fader = r#"{"tool": "fader", "state": {}}"#;
    std::fs::write(project.path().join("state/song/master.json"), fader).unwrap();
    std::fs::remove_file(project.path().join("state/song/instance.json")).unwrap();
    let opened = runtime::open_for_inspect(project.path()).unwrap();
    let error = runtime::templates::Keep::tools_of(&opened).unwrap_err();
    assert!(error.to_string().contains("(state/song)"), "{error}");
}

#[gpui::test]
fn three_returns_added_have_three_names(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), crate::song::daw()).unwrap();
    let mut window = open_folder(cx, folder);
    for name in ["song/ret-a", "song/ret-b", "song/ret-c"] {
        let add = window.until_found("div-add-track-kind");
        window.click(add);
        window.pick("menu-2");
        window.until(name, |window| window.has(name));
    }
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn an_empty_project_starts_from_the_daw_template_in_place(cx: &mut TestAppContext) {
    let folder = crate::support::new_project();
    // The window makes it for every project it opens.
    std::fs::create_dir_all(folder.path().join("extensions")).unwrap();
    let mut window = open_folder(cx, folder);

    let daw = window.until_found("template-DAW");
    window.click(daw);
    let last = (window.folder.path().join(runtime::templates::KEPT)).join("template.json");
    window.until("the template written", |_| last.exists());
    // What the watchers hear.
    let written = ["project.json", "state"].map(|path| window.path(path));
    window.edit(|project| project.apply_outside_changes(&written));
    window.code_changed();
    window.until_found("div-add-track");
    window.wait_for_bun();
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn a_newer_template_for_code_the_project_changed_shows_a_notice(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), crate::song::daw()).unwrap();
    // As an older release kept it, and the project changed it since.
    let song = "extensions/song.tsx";
    let kept = folder.path().join(runtime::templates::KEPT).join(song);
    std::fs::write(&kept, "// an older template").unwrap();
    let ours = std::fs::read_to_string(folder.path().join(song)).unwrap() + "\n// ours\n";
    std::fs::write(folder.path().join(song), &ours).unwrap();

    let mut window = open_folder(cx, folder);
    window.until_found("notice-template");
    let root = window.folder.path().to_path_buf();
    assert_eq!(std::fs::read_to_string(root.join(song)).unwrap(), ours);
}
