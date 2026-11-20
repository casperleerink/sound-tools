//! The composer's two pills and what they open: the model picker and the access menu; and
//! **New** beside the panel's tabs, with its menu of recent threads and instructions.

use rune::{Align, El, Key, KeyEvent, ScrollSpec, Side, TextEdit, Tx, col, icon, row, text, text_input};
use rune_kit::{Button, ButtonVariant, ControlSize, MenuEntry, MenuItem, Placement};
use sound_agent::{
    Chat, INSTRUCTIONS, NEW, NEW_MENU, NEW_THREAD, OPEN_RECENT, Provider, SEARCH_MODELS, Scope,
    Shows,
};

use super::thread::press;
use super::{Act, Menu, Sidebar, control, icons};
use crate::app::State;
use crate::controls::Control;
use crate::theme::Theme;

pub(super) const MODEL_PILL: &str = "agent-model";
/// Keyed as rune-kit's menus anchor: their key and `-trigger`.
pub(super) const ACCESS_PILL: &str = "agent-access-trigger";
const NEW_BUTTON: &str = "agent-new";
const NEW_MENU_BUTTON: &str = "agent-new-menu-trigger";
const SEARCH: &str = "agent-model-search";
const PICKER_WIDTH: f32 = 340.0;
const PICKER_HEIGHT: f32 = 420.0;

/// A pill of the composer: an icon, its words, and a chevron.
fn pill(theme: &Theme, key: &str, label: String, mark: Option<El>, open: bool, menu: Menu) -> El {
    let muted = theme.color("text-secondary");
    let e = row()
        .key(key)
        .role("button")
        .label(label.clone())
        .expanded(open)
        .h(28.0)
        .px(6.0)
        .gap(6.0)
        .items_center()
        .radius(theme.kit.radius.md)
        .focusable(true)
        .fill(if open {
            theme.color("fill")
        } else {
            rune::Color::TRANSPARENT
        })
        .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, Act::Menu(menu), tx))
        .children(mark)
        .child(text(label).font(theme.text("label")).color(muted).truncate().shrink())
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(theme.color("text-muted")));
    theme.kit.interactive(e).shrink()
}

/// The model the agent runs, and the way to pick another.
pub(super) fn model_pill(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let chat = &sidebar.chat;
    let logo = chat.model_provider().map(|provider| icon(icons::logo(provider), 14.0));
    let open = sidebar.menu == Some(Menu::Model);
    pill(theme, MODEL_PILL, chat.model_label(), logo, open, Menu::Model)
}

/// How much the agent may do without asking. It always says so, so it is never a surprise.
pub(super) fn access_pill(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let mode = sidebar.chat.approval_mode();
    let mark = icon(icons::named(mode.icon()), 14.0).color(theme.color("text-secondary"));
    let open = sidebar.menu == Some(Menu::Access);
    pill(theme, ACCESS_PILL, mode.label().into(), Some(mark), open, Menu::Access)
}

/// **New** beside the tabs, with its menu, while there is a thread to leave.
pub(super) fn new_button(state: &State, sidebar: &Sidebar) -> Option<El> {
    if !has_new(&sidebar.chat) {
        return None;
    }
    let kit = &state.theme.kit;
    let new = Button::new(NEW)
        .icon(&icons::PLUS)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .help(NEW_THREAD)
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::NewThread, tx))
        .build(kit)
        .key(NEW_BUTTON);
    let open = matches!(sidebar.menu, Some(Menu::New | Menu::Recent));
    let more = rune_kit::IconButton::new(&icons::CHEVRON_DOWN)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(NEW_MENU)
        .expanded(open)
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::Menu(Menu::New), tx))
        .build(kit)
        .key(NEW_MENU_BUTTON);
    Some(row().gap(2.0).items_center().child(new).child(more))
}

fn has_new(chat: &Chat) -> bool {
    matches!(chat.shows(), Shows::Thread(_) | Shows::Instructions)
}

