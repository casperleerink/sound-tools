//! The composer: the line about going back, the files it attaches, the field, the two pills
//! and the button that sends, or stops the agent while it works.

use rune::{
    ClipboardContent, El, Key, KeyEvent, TextEdit, Tx, col, el, icon, row, text, text_input,
};
use sound_agent::{Arrow, COMPOSER_PLACEHOLDER};

use super::thread::press;
use super::{Act, COMPOSER, Sidebar, control, icons, menus, with_chat};
use crate::app::State;
use crate::controls::Control;
use crate::theme::Theme;

const SEND: &str = "agent-send";

pub(super) fn view(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let went_back = sidebar
        .chat
        .went_back_line()
        .map(|line| went_back(theme, line));
    let field = text_input(&sidebar.composer, theme.text("body"))
        .key(COMPOSER)
        .label(COMPOSER_PLACEHOLDER)
        .w_full()
        .color(theme.color("text"))
        .placeholder(COMPOSER_PLACEHOLDER, theme.color("text-muted"))
        .caret(theme.color("text"), kit.selection())
        .on_edit(composer_edit)
        // Return sends; shift-Return adds a line.
        .on_submit(|s: &mut State, tx: &mut Tx| press(s, Act::Send, tx))
        .on_input_key(recall)
        .on_input_paste(paste);
    let bottom = row()
        .w_full()
        .gap(theme.space(1))
        .items_center()
        // The pills sit their wash, not their words, on the box's edge.
        .ml(-6.0)
        .child(menus::model_pill(state, sidebar))
        .child(el().w(1.0).h(16.0).fill(theme.color("border-strong")))
        .child(menus::access_pill(state, sidebar))
        .child(el().grow(1.0))
        .child(send_button(theme, sidebar));
    let border = theme.color(if sidebar.dropping {
        "focus"
    } else {
        "border-strong"
    });
    let chips = (!sidebar.chat.attachments().is_empty()).then(|| chips(theme, sidebar));
    let composer = col()
        .key("agent-composer-box")
        .w_full()
        .gap(theme.space(2))
        .pt(14.0)
        .px(14.0)
        .pb(10.0)
        .radius(kit.radius.lg)
        .border(1.0, border)
        .fill(theme.color("surface"))
        .on_drop_files(|s: &mut State, paths: &[std::path::PathBuf], _: &mut Tx| {
            with_chat(s, |sidebar, _| {
                sidebar.dropping = false;
                sidebar.chat.attach_files(paths.to_vec());
            });
            s.handled();
        })
        .on_drop_hover(|s: &mut State, over: bool, _: &mut Tx| {
            with_chat(s, |sidebar, _| sidebar.dropping = over);
            s.handled();
        })
        .children(chips)
        .child(
            el().w_full()
                .min_h(44.0)
                .child(rune::auto_grow(field, 1, 8)),
        )
        .child(bottom);
    col()
        .w_full()
        .gap(theme.space(1))
        .px(theme.space(3))
        .pb(theme.space(3))
        .pt(theme.space(2))
        .children(went_back)
        .child(kit.elevate(composer, &kit.elevation.floating))
}

fn composer_edit(s: &mut State) -> &mut TextEdit {
    match s.panel.agent_mut::<Sidebar>() {
        Some(sidebar) => &mut sidebar.composer,
        // Only the sidebar draws the composer.
        None => Box::leak(Box::default()),
    }
}

/// Up on the first row, or down on the last: the earlier messages, newest first, never over a
/// draft.
fn recall(s: &mut State, key: &KeyEvent, _: &mut Tx) -> bool {
    let arrow = match key.key {
        Key::Up => Arrow::Up,
        Key::Down => Arrow::Down,
        _ => return false,
    };
    if key.mods.shift || key.mods.cmd || key.mods.alt {
        return false;
    }
    let recalled = with_chat(s, |sidebar, _| {
        let edit = &sidebar.composer;
        let caret = edit.focus.min(edit.text.len());
        let (before, after) = edit.text.split_at(caret);
        let edge = match arrow {
            Arrow::Up => !before.contains('\n'),
            Arrow::Down => !after.contains('\n'),
        };
        let recalled = edge
            .then(|| sidebar.chat.recall(&edit.text, arrow))
            .flatten();
        recalled.map(|text| sidebar.set_composer(&text)).is_some()
    });
    let recalled = recalled.unwrap_or(false);
    if recalled {
        s.handled();
    }
    recalled
}

/// Copied files, or a picture, go with the next message. Text is the field's own to paste.
fn paste(s: &mut State, content: &ClipboardContent, _: &mut Tx) -> bool {
    let taken = with_chat(s, |sidebar, _| {
        if !content.files().is_empty() {
            sidebar.chat.attach_files(content.files().to_vec());
            return true;
        }
        match content.image().filter(|_| content.text().is_empty()) {
            Some(picture) => {
                sidebar.chat.attach_picture(picture.png().to_vec());
                sidebar.sync_thumbs();
                true
            }
            None => false,
        }
    });
    let taken = taken.unwrap_or(false);
    if taken {
        s.handled();
    }
    taken
}

