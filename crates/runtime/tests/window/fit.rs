//! Fitting the tempo from the window: the action in the project menu, and the steadiness
//! control in the transport, driven with the real mouse and keys.

use fit_tempo::FitState;
use gpui::{TestAppContext, px};
use sound_core::{Changes, Ticks};
use sound_notes::Clip;

use crate::support::{self, Opened, id};

#[path = "../projects/generated_take.rs"]
mod generated_take;

const CLIP: &str = "arrangement/track-1/take";

/// A project with the clip of a recorded take on its one track, as a recording leaves it.
fn recorded(cx: &mut TestAppContext) -> Opened<'_> {
    support::open_with(cx, |project| {
        let take = generated_take::generated_take(8);
        let name = take.write(project.assets()).unwrap();
        let clock = project.clock().clone();
        let mut clip = take.clip(|time_us| clock.tick_at_micros(time_us)).unwrap();
        clip.take = Some(name);
        let mut changes = Changes::new();
        changes.create(id(CLIP), clip);
        project.commit("Record", changes).unwrap();
    })
}

fn select(opened: &mut Opened<'_>, clip: &str) {
    let clip = id(clip);
    opened.timeline.update(opened.cx, |timeline, cx| {
        timeline.select_clip(Some(clip), cx)
    });
    opened.settle();
}

