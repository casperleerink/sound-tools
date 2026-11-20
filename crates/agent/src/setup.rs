//! How far a provider is set up, and what the Agents section of Settings shows of it: the
//! download, the ways in and the steps of a sign-in, and the account; for Pi, each of its
//! services. [`Section`] is plain data in the composer's words, so a view draws it with no
//! rules of its own and a gallery shows every state with no process.

use crate::agents::Program;
use crate::{
    Account, Answer, Provider, Service, SignInChoice, SignInQuestion, SignInStep, SignedIn,
};

/// Above the providers.
pub const AGENTS_INTRO: &str = "The agent in the sidebar runs on Claude Code or Pi. Set up one \
                                or both, then pick a model in the sidebar.";
/// While the login shell is read.
pub const LOOKING_FOR_AGENTS: &str = "Looking for agents…";
pub const SEARCH_SERVICES: &str = "Search services";
/// The button of a [`Reason`] with details, and what it says once they are copied.
pub const COPY_DETAILS: &str = "Copy details";
pub const COPIED: &str = "Copied";

/// Where the app is in setting up the agent of one provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setup {
    /// The login shell is read and the agent asked whether it is signed in.
    Checking,
    NotInstalled,
    /// `received` of `size` bytes are on disk.
    Downloading {
        received: u64,
        size: u64,
    },
    DownloadFailed {
        message: String,
    },
    /// `reason` says why an attempt to sign in ended without an account.
    SignedOut {
        reason: Option<Reason>,
    },
    SigningIn(SigningIn),
    /// A thread can run on it. `reason` says why a sign-in to another of Pi's providers
    /// ended without it.
    Ready {
        account: Account,
        reason: Option<Reason>,
    },
    /// The agent's program did not run. `message` is the first line it wrote.
    Stopped {
        message: String,
    },
}

/// Why a sign-in ended without an account: our sentence, and the program's own words, which
/// are a programmer's, to copy for whoever helps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub sentence: String,
    pub details: Option<String>,
}

impl Reason {
    pub fn new(sentence: impl Into<String>) -> Self {
        Reason {
            sentence: sentence.into(),
            details: None,
        }
    }
}

/// What a running sign-in has shown so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SigningIn {
    /// The provider's page, once it said which.
    pub page: Option<String>,
    /// A code to enter on the page.
    pub code: Option<String>,
    /// The provider's last words on it.
    pub note: Option<String>,
    pub question: Option<SignInQuestion>,
}

impl SigningIn {
    pub fn show(&mut self, step: SignInStep) {
        match step {
            SignInStep::Page { url } => self.page = Some(url),
            SignInStep::Code { url, code } => {
                self.page = Some(url);
                self.code = Some(code);
            }
            SignInStep::Note(note) => self.note = Some(note),
            SignInStep::Question(question) => self.question = Some(question),
        }
    }
}

/// What a button of the Agents section asks for, see [`crate::Agents::act`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupAction {
    /// Set up, and Try again after a failed download.
    Download,
    /// Stops the download or the sign-in.
    Cancel,
    SignIn(SignInChoice),
    /// Opens the provider's page again, or starts the sign-in again when it never said which.
    OpenPageAgain,
    /// Answers the question with the id of a pick.
    Pick(String),
    /// Answers the question with what the field holds: the view calls
    /// [`crate::Agents::answer`] with it.
    Continue,
    /// Signs out of the program, or of the service of Pi with this id.
    SignOut(Option<String>),
    /// Try again after the program did not run.
    Check,
    /// Opens the page that tells how to install the program.
    GetProgram,
    /// Looks for the program again, once the composer installed it.
    LookAgain,
}

/// One provider in the Agents section: its logo and name over its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub provider: Provider,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Steps(Steps),
    /// Its account, and the way out; a reason above it when a sign-in ended without another
    /// of its ways in.
    Ready {
        reason: Option<Reason>,
        signed_in: String,
        sign_out: Button,
    },
    Services(Services),
}

