//! The Shortcuts section of Settings: every action of the app and of the tools of the project,
//! with the keys that do it. A click on an action, or enter, makes it wait for a key, which is
//! then its one key. Beside it its key is removed, or put back as it was at first.

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, Keystroke, MouseDownEvent, ScrollHandle,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::kbd::Kbd;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::components::tooltip::Tooltip;
use sound_ui::focus::TabStop;
use sound_ui::shortcuts::{self, Group, ShortcutsError};
use sound_ui::{ActiveTheme, Shortcuts};

use super::super::app_keys;
use super::{Section, column, heading};

pub(super) struct ShortcutList {
    /// The action that waits for its key, and what takes the next key before anything else in
    /// the app hears it.
    waiting: Option<(String, Subscription)>,
    /// Why the last change was not kept.
    error: Option<SharedString>,
    scroll: ScrollHandle,
    /// The tab stop of the row of each action, by its id.
    rows: HashMap<String, FocusHandle>,
    /// The action whose row the focus was in at the last frame.
    focused: Option<String>,
}

/// An action as its row shows it.
struct Row {
    id: String,
    title: String,
    /// Its keys as [`Kbd`] takes them.
    keys: Vec<String>,
    changed: bool,
    /// The other actions one of its keys does too, of its own group and of the app: those it
    /// shares the key with where it works.
    also: Vec<String>,
}

fn rows_of(group: &Group, shortcuts: &Shortcuts) -> Vec<Row> {
    let row = |action: &shortcuts::Action| {
        let id = shortcuts::id(&group.name, &action.name);
        let keys = shortcuts.keys(&id);
        let mut also = Vec::new();
        for (other_group, other) in (keys.iter()).flat_map(|key| shortcuts.others_with(&id, key)) {
            let shares = other_group.name == group.name || other_group.name == app_keys::GROUP;
            if shares && !also.contains(&other.title) {
                also.push(other.title.clone());
            }
        }
        Row {
            keys: keys.iter().map(|key| shortcuts::drawn(key)).collect(),
            changed: shortcuts.is_changed(&id),
            also,
            title: action.title.clone(),
            id,
        }
    };
    group.actions.iter().map(row).collect()
}

