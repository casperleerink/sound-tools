//! The agent sidebar, the Agent tab of the left panel: [`sound_agent::Chat`] drawn with Rune.
//! The chat holds the rules; this holds what only a window has (the composer's text, which menu
//! is open, the focus) and runs what a control asks, for a pointer, a key and `--act` alike.

mod composer;
mod icons;
mod menus;
mod scenes;
pub(crate) mod settings;
mod thread;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rune::{El, TextEdit, Tx};
use sound_agent::{
    AgentSettings, Agents, ApprovalAnswer, ApprovalMode, Chat, FIX_NOT_LIVE, Provider, Scope,
};
use sound_core::{Project, ProjectEvent};

use crate::app::{PanelTab, State};
use crate::controls::{Control, Target};

pub(crate) use menus::overlays;
pub(crate) use scenes::SCENES;

/// The keys tests, `--act` and the panel's cmd-L reach.
const COMPOSER: &str = "agent-composer";
const INSTRUCTIONS: &str = "agent-instructions";
const THREAD: &str = "agent-thread";

/// What the sidebar keeps that the chat does not: what only a window has.
pub(crate) struct Sidebar {
    chat: Chat,
    composer: TextEdit,
    /// The instructions' text while they show, once read.
    instructions: Option<TextEdit>,
    menu: Option<Menu>,
    /// The search of the model picker, and the row its arrows lit.
    search: TextEdit,
    lit: usize,
    /// Files are dragged over the composer.
    dropping: bool,
    /// What the chat's background work handed in since the last frame woke it.
    woken: Arc<AtomicBool>,
    /// What the working line said last, so a frame draws it again only when it changed.
    elapsed: Option<String>,
    /// The pictures of the chips, by the bytes they show.
    images: rune::ImageCache,
    thumbs: Vec<(usize, rune::Image)>,
    pages: Vec<String>,
    settings: settings::Page,
    /// A picture of the Agents section of Settings, until the Settings page holds it.
    shows_settings: bool,
}

/// The menu open now. Only one is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Menu {
    Model,
    Access,
    New,
    /// **New**'s menu with **Open recent** open.
    Recent,
}

/// The Agent tab of a window on `project`. A run without a window finds no agent and saves
/// nothing: its pictures feed the chat recorded events ([`SCENES`]).
pub(crate) fn open(
    project: &Project,
    headless: bool,
    waker: &Arc<Mutex<Option<rune::Waker>>>,
) -> Option<Box<dyn PanelTab>> {
    let woken = Arc::new(AtomicBool::new(false));
    let wake: rune::Waker = {
        let (waker, woken) = (waker.clone(), woken.clone());
        Arc::new(move || {
            woken.store(true, Ordering::Relaxed);
            if let Some(wake) = waker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                wake();
            }
        })
    };
    let agent_waker = {
        let wake = wake.clone();
        sound_agent::Waker::new(move || wake())
    };
    let support = (!headless)
        .then(runtime::app::support_folder)
        .and_then(Result::ok);
    let file = support.as_deref().map(runtime::app::agent_settings_file);
    let settings = AgentSettings::new(file, agent_waker.clone());
    let agents = match &support {
        _ if headless => Agents::with_programs(Vec::new(), settings, agent_waker.clone()),
        support => {
            // The agent checks the types of the project's tools with the Bun that runs them.
            let bun = sound_typescript::bundled_bun()
                .and_then(|bun| bun.parent().map(std::path::Path::to_path_buf));
            let downloads = support.as_deref().map(runtime::app::agents_folder);
            Agents::new(downloads, bun, settings, agent_waker.clone())
        }
    };
    let threads = support.as_deref().map(runtime::app::threads_folder);
    let chat = Chat::new(agents, project, threads, agent_waker);
    Some(Box::new(Sidebar::new(chat, woken, wake)))
}

impl Sidebar {
    fn new(chat: Chat, woken: Arc<AtomicBool>, wake: rune::Waker) -> Self {
        Self {
            chat,
            composer: TextEdit::new(""),
            instructions: None,
            menu: None,
            search: TextEdit::new(""),
            lit: 0,
            dropping: false,
            woken,
            elapsed: None,
            images: rune::ImageCache::new(wake),
            thumbs: Vec::new(),
            pages: Vec::new(),
            settings: settings::Page::default(),
            shows_settings: false,
        }
    }

