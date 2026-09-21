//! The click next to a real project: it sounds in the engine the window plays, and it never
//! reaches an offline render.

use metronome::Click;

use crate::support::{BAR, Harness};

/// The `--render` path: a second, read-only project on the same folder, next to the live one.
fn render_offline(harness: &Harness, frames: usize) -> Vec<f32> {
    let (mut project, mut engine, _plugins) =
        runtime::open_read_only(harness.project.root()).unwrap();
    project.engine().play();
    runtime::render(&mut project, &mut engine, frames).unwrap()
}

#[test]
fn a_render_is_the_same_whatever_the_click_does() {
    let mut harness = Harness::piece();
    let quiet = render_offline(&harness, 2 * BAR);

    let mut click = Click::attach(harness.project.engine()).unwrap();
    click.set_on(harness.project.engine(), true).unwrap();
    // The live project, which is what the window plays, now has the click in it.
    let heard = harness.play(2 * BAR);
    assert_ne!(heard, quiet, "the click did not sound in the window");

    // The render path builds its own project and never attaches a click, so it is untouched.
    assert_eq!(render_offline(&harness, 2 * BAR), quiet);
    click.set_on(harness.project.engine(), false).unwrap();
    assert_eq!(render_offline(&harness, 2 * BAR), quiet);
}

#[test]
fn a_click_that_is_off_changes_no_sample() {
    let mut with_click = Harness::piece();
    let mut plain = Harness::piece();
    let mut click = Click::attach(with_click.project.engine()).unwrap();
    assert!(!click.is_on(), "a click starts off");
    assert_eq!(with_click.play(BAR), plain.play(BAR));

    // On and off again: the samples are the same as without a click at all.
    click.set_on(with_click.project.engine(), true).unwrap();
    click.set_on(with_click.project.engine(), false).unwrap();
    with_click.render(BAR / 4);
    plain.render(BAR / 4);
    assert_eq!(with_click.render(BAR), plain.render(BAR));
}
