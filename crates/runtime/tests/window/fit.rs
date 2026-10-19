//! Fitting the tempo from the window: the action in the menu of a clip, and the steadiness
//! control in the transport, driven with the real mouse and keys.

use fit_tempo::FitState;
use gpui::{TestAppContext, px};
use runtime::arrangement_wiring::transport::TransportPill;
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

/// Right-clicks a clip on the first track, and picks `Fit tempo to take` when its menu offers
/// it. Gives whether it was offered: an item that cannot run does nothing when it is clicked.
fn pick_fit(opened: &mut Opened<'_>, clip: &str) -> bool {
    let start = opened.clip(clip).expect("the clip").start.0;
    let on_clip = opened.at(start + 100, 0);
    opened.right_click(on_clip);
    let row = opened.find("menu-fit-tempo");
    match row {
        Some(row) => opened.click(row),
        None => opened.keys("escape"),
    }
    opened.settle();
    row.is_some()
}

/// Writes the fit record from outside and applies it, as an agent's edit arrives.
fn write_fit(opened: &mut Opened<'_>, state: &str) {
    let path = opened.path("state/fit-tempo.json");
    let record = format!(r#"{{"tool": "fit-tempo", "state": {state}}}"#);
    std::fs::write(&path, record).unwrap();
    opened.edit(|project| project.apply_outside_changes(std::slice::from_ref(&path)));
    opened.settle();
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
    let transport = opened.cx.read(|cx| {
        opened
            .shell
            .read(cx)
            .chrome()
            .clone()
            .downcast::<TransportPill>()
            .unwrap()
    });
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

/// The menu of a clip that has a take offers the fit, and the menu of any other clip does not.
#[gpui::test]
fn the_fit_is_offered_for_a_clip_that_was_recorded(cx: &mut TestAppContext) {
    let mut opened = recorded(cx);
    let drawn = "arrangement/track-1/drawn";
    opened.session.update(opened.cx, |session, cx| {
        session.edit(cx, |project| {
            let mut changes = Changes::new();
            changes.create(id(drawn), support::clip(40 * 3840, 3840, vec![]));
            project.commit("Add clip", changes)
        });
    });
    opened.settle();
    assert!(!pick_fit(&mut opened, drawn), "a drawn clip offers the fit");
    assert!(fit_state(&mut opened).is_none());

    // The clip of the take: the fit happens, as one undo step.
    assert!(pick_fit(&mut opened, CLIP));
    assert!(fit_state(&mut opened).is_some());
    assert_eq!(opened.undo_label().as_deref(), Some(fit_tempo::FIT_LABEL));
    assert_eq!(steadiness(&mut opened), Some(0.0));
    assert_eq!(opened.project(|project| project.problems().len()), 0);
}

/// A project made before the fit existed does not list `fit-tempo` in `extensions`. The action
/// is then at 40 % with the one edit under it, as an instrument such a project cannot load is,
/// and clicking it does nothing instead of failing with a tool name.
#[gpui::test]
fn the_fit_says_why_it_is_off_in_a_project_without_the_extension(cx: &mut TestAppContext) {
    let mut opened = support::open_without_extensions(
        cx,
        r#"["arrangement", "instrument", "plugin-host", "tone"]"#,
        |project| {
            let take = generated_take::generated_take(8);
            let name = take.write(project.assets()).unwrap();
            let clock = project.clock().clone();
            let mut clip = take.clip(|time_us| clock.tick_at_micros(time_us)).unwrap();
            clip.take = Some(name);
            let mut changes = Changes::new();
            changes.create(id(CLIP), clip);
            project.commit("Record", changes).unwrap();
        },
    );
    assert!(
        pick_fit(&mut opened, CLIP),
        "the menu does not offer the fit"
    );
    let item = opened.cx.read(|cx| {
        let menu = opened.timeline.read(cx).clip_menu().clone();
        menu.read(cx).item("fit-tempo").cloned()
    });
    let item = item.expect("the menu offers the fit");
    assert!(item.is_disabled(), "the fit can be picked");
    assert_eq!(
        item.description.as_deref(),
        // Why, in words. The file edit is for an agent, in the agent docs.
        Some("This project does not include the tempo fit.")
    );
    // And picking it anyway changed nothing and reported nothing.
    assert!(fit_state(&mut opened).is_none());
    assert_eq!(opened.notice(), None);
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

    let control = opened.control("number-steadiness");
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
    opened.keys("secondary-z");
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