    /// One frame: what the project said, then what came in from the background. Whether the
    /// sidebar shows something new.
    fn pump(&mut self, project: &mut Project, events: &[ProjectEvent]) -> bool {
        for event in events {
            self.chat.heard(event, project);
        }
        let woken = self.woken.swap(false, Ordering::Relaxed);
        let pages = self.chat.poll(project);
        self.pages.extend(
            pages
                .into_iter()
                .filter(|page| sound_agent::link_opens(page)),
        );
        let mut changed = woken;
        changed |= self.follow_instructions();
        changed |= self.settings.follow(self.chat.agents());
        self.sync_thumbs();
        // The working line counts the seconds.
        let elapsed = thread::working_elapsed(&self.chat, project);
        changed |= elapsed != self.elapsed;
        self.elapsed = elapsed;
        changed
    }

    /// The instructions' text comes once read, with the caret at its end; it goes with them.
    fn follow_instructions(&mut self) -> bool {
        let text = self.chat.instructions().and_then(|shown| shown.text());
        match (&self.instructions, text) {
            (None, Some(text)) => {
                let mut edit = TextEdit::new(text);
                edit.set_caret(text.len());
                edit.focused = true;
                self.instructions = Some(edit);
                true
            }
            (Some(_), None) => {
                self.instructions = None;
                true
            }
            _ => false,
        }
    }

    /// A picture per chip, made once for the bytes it shows.
    fn sync_thumbs(&mut self) {
        let pictures: Vec<&[u8]> = (self.chat.attachments().iter())
            .filter_map(|attached| Some(attached.picture()?.bytes.as_slice()))
            .collect();
        let same = pictures.len() == self.thumbs.len()
            && (pictures.iter().zip(&self.thumbs))
                .all(|(bytes, (at, _))| bytes.as_ptr() as usize == *at);
        if same {
            return;
        }
        let images = &self.images;
        self.thumbs = (pictures.into_iter())
            .map(|bytes| (bytes.as_ptr() as usize, images.load(bytes.to_vec())))
            .collect();
    }

    fn thumb(&self, bytes: &[u8]) -> Option<&rune::Image> {
        let at = bytes.as_ptr() as usize;
        (self.thumbs.iter()).find_map(|(of, image)| (*of == at).then_some(image))
    }

    fn set_composer(&mut self, text: &str) {
        let mut edit = TextEdit::new(text);
        edit.set_caret(text.len());
        edit.focused = self.composer.focused;
        self.composer = edit;
    }
}

impl PanelTab for Sidebar {
    fn view(&self, state: &State) -> El {
        thread::body(state, self)
    }

    fn header(&self, state: &State) -> Option<El> {
        menus::new_button(state, self)
    }

    fn focus(&self) -> Option<String> {
        if self.chat.instructions().is_some() {
            return Some(INSTRUCTIONS.into());
        }
        self.chat.has_composer().then(|| COMPOSER.into())
    }

    fn busy(&self) -> bool {
        self.chat.is_working()
    }

    fn compose(&mut self, words: &str) -> bool {
        if !self.chat.has_composer() {
            return false;
        }
        self.set_composer(words);
        self.composer.focused = true;
        true
    }

    fn ask(&mut self, message: &str, project: &mut Project) -> bool {
        self.chat.ask(message, project)
    }

    fn stop(&mut self) {
        self.chat.stop();
    }
}

/// For a test: `f` on the chat of the sidebar of `state` and its project.
#[cfg(test)]
pub(crate) fn chat<R>(state: &mut State, f: impl FnOnce(&mut Chat, &mut Project) -> R) -> R {
    #[allow(clippy::unwrap_used)]
    with_chat(state, |sidebar, project| f(&mut sidebar.chat, project)).unwrap()
}

#[cfg(test)]
pub(crate) use scenes::{scene, signed_in_saving, started};

/// The sidebar of `state`, when its panel has one.
fn sidebar(state: &State) -> Option<&Sidebar> {
    let agent: &dyn std::any::Any = state.panel.agent.as_deref()?;
    agent.downcast_ref()
}

/// Runs `f` on the sidebar and the project it works on: the one place the window hands the
/// chat its project.
fn with_chat<R>(state: &mut State, f: impl FnOnce(&mut Sidebar, &mut Project) -> R) -> Option<R> {
    let project = &mut state.project;
    let sidebar = state.panel.agent_mut::<Sidebar>()?;
    Some(f(sidebar, project))
}

