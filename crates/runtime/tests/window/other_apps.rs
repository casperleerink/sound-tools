//! `project.json` hears another app: the window taps it while a connection starts at it.

use std::time::Duration;

use gpui::TestAppContext;
use sound_core::{AppSound, Changes, SavedConnection, SavedDestination, SavedSource};

use crate::support::{Opened, open_with_app};

/// The window looks at the processes of the apps this often.
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

fn problems(opened: &mut Opened<'_>) -> Vec<String> {
    opened.project(|project| {
        let problems = project.problems().into_iter();
        problems.map(|problem| problem.message).collect()
    })
}

/// An app that is not running is a problem until it starts, then the engine plays it. Without
/// the connection its tap closes.
#[gpui::test]
fn a_connection_from_an_app_hears_it_once_it_runs(cx: &mut TestAppContext) {
    let (mut opened, music) = open_with_app(cx);
    let hear = SavedConnection {
        from: SavedSource::App(AppSound::Named("Music".to_string())),
        to: SavedDestination::DeviceOutput(0),
    };
    opened.edit(|project| {
        let mut changes = Changes::new();
        changes.connect(hear.clone());
        project.commit("Hear Music", changes)
    });
    opened.settle();
    if cfg!(target_os = "macos") {
        let expected = r#"connections[0]: not heard, because "Music" is not running, or has played no sound yet"#;
        assert_eq!(problems(&mut opened), [expected]);
    }

    music.set_processes(vec![71]);
    opened.cx.executor().advance_clock(CHECK_INTERVAL);
    opened.settle();
    assert!(music.is_heard());
    music.play([0.5, 0.25], 4_096);
    let output = opened.render(512);
    assert_eq!(output[output.len() - 2..], [0.5, 0.25]);
    if cfg!(target_os = "macos") {
        assert_eq!(problems(&mut opened), [] as [String; 0]);
    }

    opened.edit(|project| {
        let mut changes = Changes::new();
        changes.disconnect(hear);
        project.commit("Hear Music no more", changes)
    });
    opened.settle();
    opened.render(512);
    assert!(!music.is_heard());
}
