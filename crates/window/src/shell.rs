//! Starting the app and moving between projects: the start state with no project, the dialogs
//! of a new project and of a saved template, and opening a project. One project a process:
//! opening another remembers it first of the recent ones, quits, and starts the app again with
//! no folder, which opens the last project, as the GPUI app did. A fresh process gives the new
//! project its own lock, plugins and MIDI ports.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use rune::{TextEdit, Tx};
use runtime::templates::{self, Choice, Saved};
use sound_core::Project;

pub(crate) mod dialog;
mod new_project;
mod save_template;
pub(crate) mod start;

pub(crate) use new_project::{NewProject, template_row};
pub(crate) use save_template::SaveTemplate;

/// Set once the window quits to open another project or an update: `main` starts the app
/// again after Rune let go of this one.
static STARTS_AGAIN: AtomicBool = AtomicBool::new(false);

/// A folder as the composer reads it: the home folder is `~`.
pub(crate) fn short_path(folder: &Path) -> String {
    let home = std::env::home_dir();
    let inside = home
        .as_deref()
        .and_then(|home| folder.strip_prefix(home).ok());
    match inside {
        Some(inside) if inside.as_os_str().is_empty() => "~".to_string(),
        Some(inside) => Path::new("~").join(inside).display().to_string(),
        None => folder.display().to_string(),
    }
}

/// Whether the app quit to start again.
pub(crate) fn starts_again() -> bool {
    STARTS_AGAIN.load(Ordering::Relaxed)
}

/// Quits, and `main` starts the app again on the last project: the way to another project
/// and to a downloaded update, which installs before any window.
pub(crate) fn start_again(tx: &mut Tx) {
    STARTS_AGAIN.store(true, Ordering::Relaxed);
    tx.quit();
}

/// What a window with dialogs of projects gives them.
pub(crate) trait Host: 'static {
    fn shell(&self) -> &Projects;
    fn shell_mut(&mut self) -> &mut Projects;
    /// The open project, which a template is saved from.
    fn project(&self) -> Option<&Project>;
    /// Says what went wrong where the window says it.
    fn tell(&mut self, message: String);
}

/// The projects of this machine as a window sees them, and its dialog of them.
pub(crate) struct Projects {
    /// The machine's support folder: the recent projects and the saved templates. None keeps
    /// nothing and reads nothing.
    support: Option<PathBuf>,
    /// A run without a window opens nothing: it keeps what it would open in `opened`.
    headless: bool,
    /// The folder of the open project; None in the start state.
    here: Option<PathBuf>,
    pub(crate) dialog: Option<Dialog>,
    /// The name field of the open dialog.
    pub(crate) name: TextEdit,
    /// The project this window opened last.
    pub(crate) opened: Option<PathBuf>,
    /// The templates as they were read last, for an empty project to start from.
    templates: Vec<Choice>,
}

pub(crate) enum Dialog {
    New(NewProject),
    Save(SaveTemplate),
}

/// What a press in a dialog does, by key or by `--act`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// The template of this row of the new project.
    Pick(usize),
    /// Asks to remove this saved template, then does on `Confirm`.
    Remove(usize),
    /// The folder the new project goes in, in the system's folder panel.
    Place,
    /// Makes the new project, saves the template, or removes the template asked about.
    Confirm,
    /// Saves the template with its content, or with its tools only.
    Content(bool),
    /// The name field: `--act` gives it its text.
    Name,
    Close,
}

/// A control of a dialog, for `--act` and `--tree`: its key, label and role, and what it does.
pub(crate) struct Pressable {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) role: &'static str,
    pub(crate) checked: Option<bool>,
    /// What a text field says.
    pub(crate) value: Option<String>,
    pub(crate) act: Act,
}

impl Projects {
    /// `support` None for a run that keeps nothing, such as a test. The open project, `here`,
    /// becomes the first of the recent ones, but in a run without a window.
    pub(crate) fn new(support: Option<PathBuf>, headless: bool, here: Option<PathBuf>) -> Self {
        if let (Some(support), Some(here), false) = (&support, &here, headless)
            && let Err(error) = runtime::app::remember_project_in(support, here)
        {
            eprintln!("error: {error:#}");
        }
        let mut projects = Self {
            support,
            headless,
            here,
            dialog: None,
            name: TextEdit::new(""),
            opened: None,
            templates: Vec::new(),
        };
        projects.templates = projects
            .choices()
            .unwrap_or_else(|_| app_templates().collect());
        projects
    }

    pub(crate) fn templates(&self) -> &[Choice] {
        &self.templates
    }

    /// The machine's support folder, as the app keeps it.
    pub(crate) fn machine() -> Option<PathBuf> {
        runtime::app::support_folder().ok()
    }

    /// The projects opened before, the last first, whose folders are there; not the open one.
    pub(crate) fn recent(&self) -> Vec<PathBuf> {
        let recent = (self.support.as_deref()).map(runtime::app::recent_projects_in);
        let mut recent = recent.unwrap_or_default();
        recent.retain(|folder| Some(folder) != self.here.as_ref());
        recent
    }

