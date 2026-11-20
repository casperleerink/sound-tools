//! What the code of the tools asks of the window (see `Want`), and the window's part of the clock.
//! The composer's choices no record holds live in [`Interface`] until the window owns a
//! `runtime::session::Session`, whose calls of the same names take its place.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{PoisonError, mpsc};

use plugin_host::{PluginRecord, Plugins};
use rune::{FileDialogOptions, FileDialogResult, Tx};
use runtime::session::{Loop, TransportState};
use serde_json::Value;
use sound_core::{InstanceId, Project, ProjectError, Ticks};
use sound_typescript::devices::{self, Offer, Takes};
use sound_typescript::next::{Answer, Clock, Shown, Tools, Transport, Want};

use super::State;

/// The composer's choices for how the piece plays and what they work on: never saved, no undo
/// step.
pub(crate) struct Interface {
    pub(crate) transport: TransportState,
    selected: Option<InstanceId>,
    span: Option<Range<Ticks>>,
}

impl Interface {
    /// The loop off over bars 1 to 4, as a session opens.
    pub(crate) fn new(project: &Project) -> Self {
        let signatures = project.project_file().tempo_map.time_signatures();
        let bar_five = signatures.bar(5).map_or(Ticks(0), |bar| bar.start);
        Self {
            transport: TransportState {
                looped: Loop {
                    range: Ticks(0)..bar_five,
                    on: false,
                },
                click: false,
                keys: false,
                automation: false,
            },
            selected: None,
            span: None,
        }
    }

    pub(crate) fn set_loop(&mut self, project: &mut Project, looped: Loop) {
        if self.transport.looped != looped {
            project
                .engine()
                .set_loop(looped.on.then(|| looped.range.clone()));
            self.transport.looped = looped;
        }
    }

    pub(crate) fn select(&mut self, instance: Option<InstanceId>, span: Option<Range<Ticks>>) {
        (self.selected, self.span) = (instance, span);
    }

    /// The selected instance, while it is in the project.
    pub(crate) fn selected(&self, project: &Project) -> Option<&InstanceId> {
        let selected = self.selected.as_ref()?;
        project.tool_of(selected).map(|_| selected)
    }

    /// The selected stretch; none when it is empty or nothing is selected.
    pub(crate) fn selected_span(&self, project: &Project) -> Option<Range<Ticks>> {
        self.selected(project)?;
        self.span.clone().filter(|span| span.start < span.end)
    }
}

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
        let children: Vec<InstanceId> = (self.project.instances())
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
                let tool = self.project.tool_of(id);
                tool.is_some_and(|tool| !tools.is_some_and(|tools| tools.has_view(tool)))
            })
            .filter_map(|id| Some((id.clone(), facts.get(id)?.clone())))
            .collect();
        if let Some(tools) = tools {
            self.keys.keymap.set_tool_groups(tools.shortcut_groups());
        }
        Shown {
            holds: self.holds(&instances),
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
        let tool = self.project.tool_of(id)?.to_string();
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
        devices::in_project(offers, &self.project)
    }

    /// Where the piece plays and how, as code is told it.
    pub(crate) fn clock(&self) -> Clock {
        let tempo_map = &self.project.project_file().tempo_map;
        let at = tempo_map.time_signatures().bar_beat_of(self.playhead);
        let TransportState {
            looped,
            click,
            keys,
            automation,
        } = &self.interface.transport;
        Clock {
            time: self.engine_time,
            transport: Transport {
                playing: self.playing,
                tick: self.playhead.0,
                bar: at.bar,
                beat: at.beat,
                bpm: tempo_map.change_at(self.playhead).bpm.bpm(),
                repeat: (looped.on).then_some([looped.range.start.0, looped.range.end.0]),
                click: *click,
                keys: *keys,
                automation: *automation,
            },
        }
    }

    /// Does what code asked since the last call. Whether what the window shows changed.
    pub(super) fn serve_wants(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        let wants = tools.take_wants();
        let changed = !wants.is_empty();
        for want in wants {
            let interface = &mut self.interface;
            match want {
                Want::Select { instance, span } => interface.select(instance, span),
                Want::Repeat(range) => {
                    let looped = match range {
                        Some(range) => Loop { range, on: true },
                        None => Loop {
                            on: false,
                            ..interface.transport.looped.clone()
                        },
                    };
                    interface.set_loop(&mut self.project, looped);
                }
                Want::Click(on) => interface.transport.click = on,
                Want::ComputerKeys(on) => interface.transport.keys = on,
                Want::WriteAutomation(on) => interface.transport.automation = on,
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
                    self.notice = Some(format!(
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
                    let root = self.project.root();
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
                            state.notice = Some(error);
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
                self.notice = Some(format!("The file picker did not open: {error}"));
                return self.answer(question, &Answer::Asset(None));
            }
        };
        let (send, imported) = mpsc::channel();
        let assets = self.project.assets().clone();
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
                self.notice = Some(error.to_string());
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
                    self.notice = Some(error);
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