/// One frame of the agent: the project's `events`, then what came in from the background.
/// Whether the sidebar shows something new.
pub(crate) fn pump(state: &mut State, events: &[ProjectEvent]) -> bool {
    with_chat(state, |sidebar, project| sidebar.pump(project, events)).unwrap_or(false)
}

/// Opens the pages the agent asked for, such as a sign-in's, in the browser.
pub(crate) fn open_pages(state: &mut State, tx: &mut Tx) {
    let Some(sidebar) = state.panel.agent_mut::<Sidebar>() else {
        return;
    };
    for page in sidebar.pages.drain(..) {
        // Only web pages come here, which open unless the system has no browser.
        if let Err(error) = tx.open_url(&page) {
            eprintln!("{page} did not open: {error}");
        }
    }
}

/// In place of the main view of a project that has none: the agent makes it.
pub(crate) fn no_main_tool(state: &State) -> El {
    let theme = &state.theme;
    let ask = sidebar(state).map(|_| {
        rune_kit::Button::new(ASK_THE_AGENT)
            .variant(rune_kit::ButtonVariant::Neutral)
            .control_size(rune_kit::ControlSize::Large)
            .on_press_tx(|s: &mut State, tx: &mut Tx| {
                ask_the_agent(s, Some(tx));
                s.handled();
            })
            .build(&theme.kit)
            .key("ask-the-agent")
    });
    rune::col()
        .gap(theme.space(3))
        .items_start()
        .padding(theme.space(4))
        .child(
            rune::text("This project has no main tool.")
                .font(theme.text("body"))
                .color(theme.color("text-muted")),
        )
        .children(ask)
}

const ASK_THE_AGENT: &str = "Ask the agent";

/// Opens the panel on the composer.
fn ask_the_agent(state: &mut State, tx: Option<&mut Tx>) {
    state.show_tab(crate::app::Tab::Agent);
    if !state.panel.open {
        state.set_panel_open(true);
    }
    if let Some(tx) = tx {
        tx.focus_when_shown(COMPOSER, false);
    }
}

/// The window gained the keys: another window of the app may have changed the agent's settings.
pub(crate) fn focused(state: &mut State, focused: bool, _: &mut Tx) {
    if let (true, Some(sidebar)) = (focused, state.panel.agent_mut::<Sidebar>()) {
        sidebar.chat.agents_mut().read_settings_again();
    }
}

