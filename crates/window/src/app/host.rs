//! What the code of the tools asks of the window (see `Want`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{PoisonError, mpsc};

use plugin_host::{PluginRecord, Plugins};
use rune::{FileDialogOptions, FileDialogResult, Tx};
use runtime::session::Loop;
use serde_json::Value;
use sound_core::{InstanceId, ProjectError};
use sound_typescript::devices::{self, Offer, Takes};
use sound_typescript::next::{Answer, Shown, Tools, Want};

use super::State;

/// A question of code only the composer answers, in the window's own dialog: its yes button
/// says `confirm`, the other Cancel.
pub(crate) struct Asked {
    pub(crate) question: u64,
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
    pub(crate) confirm: String,
}

/// What waits for the composer or for a thread, oldest first.
#[derive(Default)]
pub(crate) struct Waiting {
    /// The questions the dialog asks, one at a time.
    pub(crate) asked: VecDeque<Asked>,
    /// File dialogs to open, which need the frame's `Tx`.
    files: Vec<Want>,
    /// Files chosen that a thread copies into `assets/audio/`, by question.
    imports: Vec<(u64, mpsc::Receiver<Result<String, String>>)>,
}

impl State {
    /// What the window shows of `shown`, with what it knows besides the records: the one place
    /// facts, the catalog and the keys of actions are gathered.
    pub(super) fn gathered(&mut self, instances: BTreeSet<InstanceId>) -> Shown {
        let children: Vec<InstanceId> = (self.project().instances())
            .map(|(id, _)| id)
            .filter(|id| {
                id.parent()
                    .is_some_and(|parent| instances.contains(&parent))
            })
            .cloned()
            .collect();
        let known: BTreeSet<InstanceId> = instances.iter().chain(&children).cloned().collect();
        self.facts.retain(|id, _| known.contains(id));
        let facts: BTreeMap<InstanceId, Value> = (known.iter())
            .filter_map(|id| Some((id.clone(), self.facts_of(id)?)))
            .collect();
        let tools = self.tools.as_ref();
        let cards = (instances.iter())
            .filter(|id| {
                let tool = self.project().tool_of(id);
                tool.is_some_and(|tool| !tools.is_some_and(|tools| tools.has_view(tool)))
            })
            .filter_map(|id| Some((id.clone(), facts.get(id)?.clone())))
            .collect();
        if let Some(tools) = tools {
            self.keys.keymap.set_tool_groups(tools.shortcut_groups());
        }
        let holds = self.holds(&instances);
        // A hand on a lane's number lets the lane go, which then names none: what was written
        // into as the drag began still is.
        let writes = match (self.plays.idle(), &self.tools) {
            (false, Some(tools)) => tools.writes().clone(),
            _ => written(self.project(), &instances, &holds),
        };
        Shown {
            holds,
            writes,
            instances,
            cards,
            facts,
            catalog: self.catalog(),
            keys: self.action_keys(),
        }
    }

    /// What the window knows of `id` that no record holds, as `Facts` of `sdk/view.ts`: its
    /// title and fields, from the schema of its tool. Made again only when its tool or that
    /// tool's code changed.
    fn facts_of(&mut self, id: &InstanceId) -> Option<Value> {
        let tool = self.project().tool_of(id)?.to_string();
        let loads = self.tools.as_ref().map_or(0, Tools::loads);
        let fresh = !matches!(self.facts.get(id), Some((made_for, made_at, _)) if *made_for == tool && *made_at == loads);
        if fresh {
            let facts = crate::fields::card_of(self, id)?;
            self.facts.insert(id.clone(), (tool, loads, facts));
        }
        self.facts.get(id).map(|(_, _, facts)| facts.clone())
    }

    /// What `addOffer` puts: the built-in devices, the tools of the project and the plugins of
    /// this computer, each with why it cannot be put here when it cannot.
    fn catalog(&mut self) -> Vec<Offer> {
        let mut offers = devices::built_in();
        offers.extend(self.tools.iter().flat_map(Tools::offers));
        if !self.headless() {
            self.plugins.start_scanning();
        }
        let generation = self.plugins.scan_generation();
        if self.plugin_offers.0 != generation {
            self.plugin_offers = (generation, plugin_offers(&self.plugins));
        }
        offers.extend(self.plugin_offers.1.iter().cloned());
        devices::in_project(offers, self.session.project())
    }

