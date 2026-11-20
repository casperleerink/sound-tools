//! The ruler of the song page, driven by the pointer as a composer drives it.

use super::*;

fn window() -> (tempfile::TempDir, rune::Runtime<State>) {
    let scratch = new_project();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    redraw(&mut window);
    (scratch, window)
}

/// Frames until the clock's answer is back in the view.
fn answered(window: &mut rune::Runtime<State>) {
    for _ in 0..6 {
        redraw(window);
    }
}

fn repeat(state: &State) -> Value {
    state.report()["repeat"].clone()
}

/// Points of a beat at the zoom a song opens with.
const BEAT: f32 = 24.0;

#[test]
fn a_drag_over_the_ruler_repeats_its_beats_and_an_end_of_the_bar_stretches_it_on_the_beats_as_no_step_of_undo()
 {
    let (_scratch, mut window) = window();
    let before = window.state.project().undo_label();
    let [x, y, ..] = window.presented("ruler").expect("the ruler").0;
    // From a little past bar 2 over nine beats and a bit: bars 2 to 4 and a beat.
    window.pointer_down(x + 4.0 * BEAT + 5.0, y + 30.0);
    window.frame(1.0 / 60.0);
    let end = travel(
        &mut window,
        (x + 4.0 * BEAT + 5.0, y + 30.0),
        (BEAT * 9.2 / 8.0, 0.0),
        8,
    );
    window.pointer_up(end.0, end.1);
    answered(&mut window);
    assert_eq!(repeat(&window.state), json!([3840, 3840 + 9 * 960]));
    // Its end, a beat and a half later, lands on the beat after the next.
    let (ex, ey) = window.find("repeat-end").expect("the end of the repeat");
    window.pointer_down(ex, ey);
    window.frame(1.0 / 60.0);
    let end = travel(&mut window, (ex, ey), (BEAT * 1.6 / 8.0, 0.0), 8);
    window.pointer_up(end.0, end.1);
    answered(&mut window);
    assert_eq!(repeat(&window.state), json!([3840, 3840 + 11 * 960]));
    assert_eq!(window.state.project().undo_label(), before);
}

#[test]
fn a_double_press_on_the_ruler_adds_a_section_at_the_nearest_bar_whose_name_is_typed_in_place() {
    let (_scratch, mut window) = window();
    let [x, y, ..] = window.presented("ruler").expect("the ruler").0;
    // A little before bar 3: the section starts on its line.
    let at = (x + 8.0 * BEAT - 10.0, y + 30.0);
    for _ in 0..2 {
        window.pointer_down(at.0, at.1);
        window.frame(1.0 / 60.0);
        window.pointer_up(at.0, at.1);
        window.frame(1.0 / 60.0);
    }
    answered(&mut window);
    assert_eq!(focused(&window).as_deref(), Some("section-name"));
    for letter in ["V", "e", "r", "s", "e"] {
        key(&mut window, rune::Key::Text(letter.into()));
    }
    key(&mut window, rune::Key::Enter);
    answered(&mut window);
    let sections = &window.state.project().project_file().sections;
    assert_eq!(
        serde_json::to_value(sections).unwrap(),
        json!([{ "bar": 3, "name": "Verse" }])
    );
    assert_eq!(
        window.state.project().undo_label().as_deref(),
        Some("Name a section")
    );
    assert!(window.find("section-3").is_some());
}