/// Writes the fit record from outside and applies it, as an agent's edit arrives.
fn write_fit(opened: &mut Opened<'_>, state: &str) {
    let path = opened.path("state/fit-tempo.json");
    let record = format!(r#"{{"tool": "fit-tempo", "state": {state}}}"#);
    std::fs::write(&path, record).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    opened.settle();
}

/// Opens the project menu and picks `Fit tempo to take`. Gives whether the project has a fit
/// afterwards: an item that cannot run does nothing when it is clicked.
fn pick_fit(opened: &mut Opened<'_>) -> bool {
    let menu = opened.cx.read(|cx| {
        let shell = opened.shell.read(cx);
        shell.project_menu().read(cx).menu().clone()
    });
    opened
        .cx
        .update(|window, cx| menu.update(cx, |menu, cx| menu.open(window, cx)));
    opened.cx.run_until_parked();
    let row = opened.control("menu-fit-tempo");
    opened.click(row);
    opened.settle();
    fit_state(opened).is_some()
}

fn fit_state(opened: &mut Opened<'_>) -> Option<FitState> {
    opened.session.read_with(opened.cx, |session, _| {
        let fit = fit_tempo::fit_of(session.project())?;
        session.project().state(&fit).cloned()
    })
}

fn project_file(opened: &mut Opened<'_>) -> String {
    std::fs::read_to_string(opened.path("project.json")).unwrap()
}

/// The steadiness the transport shows, as whole percent, which is what it draws. A saved
/// steadiness is an `f32`, so 0.4 of it comes back as 40.000001 percent.
fn steadiness(opened: &mut Opened<'_>) -> Option<f64> {
    let transport = opened
        .cx
        .read(|cx| opened.shell.read(cx).transport().clone());
    let percent = opened.cx.read(|cx| transport.read(cx).shown_steadiness(cx));
    percent.map(|percent| percent.round())
}

/// Fits the project to the take through the same call the menu makes.
fn fit(opened: &mut Opened<'_>) {
    opened.session.update(opened.cx, |session, cx| {
        let clip = session.project().resolve::<Clip>(&id(CLIP)).unwrap();
        let clip = session.project().state(&clip).unwrap().clone();
        session.edit(cx, |project| {
            let mut changes = Changes::new();
            fit_tempo::fit_take(project, &mut changes, &clip)?;
            project.commit(fit_tempo::FIT_LABEL, changes)
        });
    });
    opened.settle();
}

/// The action is offered for a clip that has a take, and for nothing else.
#[gpui::test]
fn the_fit_is_offered_for_a_clip_that_was_recorded(cx: &mut TestAppContext) {
    let mut opened = recorded(cx);
    // Nothing is selected: the item is there and cannot run.
    assert!(!pick_fit(&mut opened), "a fit was made with no clip");

    // A clip that was drawn by hand has no take to follow.
    opened.session.update(opened.cx, |session, cx| {
        session.edit(cx, |project| {
            let mut changes = Changes::new();
            changes.create(
                id("arrangement/track-1/drawn"),
                support::clip(0, 3840, vec![]),
            );
            project.commit("Add clip", changes)
        });
    });
    select(&mut opened, "arrangement/track-1/drawn");
    assert!(!pick_fit(&mut opened), "a fit was made from a drawn clip");

    // The clip of the take: the fit happens, as one undo step.
    select(&mut opened, CLIP);
    assert!(pick_fit(&mut opened));
    assert_eq!(opened.undo_label().as_deref(), Some(fit_tempo::FIT_LABEL));
    assert_eq!(steadiness(&mut opened), Some(0.0));
    assert_eq!(opened.project(|project| project.problems().len()), 0);
}

/// The steadiness control shows only when the project has a fit, and it comes and goes with
/// one written from outside.
#[gpui::test]
fn the_steadiness_shows_only_with_a_fit(cx: &mut TestAppContext) {
    let mut opened = recorded(cx);
    assert_eq!(steadiness(&mut opened), None);
    fit(&mut opened);
    assert_eq!(steadiness(&mut opened), Some(0.0));
    assert_eq!(
        fit_state(&mut opened).map(|state| state.steadiness),
        Some(0.0)
    );

    // An agent raises it in the file. The transport follows without being told.
    write_fit(&mut opened, r#"{"take": "take-1", "steadiness": 0.4}"#);
    assert_eq!(steadiness(&mut opened), Some(40.0));

    // And it goes when the fit does.
    opened.session.update(opened.cx, |session, cx| {
        session.edit(cx, |project| {
            let mut changes = Changes::new();
            changes.delete(&id("fit-tempo"));
            project.commit("Remove fit", changes)
        });
    });
    opened.settle();
    assert_eq!(steadiness(&mut opened), None);
}

/// A drag on the steadiness is one undo step, is heard while it runs, and puts the tempo map
/// back when escape ends it.
#[gpui::test]
fn a_steadiness_drag_is_one_undo_step(cx: &mut TestAppContext) {
    let mut opened = recorded(cx);
    fit(&mut opened);
    let map = |opened: &mut Opened<'_>| {
        opened.session.read_with(opened.cx, |session, _| {
            session.project().project_file().tempo_map.clone()
        })
    };
    let fitted = map(&mut opened);
    let fitted_file = project_file(&mut opened);

    let control = opened.control("steadiness");
    opened.mouse_down(control);
    // Up is more steady: thirty pixels is thirty percent.
    opened.drag_to(control - gpui::point(px(0.), px(30.)));
    assert_eq!(steadiness(&mut opened), Some(30.0));
    // It is heard while the drag runs: the map is already another one.
    assert_ne!(map(&mut opened), fitted);
    opened.drag_to(control - gpui::point(px(0.), px(60.)));
    assert_eq!(steadiness(&mut opened), Some(60.0));
    opened.release(control - gpui::point(px(0.), px(60.)));
    opened.settle();

    // One step, whatever the drag did in between.
    assert_eq!(
        opened.undo_label().as_deref(),
        Some(fit_tempo::STEADINESS_LABEL)
    );
    opened.keys("cmd-z");
    opened.settle();
    assert_eq!(steadiness(&mut opened), Some(0.0));
    assert_eq!(map(&mut opened), fitted, "the fitted map came back");
    // And `project.json` holds the fitted map again, byte for byte.
    assert_eq!(project_file(&mut opened), fitted_file);

    // The arrows step by five, on the focused control.
    opened.press(control);
    opened.release(control);
    opened.keys("up");
    opened.settle();
    assert_eq!(steadiness(&mut opened), Some(5.0));
    assert_eq!(
        opened.undo_label().as_deref(),
        Some(fit_tempo::STEADINESS_LABEL)
    );
}

/// The clip of the take keeps its notes while the steadiness moves: only the tempo map changes.
#[gpui::test]
fn a_steadiness_change_never_moves_a_note(cx: &mut TestAppContext) {
    let mut opened = recorded(cx);
    fit(&mut opened);
    let clip = |opened: &mut Opened<'_>| {
        opened.session.read_with(opened.cx, |session, _| {
            let clip = session.project().resolve::<Clip>(&id(CLIP)).unwrap();
            session.project().state(&clip).cloned().unwrap()
        })
    };
    let fitted = clip(&mut opened);
    assert!(fitted.start > Ticks(0), "the clip kept its place");

    write_fit(&mut opened, r#"{"take": "take-1", "steadiness": 1.0}"#);
    assert_eq!(steadiness(&mut opened), Some(100.0));
    assert_eq!(clip(&mut opened), fitted);
}