    /// Does what code asked since the last call. Whether what the window shows changed.
    pub(super) fn serve_wants(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        let wants = tools.take_wants();
        let changed = !wants.is_empty();
        for want in wants {
            let session = &mut self.session;
            match want {
                Want::Select { instance, span } => {
                    session.select(instance, span.map(|span| (span.start, span.end)));
                }
                Want::Repeat(range) => {
                    let looped = match range {
                        Some(range) => Loop { range, on: true },
                        None => Loop {
                            on: false,
                            ..session.transport().looped.clone()
                        },
                    };
                    session.set_loop(looped);
                }
                Want::Click(on) => session.set_click(on),
                Want::ComputerKeys(on) => session.set_computer_keys(on),
                Want::WriteAutomation(on) => session.set_writes_automation(on),
                Want::Write {
                    instance,
                    label,
                    values,
                } => super::plays::write(self, &instance, &label, &values),
                Want::Confirm {
                    question,
                    message,
                    detail,
                    confirm,
                } => self.waiting.asked.push_back(Asked {
                    question,
                    message,
                    detail,
                    confirm,
                }),
                Want::OpenFile { .. } | Want::ExportNotes { .. } => self.waiting.files.push(want),
                Want::Render { question, name, .. } => {
                    self.session.report(format!(
                        "{name} was not rendered: this window does not render parts of the song yet."
                    ));
                    self.answer(question, &Answer::Asset(None));
                }
            }
        }
        changed
    }

    pub(crate) fn answer(&mut self, question: u64, answer: &Answer) {
        if let Some(tools) = &mut self.tools {
            tools.answer(question, answer);
        }
    }

    /// The composer said yes or no to the question the dialog shows.
    pub(crate) fn answer_dialog(&mut self, said: bool) {
        if let Some(asked) = self.waiting.asked.pop_front() {
            self.answer(asked.question, &Answer::Confirmed(said));
        }
        self.handled();
    }

    /// Opens the file dialogs code asked for: the system's, one at a time.
    pub(crate) fn open_file_dialogs(&mut self, tx: &mut Tx) {
        for want in std::mem::take(&mut self.waiting.files) {
            match want {
                Want::OpenFile { question, title } => {
                    let options = FileDialogOptions {
                        title,
                        ..FileDialogOptions::default()
                    };
                    tx.open_file(options, move |state: &mut State, chosen, _| {
                        state.import(question, chosen);
                    });
                }
                Want::ExportNotes {
                    question,
                    name,
                    bytes,
                } => {
                    // Next to the project folder and not in it, as an exported sound.
                    let root = self.project().root();
                    let options = FileDialogOptions {
                        title: format!("Export the notes of {name}"),
                        directory: root.parent().map(PathBuf::from),
                        file_name: Some(format!("{name}.mid")),
                        ..FileDialogOptions::default()
                    };
                    tx.save_file(options, move |state: &mut State, chosen, _| {
                        let written = match chosen {
                            Ok(Some(file)) => std::fs::write(&file, &bytes)
                                .map(|()| true)
                                .map_err(|error| format!("The notes were not exported: {error}")),
                            Ok(None) => Ok(false),
                            Err(error) => Err(format!("The file picker did not open: {error}")),
                        };
                        let written = written.unwrap_or_else(|error| {
                            state.session.report(error);
                            false
                        });
                        state.answer(question, &Answer::Confirmed(written));
                        state.handled();
                    });
                }
                _ => {}
            }
        }
    }

    /// Copies the file the composer chose into `assets/audio/` on a thread; the answer goes once
    /// it is there.
    fn import(&mut self, question: u64, chosen: FileDialogResult) {
        let path = match chosen {
            Ok(Some(path)) => path,
            Ok(None) => return self.answer(question, &Answer::Asset(None)),
            Err(error) => {
                self.session
                    .report(format!("The file picker did not open: {error}"));
                return self.answer(question, &Answer::Asset(None));
            }
        };
        let (send, imported) = mpsc::channel();
        let assets = self.project().assets().clone();
        let waker = self.waker.clone();
        let started = std::thread::Builder::new()
            .name("import".into())
            .spawn(move || {
                let imported = sound_media::import(&assets, &path);
                send.send(
                    imported
                        .map(|file| file.asset.to_string())
                        .map_err(|error| error.to_string()),
                )
                .ok();
                if let Some(wake) = waker
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                {
                    wake();
                }
            });
        match started {
            Ok(_) => self.waiting.imports.push((question, imported)),
            Err(error) => {
                self.session.report(error);
                self.answer(question, &Answer::Asset(None));
            }
        }
        self.handled();
    }

