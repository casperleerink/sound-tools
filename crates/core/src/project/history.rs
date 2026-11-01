//! The history of the project folder (`sound-history`), and when a change of the files makes a
//! new point or joins the one before. Undo and redo move along it: they are going back to a
//! point, like any other.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::history::{HistoryError, ObjectId, Scope};

use super::GROUPING_WINDOW;
use super::instance::InstanceId;

/// Groups of outside file changes that follow each other within this time are one point,
/// when no request is open (see [`Project::begin_request`](super::Project::begin_request)).
///
/// An agent writes the files of one request seconds apart, far more than the quiet window
/// that groups them for live apply. Without this, undo of a track that an agent added takes
/// one step per file. It is a heuristic for an agent that cannot say where its request begins
/// and ends, such as one in a terminal.
pub const OUTSIDE_UNDO_WINDOW: Duration = Duration::from_secs(15);

/// A request of [`Project::begin_request`](super::Project::begin_request), until the watcher is
/// quiet after its end.
struct Request {
    label: String,
    /// When [`Project::end_request`](super::Project::end_request) was called.
    ended: Option<Instant>,
}

#[derive(Default)]
pub(super) struct History {
    /// `None` until [`Project::keep_history`](super::Project::keep_history).
    pub store: Option<crate::history::History>,
    /// When the point now is one of outside changes and nothing came after it: when its last
    /// group was heard. Any other point or move clears it.
    last_outside: Option<Instant>,
    request: Option<Request>,
    /// The project came to the point now by undo or redo, not by a click on a point: work
    /// left for a change here may count as undone, see
    /// [`note_undone`](crate::history::History::note_undone).
    by_undo: bool,
    /// How many open edits have published over each record.
    holders: BTreeMap<InstanceId, usize>,
}

impl History {
    pub(super) fn open(
        root: &std::path::Path,
        skip: fn(&str) -> bool,
    ) -> Result<Self, HistoryError> {
        let scope = Scope {
            roots: ["project.json", "state", "extensions"]
                .map(PathBuf::from)
                .to_vec(),
            skip,
        };
        Ok(Self {
            store: Some(crate::history::History::open(root, scope)?),
            ..Self::default()
        })
    }

    /// Saves the folder as a point named `label`, when it changed.
    pub(super) fn record(&mut self, label: &str) -> Result<(), HistoryError> {
        self.last_outside = None;
        self.record_new(label).map(drop)
    }

    /// Records a new point after the one now, and notes what an undo before it left.
    fn record_new(&mut self, label: &str) -> Result<Option<ObjectId>, HistoryError> {
        let Some(store) = &mut self.store else {
            return Ok(None);
        };
        let left = (store.redo_target()).filter(|_| self.by_undo);
        let point = store.record(label)?;
        if point.is_some() {
            self.by_undo = false;
            if let Some(left) = left {
                store.note_undone(left)?;
            }
        }
        Ok(point)
    }

    /// Saves outside changes heard at `at`. They join the point now when that is one of outside
    /// changes with nothing after it, of the open request or from less than
    /// [`OUTSIDE_UNDO_WINDOW`] before.
    pub(super) fn record_outside(&mut self, label: &str, at: Instant) -> Result<(), HistoryError> {
        let quiet_since_end = (self.request.as_ref())
            .and_then(|request| request.ended)
            .is_some_and(|ended| at.saturating_duration_since(ended) >= GROUPING_WINDOW);
        if quiet_since_end {
            self.request = None;
            self.last_outside = None;
        }
        let joins = self.last_outside.is_some_and(|last| {
            self.request.is_some() || at.saturating_duration_since(last) < OUTSIDE_UNDO_WINDOW
        });
        let label =
            (self.request.as_ref()).map_or(label.to_string(), |request| request.label.clone());
        let label = label.as_str();
        let before = self.store.as_ref().and_then(|store| store.now());
        let point = match (joins, &mut self.store) {
            (_, None) => return Ok(()),
            (true, Some(store)) => store.amend(label)?,
            (false, Some(_)) => self.record_new(label)?,
        };
        let now = self.store.as_ref().and_then(|store| store.now());
        // An agent that takes back what it just wrote leaves nothing to undo.
        self.last_outside = match (point, joins) {
            (Some(_), _) => Some(at),
            (None, true) if now != before => None,
            (None, _) => self.last_outside,
        };
        Ok(())
    }

    /// The project moved to another point, such as by undo: outside changes from here make
    /// a new point.
    pub(super) fn moved(&mut self, by_undo: bool) {
        self.last_outside = None;
        self.by_undo = by_undo;
    }

    pub(super) fn begin_request(&mut self, label: &str) {
        self.request = Some(Request {
            label: label.to_string(),
            ended: None,
        });
        // The request starts its own point, even right after other outside changes.
        self.last_outside = None;
    }

    pub(super) fn end_request_at(&mut self, at: Instant) {
        if let Some(request) = &mut self.request {
            request.ended.get_or_insert(at);
        }
    }

    /// The label of the point that `target` gives.
    pub(super) fn label(
        &self,
        target: impl Fn(&crate::history::History) -> Option<ObjectId>,
    ) -> Option<&str> {
        let store = self.store.as_ref()?;
        Some(store.point(target(store)?)?.label.as_str())
    }

    pub(super) fn hold(&mut self, id: &InstanceId) {
        *self.holders.entry(id.clone()).or_default() += 1;
    }

    pub(super) fn release(&mut self, id: &InstanceId) {
        if let Some(count) = self.holders.get_mut(id) {
            *count -= 1;
            if *count == 0 {
                self.holders.remove(id);
            }
        }
    }

    /// How many open edits hold the record `id`.
    pub(super) fn holders(&self, id: &InstanceId) -> usize {
        self.holders.get(id).copied().unwrap_or(0)
    }
}