impl ShortcutList {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        // Tools that load again while the page is open, and a change from another window.
        cx.observe_global::<Shortcuts>(|_, cx| cx.notify()).detach();
        Self {
            waiting: None,
            error: None,
            scroll: ScrollHandle::new(),
            rows: HashMap::new(),
            focused: None,
        }
    }

    fn wait_for_key(&mut self, id: String, cx: &mut Context<Self>) {
        let list = cx.weak_entity();
        let taking = cx.intercept_keystrokes(move |event, _, cx| {
            // Fails only when the page is gone.
            list.update(cx, |list, cx| list.pressed(&event.keystroke, cx))
                .ok();
        });
        self.waiting = Some((id, taking));
        cx.notify();
    }

    fn stop_waiting(&mut self, cx: &mut Context<Self>) {
        if self.waiting.take().is_some() {
            cx.notify();
        }
    }

    /// A key went down while an action waits for one. Tab stays the window's and moves the
    /// focus on. A modifier let go of by itself is no key yet.
    fn pressed(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some((id, _)) = &self.waiting else {
            return;
        };
        let alone = ["shift", "control", "alt", "platform", "function"];
        let chord = shortcuts::chord(keystroke);
        if alone.contains(&keystroke.key.as_str()) {
            return;
        }
        if keystroke.key == "tab" {
            return self.stop_waiting(cx);
        }
        cx.stop_propagation();
        let id = id.clone();
        self.waiting = None;
        if chord != "escape" {
            self.change(cx, |shortcuts| shortcuts.set_keys(&id, vec![chord]));
        }
        cx.notify();
    }

    /// A button of the row of `id` changes its keys. The button may be gone then, and the
    /// focus with it, so the row takes it.
    fn change_from_button(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Shortcuts) -> Result<(), ShortcutsError>,
    ) {
        if let Some(row) = self.rows.get(id) {
            window.focus(row, cx);
        }
        self.change(cx, change);
    }

    /// Changes the keymap, and says so on the page when the change is not kept. A file that
    /// does not read has its own line there.
    fn change(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Shortcuts) -> Result<(), ShortcutsError>,
    ) {
        let changed = change(cx.default_global::<Shortcuts>());
        if let Err(error) = &changed {
            eprintln!("error: {error}");
        }
        self.error = match changed {
            Ok(()) | Err(ShortcutsError::Read { .. }) => None,
            Err(ShortcutsError::Write { error, .. }) => {
                Some(format!("The change could not be kept: {error}").into())
            }
        };
        cx.notify();
    }

    fn row(&mut self, row: Row, last: bool, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (text, muted, accent) = (theme.gray_900, theme.gray_700, theme.lavender);
        let border = theme.alpha_at(0.08);
        let focus = (self.rows.entry(row.id.clone()))
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let waiting = (self.waiting.as_ref()).is_some_and(|(waiting, _)| *waiting == row.id);
        let group = SharedString::from(format!("shortcut-{}", row.id));

        let quiet = |id: &str, icon: &'static str, tooltip: &'static str| {
            let selector = format!("shortcut-{id}-{}", row.id);
            let mut button = Button::icon_only(SharedString::from(selector.clone()), icon)
                .debug_selector(move || selector)
                .variant(ButtonVariant::GhostColor(muted))
                .size(ButtonSize::Xs)
                .tab_stop(true);
            (button.interactivity()).tooltip(move |_, cx| Tooltip::new(tooltip).view(cx));
            button
        };
        let reset = (row.changed && !waiting).then(|| {
            let id = row.id.clone();
            quiet("reset", "undo-2", "Back to its usual key").on_click(cx.listener(
                move |list, _, window, cx| {
                    list.change_from_button(&id, window, cx, |shortcuts| shortcuts.reset(&id))
                },
            ))
        });
        let remove = (!row.keys.is_empty() && !waiting).then(|| {
            let id = row.id.clone();
            quiet("remove", "x", "Remove its key").on_click(cx.listener(
                move |list, _, window, cx| {
                    let none = |shortcuts: &mut Shortcuts| shortcuts.set_keys(&id, Vec::new());
                    list.change_from_button(&id, window, cx, none)
                },
            ))
        });
        // They show with the pointer over the row, and while the focus is in it.
        let buttons = div()
            .flex()
            .flex_none()
            .gap(px(2.))
            .when(!focus.contains_focused(window, cx), |buttons| {
                (buttons.opacity(0.)).group_hover(group.clone(), |buttons| buttons.opacity(1.))
            })
            .children(reset)
            .children(remove);

        let keys = div().flex().flex_none().items_center().gap(px(8.));
        let keys = match (waiting, row.keys.is_empty()) {
            (true, _) => {
                let selector = format!("shortcut-waiting-{}", row.id);
                (keys.text_size(px(12.)).text_color(accent))
                    .debug_selector(move || selector)
                    .font_weight(FontWeight::MEDIUM)
                    .child("Press a key")
            }
            (false, true) => keys.text_size(px(12.)).text_color(muted).child("No key"),
            (false, false) => keys.children(row.keys.into_iter().map(Kbd::new)),
        };
        let also = (!row.also.is_empty()).then(|| {
            let selector = format!("shortcut-also-{}", row.id);
            div()
                .debug_selector(move || selector)
                .truncate()
                .text_size(px(12.))
                .text_color(muted)
                .child(format!("Also on this key: {}", row.also.join(", ")))
        });
        let selector = format!("shortcut-{}", row.id);
        let action = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .group(group)
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(48.))
            // Room for its focus ring around what it shows, which stays in line with the title.
            .mx(px(-8.))
            .px(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_color(text).child(row.title))
                    .children(also),
            )
            .child(buttons)
            .child(keys);
        let id = row.id.clone();
        let action = TabStop::new(SharedString::from(format!("shortcut-row-{id}")), action)
            .focus_handle(&focus)
            .on_click(cx.listener(move |list, _, _, cx| list.wait_for_key(id.clone(), cx)));
        // The sides of the card of its group, and its bottom under the last row.
        column()
            .child(
                div()
                    .px(px(20.))
                    .border_x_1()
                    .border_color(border)
                    .when(last, |card| card.pb(px(8.)).border_b_1().rounded_b(px(12.)))
                    .child(
                        div()
                            .when(!last, |row| row.border_b_1().border_color(border))
                            .when(waiting, |row| {
                                row.on_mouse_down_out(cx.listener(
                                    |list, _: &MouseDownEvent, _, cx| list.stop_waiting(cx),
                                ))
                            })
                            .child(action),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ShortcutList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (strong, muted, border) = (theme.gray_950, theme.gray_700, theme.alpha_at(0.08));
        let (groups, unread) = match cx.try_global::<Shortcuts>() {
            Some(shortcuts) => {
                let groups = shortcuts.groups();
                let groups = groups.map(|group| (group.title.clone(), rows_of(group, shortcuts)));
                (groups.collect(), shortcuts.is_unread())
            }
            None => (Vec::new(), false),
        };
        let unread = unread.then(|| {
            Notice::new(
                "shortcuts-unread",
                "Your own shortcuts could not be read. Every action has its usual key, and a \
                 change here will not be kept.",
            )
            .tone(NoticeTone::Warning)
        });
        let error = self.error.clone().map(|error| {
            Notice::new("shortcuts-error", error).on_dismiss(cx.listener(|list, _, _, cx| {
                list.error = None;
                cx.notify();
            }))
        });
        let mut lines = vec![
            column()
                .pb(px(24.))
                .child(heading(Section::Shortcuts, cx))
                .into_any_element(),
            column()
                .text_color(muted)
                .child("Click an action, then press the key that should do it. Escape leaves it as it is.")
                .into_any_element(),
        ];
        for notice in unread.into_iter().chain(error) {
            lines.push(column().pt(px(16.)).child(notice).into_any_element());
        }
        // The row the focus moved into comes into view, as the keys reach every one of them.
        let mut focused = None;
        for (title, rows) in groups {
            let top = div()
                .px(px(20.))
                .pt(px(20.))
                .pb(px(2.))
                .rounded_t(px(12.))
                .border_t_1()
                .border_x_1()
                .border_color(border)
                .font_weight(FontWeight::MEDIUM)
                .text_color(strong)
                .child(title);
            lines.push(column().pt(px(16.)).child(top).into_any_element());
            let last = rows.len().saturating_sub(1);
            for (index, row) in rows.into_iter().enumerate() {
                let within =
                    (self.rows.get(&row.id)).is_some_and(|row| row.contains_focused(window, cx));
                if within {
                    focused = Some((row.id.clone(), lines.len()));
                }
                lines.push(self.row(row, index == last, window, cx));
            }
        }
        let focused_id = focused.as_ref().map(|(id, _)| id.clone());
        if let Some((_, line)) = focused.filter(|_| focused_id != self.focused) {
            self.scroll.scroll_to_item(line);
        }
        self.focused = focused_id;
        div()
            .id("shortcuts")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(lines)
            .child(div().h(px(32.)))
    }
}