/// A sentence, a code shown large, a quieter line, a reason, then the controls in a column as
/// wide as their words allow, and what goes under them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Steps {
    pub sentence: Option<String>,
    pub code: Option<String>,
    pub quiet: Option<String>,
    pub reason: Option<Reason>,
    pub controls: Vec<Control>,
    pub more: Vec<Control>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    Button(Button),
    /// A quiet line, such as the question beside the browser's step.
    Words(String),
    /// The field a question is answered in. A new question empties it and gives it the keys.
    Field {
        secret: bool,
    },
}

/// A button. Its `id` is the same across frames, so it keeps the keyboard's focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub action: SetupAction,
    pub emphasis: Emphasis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emphasis {
    Primary,
    Subtle,
    Ghost,
}

/// Pi's services: those signed in first, then a search over the others, those with a sign-in
/// in the browser first. The running sign-in, or how the last one ended, shows above the
/// search: in a row of the list it could scroll out of view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Services {
    /// Over the list while no service is signed in.
    pub prompt: Option<&'static str>,
    /// Under "Signed in".
    pub signed_in: Vec<ServiceRow>,
    /// Under "Add a service", above the search.
    pub panel: Option<Panel>,
    /// The services the search finds.
    pub listed: Vec<ServiceRow>,
    /// In place of the list when the search finds none.
    pub empty: Option<&'static str>,
}

pub const SIGNED_IN_GROUP: &str = "Signed in";
pub const ADD_A_SERVICE: &str = "Add a service";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceRow {
    pub id: String,
    pub name: String,
    /// Its plan, or how it is signed in.
    pub about: Option<String>,
    pub buttons: Vec<Button>,
}

/// The sign-in to one service, under its title: its steps, or why it ended without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub title: String,
    pub body: PanelBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelBody {
    Steps(Steps),
    Ended(Reason),
}

