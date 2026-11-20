//! The Settings page, over the whole window: its sections on the left, the one picked on the
//! right. Agents is what the agent sidebar gives; Audio is the output and the input device;
//! Shortcuts is the keys of every action. Escape or Back gives the keys back to where they were.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use rune::{El, Icon, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant};
use runtime::app::ChosenDevices;
use serde_json::Value;

use super::State;
use super::chrome::Act as ChromeAct;
use crate::controls::{Control, Target};

mod audio;
mod shortcuts;

pub(crate) use audio::DeviceList;

/// How the window lists the audio devices of this machine, or simulated ones. Asking can take
/// a while, so a window asks on a thread of its own.
pub(crate) type ListDevices = Arc<dyn Fn() -> DeviceList + Send + Sync>;

/// The page while it is open, and what it needs of the machine.
pub(crate) struct Settings {
    pub(crate) page: Option<Page>,
    pub(crate) list: ListDevices,
    /// Where the chosen devices are kept. None keeps them for as long as the window.
    file: Option<PathBuf>,
    pub(crate) chosen: ChosenDevices,
    /// The name of the output that plays, which is another than the chosen one when that is
    /// not there. Empty when no device plays.
    pub(crate) playing: String,
    /// The words the Shortcuts section looks for; a field outlives the page that shows it.
    pub(crate) search: rune::TextEdit,
}

pub(crate) struct Page {
    pub(crate) section: Section,
    /// What had the keys before the page took them.
    back: Option<String>,
    /// The devices to pick from, once listed.
    pub(crate) devices: Option<DeviceList>,
    listing: Option<Receiver<DeviceList>>,
    /// The action that waits for its key, by id.
    pub(crate) waiting: Option<String>,
    /// Why the last change of a key was not kept.
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Agents,
    Audio,
    Shortcuts,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Audio => "Audio",
            Self::Shortcuts => "Shortcuts",
        }
    }

    fn icon(self) -> &'static Icon {
        match self {
            Self::Agents => &BOT,
            Self::Audio => &VOLUME,
            Self::Shortcuts => &KEYBOARD,
        }
    }

    fn key(self) -> String {
        format!("settings-{}", self.name().to_lowercase())
    }
}

/// What a control of the page does, by key or by `--act`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    Back,
    Show(Section),
    /// The device of this row of the output's list, or of the input's.
    Device {
        output: bool,
        row: usize,
    },
    Shortcut(shortcuts::Act),
    Search,
}

/// The Agents section, which the agent sidebar fills: `crate::sidebar::settings::agents_settings`.
fn agents(_: &State) -> Option<El> {
    None
}

impl Settings {
    /// `file` None keeps the chosen devices for as long as the window; `playing` names the
    /// output that plays.
    pub(crate) fn new(list: ListDevices, file: Option<PathBuf>, playing: String) -> Self {
        let chosen = file.as_deref().map(ChosenDevices::read).unwrap_or_default();
        Self {
            page: None,
            list,
            file,
            chosen,
            playing,
            search: rune::TextEdit::new(""),
        }
    }

    /// The devices of this machine, as a window lists them.
    pub(crate) fn machine() -> ListDevices {
        Arc::new(|| DeviceList {
            // A failure leaves a list empty, and the page offers the default.
            outputs: sound_core::output_devices().unwrap_or_default(),
            inputs: sound_core::input_devices().unwrap_or_default(),
        })
    }

    /// No devices: a run without a window does not show this machine's.
    pub(crate) fn none() -> ListDevices {
        Arc::new(DeviceList::default)
    }
}

impl State {
    fn sections(&self) -> impl Iterator<Item = Section> {
        let agents = agents(self).map(|_| Section::Agents);
        agents
            .into_iter()
            .chain([Section::Audio, Section::Shortcuts])
    }

    /// Opens the page on its first section, with the keys on Back. Nothing when it is open.
    pub(crate) fn open_settings(&mut self, tx: &mut Tx) {
        if self.settings.page.is_some() {
            return;
        }
        let back = tx.focused().map(super::menu::outside);
        let section = self.sections().next().unwrap_or(Section::Audio);
        self.settings.page = Some(Page {
            section,
            back,
            devices: None,
            listing: None,
            waiting: None,
            error: None,
        });
        self.settings.search = rune::TextEdit::new("");
        self.list_devices();
        tx.focus_when_shown(BACK, true);
        self.handled();
    }

    pub(crate) fn close_settings(&mut self, tx: &mut Tx) {
        let Some(page) = self.settings.page.take() else {
            return;
        };
        match page.back {
            Some(back) => tx.focus_when_shown(back, false),
            None => tx.blur(),
        }
        self.handled();
    }

