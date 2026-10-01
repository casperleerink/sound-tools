//! The sidebar: the header with **+**, the onboarding until the agent is set up, then the
//! thread in a gpui `list` and the composer.
//!
//! One thread per sidebar. Its process starts on the first send and ends with **+**. Every
//! message is one request of the session, so the agent's file writes for it are one undo step.
//! The thread is saved on the machine as it goes (`crate::store`), and the sidebar opens on the
//! project's last one.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, App, BoxShadow, Context, Entity, FocusHandle, Focusable, FollowMode, FontWeight,
    Global, KeyBinding, ListAlignment, ListState, Subscription, Task, Window, actions, div, hsla,
    list, point, prelude::*, px,
};
use smol::future;
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::popover::{Align, Side};
use sound_ui::components::text_input::TextInput;

use super::entry;
use super::onboarding::{Onboarding, Setup, SetupAction};
use crate::conversation::{Conversation, Entry, request_label};
use crate::install::{self, InstallError};
use crate::store::{Line, SavedThread, ThreadStore, Write};
use crate::{
    Account, AgentEvent, ApprovalAnswer, ApprovalMode, Events, Installed, Provider, Session,
    SignInChoice, Thread, ThreadOptions, TurnOutcome, login_shell_environment,
};

actions!(agent_sidebar, [Stop]);

const KEY_CONTEXT: &str = "AgentSidebar";

/// Events that come within one frame are applied together, so a fast stream of text measures
/// the growing entry once a frame and not once a token.
const FRAME: Duration = Duration::from_millis(16);

/// How far the list draws beyond what shows, so a short scroll needs no measuring.
const OVERDRAW: f32 = 800.;

/// How long the browser may take to sign in before the sidebar gives up and asks again.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long the program may take to say whether it is signed in.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// The value of **Sign out** in the account menu.
const SIGN_OUT: &str = "sign-out";

struct BindingsInstalled;
impl Global for BindingsInstalled {}

/// Binds the keys of the sidebar once per process, as the text input does.
fn install_bindings(cx: &mut App) {
    if cx.has_global::<BindingsInstalled>() {
        return;
    }
    cx.set_global(BindingsInstalled);
    cx.bind_keys([KeyBinding::new("cmd-.", Stop, Some(KEY_CONTEXT))]);
}

/// The process of the thread, from the first send until **+** or until it ends.
struct Agent {
    thread: Thread,
    /// Reads the process on the background executor. Dropping it ends the process.
    _reading: Task<()>,
    /// Hands what was read to the sidebar, once a frame.
    _delivering: Task<()>,
}

pub struct Sidebar {
    session: Entity<sound_ui::Session>,
    provider: Provider,
    /// Where downloaded programs are kept, `agents/` in the support folder. `None` when the
    /// machine has no support folder; a program named in the environment still runs.
    agents: Option<PathBuf>,
    setup: Setup,
    /// The program and its environment, once the login shell answered. The program may not be
    /// downloaded yet.
    installed: Option<Installed>,
    /// The way in of the running sign-in, which **Open the page again** starts again.
    signing_in: Option<SignInChoice>,
    /// The download, the sign-in or a question to the program. Dropping it cancels it.
    setup_task: Option<Task<()>>,
    /// The account and **Sign out**, in the composer.
    account_menu: Entity<DropdownMenu>,
    conversation: Conversation,
    list: ListState,
    /// The finished turns whose steps show, by entry.
    expanded: HashSet<usize>,
    input: Entity<TextInput>,
    /// The sidebar itself, for when it has no composer.
    focus_handle: FocusHandle,
    new_thread_focus: FocusHandle,
    send_focus: FocusHandle,
    /// Allow, Allow for this thread, Deny.
    approval_focus: [FocusHandle; 3],
    agent: Option<Agent>,
    /// The thread shown, as it is saved. `None` until its first message.
    thread: Option<SavedThread>,
    /// Hands what to keep of the thread to its writer, in order. `None` while nothing is
    /// saved, as in a snapshot.
    writes: Option<smol::channel::Sender<Write>>,
    /// Reads the last thread of the project. No message goes until it is in.
    loading: Option<Task<()>>,
    _input: Subscription,
    _account_menu: Subscription,
}