/// Bytes as the composer reads a download: whole megabytes.
fn megabytes(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

fn make(
    key: &str,
    id: &str,
    label: impl Into<String>,
    action: SetupAction,
    emphasis: Emphasis,
) -> Button {
    Button {
        id: format!("{key}-{id}"),
        label: label.into(),
        action,
        emphasis,
    }
}

fn button(
    key: &str,
    id: &str,
    label: impl Into<String>,
    action: SetupAction,
    emphasis: Emphasis,
) -> Control {
    Control::Button(make(key, id, label, action, emphasis))
}

/// What `program` shows, with `query` in the search of Pi's services.
pub(crate) fn section(program: &Program, query: &str) -> Section {
    let provider = program.provider;
    let name = provider.name();
    let key = name.to_lowercase();
    let key = key.as_str();
    let (primary, subtle) = (Emphasis::Primary, Emphasis::Subtle);
    let steps = |steps: Steps| Body::Steps(steps);
    let body = match program.setup.clone() {
        Setup::Checking => steps(Steps {
            quiet: Some("Checking…".to_string()),
            ..Steps::default()
        }),
        Setup::NotInstalled if provider == Provider::Pi => steps(Steps {
            sentence: Some(
                "Pi runs the agent on the AI service you choose, such as ChatGPT, Gemini or \
                 OpenRouter. It is not on this computer yet: get it from its website, then \
                 come back here."
                    .to_string(),
            ),
            controls: vec![
                button(key, "get", "Get Pi", SetupAction::GetProgram, primary),
                button(
                    key,
                    "look-again",
                    "Check again",
                    SetupAction::LookAgain,
                    subtle,
                ),
            ],
            ..Steps::default()
        }),
        Setup::NotInstalled => {
            let size = provider.download().map_or(String::new(), |download| {
                format!(" {} MB", megabytes(download.size))
            });
            let maker = provider.maker();
            steps(Steps {
                sentence: Some(format!(
                    "The agent runs {name} by {maker}. Setup downloads{size}, then opens your \
                     browser to sign in."
                )),
                controls: vec![button(
                    key,
                    "set-up",
                    "Set up",
                    SetupAction::Download,
                    primary,
                )],
                ..Steps::default()
            })
        }
        Setup::Downloading { received, size } => {
            let percent = (received * 100).checked_div(size).unwrap_or(0).min(100);
            steps(Steps {
                sentence: Some(format!(
                    "Downloading {name}: {} of {} MB, {percent}%",
                    megabytes(received),
                    megabytes(size)
                )),
                controls: vec![button(key, "cancel", "Cancel", SetupAction::Cancel, subtle)],
                ..Steps::default()
            })
        }
        Setup::DownloadFailed { message } => steps(Steps {
            sentence: Some(message),
            controls: vec![button(
                key,
                "try-again",
                "Try again",
                SetupAction::Download,
                subtle,
            )],
            ..Steps::default()
        }),
        Setup::Stopped { message } => steps(Steps {
            sentence: Some(format!("{name} did not start.")),
            quiet: Some(message),
            controls: vec![button(
                key,
                "try-again",
                "Try again",
                SetupAction::Check,
                subtle,
            )],
            ..Steps::default()
        }),
        setup if provider == Provider::Pi => Body::Services(services(program, setup, query)),
        Setup::Ready { account, reason } => {
            let who: Vec<String> = [account.email, account.plan]
                .into_iter()
                .flatten()
                .collect();
            let signed_in = match who.is_empty() {
                true => "Signed in.".to_string(),
                false => format!("Signed in: {}", who.join(" · ")),
            };
            Body::Ready {
                reason,
                signed_in,
                sign_out: make(
                    key,
                    "sign-out",
                    "Sign out",
                    SetupAction::SignOut(None),
                    subtle,
                ),
            }
        }
        Setup::SignedOut { reason } => steps(Steps {
            sentence: Some("Sign in to start.".to_string()),
            reason,
            controls: choices(key, &program.choices),
            ..Steps::default()
        }),
        Setup::SigningIn(signing_in) => steps(signing_in_steps(key, provider, signing_in)),
    };
    Section { provider, body }
}

/// Claude Code's ways in, one button each.
fn choices(key: &str, choices: &[SignInChoice]) -> Vec<Control> {
    (choices.iter().enumerate())
        .map(|(index, choice)| {
            let action = SetupAction::SignIn(choice.clone());
            button(
                key,
                &format!("sign-in-{index}"),
                choice.label(),
                action,
                Emphasis::Subtle,
            )
        })
        .collect()
}

/// The steps of a running sign-in: the browser's, a code, a question, and the way out.
fn signing_in_steps(key: &str, provider: Provider, signing_in: SigningIn) -> Steps {
    let (subtle, primary) = (Emphasis::Subtle, Emphasis::Primary);
    let SigningIn {
        page,
        code,
        note,
        question,
    } = signing_in;
    // The browser's step leads while there is one; a question beside it, such as the address
    // the browser ended on, is the quieter way.
    let in_browser = page.is_some() || code.is_some();
    let mut asked = Vec::new();
    if let Some(question) = &question {
        if in_browser {
            asked.push(Control::Words(question.message.clone()));
        }
        match &question.answer {
            Answer::Pick(picks) => {
                for (index, pick) in picks.iter().enumerate() {
                    let action = SetupAction::Pick(pick.id.clone());
                    asked.push(button(
                        key,
                        &format!("pick-{index}"),
                        &pick.label,
                        action,
                        subtle,
                    ));
                }
            }
            Answer::Text { secret } => {
                asked.push(Control::Field { secret: *secret });
                let emphasis = if in_browser { subtle } else { primary };
                asked.push(button(
                    key,
                    "continue",
                    "Continue",
                    SetupAction::Continue,
                    emphasis,
                ));
            }
        }
    }
    let sentence = match &question {
        _ if code.is_some() => "Enter this code on the page in your browser:".to_string(),
        Some(question) if !in_browser => question.message.clone(),
        _ => "Finish in your browser.".to_string(),
    };
    let (mut controls, more) = match in_browser {
        true => (Vec::new(), asked),
        false => (asked, Vec::new()),
    };
    // Claude Code never names its page: only starting again opens it again.
    if in_browser || provider == Provider::Claude {
        let label = "Open the page again";
        controls.push(button(
            key,
            "open-page-again",
            label,
            SetupAction::OpenPageAgain,
            subtle,
        ));
    }
    controls.push(button(
        key,
        "cancel",
        "Cancel",
        SetupAction::Cancel,
        Emphasis::Ghost,
    ));
    Steps {
        sentence: Some(sentence),
        // Beside a code, the program's note only says it waits, in its own words.
        quiet: note.filter(|_| code.is_none()),
        code,
        controls,
        more,
        ..Steps::default()
    }
}

/// Whether the service's name or its plan, such as ChatGPT, holds what the composer searched
/// for, in any case.
fn matches(service: &Service, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    [Some(service.name.as_str()), service.plan()]
        .into_iter()
        .flatten()
        .any(|said| said.to_lowercase().contains(&query))
}

fn services(program: &Program, setup: Setup, query: &str) -> Services {
    let signing_in = (program.signing_in())
        .and_then(|choice| choice.service())
        .map(str::to_string);
    let named = |id: &str| {
        (program.services.iter())
            .find(|service| service.id == id)
            .map_or(id.to_string(), |service| service.name.clone())
    };
    let panel = match (setup, signing_in.clone()) {
        (Setup::SigningIn(mut steps), Some(id)) => {
            let name = named(&id);
            // Pi's words for it name an API.
            let asks_key = (steps.question.as_mut())
                .filter(|question| question.answer == Answer::Text { secret: true })
                .map(|question| question.message = format!("Paste your {name} key."))
                .is_some();
            let mut shown = signing_in_steps("pi", Provider::Pi, steps);
            if asks_key {
                shown.quiet = Some(
                    "A key is a long code from the service's website, usually under API keys \
                     or Developers."
                        .to_string(),
                );
            }
            Some(Panel {
                title: format!("Signing in to {name}"),
                body: PanelBody::Steps(shown),
            })
        }
        (
            Setup::Ready {
                reason: Some(reason),
                ..
            }
            | Setup::SignedOut {
                reason: Some(reason),
            },
            Some(id),
        ) => Some(Panel {
            title: named(&id),
            body: PanelBody::Ended(reason),
        }),
        _ => None,
    };
    let steps_of = match &panel {
        Some(Panel {
            body: PanelBody::Steps(_),
            ..
        }) => signing_in.as_deref(),
        _ => None,
    };
    let row = |service: &Service| service_row(service, steps_of, program.signing_out());
    let (signed_in, mut others): (Vec<_>, Vec<_>) =
        (program.services.iter()).partition(|service| service.signed_in.is_some());
    // Stable, so Pi's order holds within each group.
    others.sort_by_key(|service| service.in_browser.is_none());
    let listed: Vec<ServiceRow> = (others.into_iter())
        .filter(|service| matches(service, query))
        .map(row)
        .collect();
    Services {
        prompt: signed_in
            .is_empty()
            .then_some("Sign in to one of Pi's services to start."),
        signed_in: signed_in.into_iter().map(row).collect(),
        panel,
        empty: listed.is_empty().then_some("No service matches."),
        listed,
    }
}

/// The service's name and how it is signed in, and its buttons.
fn service_row(
    service: &Service,
    signing_in: Option<&str>,
    signing_out: Option<&str>,
) -> ServiceRow {
    let id = service.id.as_str();
    let steps = signing_in == Some(id);
    let signing_out = signing_out == Some(id);
    let about = match (service.signed_in, signing_out) {
        _ if steps => Some("Signing in…"),
        (_, true) => Some("Signing out…"),
        (Some(SignedIn::Here), false) => None,
        (Some(SignedIn::Elsewhere), false) => Some("Signed in outside Sound Tools"),
        (None, false) => service.plan(),
    };
    let key = format!("pi-{id}");
    let mut buttons = Vec::new();
    let mut add = |button| buttons.push(button);
    match (service.signed_in, steps, signing_out) {
        (_, true, _) | (_, _, true) | (Some(SignedIn::Elsewhere), _, _) => {}
        (Some(SignedIn::Here), false, false) => {
            let action = SetupAction::SignOut(Some(id.to_string()));
            add(make(&key, "sign-out", "Sign out", action, Emphasis::Subtle));
        }
        (None, false, false) => {
            if let Some(choice) = &service.in_browser {
                let action = SetupAction::SignIn(choice.clone());
                add(make(&key, "sign-in", "Sign in", action, Emphasis::Subtle));
            }
            if let Some(choice) = &service.with_key {
                let action = SetupAction::SignIn(choice.clone());
                add(make(&key, "add-key", "Add key", action, Emphasis::Ghost));
            }
        }
    }
    ServiceRow {
        id: id.to_string(),
        name: service.name.clone(),
        about: about.map(str::to_string),
        buttons,
    }
}

/// The ids of every button `sections` show, for a test that buttons never share one.
#[cfg(test)]
fn button_ids(sections: &[Section]) -> Vec<String> {
    let steps = |steps: &Steps, ids: &mut Vec<String>| {
        for control in steps.controls.iter().chain(&steps.more) {
            if let Control::Button(button) = control {
                ids.push(button.id.clone());
            }
        }
    };
    let mut ids = Vec::new();
    for section in sections {
        match &section.body {
            Body::Steps(shown) => steps(shown, &mut ids),
            Body::Ready { sign_out, .. } => ids.push(sign_out.id.clone()),
            Body::Services(services) => {
                if let Some(Panel {
                    body: PanelBody::Steps(shown),
                    ..
                }) = &services.panel
                {
                    steps(shown, &mut ids);
                }
                for row in services.signed_in.iter().chain(&services.listed) {
                    ids.extend(row.buttons.iter().map(|button| button.id.clone()));
                }
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn sizes_read_in_whole_megabytes() {
        assert_eq!(megabytes(225_167_728), 215);
        assert_eq!(megabytes(0), 0);
        assert_eq!(megabytes(1 << 19), 1);
    }

    fn pi(services: Vec<Service>, setup: Setup) -> Program {
        let mut program = Program::new(Provider::Pi, None, setup);
        program.services = services;
        program
    }

    /// Signed in first; then those with a browser sign-in, the search filtering them by name
    /// or plan. Button ids stay apart, so each keeps its focus.
    #[test]
    fn pi_lists_its_services_signed_in_first_then_those_the_search_finds() {
        let services = vec![
            Service::example("OpenRouter", None, None),
            Service::example("ChatGPT", Some("ChatGPT Plus"), None),
            Service::example("Gemini", None, Some(SignedIn::Here)),
        ];
        let ready = Setup::Ready {
            account: Account::default(),
            reason: None,
        };
        let program = pi(services, ready);
        let names =
            |rows: &[ServiceRow]| rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();
        let Body::Services(shown) = section(&program, "").body else {
            panic!("Pi lists services");
        };
        assert_eq!(names(&shown.signed_in), ["Gemini"]);
        assert_eq!(names(&shown.listed), ["ChatGPT", "OpenRouter"]);
        assert_eq!(shown.prompt, None);
        let Body::Services(found) = section(&program, "plus").body else {
            panic!("Pi lists services");
        };
        assert_eq!(names(&found.listed), ["ChatGPT"]);
        let Body::Services(none) = section(&program, "mistral").body else {
            panic!("Pi lists services");
        };
        assert_eq!(none.empty, Some("No service matches."));

        let ids = button_ids(&[section(&program, "")]);
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
    }

    /// A key a service asks for is named after the service, with where to find it, above the
    /// search; its row says it signs in.
    #[test]
    fn a_key_question_names_the_service() {
        let service = Service::example("OpenRouter", None, None);
        let choice = service.with_key.clone().expect("every example takes a key");
        let question = SignInQuestion {
            id: "key".to_string(),
            message: "Enter API key".to_string(),
            answer: Answer::Text { secret: true },
        };
        let signing_in = SigningIn {
            question: Some(question),
            ..SigningIn::default()
        };
        let mut program = pi(vec![service], Setup::SigningIn(signing_in));
        program.signing_in = Some(choice);
        let Body::Services(shown) = section(&program, "").body else {
            panic!("Pi lists services");
        };
        let Some(Panel {
            title,
            body: PanelBody::Steps(steps),
        }) = shown.panel
        else {
            panic!("no sign-in shows");
        };
        assert_eq!(title, "Signing in to OpenRouter");
        assert_eq!(
            steps.sentence.as_deref(),
            Some("Paste your OpenRouter key.")
        );
        assert!(steps.controls.contains(&Control::Field { secret: true }));
        assert_eq!(shown.listed[0].about.as_deref(), Some("Signing in…"));
        assert!(shown.listed[0].buttons.is_empty());
    }
}