/// What is open over the window: the model picker, or a menu. Mounted at the window's root,
/// after what opens them.
pub(crate) fn overlays(state: &State) -> Vec<El> {
    let Some(sidebar) = super::sidebar(state) else {
        return Vec::new();
    };
    if !state.panel.open || state.panel.tab != crate::app::Tab::Agent {
        return Vec::new();
    }
    let kit = &state.theme.kit;
    let mut layer = Vec::new();
    let close = |s: &mut State| {
        if let Some(sidebar) = s.panel.agent_mut::<Sidebar>() {
            sidebar.menu = None;
        }
    };
    match sidebar.menu {
        Some(Menu::Model) => layer.push(picker(state, sidebar)),
        Some(Menu::Access) => {
            let items = sidebar.chat.access_choices().map(|choice| {
                let act = Act::Access(choice.mode);
                MenuItem::new(choice.mode.label())
                    .icon(icons::named(choice.mode.icon()))
                    .description(choice.mode.description())
                    .checked(choice.checked)
                    .on_select_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx))
            });
            let menu = items
                .into_iter()
                .fold(rune_kit::Menu::new("agent-access"), rune_kit::Menu::item);
            menu.placement(Placement::TopStart)
                .min_width(300.0)
                .open(true)
                .on_close(close)
                .build(kit, &mut layer);
        }
        Some(menu @ (Menu::New | Menu::Recent)) => {
            let chat = &sidebar.chat;
            let recent: Vec<MenuEntry> = (chat.recent_threads().iter().enumerate())
                .map(|(index, thread)| {
                    let act = Act::OpenRecent(index);
                    MenuEntry::item(
                        MenuItem::new(thread.title.clone())
                            .on_select_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx)),
                    )
                })
                .collect();
            let none = recent.is_empty();
            let instructions = [Scope::Project, Scope::AllProjects].map(|scope| {
                let act = Act::Instructions(scope);
                MenuItem::new(scope.label())
                    .disabled(!chat.can_open_instructions(scope))
                    .on_select_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx))
            });
            let menu = rune_kit::Menu::new("agent-new-menu")
                .item(
                    MenuItem::new(NEW_THREAD)
                        .icon(&icons::PLUS)
                        .on_select_tx(|s: &mut State, tx: &mut Tx| press(s, Act::NewThread, tx)),
                )
                .item(
                    MenuItem::new(OPEN_RECENT)
                        .disabled(none)
                        .submenu(recent, menu == Menu::Recent)
                        .on_select_tx(|s: &mut State, tx: &mut Tx| {
                            press(s, Act::Menu(Menu::Recent), tx);
                        }),
                )
                .separator()
                .group_label(INSTRUCTIONS);
            instructions
                .into_iter()
                .fold(menu, rune_kit::Menu::item)
                .placement(Placement::BottomEnd)
                .min_width(220.0)
                .open(true)
                .on_close(close)
                .build(kit, &mut layer);
        }
        None => {}
    }
    layer
}

/// The models of each provider signed in, by name, with a search over them. The arrows move
/// the lit row and Return picks it; Escape or a press outside closes it.
fn picker(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let groups = sidebar.chat.model_groups(&sidebar.search.text);
    let several = groups.len() > 1;
    let mut row_number = 0;
    let mut list = col().key("agent-models").w_full().padding(4.0);
    for group in groups {
        if several {
            list = list.child(
                row()
                    .h(28.0)
                    .px(8.0)
                    .gap(theme.space(2))
                    .items_center()
                    .child(icon(icons::logo(group.provider), 14.0))
                    .child(
                        text(group.provider.name())
                            .font(kit.ty.small_medium)
                            .color(theme.color("text-secondary")),
                    ),
            );
        }
        if let Some(off) = group.off {
            list = list.child(quiet_line(theme, off));
        }
        if let Some(empty) = group.empty {
            list = list.child(quiet_line(theme, empty.into()));
        }
        for model in group.models {
            let at = row_number;
            row_number += 1;
            let lit = sidebar.lit == at;
            let second = model.second.map(|second| {
                text(second)
                    .font(theme.text("label"))
                    .color(theme.color("text-muted"))
                    .truncate()
            });
            let check = icon(&icons::CHECK, 14.0)
                .color(theme.color("text"))
                .opacity(if model.picked { 1.0 } else { 0.0 });
            let e = row()
                .key(model_key(at))
                .role("option")
                .label(model.name)
                .checked(model.picked)
                .min_h(32.0)
                .py(4.0)
                .px(8.0)
                .gap(theme.space(2))
                .items_center()
                .radius(kit.radius.sm)
                .fill(if lit {
                    theme.color("fill")
                } else {
                    rune::Color::TRANSPARENT
                })
                .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, Act::Pick(at), tx))
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(text(model.name).font(theme.text("body")).color(theme.color("text")).truncate())
                        .children(second),
                )
                .child(check);
            list = list.child(kit.interactive(e));
        }
    }
    let search = text_input(&sidebar.search, theme.text("body"))
        .single_line()
        .key(SEARCH)
        .label(SEARCH_MODELS)
        .w_full()
        .color(theme.color("text"))
        .placeholder(SEARCH_MODELS, theme.color("text-muted"))
        .caret(theme.color("text"), kit.selection())
        .on_edit(search_edit)
        .on_input_change(|s: &mut State, _: &mut Tx| {
            if let Some(sidebar) = s.panel.agent_mut::<Sidebar>() {
                sidebar.lit = 0;
            }
        })
        .on_input_key(picker_key);
    let search = row()
        .h(36.0)
        .px(12.0)
        .gap(theme.space(2))
        .items_center()
        .border_sides([0.0, 0.0, 0.0, 1.0], theme.color("border"))
        .child(icon(&icons::SEARCH, 14.0).color(theme.color("text-muted")))
        .child(search);
    let panel = col()
        .key("agent-model-picker")
        .role("dialog")
        .label(SEARCH_MODELS)
        .w(PICKER_WIDTH)
        .max_h(PICKER_HEIGHT)
        .radius(kit.radius.lg)
        .fill(theme.color("raised"))
        .border(1.0, theme.color("border-strong"))
        .child(search)
        .child(
            col()
                .min_h(0.0)
                .shrink()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        );
    kit.elevate(panel, &kit.elevation.floating)
        .anchored(MODEL_PILL, Side::Top, Align::Start, 6.0)
        .modal(true, |s: &mut State| {
            if let Some(sidebar) = s.panel.agent_mut::<Sidebar>() {
                sidebar.menu = None;
            }
        })
        .focus_return(MODEL_PILL)
}

