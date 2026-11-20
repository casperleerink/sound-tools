//! Exporting the piece: a dialog of how (the format, the sample rate, the bit depth of a WAV, the
//! loudness and, while something is selected, whether only that), then the system's save panel
//! next to the project folder, the render in a process of its own, and a dialog once it is done.
//! The last choice is kept for this machine.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rune::{El, FileDialogOptions, FileDialogResult, Tx, col, row, text};
use rune_kit::{ButtonVariant, SegmentedControl};
use runtime::session::Rendered;
use sound_export::{CEILING_DBTP, Depth, Format, Loudness, SampleRate, Settings};

use super::State;
use super::chrome::Act as ChromeAct;
use super::render::Purpose;
use crate::controls::{Control, Target};
use crate::shell::dialog::{Dialog, Note, button};

const NOTHING: &str =
    "Nothing in the project plays to an end, such as a clip, so there is nothing to export.";
const BUSY: &str = "Another render is running: try again once it is done.";

pub(crate) struct Export {
    /// Where the last choice is kept. None keeps nothing: every export starts from the defaults.
    pub(crate) kept: Option<PathBuf>,
    open: Option<Open>,
    /// The choice whose save panel the next frame opens.
    asking: Option<Choice>,
    /// What Show in Finder showed last in a run without a window, which shows no folder.
    pub(crate) revealed: Option<PathBuf>,
}

enum Open {
    Choosing(Choice),
    Done { file: PathBuf, rendered: Rendered },
}

#[derive(Clone, Copy)]
struct Choice {
    settings: Settings,
    /// The depth of a WAV, kept while MP3 is chosen.
    depth: Depth,
    selection: bool,
}

/// What a press in the dialogs does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Act {
    Format(Format),
    Rate(SampleRate),
    Depth(Depth),
    Loudness(Option<Loudness>),
    /// Only what is selected.
    Selection(bool),
    Export,
    Reveal,
    Close,
}

/// Each choice of a row: a value for its key, its label, and what it sets.
type Choices<T> = &'static [(&'static str, &'static str, T)];

/// A WAV of any depth is the one WAV choice of the format row.
const FORMATS: Choices<Format> = &[
    ("wav", "WAV", Format::Wav(Depth::Float)),
    ("mp3", "MP3", Format::Mp3),
];
const RATES: Choices<SampleRate> = &[
    ("44100", "44.1 kHz", SampleRate::Hz44100),
    ("48000", "48 kHz", SampleRate::Hz48000),
];
const DEPTHS: Choices<Depth> = &[
    ("16", "16-bit", Depth::Sixteen),
    ("24", "24-bit", Depth::TwentyFour),
    ("32", "32-bit float", Depth::Float),
];
const LOUDNESSES: Choices<Option<Loudness>> = &[
    ("as-it-plays", "As it plays", None),
    ("streaming", "Streaming", Some(Loudness::Streaming)),
    ("broadcast", "Broadcast", Some(Loudness::Broadcast)),
];
const PARTS: Choices<bool> = &[
    ("whole", "Whole piece", false),
    ("selection", "Selection", true),
];