impl Sidebar {
    /// Reads the login shell in the background, then asks the program whether it is signed
    /// in, or offers **Set up** when it is not downloaded yet. The program is the download in
    /// `agents`, or the one the provider's environment variable names. The threads of the
    /// project are kept in `threads`, `agent/threads` in the support folder of the machine;
    /// with `None` nothing is saved.
    pub fn new(
        session: Entity<sound_ui::Session>,
        agents: Option<PathBuf>,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut sidebar = Self::with(session, agents, Setup::Checking, threads, cx);
        let provider = sidebar.provider;
        let downloaded = sidebar
            .agents
            .clone()
            .zip(provider.download())
            .map(|(agents, download)| download.program(&agents));
        sidebar.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            let found = cx
                .background_spawn(async move {
                    let (environment, error) = match login_shell_environment().await {
                        Ok(environment) => (environment, None),
                        Err(error) => (std::env::vars_os().collect(), Some(error)),
                    };
                    let named = std::env::var_os(provider.program_variable()).map(PathBuf::from);
                    let program = named.or(downloaded);
                    let present = program.as_ref().is_some_and(|program| program.is_file());
                    let installed = program.map(|program| Installed {
                        program,
                        environment,
                    });
                    (installed, present, error)
                })
                .await;
            // A sidebar that went in the meantime has nobody to tell.
            sidebar
                .update(cx, |sidebar, cx| sidebar.found(found, cx))
                .ok();
        }));
        sidebar
    }

    /// For a test or a snapshot: set up and signed in with `installed` as the program, or not
    /// installed with `None`. Nothing runs in the background.
    pub fn with_program(
        session: Entity<sound_ui::Session>,
        installed: Option<Installed>,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let setup = match installed {
            Some(_) => Setup::Ready {
                account: Account::default(),
            },
            None => Setup::NotInstalled,
        };
        let mut sidebar = Self::with(session, None, setup, threads, cx);
        sidebar.installed = installed;
        sidebar
    }

    fn with(
        session: Entity<sound_ui::Session>,
        agents: Option<PathBuf>,
        setup: Setup,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        install_bindings(cx);
        let loading = threads.map(|threads| {
            let project = session.read(cx).project().root().to_path_buf();
            Self::load(threads, project, cx)
        });
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Ask for a change")
                .multi_line(8)
                .bare(true)
        });
        let weak = cx.weak_entity();
        input.update(cx, |input, _| {
            input.set_on_submit(move |_, _, cx| {
                let weak = weak.clone();
                // The handler runs inside the input's own update, and sending clears it.
                cx.defer(move |cx| {
                    // A sidebar that went in the meantime sends nothing.
                    weak.update(cx, |sidebar, cx| sidebar.send(cx)).ok();
                });
            });
        });
        let list = ListState::new(0, ListAlignment::Top, px(OVERDRAW));
        list.set_follow_mode(FollowMode::Tail);
        let account = match &setup {
            Setup::Ready { account } => account.clone(),
            _ => Account::default(),
        };
        let account_menu = cx.new(|cx| {
            DropdownMenu::new(account_label(&account), account_entries(&account), cx)
                .debug_name("account-menu")
                .trigger(Trigger::Ghost)
                .side(Side::Top)
                .align(Align::Start)
                .width(280.)
        });
        Self {
            session,
            provider: Provider::Claude,
            agents,
            setup,
            installed: None,
            signing_in: None,
            setup_task: None,
            _account_menu: cx.subscribe(&account_menu, |sidebar, _, picked: &MenuPicked, cx| {
                if picked.0.as_ref() == SIGN_OUT {
                    sidebar.sign_out(cx);
                }
            }),
            account_menu,
            conversation: Conversation::default(),
            list,
            expanded: HashSet::new(),
            _input: cx.observe(&input, |_, _, cx| cx.notify()),
            input,
            focus_handle: cx.focus_handle(),
            new_thread_focus: cx.focus_handle().tab_stop(true),
            send_focus: cx.focus_handle().tab_stop(true),
            approval_focus: [(); 3].map(|_| cx.focus_handle().tab_stop(true)),
            agent: None,
            thread: None,
            writes: None,
            loading,
        }
    }

    /// Where the sidebar is in setting up its agent.
    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    /// Reads the current thread of `project` from `threads` in the background.
    fn load(threads: PathBuf, project: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |sidebar, cx| {
            let (store, current) = cx
                .background_spawn(async move {
                    let store = ThreadStore::new(&threads, &project);
                    let current = store.current();
                    (store, current)
                })
                .await;
            // A sidebar that went in the meantime shows nothing.
            sidebar
                .update(cx, |sidebar, cx| sidebar.loaded(store, current, cx))
                .ok();
        })
    }

    fn loaded(
        &mut self,
        store: ThreadStore,
        current: Result<Option<(SavedThread, Conversation)>, String>,
        cx: &mut Context<Self>,
    ) {
        self.loading = None;
        self.writes = Some(write_in_order(store, cx));
        match current {
            Ok(Some((thread, conversation))) => {
                self.thread = Some(thread);
                // Only notices came meanwhile: no message goes while it loads.
                let meanwhile = std::mem::replace(&mut self.conversation, conversation);
                self.conversation.append(meanwhile);
            }
            Ok(None) => {}
            Err(error) => self
                .conversation
                .notice(format!("The saved thread could not be read: {error}")),
        }
        self.expanded.clear();
        self.list.reset(self.conversation.entries().len());
        cx.notify();
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// What the next process of the thread continues: the thread's own session once its
    /// agent has started, so a thread opened again resumes where it left off.
    pub fn next_session(&self) -> Session {
        self.thread
            .as_ref()
            .map_or(Session::New, SavedThread::session)
    }

    fn keep(&self, writes: impl IntoIterator<Item = Write>) {
        let Some(sender) = &self.writes else {
            return;
        };
        for write in writes {
            // The writer ends only with the sidebar, which owns the sender.
            sender.try_send(write).ok();
        }
    }

    /// Whether the agent works or waits on the composer, so a closed sidebar can say so.
    pub fn is_busy(&self, _: &App) -> bool {
        self.conversation.is_working()
    }

    /// Shows the composer's message and opens its request. [`Self::send`] calls it before the
    /// message goes to the agent; a test calls it to replay a recorded turn with no process.
    pub fn begin(&mut self, message: &str, cx: &mut Context<Self>) {
        self.begin_at(message, SystemTime::now(), cx);
    }

    /// [`Self::begin`] with the turn started at `started`, so a snapshot shows how long it
    /// worked.
    pub fn begin_at(&mut self, message: &str, started: SystemTime, cx: &mut Context<Self>) {
        let label = request_label(message);
        self.session
            .update(cx, |session, _| session.begin_request(&label));
        self.conversation.send(message, started);
        let thread = self.thread.get_or_insert_with(SavedThread::fresh);
        let line = Line::Sent {
            at: started,
            message: message.to_string(),
        };
        let writes = [
            Write::Current(Some(thread.clone())),
            Write::Lines {
                thread: thread.id.clone(),
                lines: vec![line],
            },
        ];
        self.keep(writes);
        self.show(None, cx);
    }

    /// What the agent did, in order. The process hands its events here once a frame; a test
    /// calls it with recorded ones.
    pub fn receive(
        &mut self,
        events: impl IntoIterator<Item = AgentEvent>,
        cx: &mut Context<Self>,
    ) {
        let now = SystemTime::now();
        let mut changed: Option<usize> = None;
        let mut lines = Vec::new();
        let mut failed = false;
        for event in events {
            lines.extend(Line::of(&event, now));
            match &event {
                // The account the running agent reports is the newest word on it.
                AgentEvent::Started { account, .. }
                    if matches!(self.setup, Setup::Ready { .. }) =>
                {
                    let account = account.clone();
                    self.set_setup(Setup::Ready { account }, cx);
                }
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Failed { .. },
                } => failed = true,
                _ => {}
            }
            let ends = matches!(
                event,
                AgentEvent::TurnEnded { .. } | AgentEvent::Exited { .. }
            );
            // Only the end of a turn that works ends its request: an exit after the turn
            // ended must not move the end of a request that finished long ago.
            if ends && self.conversation.is_working() {
                self.session.update(cx, |session, _| session.end_request());
            }
            if matches!(event, AgentEvent::Exited { .. }) {
                // The next message starts a new process.
                self.agent = None;
            }
            if let Some(index) = self.conversation.apply(event, now) {
                changed = Some(changed.map_or(index, |earlier| earlier.min(index)));
            }
        }
        // A thread is saved from its first message on.
        if let Some(thread) = &self.thread
            && !lines.is_empty()
        {
            let thread = thread.id.clone();
            self.keep([Write::Lines { thread, lines }]);
        }
        self.show(changed, cx);
        if failed {
            self.check_still_signed_in(cx);
        }
    }

    /// After a failed turn: the program may have been signed out meanwhile, such as in a
    /// terminal. Only an answer of signed out changes anything; the failed turn says the rest.
    fn check_still_signed_in(&mut self, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        let provider = self.provider;
        let asking = cx.background_spawn(async move { provider.account(&installed).await });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            match asking.await {
                Ok(None) => {
                    let reason = Some(format!("{} is signed out.", provider.name()));
                    sidebar
                        .update(cx, |sidebar, cx| {
                            sidebar.end_agent(cx);
                            sidebar.set_setup(Setup::SignedOut { reason }, cx);
                        })
                        .ok();
                }
                // Signed in, or no answer: the next message shows whether the agent runs.
                Ok(Some(_)) | Err(_) => {}
            }
        }));
    }

    /// Tells the list what changed: entries added at the end, and `changed` and what comes
    /// after it measured again, for an entry that grew.
    fn show(&mut self, changed: Option<usize>, cx: &mut Context<Self>) {
        let shown = self.list.item_count();
        if let Some(index) = changed.filter(|index| *index < shown) {
            self.list.remeasure_items(index..shown);
        }
        let count = self.conversation.entries().len();
        if count > shown {
            self.list.splice(shown..shown, count - shown);
        }
        cx.notify();
    }

    fn found(
        &mut self,
        (installed, present, error): (Option<Installed>, bool, Option<io::Error>),
        cx: &mut Context<Self>,
    ) {
        if let Some(error) = error {
            self.conversation.notice(format!(
                "The agent runs without the settings of your shell: {error}."
            ));
            self.show(None, cx);
        }
        self.installed = installed;
        if present {
            self.check(None, cx);
        } else {
            self.set_setup(Setup::NotInstalled, cx);
        }
    }

    fn set_setup(&mut self, setup: Setup, cx: &mut Context<Self>) {
        if let Setup::Ready { account } = &setup {
            let (label, entries) = (account_label(account), account_entries(account));
            self.account_menu.update(cx, |menu, cx| {
                menu.set_label(label, cx);
                menu.set_entries(entries, cx);
            });
        }
        self.setup = setup;
        cx.notify();
    }

    fn act(&mut self, action: SetupAction, cx: &mut Context<Self>) {
        match action {
            SetupAction::Download => self.download(cx),
            SetupAction::Cancel => self.cancel(cx),
            SetupAction::SignIn(choice) => self.sign_in(choice, cx),
            SetupAction::OpenPageAgain => {
                if let Some(choice) = self.signing_in {
                    self.sign_in(choice, cx);
                }
            }
            SetupAction::Check => self.check(None, cx),
        }
    }

    /// Asks the program whether it is signed in. `reason` is what the composer reads when it
    /// is not, after a sign-in.
    fn check(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            self.set_setup(Setup::NotInstalled, cx);
            return;
        };
        let provider = self.provider;
        self.set_setup(Setup::Checking, cx);
        let asking = cx.background_spawn(async move { provider.account(&installed).await });
        let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            // The question that loses the race is dropped, which ends its process.
            let answer = future::or(async { Some(asking.await) }, async {
                timeout.await;
                None
            })
            .await;
            let setup = match answer {
                Some(Ok(Some(account))) => Setup::Ready { account },
                Some(Ok(None)) => Setup::SignedOut { reason },
                Some(Err(error)) => Setup::Stopped {
                    message: error.to_string(),
                },
                None => Setup::Stopped {
                    message: "it did not answer within 30 seconds.".to_string(),
                },
            };
            // A sidebar that went in the meantime has nobody to tell.
            sidebar
                .update(cx, |sidebar, cx| sidebar.set_setup(setup, cx))
                .ok();
        }));
    }

    /// Downloads the pinned program, from where an earlier try stopped.
    fn download(&mut self, cx: &mut Context<Self>) {
        let name = self.provider.name();
        let (Some(agents), Some(download)) = (self.agents.clone(), self.provider.download()) else {
            let message = match self.agents {
                None => format!("There is no folder on this computer to keep {name} in."),
                Some(_) => format!("{name} does not run on this computer."),
            };
            self.set_setup(Setup::DownloadFailed { message }, cx);
            return;
        };
        let size = download.size;
        self.set_setup(Setup::Downloading { received: 0, size }, cx);
        let (sender, receiver) = smol::channel::unbounded();
        let installing = cx.background_spawn(async move {
            install::install(&download, &agents, move |received| {
                // Closed only when the sidebar went, and then nobody reads it.
                sender.try_send(received).ok();
            })
            .await
        });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            // Ends with the download, which drops the sender.
            while let Ok(received) = receiver.recv().await {
                let downloading = Setup::Downloading { received, size };
                if sidebar
                    .update(cx, |sidebar, cx| sidebar.set_setup(downloading, cx))
                    .is_err()
                {
                    return;
                }
            }
            let installed = installing.await;
            sidebar
                .update(cx, |sidebar, cx| sidebar.downloaded(installed, cx))
                .ok();
        }));
    }

    fn downloaded(&mut self, installed: Result<PathBuf, InstallError>, cx: &mut Context<Self>) {
        match installed {
            Ok(program) => {
                // The login shell answered before **Set up** showed.
                let environment = self.installed.take().map_or_else(
                    || std::env::vars_os().collect(),
                    |installed| installed.environment,
                );
                self.installed = Some(Installed {
                    program,
                    environment,
                });
                self.check(None, cx);
            }
            Err(error) => {
                let message = error.sentence(self.provider.name());
                self.set_setup(Setup::DownloadFailed { message }, cx);
            }
        }
    }

    /// Runs the provider's sign-in in the browser and waits for it, at most ten minutes.
    /// Started again, it ends the one before.
    fn sign_in(&mut self, choice: SignInChoice, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        self.signing_in = Some(choice);
        self.set_setup(Setup::SigningIn, cx);
        let running = cx.background_spawn(async move { choice.run(&installed).await });
        let timeout = cx.background_executor().timer(SIGN_IN_TIMEOUT);
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            // The one that loses the race is dropped: a sign-in out of time ends its process.
            let ended = future::or(
                async {
                    match running.await {
                        Ok(()) => SignInEnded::Finished,
                        Err(error) => SignInEnded::Failed(error),
                    }
                },
                async {
                    timeout.await;
                    SignInEnded::TimedOut
                },
            )
            .await;
            sidebar
                .update(cx, |sidebar, cx| sidebar.signed_in(ended, cx))
                .ok();
        }));
    }

    fn signed_in(&mut self, ended: SignInEnded, cx: &mut Context<Self>) {
        self.signing_in = None;
        let reason = Some(ended.reason());
        match ended {
            // The program says whether it worked.
            SignInEnded::Finished => self.check(reason, cx),
            SignInEnded::Failed(_) | SignInEnded::TimedOut | SignInEnded::Cancelled => {
                self.set_setup(Setup::SignedOut { reason }, cx);
            }
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        // Dropping the task ends the download or the sign-in, and its process.
        self.setup_task = None;
        self.signing_in = None;
        let setup = match self.setup {
            Setup::Downloading { .. } => Setup::NotInstalled,
            _ => Setup::SignedOut {
                reason: Some(SignInEnded::Cancelled.reason()),
            },
        };
        self.set_setup(setup, cx);
    }

    /// Signs the program out, for every app that shares its login, and ends the thread's
    /// agent.
    fn sign_out(&mut self, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        self.end_agent(cx);
        let provider = self.provider;
        let signing_out = cx.background_spawn(async move { provider.sign_out(&installed).await });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            let signed_out = signing_out.await;
            sidebar
                .update(cx, |sidebar, cx| match signed_out {
                    Ok(()) => sidebar.check(None, cx),
                    Err(error) => {
                        let notice = format!("Could not sign out: {error}");
                        sidebar.conversation.notice(notice);
                        sidebar.show(None, cx);
                    }
                })
                .ok();
        }));
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let message = self.input.read(cx).text().trim().to_string();
        let ready = self.loading.is_none() && self.conversation.can_continue();
        if message.is_empty() || self.conversation.is_working() || !ready {
            return;
        }
        if self.agent.is_none() {
            let (Setup::Ready { .. }, Some(installed)) = (&self.setup, &self.installed) else {
                return;
            };
            match self.start(installed.clone(), cx) {
                Ok(agent) => self.attach(agent),
                Err(error) => {
                    let name = self.provider.name();
                    self.conversation
                        .notice(format!("{name} did not start: {error}"));
                    self.show(None, cx);
                    return;
                }
            }
        }
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.begin(&message, cx);
        let sent = self.agent.as_ref().map(|agent| agent.thread.send(message));
        if let Some(Err(error)) = sent {
            let outcome = TurnOutcome::Failed {
                message: error.to_string(),
            };
            self.receive([AgentEvent::TurnEnded { outcome }], cx);
        }
    }

    fn start(&self, installed: Installed, cx: &mut Context<Self>) -> io::Result<Agent> {
        let folder = self.session.read(cx).project().root().to_path_buf();
        let (thread, events) = Thread::start(ThreadOptions {
            provider: self.provider,
            installed,
            folder,
            // The CLI's own default until the model picker comes.
            model: None,
            approval_mode: ApprovalMode::AskBeforeCommands,
            session: self.next_session(),
        })?;
        let (sender, receiver) = smol::channel::unbounded();
        let reading = cx.background_spawn(read(events, sender));
        let delivering = cx.spawn(async move |sidebar, cx| {
            while let Ok(first) = receiver.recv().await {
                let mut batch = vec![first];
                while let Ok(event) = receiver.try_recv() {
                    batch.push(event);
                }
                if sidebar
                    .update(cx, |sidebar, cx| sidebar.receive(batch, cx))
                    .is_err()
                {
                    break;
                }
                // What comes during the wait is the next batch.
                cx.background_executor().timer(FRAME).await;
            }
        });
        Ok(Agent {
            thread,
            _reading: reading,
            _delivering: delivering,
        })
    }

    /// Talks to `thread` from now on, in place of starting a process at the next send. For a
    /// test with [`Thread::without_agent`], which hands the events to [`Self::receive`].
    pub fn connect(&mut self, thread: Thread) {
        self.attach(Agent {
            thread,
            _reading: Task::ready(()),
            _delivering: Task::ready(()),
        });
    }

    /// Talks to `agent` from now on. Its session is the thread's: the next message saves it,
    /// before the agent has said anything, so a thread quit early still resumes.
    fn attach(&mut self, agent: Agent) {
        let thread = self.thread.get_or_insert_with(SavedThread::fresh);
        thread.session_id = Some(agent.thread.session_id().to_string());
        self.agent = Some(agent);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.conversation.is_working() {
            return;
        }
        let stopped = self.agent.as_ref().map(|agent| agent.thread.interrupt());
        if let Some(Err(error)) = stopped {
            self.conversation.notice(error.to_string());
            self.show(None, cx);
        }
    }

    fn answer(&mut self, answer: ApprovalAnswer, cx: &mut Context<Self>) {
        let Some((approval, turn)) = self.conversation.answered() else {
            return;
        };
        let answered = self
            .agent
            .as_ref()
            .map(|agent| agent.thread.answer(approval, answer));
        if let Some(Err(error)) = answered {
            self.conversation.notice(error.to_string());
        }
        self.show(Some(turn), cx);
    }

    /// Drops the thread and its process, and starts empty.
    fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_agent(cx);
        // The old one stays in the store, and no thread is current until the next message.
        self.thread = None;
        self.keep([Write::Current(None)]);
        self.conversation = Conversation::default();
        self.expanded.clear();
        self.list.reset(0);
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// Ends the thread's process. A dropped process says nothing more, so the turn it was
    /// working on ends here as stopped, and its request with it.
    fn end_agent(&mut self, cx: &mut Context<Self>) {
        self.agent = None;
        if self.conversation.is_working() {
            let outcome = TurnOutcome::Interrupted;
            self.receive([AgentEvent::TurnEnded { outcome }], cx);
        }
    }

    fn toggle_steps(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&index) {
            self.expanded.insert(index);
        }
        self.list.remeasure_items(index..index + 1);
        cx.notify();
    }

    fn render_entry(&mut self, index: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.conversation.entries().get(index) {
            Some(Entry::Message(text)) => entry::message(text, cx),
            Some(Entry::Notice(text)) => entry::notice(text, cx),
            Some(Entry::Turn(turn)) => {
                let approval = turn
                    .approval
                    .as_ref()
                    .map(|approval| self.approval_row(&approval.title, cx));
                let toggle = cx.listener(move |sidebar, _, _, cx| sidebar.toggle_steps(index, cx));
                let expanded = self.expanded.contains(&index);
                entry::turn(turn, index, expanded, toggle, approval, cx)
            }
            None => div().into_any_element(),
        };
        div()
            .px(px(24.))
            .pb(px(24.))
            .when(index == 0, |item| item.pt(px(8.)))
            .child(content)
            .into_any_element()
    }

    /// The question the agent waits on, as the last thing of the thread. Nothing is modal:
    /// music and editing go on while it waits.
    fn approval_row(&self, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let lavender = cx.theme().lavender;
        let answers = [
            ("allow", "Allow", ApprovalAnswer::Allow),
            (
                "allow-for-thread",
                "Allow for this thread",
                ApprovalAnswer::AllowForThread,
            ),
            ("deny", "Deny", ApprovalAnswer::Deny),
        ];
        let buttons =
            answers
                .into_iter()
                .zip(&self.approval_focus)
                .map(|((id, label, answer), focus)| {
                    let variant = match answer {
                        ApprovalAnswer::Allow => ButtonVariant::SubtleColor(lavender),
                        ApprovalAnswer::AllowForThread | ApprovalAnswer::Deny => {
                            ButtonVariant::GhostColor(lavender)
                        }
                    };
                    Button::new(id, label)
                        .debug_selector(move || format!("approval-{id}"))
                        .variant(variant)
                        .size(ButtonSize::Sm)
                        .focus_handle(focus)
                        .on_click(cx.listener(move |sidebar, _, _, cx| sidebar.answer(answer, cx)))
                });
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(lavender)
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .child(title.to_string()),
            )
            .child(div().flex().flex_wrap().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(56.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Agent"),
            )
            .child(
                div().absolute().right(px(16.)).child(
                    Button::icon_only("new-thread", "plus")
                        .debug_selector(|| "agent-new-thread".to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .focus_handle(&self.new_thread_focus)
                        .on_click(
                            cx.listener(|sidebar, _, window, cx| sidebar.new_thread(window, cx)),
                        ),
                ),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, fill) = (theme.alpha_at(0.10), theme.gray_50);
        let working = self.conversation.is_working();
        let can_send = !self.input.read(cx).text().trim().is_empty();
        // While a turn runs the send button stops it: one turn at a time.
        let button = if working {
            Button::icon_only("stop", "square")
                .debug_selector(|| "agent-stop".to_string())
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.stop(cx)))
        } else {
            Button::icon_only("send", "arrow-up")
                .disabled(!can_send)
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.send(cx)))
        };
        div().flex_none().p(px(24.)).pt(px(8.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(16.))
                .rounded(px(16.))
                .border_1()
                .border_color(border)
                .bg(fill)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.25),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(-8.),
                    inset: false,
                }])
                .child(self.input.clone())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(self.account_menu.clone())
                        .child(
                            button
                                .variant(ButtonVariant::Subtle)
                                .size(ButtonSize::Sm)
                                .rounded(true)
                                .focus_handle(&self.send_focus),
                        ),
                ),
        )
    }

    /// In place of the composer once the agent lost the session: the thread stays to read,
    /// and **+** is the way on.
    fn cannot_continue(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(24.))
            .pt(px(8.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.))
                    .text_color(cx.theme().gray_700)
                    .child("This thread can't continue. Start a new one."),
            )
            .child(
                Button::icon_only("new-thread-instead", "plus")
                    .debug_selector(|| "agent-cannot-continue".to_string())
                    .variant(ButtonVariant::Subtle)
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .focus_handle(&self.send_focus)
                    .on_click(cx.listener(|sidebar, _, window, cx| sidebar.new_thread(window, cx))),
            )
    }
}