fn quiet_line(theme: &Theme, words: String) -> El {
    text(words)
        .font(theme.text("label"))
        .color(theme.color("text-muted"))
        .wrap()
        .px(8.0)
        .py(6.0)
}

fn model_key(row: usize) -> String {
    format!("agent-model-{row}")
}

fn search_edit(s: &mut State) -> &mut TextEdit {
    match s.panel.agent_mut::<Sidebar>() {
        Some(sidebar) => &mut sidebar.search,
        // Only the sidebar draws the search.
        None => Box::leak(Box::default()),
    }
}

/// The arrows move the lit row, Return picks it.
fn picker_key(s: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(sidebar) = s.panel.agent_mut::<Sidebar>() else {
        return false;
    };
    let rows = pickable(&sidebar.chat, &sidebar.search.text).len();
    match key.key {
        Key::Down if rows > 0 => sidebar.lit = (sidebar.lit + 1).min(rows - 1),
        Key::Up => sidebar.lit = sidebar.lit.saturating_sub(1),
        Key::Enter => {
            let lit = sidebar.lit;
            if lit < rows {
                press(s, Act::Pick(lit), tx);
            }
            return true;
        }
        _ => return false,
    }
    let lit = sidebar.lit;
    tx.scroll_into_view(model_key(lit));
    s.handled();
    true
}

/// The models the picker lists for `query`, in its order.
pub(super) fn pickable(chat: &Chat, query: &str) -> Vec<(Provider, String)> {
    (chat.model_groups(query).into_iter())
        .flat_map(|group| {
            let provider = group.provider;
            (group.models.into_iter()).map(move |model| (provider, model.id.to_string()))
        })
        .collect()
}

/// The pills, **New** and what they opened, for `--tree` and `--act`.
pub(super) fn controls(sidebar: &Sidebar) -> Vec<Control> {
    let chat = &sidebar.chat;
    let mut controls = Vec::new();
    let menu = sidebar.menu;
    if has_new(chat) {
        controls.push(control(NEW_BUTTON, NEW, "button", Act::NewThread));
        controls.push(Control {
            expanded: Some(matches!(menu, Some(Menu::New | Menu::Recent))),
            ..control(NEW_MENU_BUTTON, NEW_MENU, "button", Act::Menu(Menu::New))
        });
    }
    if chat.has_composer() {
        controls.push(Control {
            expanded: Some(menu == Some(Menu::Model)),
            ..control(MODEL_PILL, chat.model_label(), "button", Act::Menu(Menu::Model))
        });
        controls.push(Control {
            expanded: Some(menu == Some(Menu::Access)),
            ..control(
                ACCESS_PILL,
                chat.approval_mode().label(),
                "button",
                Act::Menu(Menu::Access),
            )
        });
    }
    match menu {
        Some(Menu::Model) => {
            let groups = chat.model_groups(&sidebar.search.text);
            let rows = groups.into_iter().flat_map(|group| group.models);
            controls.extend(rows.enumerate().map(|(at, model)| Control {
                checked: Some(model.picked),
                ..control(model_key(at), model.name, "option", Act::Pick(at))
            }));
        }
        Some(Menu::Access) => controls.extend(chat.access_choices().map(|choice| Control {
            checked: Some(choice.checked),
            ..control(
                format!("agent-access-{}", choice.mode.icon()),
                choice.mode.label(),
                "menuitemcheckbox",
                Act::Access(choice.mode),
            )
        })),
        Some(Menu::New | Menu::Recent) => {
            controls.push(control("agent-new-thread", NEW_THREAD, "menuitem", Act::NewThread));
            if !chat.recent_threads().is_empty() {
                controls.push(Control {
                    expanded: Some(menu == Some(Menu::Recent)),
                    ..control("agent-open-recent", OPEN_RECENT, "menuitem", Act::Menu(Menu::Recent))
                });
            }
            if menu == Some(Menu::Recent) {
                controls.extend((chat.recent_threads().iter().enumerate()).map(|(index, thread)| {
                    control(
                        format!("agent-recent-{index}"),
                        thread.title.clone(),
                        "menuitem",
                        Act::OpenRecent(index),
                    )
                }));
            }
            for scope in [Scope::Project, Scope::AllProjects] {
                if chat.can_open_instructions(scope) {
                    controls.push(control(
                        format!("agent-instructions-{}", scope.label()),
                        scope.label(),
                        "menuitem",
                        Act::Instructions(scope),
                    ));
                }
            }
        }
        None => {}
    }
    controls
}
