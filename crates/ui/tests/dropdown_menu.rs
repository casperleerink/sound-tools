//! The dropdown menu with a keyboard, while its items change under it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{SharedString, TestAppContext};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked,
};

fn entries(undo_label: &str, values: &[&'static str]) -> Vec<MenuEntry> {
    let items = values
        .iter()
        .map(|value| MenuItem::new(*value, format!("{undo_label} {value}")).selectable(false));
    vec![MenuEntry::Group(MenuGroup::new().items(items))]
}

#[gpui::test]
fn new_labels_keep_the_highlight_and_new_items_drop_it(cx: &mut TestAppContext) {
    cx.update(sound_ui::init);
    let (menu, cx) = cx.add_window_view(|_, cx| {
        DropdownMenu::new("Project", entries("first", &["add", "undo", "redo"]), cx)
    });
    let picked: Rc<RefCell<Vec<SharedString>>> = Rc::default();
    cx.update(|_, cx| {
        let picked = picked.clone();
        cx.subscribe(&menu, move |_, event: &MenuPicked, _| {
            picked.borrow_mut().push(event.0.clone())
        })
        .detach();
    });

    // Down twice highlights `undo`. Its label changes, as after an edit from outside.
    menu.update_in(cx, |menu, window, cx| menu.open(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("down down");
    menu.update(cx, |menu, cx| {
        menu.set_entries(entries("second", &["add", "undo", "redo"]), cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert_eq!(*picked.borrow(), ["undo"]);
    assert!(!menu.read_with(cx, |menu, _| menu.is_open()));

    // Other items: the old index would point at something else, so the highlight goes.
    menu.update_in(cx, |menu, window, cx| menu.open(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("down down");
    menu.update(cx, |menu, cx| {
        menu.set_entries(entries("third", &["add", "redo"]), cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert_eq!(*picked.borrow(), ["undo"]);
    assert!(menu.read_with(cx, |menu, _| menu.is_open()));
}

#[gpui::test]
fn enter_on_a_row_that_turned_disabled_picks_nothing(cx: &mut TestAppContext) {
    cx.update(sound_ui::init);
    let (menu, cx) = cx.add_window_view(|_, cx| {
        DropdownMenu::new("Project", entries("first", &["add", "undo"]), cx)
    });
    let picked: Rc<RefCell<Vec<SharedString>>> = Rc::default();
    cx.update(|_, cx| {
        let picked = picked.clone();
        cx.subscribe(&menu, move |_, event: &MenuPicked, _| {
            picked.borrow_mut().push(event.0.clone())
        })
        .detach();
    });
    menu.update_in(cx, |menu, window, cx| menu.open(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("down down");
    // The same items, so the highlight stays on `undo`, which has nothing to undo now.
    let items = ["add", "undo"].map(|value| {
        MenuItem::new(value, value)
            .selectable(false)
            .disabled(value == "undo")
    });
    let disabled = vec![MenuEntry::Group(MenuGroup::new().items(items))];
    menu.update(cx, |menu, cx| menu.set_entries(disabled, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    assert!(picked.borrow().is_empty());
}