    /// The templates of the app, then the ones saved on this machine.
    fn choices(&self) -> Result<Vec<Choice>> {
        let saved = match &self.support {
            Some(support) => templates::saved_in(&support.join(SAVED))?,
            None => Vec::new(),
        };
        Ok(app_templates()
            .chain(saved.into_iter().map(Choice::Saved))
            .collect())
    }

    fn saved_folder(&self) -> Result<PathBuf> {
        let support = self.support.as_ref();
        let support = support.ok_or_else(|| anyhow::anyhow!("This run keeps no templates."))?;
        Ok(support.join(SAVED))
    }

    /// The controls of the open dialog.
    pub(crate) fn controls(&self) -> Vec<Pressable> {
        match &self.dialog {
            Some(Dialog::New(new)) => new.controls(self),
            Some(Dialog::Save(save)) => save.controls(self),
            None => Vec::new(),
        }
    }

    /// Opens `folder` as the project of the window, in place of the one open: refused with
    /// plain words for a folder that is gone or holds someone's other files.
    pub(crate) fn open(&mut self, folder: &Path, tx: &mut Tx) -> Result<()> {
        // A recent project that went away is no new project.
        if !folder.is_dir() {
            anyhow::bail!("{} is not there anymore.", folder.display());
        }
        runtime::app::check_project_folder(folder)?;
        self.opened = Some(folder.to_path_buf());
        if self.headless {
            return Ok(());
        }
        if let Some(support) = &self.support {
            runtime::app::remember_project_in(support, folder)?;
        }
        start_again(tx);
        Ok(())
    }

    /// The system's folder panel, for a project to open.
    pub(crate) fn choose_project<S: Host>(tx: &mut Tx) {
        let options = rune::FileDialogOptions {
            title: "Open a project".into(),
            ..Default::default()
        };
        tx.open_folder(options, |s: &mut S, picked, tx| match picked {
            Ok(Some(folder)) => {
                if let Err(error) = s.shell_mut().open(&folder, tx) {
                    s.tell(format!("{error:#}"));
                }
            }
            Ok(None) => {}
            Err(error) => s.tell(format!("The folder panel did not open: {error}")),
        });
    }

    /// The dialog of a new project, in the folder next to the open project's.
    pub(crate) fn new_project(&mut self) {
        let place = (self.here.as_deref().and_then(Path::parent))
            .map(Path::to_path_buf)
            .or_else(|| {
                self.recent()
                    .first()
                    .and_then(|last| Some(last.parent()?.to_path_buf()))
            })
            .or_else(std::env::home_dir)
            .unwrap_or_default();
        let (choices, message) = match self.choices() {
            Ok(choices) => {
                self.templates = choices.clone();
                (choices, None)
            }
            Err(error) => (app_templates().collect(), Some(format!("{error:#}"))),
        };
        self.name = TextEdit::new("");
        self.name.focused = true;
        self.dialog = Some(Dialog::New(NewProject::new(choices, place, message)));
    }

    /// The dialog that saves the open project as a template: its name first, all selected.
    pub(crate) fn save_as_template(&mut self) {
        let Some(here) = &self.here else {
            return;
        };
        let name = here.file_name().unwrap_or_default().to_string_lossy();
        self.name = TextEdit::new(name.as_ref());
        self.name.set_selection(0, self.name.text.len());
        self.name.focused = true;
        self.dialog = Some(Dialog::Save(SaveTemplate::default()));
    }
}

/// Where the saved templates are in the support folder, as `runtime::templates::saved_folder`.
const SAVED: &str = "templates";

/// The templates of the app a new project of this window starts from: the DAW of the window,
/// and the others of the runtime but its DAW, whose views this window does not draw.
fn app_templates() -> impl Iterator<Item = Choice> {
    let others = (templates::TEMPLATES.iter()).filter(|template| template.name != "DAW");
    std::iter::once(&crate::app::TEMPLATE)
        .chain(others)
        .map(Choice::Bundled)
}

/// Runs `act` of the open dialog, with `value` for the name field.
pub(crate) fn run<S: Host>(s: &mut S, act: Act, value: Option<String>, tx: &mut Tx) {
    if act == Act::Close {
        s.shell_mut().dialog = None;
        return;
    }
    if let (Act::Name, Some(value)) = (act, value) {
        s.shell_mut().name = TextEdit::new(value);
        return;
    }
    match s.shell().dialog {
        Some(Dialog::New(_)) => new_project::run(s, act, tx),
        Some(Dialog::Save(_)) => save_template::run(s, act),
        None => {}
    }
}

/// The dialog open now, drawn over the window.
pub(crate) fn overlay<S: Host>(s: &S, theme: &crate::theme::Theme) -> Vec<rune::El> {
    match &s.shell().dialog {
        Some(Dialog::New(new)) => new.view::<S>(s.shell(), theme),
        Some(Dialog::Save(save)) => save.view::<S>(s.shell(), theme),
        None => Vec::new(),
    }
}

/// A saved template, by its row.
fn saved_of(choices: &[Choice], index: usize) -> Option<&Saved> {
    match choices.get(index)? {
        Choice::Saved(saved) => Some(saved),
        Choice::Bundled(_) => None,
    }
}

#[cfg(test)]
mod tests;