/// Hands each write to the background in turn, so the files get them in order. Detached: it
/// ends when the sidebar does, after the writes still queued.
fn write_in_order(store: ThreadStore, cx: &mut Context<Sidebar>) -> smol::channel::Sender<Write> {
    let (sender, receiver) = smol::channel::unbounded::<Write>();
    cx.spawn(async move |sidebar, cx| {
        let mut told = false;
        while let Ok(write) = receiver.recv().await {
            let store = store.clone();
            let written = cx
                .background_spawn(async move { store.write(&write) })
                .await;
            // Said once: a full disk would otherwise add a line at every event.
            if let Err(error) = written
                && !told
            {
                told = true;
                // A sidebar that went has nobody to tell.
                sidebar
                    .update(cx, |sidebar, cx| {
                        let notice = format!("This thread is not saved: {error}");
                        sidebar.conversation.notice(notice);
                        sidebar.show(None, cx);
                    })
                    .ok();
            }
        }
    })
    .detach();
    sender
}

/// How a sign-in ended.
#[derive(Debug)]
enum SignInEnded {
    /// The program asks whether it worked.
    Finished,
    Failed(io::Error),
    TimedOut,
    Cancelled,
}

impl SignInEnded {
    /// The line the composer reads when the sign-in ended with nobody signed in.
    fn reason(&self) -> String {
        match self {
            SignInEnded::Finished => "The sign-in did not finish.".to_string(),
            SignInEnded::Failed(error) => format!("The sign-in failed: {error}"),
            SignInEnded::TimedOut => "The sign-in took over 10 minutes.".to_string(),
            SignInEnded::Cancelled => "The sign-in was cancelled.".to_string(),
        }
    }
}