/// A row of the dialog: its label and its choices, with the one chosen.
struct Row {
    key: &'static str,
    label: &'static str,
    picks: Vec<(String, &'static str, Act)>,
    chosen: Option<usize>,
}

fn row_of<T: Copy + PartialEq>(
    key: &'static str,
    label: &'static str,
    choices: Choices<T>,
    chosen: T,
    act: fn(T) -> Act,
) -> Row {
    Row {
        key,
        label,
        picks: (choices.iter())
            .map(|&(value, label, choice)| (format!("{key}-{value}"), label, act(choice)))
            .collect(),
        chosen: choices.iter().position(|choice| choice.2 == chosen),
    }
}

impl Choice {
    /// The rows: the part only when both the whole piece and a selection can be exported.
    fn rows(&self, both: bool) -> Vec<Row> {
        let Settings {
            format,
            rate,
            loudness,
        } = self.settings;
        let wav = Format::Wav(Depth::Float);
        let format_choice = if matches!(format, Format::Wav(_)) {
            wav
        } else {
            format
        };
        let mut rows = Vec::new();
        if both {
            rows.push(row_of(
                "export-part",
                "Export",
                PARTS,
                self.selection,
                Act::Selection,
            ));
        }
        rows.push(row_of(
            "export-format",
            "Format",
            FORMATS,
            format_choice,
            Act::Format,
        ));
        rows.push(row_of("export-rate", "Sample rate", RATES, rate, Act::Rate));
        if let Format::Wav(depth) = format {
            rows.push(row_of(
                "export-depth",
                "Bit depth",
                DEPTHS,
                depth,
                Act::Depth,
            ));
        }
        rows.push(row_of(
            "export-loudness",
            "Loudness",
            LOUDNESSES,
            loudness,
            Act::Loudness,
        ));
        rows
    }

    fn title(&self) -> &'static str {
        match self.selection {
            true => "Export selection",
            false => "Export audio",
        }
    }
}

impl Export {
    pub(super) fn new(kept: Option<PathBuf>) -> Self {
        Self {
            kept,
            open: None,
            asking: None,
            revealed: None,
        }
    }
}

impl State {
    /// Opens the export dialog with the last choice; says why when nothing can be exported or
    /// another render runs.
    pub(crate) fn export(&mut self) {
        if self.session.rendering().is_some() {
            return self.session.report(BUSY);
        }
        let (whole, selection) = self.parts();
        if !whole && !selection {
            return self.session.report(NOTHING);
        }
        let settings = remembered(self.export.kept.as_deref());
        let depth = match settings.format {
            Format::Wav(depth) => depth,
            Format::Mp3 => Depth::default(),
        };
        self.export.open = Some(Open::Choosing(Choice {
            settings,
            depth,
            selection: !whole,
        }));
    }

    /// Whether the whole piece, and a selection, can be exported.
    fn parts(&self) -> (bool, bool) {
        let whole = self.project().end().is_some();
        (whole, self.session.selected_span().is_some())
    }

    pub(super) fn export_act(&mut self, act: Act) {
        let open = &mut self.export.open;
        match (act, open.as_mut()) {
            (Act::Close, _) => *open = None,
            (Act::Reveal, Some(Open::Done { file, .. })) => {
                let file = file.clone();
                *open = None;
                self.reveal(file);
            }
            (Act::Export, Some(Open::Choosing(choice))) => {
                let choice = *choice;
                *open = None;
                remember(choice.settings, self.export.kept.as_deref());
                self.export.asking = Some(choice);
            }
            (act, Some(Open::Choosing(choice))) => match act {
                Act::Format(Format::Wav(_)) => choice.settings.format = Format::Wav(choice.depth),
                Act::Format(Format::Mp3) => choice.settings.format = Format::Mp3,
                Act::Depth(depth) => {
                    choice.depth = depth;
                    choice.settings.format = Format::Wav(depth);
                }
                Act::Rate(rate) => choice.settings.rate = rate,
                Act::Loudness(loudness) => choice.settings.loudness = loudness,
                Act::Selection(selection) => choice.selection = selection,
                _ => {}
            },
            _ => {}
        }
        self.handled();
    }

    /// The system's save panel for the choice Export took, next to the project folder and not
    /// in it: an export is not part of the project, and in it, git and the agent would see it.
    pub(super) fn ask_where_to_export(&mut self, tx: &mut Tx) {
        let Some(choice) = self.export.asking.take() else {
            return;
        };
        let folder = self.project().root();
        let name = folder.file_name().unwrap_or_default().to_string_lossy();
        let options = FileDialogOptions {
            title: choice.title().to_string(),
            directory: Some(folder.parent().unwrap_or(folder).to_path_buf()),
            file_name: Some(format!("{name}.{}", choice.settings.format.ending())),
            ..FileDialogOptions::default()
        };
        tx.save_file(options, move |state: &mut State, picked, _| {
            if let Err(error) = state.export_to(choice, picked) {
                state.session.report(format!("{error:#}"));
            }
            state.handled();
        });
    }

