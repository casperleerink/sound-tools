//! The one state application: how a group of changes is staged, derived, bound to the engine
//! and rolled back when any part of it fails. Interface edits, file changes, loading, undo,
//! redo and cancel all end here.

use std::collections::BTreeSet;

use super::binding::EngineChange;
use super::editing::{self, Applied, Change, Derived, RecordChange};
use super::instance::InstanceId;
use super::registry::DerivedFrom;
use super::storage::{self, RecordOnDisk};
use super::{Project, ProjectError, ProjectEvent, Source};
use crate::clock::TimeSignatures;

impl Project {
    /// The one state application. Interface edits, file changes, loading, undo, redo and
    /// cancel all come through here. The group applies whole, as one engine batch, or not at
    /// all. It writes nothing: the caller knows whether files need writing.
    pub(crate) fn apply(
        &mut self,
        changes: Vec<Change>,
        source: Source,
    ) -> Result<Applied, ProjectError> {
        if self.read_only && source != Source::Load {
            return Err(ProjectError::ReadOnly);
        }
        // A group that brings `project.json` itself is the one that makes it load again, so a
        // derive in it is not writing into a file the runtime has lost track of.
        let project_file_arrives = changes
            .iter()
            .any(|change| matches!(change, Change::ProjectFile(_)));
        let project_file_before = self.project_file.clone();
        let derive_problems_before = self.derive_problems.clone();
        let problems_before = self.bindings.connection_problems().to_vec();
        let live_input_problems_before = self.live_input_problems();
        // What behaviours said last time, so that `problems.txt` and the views follow a
        // behaviour that starts or stops reporting. Empty in a project with nothing to report.
        let instance_problems_before = self.instance_problems();
        let mut records = Vec::new();
        let mut derived = Vec::new();
        let result = self
            .stage(changes, source, &mut records)
            .and_then(|()| {
                self.stage_derived(
                    source,
                    project_file_before.tempo_map.time_signatures(),
                    project_file_arrives,
                    &mut records,
                    &mut derived,
                )
            })
            .and_then(|()| self.bind(&records));
        if let Err(error) = result {
            for change in records.into_iter().rev() {
                match change.before {
                    Some(before) => self.instances.insert(change.id, before),
                    None => self.instances.remove(&change.id),
                };
            }
            self.project_file = project_file_before;
            self.derive_problems = derive_problems_before;
            return Err(error);
        }

        for change in &records {
            let id = change.id.clone();
            self.events.push(match (&change.before, &change.after) {
                (None, _) => ProjectEvent::Created(id),
                (_, None) => ProjectEvent::Deleted(id),
                _ => ProjectEvent::Changed(id),
            });
        }
        for change in &mut records {
            self.history.note_committed(change, source);
        }
        let mut project_file = (project_file_before != self.project_file)
            .then(|| (project_file_before, self.project_file.clone()));
        self.history
            .note_committed_project_file(&mut project_file, source);
        if project_file.is_some() {
            self.push_event(ProjectEvent::ProjectFileChanged);
        }
        if problems_before != self.bindings.connection_problems()
            || live_input_problems_before != self.live_input_problems()
            || instance_problems_before != self.instance_problems()
        {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
        Ok(Applied {
            records,
            project_file,
            derived,
        })
    }

    /// Runs the derives this group asks for and stages what they give, as part of the same
    /// group. See [`ToolRegistration::derive`](super::ToolRegistration::derive).
    ///
    /// A derive runs when a record of its tool changed in this group, or when the project's
    /// time signatures changed. What a derive gives is staged and nothing more: it starts no
    /// second round, so this cannot loop. It does not run while the project loads, nor for
    /// undo, redo or a cancel, because the files and the undo step already hold what it would
    /// compute. So a read-only project never derives and never writes.
    fn stage_derived(
        &mut self,
        source: Source,
        time_signatures_before: &TimeSignatures,
        project_file_arrives: bool,
        records: &mut Vec<RecordChange>,
        derived: &mut Vec<InstanceId>,
    ) -> Result<(), ProjectError> {
        if !matches!(source, Source::Interface | Source::Outside) {
            return Ok(());
        }
        let derives = |project: &Self, id: &InstanceId| {
            let tool = project.instances.get(id)?.tool;
            project.registry.definition(tool)?.derive.as_ref()?;
            Some(())
        };
        let signature_changed =
            self.project_file.tempo_map.time_signatures() != time_signatures_before;
        let mut ids: BTreeSet<InstanceId> = BTreeSet::new();
        if signature_changed {
            let live = self.instances.keys();
            ids.extend(live.filter(|id| derives(self, id).is_some()).cloned());
        }
        let changed = records.iter().map(|change| &change.id);
        ids.extend(changed.filter(|id| derives(self, id).is_some()).cloned());
        let Some(first) = ids.first().cloned() else {
            return Ok(());
        };
        // A derive writes its record and the tempo map together, and `project.json` is not
        // written while it holds an outside change that did not load. Applying the record
        // alone would leave a project that plays one thing and opens as another, because a
        // derive does not run on load. So the whole group waits for that file.
        if !project_file_arrives && !self.project_file_on_disk_is_known() {
            return Err(ProjectError::DerivesIntoAStaleProjectFile {
                id: first,
                path: storage::PROJECT_FILE.to_string(),
            });
        }

        let mut changes = Vec::new();
        let mut reported = Vec::new();
        for id in ids {
            // Both borrows are shared: the definition and the project the derive reads.
            let Some(tool) = self.instances.get(&id).map(|record| record.tool) else {
                continue;
            };
            let derive = self
                .registry
                .definition(tool)
                .and_then(|it| it.derive.as_ref());
            let Some(derive) = derive else { continue };
            // What the record was, so a derive can write only what really moved.
            let from = match records.iter().find(|change| change.id == id) {
                None => DerivedFrom::Unchanged,
                Some(change) => match &change.before {
                    None => DerivedFrom::Created,
                    Some(record) => DerivedFrom::Changed(record),
                },
            };
            let mut result = Derived::default();
            derive(self, &id, from, &mut result);
            changes.extend(result.changes.changes);
            reported.push((id, result.problems));
        }
        for (id, problems) in reported {
            match problems.is_empty() {
                true => self.derive_problems.remove(&id),
                false => self.derive_problems.insert(id, problems),
            };
        }
        // What a derive said about an instance that is gone, or that is now a record of
        // another tool, would otherwise be shown on that record's path for ever.
        let stale: Vec<InstanceId> = self
            .derive_problems
            .keys()
            .filter(|id| derives(self, id).is_none())
            .cloned()
            .collect();
        for id in stale {
            self.derive_problems.remove(&id);
        }
        editing::validate(&changes)?;
        let before = records.len();
        self.stage(changes, source, records)?;
        derived.extend(records[before..].iter().map(|change| change.id.clone()));
        Ok(())
    }

    /// Changes `instances` and `project_file`, and notes every record change so that a
    /// failure can be rolled back.
    fn stage(
        &mut self,
        changes: Vec<Change>,
        source: Source,
        records: &mut Vec<RecordChange>,
    ) -> Result<(), ProjectError> {
        for change in changes {
            match change {
                Change::Set(id, record) => {
                    self.check_tool(record.tool)?;
                    let owner = id.parent().and_then(|parent| self.instances.get(&parent));
                    let owner = owner.map(|owner| owner.tool);
                    if let Some(message) = record.place.refuses(record.tool, &id, owner)
                        && id.parent().is_none_or(|_| owner.is_some())
                    {
                        return Err(ProjectError::WrongPlace { id, message });
                    }
                    if !self.instances.contains_key(&id) {
                        if let Some(parent) = id.parent() {
                            let Some(parent) = self.instances.get(&parent) else {
                                return Err(ProjectError::MissingParent(id));
                            };
                            if !parent.owns_children {
                                return Err(ProjectError::ParentOwnsNoChildren {
                                    id,
                                    parent_tool: parent.tool,
                                });
                            }
                        }
                        // A file the runtime did not load may sit at this id: a record of an
                        // unknown tool, or one with an error. Undo must not write over it
                        // either. A file the runtime knows is its own, from before a delete
                        // that is not written yet.
                        let writes = matches!(source, Source::Interface | Source::History);
                        if writes
                            && self.storage.observed(&id).is_none()
                            && !matches!(
                                self.storage.read_record(&id, None)?,
                                RecordOnDisk::Missing
                            )
                        {
                            return Err(ProjectError::IdTaken(id));
                        }
                    }
                    // The id now belongs to a tool that owns nothing: what it owned goes.
                    if !record.owns_children {
                        self.stage_delete_inside(&id, records);
                    }
                    let before = self.instances.insert(id.clone(), record.clone());
                    let unchanged = before.as_ref().is_some_and(|before| before.equals(&record));
                    if !unchanged {
                        records.push(RecordChange {
                            id,
                            before,
                            after: Some(record),
                        });
                    }
                }
                Change::Delete(id) => {
                    self.stage_delete_inside(&id, records);
                    if let Some(before) = self.instances.remove(&id) {
                        records.push(RecordChange {
                            id: id.clone(),
                            before: Some(before),
                            after: None,
                        });
                    }
                    self.project_file
                        .connections
                        .retain(|connection| !connection.touches(&id));
                }
                Change::ProjectFile(project_file) => self.project_file = project_file,
                Change::TempoMap(tempo_map) => self.project_file.tempo_map = tempo_map,
                Change::Connect(connection) => {
                    if !self.project_file.connections.contains(&connection) {
                        self.project_file.connections.push(connection);
                    }
                }
                Change::Disconnect(connection) => self
                    .project_file
                    .connections
                    .retain(|saved| *saved != connection),
            }
        }
        Ok(())
    }

    /// Removes everything `id` owns, children before parents, and the saved connections that
    /// name them.
    fn stage_delete_inside(&mut self, id: &InstanceId, records: &mut Vec<RecordChange>) {
        let inside = id.inside(&self.instances);
        let inside: Vec<InstanceId> = inside.map(|(id, _)| id.clone()).collect();
        for id in inside.into_iter().rev() {
            self.project_file
                .connections
                .retain(|connection| !connection.touches(&id));
            if let Some(before) = self.instances.remove(&id) {
                records.push(RecordChange {
                    id,
                    before: Some(before),
                    after: None,
                });
            }
        }
    }

    /// Whether the records of this tool decide state of their own, see
    /// [`ToolRegistration::derive`](super::ToolRegistration::derive).
    pub(crate) fn derives(&self, tool: &'static str) -> bool {
        let definition = self.registry.definition(tool);
        definition.is_some_and(|definition| definition.derive.is_some())
    }

    fn check_tool(&self, tool: &'static str) -> Result<(), ProjectError> {
        let enabled = self
            .registry
            .definition(tool)
            .is_some_and(|definition| definition.is_enabled_in(&self.project_file.extensions));
        if enabled {
            Ok(())
        } else {
            Err(ProjectError::UnknownTool(tool))
        }
    }

    /// Runs the behaviours the change concerns: each changed instance and all its owners.
    pub(super) fn bind(&mut self, records: &[RecordChange]) -> Result<(), ProjectError> {
        let mut dirty = BTreeSet::new();
        let mut unbound = Vec::new();
        for change in records {
            let tool_changed = match (&change.before, &change.after) {
                (Some(before), Some(after)) => before.tool != after.tool,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if tool_changed {
                unbound.push(change.id.clone());
            }
            dirty.extend(change.id.ancestors());
            dirty.insert(change.id.clone());
        }
        let change = EngineChange {
            registry: &self.registry,
            instances: &self.instances,
            unbound: &unbound,
            dirty,
            connections: &self.project_file.connections,
            tempo_map: &self.project_file.tempo_map,
        };
        Ok(self
            .bindings
            .apply(&mut self.engine, &self.assets, change)?)
    }
}