/// Not "Stop", which the transport's button is.
const STOP: &str = "Stop the agent";

/// Sends while there is something to send; stops the agent while it works: one turn at a time.
fn send_button(theme: &Theme, sidebar: &Sidebar) -> El {
    let chat = &sidebar.chat;
    let working = chat.is_working();
    let ready = chat.can_send(&sidebar.composer.text);
    let (label, glyph, fill, ink, act) = match working {
        true => (
            STOP,
            &icons::SQUARE,
            theme.color("text"),
            theme.color("background"),
            Act::Stop,
        ),
        false => (
            "Send",
            &icons::ARROW_UP,
            theme.color(if ready { "blue" } else { "fill" }),
            theme.color(if ready { "text-inverse" } else { "text-muted" }),
            Act::Send,
        ),
    };
    let enabled = working || ready;
    let button = row()
        .key(SEND)
        .role("button")
        .label(label)
        .size(28.0, 28.0)
        .center()
        .radius(14.0)
        .fill(fill)
        .disabled(!enabled)
        .child(icon(glyph, 16.0).color(ink));
    match enabled {
        true => theme
            .kit
            .interactive(button.focusable(true))
            .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx)),
        false => button,
    }
}

/// What goes with the next message, each with the way to take it out.
fn chips(theme: &Theme, sidebar: &Sidebar) -> El {
    let chips = sidebar
        .chat
        .attachments()
        .iter()
        .enumerate()
        .map(|(index, attached)| {
            let thumb = attached
                .picture()
                .and_then(|picture| sidebar.thumb(&picture.bytes))
                .map(|image| {
                    el().size(20.0, 20.0)
                        .radius(theme.kit.radius.sm)
                        .clip_rounded(theme.kit.radius.sm)
                        .pixels_fit(image.pixels(), rune::SurfaceFit::Fill)
                });
            let mark = thumb.unwrap_or_else(|| {
                el().size(20.0, 20.0)
                    .center()
                    .child(icon(&icons::PAPERCLIP, 12.0).color(theme.color("text-secondary")))
            });
            let remove = row()
                .key(remove_key(index))
                .role("button")
                .label(format!("Remove {}", attached.name))
                .size(20.0, 20.0)
                .center()
                .radius(theme.kit.radius.sm)
                .focusable(true)
                .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, Act::Remove(index), tx))
                .child(icon(&icons::X, 12.0).color(theme.color("text-secondary")));
            row()
                .key(format!("agent-chip-{index}"))
                .h(28.0)
                .max_w(220.0)
                .gap(6.0)
                .pl(4.0)
                .pr(2.0)
                .items_center()
                .radius(theme.kit.radius.md)
                .fill(theme.color("fill"))
                .child(mark)
                .child(
                    text(attached.name.clone())
                        .font(theme.text("label"))
                        .color(theme.color("text"))
                        .truncate()
                        .shrink(),
                )
                .child(theme.kit.interactive(remove))
        });
    row().w_full().flex_wrap().gap(6.0).children(chips)
}

fn remove_key(index: usize) -> String {
    format!("agent-remove-{index}")
}

/// After going back in the history: the next message tells the agent where to, unless the ×
/// drops it.
fn went_back(theme: &Theme, line: String) -> El {
    let muted = theme.color("text-muted");
    let dismiss = row()
        .key("agent-went-back-dismiss")
        .role("button")
        .label("Do not tell the agent")
        .size(20.0, 20.0)
        .center()
        .radius(theme.kit.radius.sm)
        .focusable(true)
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::DismissWentBack, tx))
        .child(icon(&icons::X, 12.0).color(muted));
    row()
        .key("agent-went-back")
        .w_full()
        .gap(theme.space(1))
        .px(theme.space(1))
        .items_center()
        .child(
            text(line)
                .font(theme.text("label"))
                .color(muted)
                .truncate()
                .grow(1.0)
                .shrink(),
        )
        .child(theme.kit.interactive(dismiss))
}

pub(super) fn controls(sidebar: &Sidebar) -> Vec<Control> {
    let chat = &sidebar.chat;
    if !chat.has_composer() {
        return Vec::new();
    }
    let mut controls = Vec::new();
    if chat.went_back_line().is_some() {
        controls.push(control(
            "agent-went-back-dismiss",
            "Do not tell the agent",
            "button",
            Act::DismissWentBack,
        ));
    }
    for (index, attached) in chat.attachments().iter().enumerate() {
        controls.push(control(
            remove_key(index),
            format!("Remove {}", attached.name),
            "button",
            Act::Remove(index),
        ));
    }
    controls.push(Control {
        value: Some(sidebar.composer.text.clone().into()),
        // What it says goes as a message; `--script` types into it.
        actions: vec!["send".into()],
        ..control(COMPOSER, COMPOSER_PLACEHOLDER, "textbox", Act::Send)
    });
    match chat.is_working() {
        true => controls.push(control(SEND, STOP, "button", Act::Stop)),
        false if chat.can_send(&sidebar.composer.text) => {
            controls.push(control(SEND, "Send", "button", Act::Send));
        }
        false => {}
    }
    controls
}
