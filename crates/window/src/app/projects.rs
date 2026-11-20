//! The projects of the project's window: its actions New project, Save as template and Open
//! project, their dialogs for `--act`, and what an empty project shows.

use anyhow::{Result, bail};
use rune::{El, Tx, col, text};
use rune_kit::{Button, ButtonVariant};
use runtime::templates::Choice;
use serde_json::Value;
use sound_core::Project;

use super::State;
use super::chrome::Act;
use crate::controls::{Control, Target};
use crate::shell::{self, Host, Projects};

impl Host for State {
    fn shell(&self) -> &Projects {
        &self.shell
    }

    fn shell_mut(&mut self) -> &mut Projects {
        &mut self.shell
    }

    fn project(&self) -> Option<&Project> {
        Some(self.session.project())
    }

    fn tell(&mut self, message: String) {
        self.session.report(message);
    }
}

/// The app's actions of projects, by their names in `--act` and the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppAction {
    NewProject,
    SaveAsTemplate,
    OpenProject,
}

impl AppAction {
    pub(crate) fn named(name: &str) -> Option<Self> {
        match name {
            "app.new-project" => Some(Self::NewProject),
            "app.save-as-template" => Some(Self::SaveAsTemplate),
            "app.open-project" => Some(Self::OpenProject),
            _ => None,
        }
    }
}

impl State {
    /// Runs `action`; Open project shows the system's folder panel, which `tx` asks for.
    pub(crate) fn app_action(&mut self, action: AppAction, tx: &mut Tx) {
        match action {
            AppAction::NewProject => self.shell.new_project(),
            AppAction::SaveAsTemplate => self.shell.save_as_template(),
            AppAction::OpenProject => Projects::choose_project::<State>(tx),
        }
    }

    /// An app action of `--act`, by name: Open project takes the folder as its value, for the
    /// panel a run without a window cannot show. None for a name that is none.
    pub(crate) fn act_app(&mut self, name: &str, value: Option<Value>) -> Option<Result<()>> {
        let action = AppAction::named(name)?;
        if action != AppAction::OpenProject {
            self.app_action(action, &mut Tx::default());
            return Some(Ok(()));
        }
        let Some(Value::String(folder)) = value else {
            return Some(Err(anyhow::anyhow!(
                "app.open-project takes the project folder as its value"
            )));
        };
        Some(self.shell.open(folder.as_ref(), &mut Tx::default()))
    }
}

/// The controls of the open dialog, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Vec<Control> {
    let controls = state.shell.controls().into_iter();
    controls
        .map(|control| Control {
            key: control.key,
            label: control.label,
            role: Some(control.role.into()),
            value: control.value.map(Value::String),
            checked: control.checked,
            expanded: None,
            actions: vec![
                match control.role {
                    "textbox" => "commit",
                    _ => "press",
                }
                .into(),
            ],
            place: None,
            target: Target::Chrome(Act::Shell(control.act)),
            menu: None,
            drag: None,
        })
        .collect()
}

/// Runs a control of the dialog for `--act`, with the text of a field.
pub(super) fn run(state: &mut State, act: shell::Act, value: Option<Value>) -> Result<()> {
    let value = match (act, value) {
        (shell::Act::Name, Some(Value::String(text))) => Some(text),
        (shell::Act::Name, _) => bail!("a name takes its text as the value"),
        _ => None,
    };
    shell::run(state, act, value, &mut Tx::default());
    Ok(())
}

/// What the main area shows with no main view: in an empty project, the ways to start one.
pub(super) fn empty(state: &State) -> El {
    let theme = &state.theme;
    let has_tools = state.project().instances().next().is_some();
    let (title, words) = match has_tools {
        true => (
            "Nothing to show",
            "Nothing in this project has a view yet. Ask the agent for one.",
        ),
        false => (
            "An empty project",
            "Ask the agent for the tools you want, or start from a template.",
        ),
    };
    // Not Bare, whose project file is all it writes.
    let some_tools = |template: &runtime::Template| {
        (template.files.iter()).any(|(path, _)| *path != runtime::PROJECT_FILE)
    };
    let choices = (state.shell.templates().iter())
        .filter(|choice| !matches!(choice, Choice::Bundled(template) if !some_tools(template)));
    let rows = choices.enumerate().map(|(index, choice)| {
        let choice = choice.clone();
        shell::template_row(
            theme,
            format!("start-from-{index}"),
            &choice.clone(),
            None,
            None,
            move |state: &mut State, _: &mut Tx| state.start_from(&choice),
        )
    });
    let ask = state.panel.agent.as_ref().map(|_| {
        Button::new("Ask the agent")
            .variant(ButtonVariant::Neutral)
            .on_press_tx(|state: &mut State, tx: &mut Tx| state.agent_keys(false, tx))
            .build(&theme.kit)
            .key("ask-the-agent")
    });
    let rows = (!has_tools).then(|| col().w_full().gap(theme.space(2)).children(rows));
    col()
        .key("empty-project")
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .child(
            col()
                .w(420.0)
                .gap(theme.space(4))
                .child(
                    col()
                        .gap(theme.space(1))
                        .child(
                            text(title)
                                .font(theme.text("title"))
                                .color(theme.color("text")),
                        )
                        .child(
                            text(words)
                                .font(theme.text("body"))
                                .color(theme.color("text-secondary"))
                                .wrap(),
                        ),
                )
                .children(rows)
                .children(ask),
        )
}

impl State {
    /// Writes `template` into this empty project, which then shows it as it would a change of
    /// the agent's.
    fn start_from(&mut self, template: &Choice) {
        if let Err(error) = template.write(self.project().root()) {
            self.session.report(format!("{error:#}"));
        }
    }
}
