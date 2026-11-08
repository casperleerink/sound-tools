//! The one state application: how a group of changes is staged, derived, bound to the engine
//! and rolled back when any part of it fails. Interface edits, file changes, loading, undo,
//! redo and cancel all end here.

use std::collections::BTreeSet;

use super::binding::EngineChange;
use super::editing::{Applied, Change, RecordChange};
use super::instance::InstanceId;
use super::storage::RecordOnDisk;
use super::{Project, ProjectError, ProjectEvent, Source};

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
        let project_file_before = self.project_file.clone();
        let problems_before = self.bindings.connection_problems().to_vec();
        let live_input_problems_before = self.live_input_problems();
        // What behaviours said last time, so that `problems.txt` and the views follow a
        // behaviour that starts or stops reporting. Empty in a project with nothing to report.
        let instance_problems_before = self.instance_problems();
        let mut records = Vec::new();
        let result = self
            .stage(changes, source, &mut records)
            .and_then(|()| self.bind(&records));
        if let Err(error) = result {
            for change in records.into_iter().rev() {
                match change.before {
                    Some(before) => self.instances.insert(change.id, before),
                    None => self.instances.remove(&change.id),
                };
            }
            self.project_file = project_file_before;
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
            self.open_edits.note_committed(change, source);
        }
        let mut project_file = (project_file_before != self.project_file)
            .then(|| (project_file_before, self.project_file.clone()));
        self.open_edits
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
        })
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
                        // unknown tool, or one with an error. A file the runtime knows is its
                        // own, from before a delete that is not written yet.
                        let taken = source == Source::Interface
                            && self.storage.observed(&id).is_none()
                            && !matches!(
                                self.storage.read_record(&id, None)?,
                                RecordOnDisk::Missing
                            );
                        if taken {
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
                Change::Sections(sections) => self.project_file.sections = sections,
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