    /// Answers the imports that ended; a run without a window waits for them, as it does for Bun.
    pub(super) fn imported(&mut self) {
        let headless = self.headless();
        let mut ended = Vec::new();
        self.waiting.imports.retain(|(question, imported)| {
            let result = match headless {
                true => imported
                    .recv()
                    .map_err(|_| mpsc::TryRecvError::Disconnected),
                false => imported.try_recv(),
            };
            match result {
                Ok(result) => ended.push((*question, result)),
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => {
                    ended.push((*question, Err("the copy stopped".to_string())));
                }
            }
            false
        });
        for (question, result) in ended {
            let asset = match result {
                Ok(asset) => Some(asset),
                Err(error) => {
                    self.session.report(error);
                    None
                }
            };
            self.answer(question, &Answer::Asset(asset));
        }
    }
}

/// The plugins of this computer as offers, one for a plugin installed in two formats, then the
/// bundles that could not be scanned, and a line while the scan still runs. Nothing until a scan
/// started: looking at the machine is not the catalog's to start.
fn plugin_offers(plugins: &Plugins) -> Vec<Offer> {
    let scanning = plugins.scan_is_running();
    if !scanning && plugins.scan_generation() == 0 {
        return Vec::new();
    }
    let scanned = plugins.scan().plugins;
    let found = plugin_host::one_per_plugin(scanned)
        .into_iter()
        .map(|found| {
            let takes = match found.is_instrument() {
                true => Takes::Notes,
                false => Takes::Audio,
            };
            let (id, name, format) = (found.id.clone(), found.name.clone(), found.format);
            let write: devices::Write =
                std::rc::Rc::new(move |project, slot, changes| {
                    // A state file of its own that no plugin ever wrote into, so a plugin picked never
                    // comes up holding the sound an older one left behind.
                    let state_asset = plugin_host::new_state_asset(project.assets(), &name)
                        .map_err(|error| ProjectError::InvalidState {
                            id: slot.clone(),
                            message: error.to_string(),
                        })?;
                    changes.create(
                        slot.clone(),
                        PluginRecord {
                            format,
                            plugin_id: id.clone(),
                            state_asset,
                            parameters: Default::default(),
                        },
                    );
                    Ok(())
                });
            Offer {
                key: found.offer_key(),
                name: found.name.clone(),
                detail: (!found.vendor.is_empty()).then(|| found.vendor.clone()),
                group: devices::Group::Plugins,
                icon: "device-plugin".to_string(),
                takes,
                unavailable: None,
                write: Some(write),
                needs: Some((
                    plugin_host::EXTENSION,
                    "This project does not load plugins.".to_string(),
                )),
            }
        });
    let failed = plugins.failures().into_iter().map(|failure| {
        let name = failure.path.file_stem().unwrap_or(failure.path.as_os_str());
        Offer::unavailable(
            format!("failed:{}", failure.path.display()),
            name.to_string_lossy().into_owned(),
            Takes::Audio,
            format!("Could not be scanned: {}", failure.message),
        )
    });
    let still = scanning.then(|| {
        Offer::unavailable(
            "plugins:scanning".to_string(),
            "Still looking for the plugins of this computer…".to_string(),
            Takes::Audio,
            "The plugins show here once the search is done.".to_string(),
        )
    });
    found.chain(failed).chain(still).collect()
}

/// The numbers a lane moves of each instance of `shown` that `holds` leaves free: a hand writes
/// into their lanes now.
fn written(
    project: &sound_core::Project,
    shown: &BTreeSet<InstanceId>,
    holds: &BTreeMap<InstanceId, Vec<String>>,
) -> BTreeMap<InstanceId, Vec<String>> {
    let mut writes = runtime::session::held(project, shown, false);
    for (id, numbers) in &mut writes {
        let held = holds.get(id);
        numbers.retain(|number| !held.is_some_and(|held| held.contains(number)));
    }
    writes.retain(|_, numbers| !numbers.is_empty());
    writes
}