    fn export_to(&mut self, choice: Choice, picked: FileDialogResult) -> Result<()> {
        let file = match picked {
            Ok(Some(file)) => file,
            Ok(None) => return Ok(()),
            Err(error) => anyhow::bail!("The save panel did not open: {error}"),
        };
        let ending = choice.settings.format.ending();
        // The save panel asked about this very name, so it is not changed here.
        if !choice.settings.format.names(&file) {
            anyhow::bail!("The name must end in .{ending}.");
        }
        let mut arguments = choice.settings.arguments();
        if choice.selection {
            let (from, to) = self
                .session
                .selected_span()
                .ok_or_else(|| anyhow::anyhow!(NOTHING))?;
            arguments.extend([
                "--from".into(),
                from.0.to_string(),
                "--to".into(),
                to.0.to_string(),
            ]);
        } else if self.project().end().is_none() {
            anyhow::bail!(NOTHING);
        }
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let notice = format!("Exporting {name}");
        let purpose = Purpose::Export(file.clone());
        (self.start_render(notice, &file, &arguments, purpose))
            .map_err(|error| anyhow::anyhow!("The export failed: {error:#}"))
    }

    /// What an export ended with: the dialog that says it is done, or why it failed.
    pub(super) fn exported(&mut self, file: PathBuf, rendered: Result<Rendered>) {
        match rendered {
            Ok(rendered) => self.export.open = Some(Open::Done { file, rendered }),
            Err(error) => (self.session).report(format!("The export failed: {error:#}")),
        }
    }

    fn reveal(&mut self, file: PathBuf) {
        if self.headless() {
            self.export.revealed = Some(file);
        } else if let Err(error) = reveal(&file) {
            (self.session).report(format!("The folder did not open: {error}"));
        }
    }
}

pub(super) const REVEAL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else {
    "Show in folder"
};

