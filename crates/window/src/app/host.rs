//! What the code of the tools asks of the window (see `Want`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use plugin_host::{PluginRecord, Plugins};
use rune::{FileDialogOptions, Tx};
use runtime::input::{Brought, Importing};
use runtime::session::Loop;
use sound_core::facts::{self, Facts};
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
    /// Files on their way into `assets/audio/`, by question.
    imports: Vec<Import>,
}

struct Import {
    question: u64,
    importing: Importing,
    /// The folder of a render, which goes once its file is in.
    _scratch: Option<tempfile::TempDir>,
}

impl State {
    /// What the window shows of `shown`, with what it knows besides the records: the one place
    /// facts, the catalog and the keys of actions are gathered.
    pub(super) fn gathered(&mut self, instances: BTreeSet<InstanceId>) -> Shown {
        self.facts = self.facts_now(&instances);
        let facts = (self.facts.iter())
            .map(|(id, facts)| (id.clone(), facts.to_value()))
            .collect();
        let tools = self.tools.as_ref();
        let cards = (instances.iter())
            .filter(|id| {
                let tool = self.project().tool_of(id);
                tool.is_some_and(|tool| !tools.is_some_and(|tools| tools.has_view(tool)))
            })
            .cloned()
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

    /// The facts of `shown` and of their children and grandchildren, which views and actions
    /// read: what no record holds, such as the fields of a record or the length of a file.
    pub(super) fn facts_now(&self, shown: &BTreeSet<InstanceId>) -> BTreeMap<InstanceId, Facts> {
        let project = self.project();
        let reached = |id: &InstanceId| {
            let parent = id.parent();
            let grandparent = parent.as_ref().and_then(InstanceId::parent);
            [Some(id.clone()), parent, grandparent]
                .iter()
                .flatten()
                .any(|above| shown.contains(above))
        };
        (project.instances().map(|(id, _)| id))
            .filter(|id| reached(id))
            .filter_map(|id| {
                // A tool of the project gives its schema; a built-in one registered it.
                let schema =
                    (project.tool_of(id)).and_then(|tool| self.tools.as_ref()?.schema(tool));
                let facts = facts::of(project, &self.sources(), id, schema.as_ref())?;
                Some((id.clone(), facts))
            })
            .collect()
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
                Want::Render {
                    question,
                    instance,
                    span,
                    tail,
                    name,
                } => self.render_part(question, &instance, span, tail, name),
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

    /// Opens the file dialogs code and the export asked for: the system's, one at a time.
    pub(crate) fn open_file_dialogs(&mut self, tx: &mut Tx) {
        self.ask_where_to_export(tx);
        for want in std::mem::take(&mut self.waiting.files) {
            match want {
                Want::OpenFile { question, title } => {
                    let options = FileDialogOptions {
                        title,
                        ..FileDialogOptions::default()
                    };
                    tx.open_file(options, move |state: &mut State, chosen, _| match chosen {
                        Ok(Some(file)) => state.import(question, file, None),
                        Ok(None) => state.answer(question, &Answer::Asset(None)),
                        Err(error) => {
                            (state.session)
                                .report(format!("The file picker did not open: {error}"));
                            state.answer(question, &Answer::Asset(None));
                        }
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

    /// Brings `file` into `assets/audio/` on a thread; the answer goes once it is there.
    pub(super) fn import(
        &mut self,
        question: u64,
        file: PathBuf,
        scratch: Option<tempfile::TempDir>,
    ) {
        let waker = self.waker.clone();
        let wake = Arc::new(move || {
            if let Some(wake) = waker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                wake();
            }
        });
        let importing = runtime::input::import(self.project().assets(), file, wake);
        self.waiting.imports.push(Import {
            question,
            importing,
            _scratch: scratch,
        });
        self.handled();
    }

    /// Answers the imports that ended; a run without a window waits for them, as it does for Bun.
    pub(super) fn imported(&mut self) {
        let headless = self.headless();
        let mut ended = Vec::new();
        self.waiting.imports.retain(|import| {
            let mut brought = import.importing.poll();
            while headless && brought.is_none() {
                std::thread::sleep(Duration::from_millis(2));
                brought = import.importing.poll();
            }
            let Some(brought) = brought else {
                return true;
            };
            ended.push((import.question, brought));
            false
        });
        for (question, brought) in ended {
            let asset = match brought {
                Ok(Brought::Sound(asset)) => Some(asset),
                Ok(Brought::Parts(_)) => {
                    (self.session).report("That file holds notes: choose a sound file.");
                    None
                }
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