    fn show_section(&mut self, section: Section) {
        let Some(page) = &mut self.settings.page else {
            return;
        };
        page.section = section;
        page.waiting = None;
        if section == Section::Audio {
            self.list_devices();
        }
    }

    /// Lists the devices each time Audio shows, so an interface plugged in since is offered.
    /// Never on a timer: listing can take a while, and on Linux it opens each device. A run
    /// without a window lists at once, as its clock does not wait for a thread.
    fn list_devices(&mut self) {
        let list = self.settings.list.clone();
        let headless = self.headless();
        let Some(page) = &mut self.settings.page else {
            return;
        };
        if headless {
            page.devices = Some(list());
            return;
        }
        let (send, listed) = channel();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            if send.send(list()).is_ok()
                && let Some(wake) = waker.lock().ok().as_deref().and_then(Option::as_ref)
            {
                wake();
            }
        });
        page.listing = Some(listed);
    }

    /// The devices a thread listed, for the page. Whether they came now.
    pub(crate) fn devices_listed(&mut self) -> bool {
        let Some(page) = &mut self.settings.page else {
            return false;
        };
        let Some(listed) = page
            .listing
            .as_ref()
            .and_then(|listing| listing.try_recv().ok())
        else {
            return false;
        };
        page.devices = Some(listed);
        page.listing = None;
        true
    }

    /// Picks the device of `row` of the output's list or the input's, the first being the
    /// system's default. It is kept for this machine, never in the project, and the app starts
    /// again on it: the engine runs at the rate of its output.
    fn pick_device(&mut self, output: bool, row: usize, tx: &mut Tx) {
        let Some(devices) = self
            .settings
            .page
            .as_ref()
            .and_then(|page| page.devices.as_ref())
        else {
            return;
        };
        let list = if output {
            &devices.outputs
        } else {
            &devices.inputs
        };
        let device = match row {
            0 => None,
            row => match list.get(row - 1) {
                Some(device) => Some(device),
                None => return,
            },
        };
        let (id, name) = (device.map(|d| d.id.clone()), device.map(|d| d.name.clone()));
        let mut chosen = self.settings.chosen.clone();
        match output {
            true => (chosen.output, chosen.output_name) = (id, name),
            false => (chosen.input, chosen.input_name) = (id, name),
        }
        if chosen == self.settings.chosen {
            return;
        }
        if let Some(file) = &self.settings.file
            && let Err(error) = chosen.remember(file)
        {
            self.session.report(format!("{error:#}"));
            return;
        }
        self.settings.chosen = chosen;
        if !self.headless() {
            crate::shell::start_again(tx);
        }
    }
}

/// The keys of the window while the page shows: Escape gives them back. Whether it took `key`.
pub(super) fn key(state: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    if !matches!(key.key, Key::Escape) {
        return false;
    }
    match state.settings.page.as_mut() {
        Some(page) if page.waiting.is_some() => page.waiting = None,
        _ => state.close_settings(tx),
    }
    true
}

const BACK: &str = "settings-back";
/// As wide as the left panel, so the page lines up with the window under it.
const SIDE: f32 = 400.0;
const TOP: f32 = 48.0;
/// The column a section is laid out in, in the middle of the page.
const COLUMN: f32 = 640.0;

/// The page in place of the window, while it is open.
pub(super) fn view(state: &State) -> Option<El> {
    let page = state.settings.page.as_ref()?;
    let theme = &state.theme;
    let body = match page.section {
        Section::Agents => agents(state).unwrap_or_else(el),
        Section::Audio => audio::view(state, page),
        Section::Shortcuts => shortcuts::view(state, page),
    };
    let section = col()
        .key("settings-section")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .w_full()
                .max_w(COLUMN)
                .px(40.0)
                .pb(theme.space(8))
                .gap(theme.space(6))
                .self_center()
                .child(
                    text(page.section.name())
                        .font(theme.text("title"))
                        .color(theme.color("text")),
                )
                .child(body),
        );
    let content = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(el().w_full().h(TOP).window_drag())
        .child(section);
    Some(
        row()
            .key("settings")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .on_key(key)
            .on_background_press(|s: &mut State| {
                if let Some(page) = &mut s.settings.page {
                    page.waiting = None;
                }
            })
            .child(sections(state, page))
            .child(content),
    )
}