/// Shows `file` chosen in the system's file browser.
pub(crate) fn reveal(file: &Path) -> std::io::Result<()> {
    let mut command;
    if cfg!(target_os = "macos") {
        command = sound_core::process::background_command("/usr/bin/open");
        command.arg("-R").arg(file);
    } else if cfg!(windows) {
        command = sound_core::process::background_command("explorer");
        command.arg(format!("/select,{}", file.display()));
    } else {
        command = sound_core::process::background_command("xdg-open");
        command.arg(file.parent().unwrap_or(file));
    }
    // Starting it is quick; a thread waits for it to end.
    #[allow(clippy::disallowed_methods)]
    let mut child = command.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The settings last chosen, or the defaults when there are none or they do not read.
fn remembered(kept: Option<&Path>) -> Settings {
    let bytes = kept.and_then(|file| std::fs::read(file).ok());
    bytes
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Keeps `settings` for the next export. Not keeping them loses only that.
fn remember(settings: Settings, kept: Option<&Path>) {
    let Some(file) = kept else {
        return;
    };
    let written = serde_json::to_vec_pretty(&settings)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| {
            if let Some(folder) = file.parent() {
                std::fs::create_dir_all(folder)?;
            }
            Ok(std::fs::write(file, bytes)?)
        });
    if let Err(error) = written {
        eprintln!("error: the export settings were not kept: {error:#}");
    }
}

/// The open dialog, over the window.
pub(super) fn view(state: &State) -> Vec<El> {
    let theme = &state.theme;
    let press = |act| move |state: &mut State, _: &mut Tx| state.export_act(act);
    let close = |state: &mut State| state.export_act(Act::Close);
    let dialog = match &state.export.open {
        None => return Vec::new(),
        Some(Open::Choosing(choice)) => {
            let rows = choice.rows(state.parts() == (true, true));
            let body = rows.into_iter().map(|line| {
                let labels: Vec<&str> = line.picks.iter().map(|pick| pick.1).collect();
                let acts: Vec<Act> = line.picks.iter().map(|pick| pick.2).collect();
                let control = SegmentedControl::new(&labels, line.chosen)
                    .label(line.label)
                    .fit()
                    .on_pick(move |state: &mut State, at, _| state.export_act(acts[at]))
                    .build(&theme.kit)
                    .key(line.key);
                row()
                    .w_full()
                    .gap(theme.space(4))
                    .items_center()
                    .justify_between()
                    .child(
                        text(line.label)
                            .font(theme.text("body"))
                            .color(theme.color("text-secondary")),
                    )
                    .child(control)
            });
            let hint = choice.settings.loudness.map(|loudness| {
                let minus = |value: f64| format!("{value:.0}").replace('-', "−");
                let (lufs, peak) = (minus(loudness.lufs()), minus(CEILING_DBTP));
                Note::Quiet(format!("{lufs} LUFS, peaks under {peak} dB"))
            });
            Dialog::<State> {
                key: "export",
                title: choice.title().into(),
                description: None,
                body: vec![col().w_full().gap(theme.space(3)).children(body)],
                note: hint,
                buttons: vec![
                    button(
                        theme,
                        "export-cancel",
                        "Cancel",
                        ButtonVariant::Neutral,
                        true,
                        press(Act::Close),
                    ),
                    button(
                        theme,
                        "export-confirm",
                        "Export…",
                        ButtonVariant::Contrast,
                        true,
                        press(Act::Export),
                    ),
                ],
                close,
            }
        }
        Some(Open::Done { file, rendered }) => {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            // A plugin that did not load or answer in the render is missing from the file.
            let incomplete = (!rendered.problems.is_empty()).then(|| {
                Note::Wrong(format!(
                    "It may be incomplete: {}",
                    rendered.problems.join("; ")
                ))
            });
            Dialog::<State> {
                key: "exported",
                title: format!("Exported {name}"),
                description: (!rendered.notes.is_empty()).then(|| rendered.notes.join("\n\n")),
                body: Vec::new(),
                note: incomplete,
                buttons: vec![
                    button(
                        theme,
                        "exported-reveal",
                        REVEAL,
                        ButtonVariant::Neutral,
                        true,
                        press(Act::Reveal),
                    ),
                    button(
                        theme,
                        "exported-ok",
                        "OK",
                        ButtonVariant::Contrast,
                        true,
                        press(Act::Close),
                    ),
                ],
                close,
            }
        }
    };
    dialog.build(theme)
}

/// The choices and buttons of the open dialog, for `--tree` and `--act`.
pub(super) fn controls(state: &State) -> Vec<Control> {
    let control = |key: String, label: &str, role: &str, act| Control {
        key,
        label: label.into(),
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: vec!["press".into()],
        target: Target::Chrome(ChromeAct::Export(act)),
    };
    match &state.export.open {
        None => Vec::new(),
        Some(Open::Choosing(choice)) => {
            let rows = choice.rows(state.parts() == (true, true)).into_iter();
            let picks = rows.flat_map(|row| {
                (row.picks.into_iter().enumerate()).map(move |(at, (key, label, act))| Control {
                    checked: Some(row.chosen == Some(at)),
                    ..control(key, label, "radio", act)
                })
            });
            picks
                .chain([
                    control("export-cancel".into(), "Cancel", "button", Act::Close),
                    control("export-confirm".into(), "Export…", "button", Act::Export),
                ])
                .collect()
        }
        Some(Open::Done { .. }) => vec![
            control("exported-reveal".into(), REVEAL, "button", Act::Reveal),
            control("exported-ok".into(), "OK", "button", Act::Close),
        ],
    }
}