/// Before the app quits: the thread and the instructions are written to the end, so the
/// next start opens them as they were. A disk that hangs gets a few seconds.
pub(crate) fn before_quit(state: &mut State, _: &mut Tx) {
    for _ in 0..1000 {
        let saving = with_chat(state, |sidebar, project| {
            sidebar.chat.poll(project);
            sidebar.chat.is_saving()
        });
        if saving != Some(true) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// What a control of the sidebar does, for a pointer, a key and `--act`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Act {
    Send,
    Stop,
    Steps(usize),
    Problems(usize),
    Fix,
    TryAgain,
    Answer(ApprovalAnswer),
    Remove(usize),
    DismissWentBack,
    OpenSettings,
    NewThread,
    Menu(Menu),
    OpenRecent(usize),
    Instructions(Scope),
    CloseInstructions,
    /// The row of the model picker, among those its search finds.
    Pick(usize),
    Access(ApprovalMode),
    /// The button of `Provider`'s part of the Agents section, by its place there.
    Setup(Provider, usize),
    CopyDetails(Provider),
    /// The empty project's way to the agent.
    AskTheAgent,
}

impl Act {
    /// Runs it. With `tx`, as a pointer or a key does: the keys go where they belong after.
    pub(crate) fn run(self, state: &mut State, tx: Option<&mut Tx>) {
        if let Act::AskTheAgent = self {
            return ask_the_agent(state, tx);
        }
        let focus = with_chat(state, |sidebar, project| sidebar.act(self, project)).flatten();
        if let (Some(tx), Some(focus)) = (tx, focus) {
            match focus {
                Focus::Composer { scroll } => {
                    tx.focus(COMPOSER, false);
                    if scroll {
                        tx.scroll_to_bottom(THREAD);
                    }
                }
                Focus::Key(key) => tx.focus(key, false),
                Focus::Copy(text) => tx.copy(text),
            }
        }
    }
}

/// Where the keys go after an act, or what it copies.
enum Focus {
    Composer { scroll: bool },
    Key(&'static str),
    Copy(String),
}

impl Sidebar {
    fn act(&mut self, act: Act, project: &mut Project) -> Option<Focus> {
        let sent = Focus::Composer { scroll: true };
        let composer = Focus::Composer { scroll: false };
        match act {
            Act::Send => {
                let text = self.composer.text.clone();
                if self.chat.send(&text, project) {
                    self.composer = TextEdit::new("");
                    return Some(sent);
                }
                None
            }
            Act::Stop => {
                self.chat.stop();
                None
            }
            Act::Steps(index) => {
                self.chat.toggle_steps(index);
                None
            }
            Act::Problems(index) => {
                self.chat.toggle_problems(index);
                None
            }
            Act::Fix => self.chat.ask(FIX_NOT_LIVE, project).then_some(sent),
            Act::TryAgain => self.chat.try_again(project).then_some(sent),
            Act::Answer(answer) => {
                self.chat.answer(answer);
                Some(composer)
            }
            Act::Remove(index) => {
                self.chat.remove_attachment(index);
                self.sync_thumbs();
                Some(composer)
            }
            Act::DismissWentBack => {
                self.chat.dismiss_went_back();
                Some(composer)
            }
            Act::OpenSettings => None,
            Act::NewThread => {
                self.menu = None;
                self.chat.new_thread(project);
                self.instructions = None;
                self.chat.has_composer().then_some(composer)
            }
            Act::Menu(menu) => {
                self.menu = match (self.menu, menu) {
                    (Some(Menu::Recent), Menu::Recent) => Some(Menu::New),
                    (Some(Menu::New | Menu::Recent), Menu::New) => None,
                    (open, menu) if open == Some(menu) => None,
                    (_, menu) => Some(menu),
                };
                if self.menu == Some(Menu::Model) {
                    self.search = TextEdit::new("");
                    self.search.focused = true;
                    self.lit = 0;
                }
                None
            }
            Act::OpenRecent(index) => {
                self.menu = None;
                let id = self.chat.recent_threads().get(index)?.id.clone();
                self.chat.open_thread(&id, project);
                self.instructions = None;
                Some(composer)
            }
            Act::Instructions(scope) => {
                self.menu = None;
                self.chat.open_instructions(scope);
                self.instructions = None;
                None
            }
            Act::CloseInstructions => {
                self.chat.close_instructions();
                self.instructions = None;
                Some(composer)
            }
            Act::Pick(row) => {
                self.menu = None;
                let picked = menus::pickable(&self.chat, &self.search.text)
                    .into_iter()
                    .nth(row);
                if let Some((provider, id)) = picked {
                    self.chat.pick_model(provider, &id);
                }
                Some(Focus::Key(menus::MODEL_PILL))
            }
            Act::Access(mode) => {
                self.menu = None;
                self.chat.set_approval_mode(mode);
                Some(Focus::Key(menus::ACCESS_PILL))
            }
            Act::Setup(provider, button) => {
                self.settings
                    .press(self.chat.agents_mut(), provider, button);
                None
            }
            Act::CopyDetails(provider) => self
                .settings
                .copy(self.chat.agents(), provider)
                .map(Focus::Copy),
            Act::AskTheAgent => None,
        }
    }
}

/// A control of the sidebar as `--tree` lists it and `--act` runs it.
fn control(key: impl Into<String>, label: impl Into<String>, role: &str, act: Act) -> Control {
    Control {
        key: key.into(),
        label: label.into(),
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        target: Target::Chrome(crate::app::Act::Sidebar(act)),
    }
}

/// The controls the sidebar shows now, for `--tree` and `--act`.
pub(crate) fn controls(state: &State) -> Vec<Control> {
    let Some(sidebar) = sidebar(state) else {
        return Vec::new();
    };
    let mut controls = Vec::new();
    if state.main().is_none() {
        controls.push(control(
            "ask-the-agent",
            ASK_THE_AGENT,
            "button",
            Act::AskTheAgent,
        ));
    }
    if !state.panel.open || state.panel.tab != crate::app::Tab::Agent {
        return controls;
    }
    if sidebar.shows_settings {
        controls.extend(settings::controls(sidebar.chat.agents(), &sidebar.settings));
        return controls;
    }
    controls.extend(menus::controls(sidebar));
    controls.extend(thread::controls(state, sidebar));
    controls.extend(composer::controls(sidebar));
    controls
}