fn sections(state: &State, page: &Page) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let back = Button::new("Back")
        .icon(&ARROW_LEFT)
        .variant(ButtonVariant::GhostMuted)
        .on_press_tx(|s: &mut State, tx: &mut Tx| s.close_settings(tx))
        .build(kit)
        .key(BACK);
    let rows = state.sections().map(|section| {
        let picked = section == page.section;
        let ink = theme.color(if picked { "text" } else { "text-muted" });
        let e = row()
            .key(section.key())
            .role("tab")
            .label(section.name())
            .checked(picked)
            .w_full()
            .h(32.0)
            .px(10.0)
            .gap(10.0)
            .items_center()
            .radius(theme.radius("lg").unwrap_or(8.0))
            .fill(match picked {
                true => theme.color("selected"),
                false => rune::Color::TRANSPARENT,
            })
            .focusable(true)
            .on_press(move |s: &mut State| s.show_section(section))
            .child(icon(section.icon(), 16.0).color(ink).a11y_hidden())
            .child(
                text(section.name())
                    .font(theme.text("heading"))
                    .color(ink)
                    .grow(1.0)
                    .a11y_hidden(),
            )
            .child(
                icon(kit.glyph.caret_right, 14.0)
                    .color(theme.color("text-muted"))
                    .a11y_hidden(),
            );
        kit.interactive(e)
    });
    col()
        .key("settings-sections")
        .w(SIDE)
        .h_full()
        .border_sides([0.0, 1.0, 0.0, 0.0], theme.color("border"))
        .child(el().w_full().h(TOP).window_drag())
        .child(
            col()
                .px(12.0)
                .gap(2.0)
                .child(row().pb(14.0).child(back))
                .child(
                    col()
                        .role("tablist")
                        .label("Sections")
                        .focus_group()
                        .gap(2.0)
                        .children(rows),
                ),
        )
        .child(el().w_full().grow(1.0).window_drag())
        // The device rows write "VST" for such plugins, so the app owes this line.
        .child(
            text(plugin_host::VST_TRADEMARK)
                .font(theme.text("label"))
                .color(theme.color("text-muted"))
                .wrap()
                .px(22.0)
                .pb(16.0),
        )
}

/// The controls of the page, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Vec<Control> {
    let Some(page) = &state.settings.page else {
        return Vec::new();
    };
    let mut controls = vec![control(BACK.into(), "Back", "button", Act::Back)];
    controls.extend(state.sections().map(|section| Control {
        checked: Some(section == page.section),
        ..control(section.key(), section.name(), "tab", Act::Show(section))
    }));
    match page.section {
        Section::Agents => {}
        Section::Audio => controls.extend(audio::controls(state, page)),
        Section::Shortcuts => controls.extend(shortcuts::controls(state, page)),
    }
    controls
}

fn control(key: String, label: &str, role: &str, act: Act) -> Control {
    Control {
        key,
        label: label.into(),
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        target: Target::Chrome(ChromeAct::Settings(act)),
    }
}

/// Runs `act` for `--act`, with the text of the search or a key for an action.
pub(super) fn run(
    state: &mut State,
    act: Act,
    action: &str,
    value: Option<Value>,
) -> anyhow::Result<()> {
    let tx = &mut Tx::default();
    match act {
        Act::Back => state.close_settings(tx),
        Act::Show(section) => state.show_section(section),
        Act::Device { output, row } => state.pick_device(output, row, tx),
        Act::Shortcut(act) => shortcuts::run(state, act, action, value)?,
        Act::Search => {
            let Some(Value::String(words)) = value else {
                anyhow::bail!("the search takes its words as the value");
            };
            state.settings.search = rune::TextEdit::new(words);
        }
    }
    Ok(())
}

macro_rules! lucide {
    ($name:ident, $key:literal, $body:literal) => {
        static $name: Icon = Icon {
            name: $key,
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        };
    };
}
lucide!(
    VOLUME,
    "settings-volume",
    r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#
);
lucide!(
    KEYBOARD,
    "settings-keyboard",
    r#"<path d="M10 8h.01"/><path d="M12 12h.01"/><path d="M14 8h.01"/><path d="M16 12h.01"/><path d="M18 8h.01"/><path d="M6 8h.01"/><path d="M7 16h10"/><path d="M8 12h.01"/><rect width="20" height="16" x="2" y="4" rx="2"/>"#
);
lucide!(
    BOT,
    "settings-bot",
    r#"<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>"#
);
lucide!(
    MIC,
    "settings-mic",
    r#"<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" x2="12" y1="19" y2="22"/>"#
);
lucide!(
    UNDO,
    "settings-undo",
    r#"<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>"#
);
lucide!(
    ARROW_LEFT,
    "settings-back",
    r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#
);