/// What the account menu's trigger says: the plan, since an email is long.
fn account_label(account: &Account) -> String {
    account
        .plan
        .clone()
        .unwrap_or_else(|| "Account".to_string())
}

/// The account, then **Sign out**. The model and the approval setting join them later.
fn account_entries(account: &Account) -> Vec<MenuEntry> {
    let who: Vec<&str> = [&account.email, &account.plan]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let sign_out = MenuItem::new(SIGN_OUT, "Sign out").selectable(false);
    let group = MenuGroup::new().item(sign_out);
    let group = if who.is_empty() {
        group
    } else {
        group.label(who.join(" · "))
    };
    vec![MenuEntry::Group(group)]
}

/// Every event of the process, until it ends or the sidebar stops listening.
async fn read(mut events: Events, sender: smol::channel::Sender<AgentEvent>) {
    while let Some(event) = events.next().await {
        if sender.send(event).await.is_err() {
            break;
        }
    }
}

impl Focusable for Sidebar {
    /// The composer, which cmd-L focuses, or the sidebar itself while it has none, so cmd-L
    /// and escape still work there.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.setup {
            Setup::Ready { .. } if self.conversation.can_continue() => self.input.focus_handle(cx),
            _ => self.focus_handle.clone(),
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border, text) = (theme.gray_50, theme.alpha_at(0.10), theme.gray_950);
        let body = match &self.setup {
            setup @ (Setup::Checking
            | Setup::NotInstalled
            | Setup::Downloading { .. }
            | Setup::DownloadFailed { .. }
            | Setup::SignedOut { .. }
            | Setup::SigningIn
            | Setup::Stopped { .. }) => Onboarding::new(self.provider, setup.clone())
                .on_action(
                    cx.listener(|sidebar, action: &SetupAction, _, cx| sidebar.act(*action, cx)),
                )
                .into_any_element(),
            Setup::Ready { .. } => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    list(self.list.clone(), cx.processor(Self::render_entry))
                        .flex_1()
                        .min_h_0(),
                )
                .map(|body| {
                    if self.conversation.can_continue() {
                        body.child(self.composer(cx))
                    } else {
                        body.child(self.cannot_continue(cx))
                    }
                })
                .into_any_element(),
        };
        div()
            .id("agent-sidebar")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|sidebar, _: &Stop, _, cx| sidebar.stop(cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .border_r_1()
            .border_color(border)
            .text_color(text)
            .child(self.header(cx))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_out_of_time_failed_or_cancelled_says_why() {
        let reasons = [
            SignInEnded::TimedOut,
            SignInEnded::Cancelled,
            SignInEnded::Failed(io::Error::other("OAuth error: access denied")),
        ]
        .map(|ended| ended.reason());
        assert_eq!(
            reasons,
            [
                "The sign-in took over 10 minutes.",
                "The sign-in was cancelled.",
                "The sign-in failed: OAuth error: access denied",
            ]
        );
    }
}
