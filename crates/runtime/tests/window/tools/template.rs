//! A new project from the DAW template, as a composer starts one: the song of `templates/daw`
//! opens on its own view, takes a track and a clip, plays the
//! keyboard on the track it selects and plays from space.

use gpui::TestAppContext;

use super::{Window, open_folder};
use crate::song::BAR;

#[gpui::test]
fn a_new_daw_project_opens_its_song_and_plays(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), crate::song::daw()).unwrap();
    let Some(mut window) = open_folder(cx, folder) else {
        return;
    };
    window.wait_for_bun();

    // A track from "+ Track".
    let add = window.until_found("div-add-track");
    window.click(add);
    window.pick("menu-0");
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
    let clips = |window: &mut Window| {
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
    window.until("playing", Window::playing);
    assert_eq!(window.problems(), []);
}

#[gpui::test]
fn three_returns_added_have_three_names(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    runtime::templates::write(folder.path(), crate::song::daw()).unwrap();
    let Some(mut window) = open_folder(cx, folder) else {
        return;
    };
    for name in ["song/ret-a", "song/ret-b", "song/ret-c"] {
        let add = window.until_found("div-add-track");
        window.click(add);
        window.pick("menu-2");
        window.until(name, |window| window.has(name));
    }
    assert_eq!(window.problems(), []);
}
