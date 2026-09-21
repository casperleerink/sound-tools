//! Instance ids, typed instance handles and the saved state of a tool.

use std::any::Any;
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Bound;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The file name, without `.json`, of the record of an instance that is a folder.
pub(crate) const FOLDER_RECORD: &str = "instance";

/// Where instances of a tool may live in the ownership tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Anywhere,
    /// Only at the top of `state/`.
    Root,
    /// Only as a direct child of an instance of the tool with this name.
    In(&'static str),
    /// Only at the top of `state/` and only under this exact name, so a project has at most
    /// one instance of the tool. For a tool that decides something about the whole project,
    /// such as the tempo fit: two of them would write over each other's results with no way to
    /// say which won. A record anywhere else is not loaded and the problem names the one path.
    Only(&'static str),
}

impl Place {
    /// Why an instance of `tool` may not live under `owner`, the tool of its direct owner.
    /// The text tells an agent where the record belongs.
    pub(crate) fn refuses(
        self,
        tool: &str,
        id: &InstanceId,
        owner: Option<&str>,
    ) -> Option<String> {
        match (self, owner) {
            (Self::Root, Some(_)) => Some(format!(
                "an instance of {tool:?} belongs at the top of state/, not inside another instance"
            )),
            (Self::Only(name), _) if id.as_str() != name => Some(format!(
                "a project has one instance of {tool:?} and it lives at state/{name}.json, not at state/{id}.json"
            )),
            (Self::In(wanted), Some(owner)) if owner != wanted => Some(format!(
                "an instance of {tool:?} belongs directly inside an instance of {wanted:?}, and its owner here is a {owner:?}"
            )),
            (Self::In(wanted), None) => Some(format!(
                "an instance of {tool:?} belongs directly inside an instance of {wanted:?}, not at the top of state/"
            )),
            _ => None,
        }
    }
}

/// The saved state of one tool. The type is the handle of the tool: every typed call names it.
///
/// Derive `Serialize`, `Deserialize`, `Clone` and `PartialEq`. Add `#[serde(deny_unknown_fields)]`
/// so a misspelled field in an outside edit is an error and not silently ignored.
pub trait State: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static {
    /// The stable tool name that records carry, for example `"arrangement.clip"`.
    const TOOL: &'static str;

    /// Whether instances of this tool can own child instances. It decides the place of the
    /// record for good: `<name>/instance.json` with the children next to it when true,
    /// `<name>.json` when false.
    const OWNS_CHILDREN: bool = false;

    /// Where instances may live. A record somewhere else is not loaded and is reported, so
    /// a clip that an agent wrote outside a track is an error it sees, not silence.
    const PLACE: Place = Place::Anywhere;

    /// Rules that the types alone do not express, such as ranges. It runs on every path into
    /// the project: files, interface edits, undo. The message should name the field.
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid instance id {id:?}: names use lowercase letters, digits, `-` and `_`, are joined by `/`, and `{FOLDER_RECORD}` is reserved"
)]
pub struct InvalidInstanceId {
    pub id: String,
}

/// The stable id of an instance: its path under `state/`, without `.json`, with `/` between
/// names. `arrangement/piano` is a child of `arrangement`. The id never changes; display names
/// live in the record.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InstanceId(String);

impl InstanceId {
    pub fn new(id: &str) -> Result<Self, InvalidInstanceId> {
        if id.split('/').all(is_valid_name) {
            Ok(Self(id.to_string()))
        } else {
            Err(InvalidInstanceId { id: id.to_string() })
        }
    }

    /// The id of an owned child of this instance.
    pub fn child(&self, name: &str) -> Result<Self, InvalidInstanceId> {
        Self::new(&format!("{}/{name}", self.0))
    }

    /// `id-2`, `id-3` and so on: the same id with a number, for a name that is taken.
    pub(crate) fn numbered(id: &InstanceId, number: u32) -> Self {
        // Still a valid id: digits and `-` are valid in a name.
        Self(format!("{}-{number}", id.0))
    }

    /// The owner. `None` for a root instance.
    pub fn parent(&self) -> Option<Self> {
        let (parent, _) = self.0.rsplit_once('/')?;
        Some(Self(parent.to_string()))
    }

    /// The last name of the path.
    pub fn name(&self) -> &str {
        self.0.rsplit_once('/').map_or(&self.0, |(_, name)| name)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this instance is below `ancestor` in the ownership tree, at any depth.
    pub fn is_inside(&self, ancestor: &InstanceId) -> bool {
        self.0
            .strip_prefix(ancestor.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    }

    /// Every owner from the parent up to the root.
    pub fn ancestors(&self) -> impl Iterator<Item = InstanceId> {
        std::iter::successors(self.parent(), InstanceId::parent)
    }

    pub(crate) fn depth(&self) -> usize {
        self.0.matches('/').count()
    }

    /// Everything in `map` that is inside this instance, at any depth, parents first.
    pub(crate) fn inside<'a, V>(
        &self,
        map: &'a BTreeMap<InstanceId, V>,
    ) -> impl Iterator<Item = (&'a InstanceId, &'a V)> + use<'a, V> {
        // Ids inside this one start with `id/`. `0` is the character after `/`.
        let (start, end) = (format!("{}/", self.0), format!("{}0", self.0));
        map.range::<str, _>((
            Bound::Included(start.as_str()),
            Bound::Excluded(end.as_str()),
        ))
    }

    /// The direct children of this instance in `map`, in name order. It walks on from child to
    /// child and jumps over what a child owns, so the cost follows the number of children, not
    /// the size of the subtree, and a child without children of its own costs one step.
    pub(crate) fn children_in<'a, V>(
        &self,
        map: &'a BTreeMap<InstanceId, V>,
    ) -> impl Iterator<Item = (&'a InstanceId, &'a V)> + use<'a, V> {
        let depth = self.depth() + 1;
        // Ids inside this one start with `id/`. `0` is the character after `/`.
        let end = format!("{}0", self.0);
        // A range does not keep its bounds, so the closure below can own `end`.
        let from = move |start: &str| {
            map.range::<str, _>((Bound::Included(start), Bound::Excluded(end.as_str())))
        };
        let mut inside = from(&format!("{}/", self.0));
        std::iter::from_fn(move || {
            loop {
                let (id, value) = inside.next()?;
                if id.depth() == depth {
                    return Some((id, value));
                }
                // Inside some child `c`. Siblings such as `c-2` sort before `c/`, so they are
                // done already. Everything inside `c` sorts before `c0`.
                let child: Vec<&str> = id.0.split('/').take(depth + 1).collect();
                inside = from(&format!("{}0", child.join("/")));
            }
        })
    }
}

/// The one rule for a name the runtime writes into the project folder: an instance name, and
/// the name of an agent doc.
pub(crate) fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != FOLDER_RECORD
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

impl TryFrom<String> for InstanceId {
    type Error = InvalidInstanceId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(&id)
    }
}

impl From<InstanceId> for String {
    fn from(id: InstanceId) -> Self {
        id.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Lets an ordered map of ids be searched by string bounds, for subtree ranges.
impl Borrow<str> for InstanceId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// An instance id that was checked to hold state of type `S`. It owns no state: read the
/// current state with `Project::state`, which gives `None` once the instance is gone.
pub struct Instance<S> {
    id: InstanceId,
    state: PhantomData<fn() -> S>,
}

impl<S> Instance<S> {
    pub(crate) fn new(id: InstanceId) -> Self {
        Self {
            id,
            state: PhantomData,
        }
    }

    pub fn id(&self) -> &InstanceId {
        &self.id
    }
}

impl<S> Clone for Instance<S> {
    fn clone(&self) -> Self {
        Self::new(self.id.clone())
    }
}

impl<S> fmt::Debug for Instance<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Instance({})", self.id)
    }
}

/// A [`State`] without its type, so one map can hold every tool.
pub(crate) trait ErasedState: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn equals(&self, other: &dyn ErasedState) -> bool;
    fn validate(&self) -> Result<(), String>;
    /// Compact JSON. Not through `serde_json::Value`, which would widen an `f32` of 0.2 to
    /// 0.20000000298023224.
    fn to_json(&self) -> Result<String, serde_json::Error>;
}

impl<S: State> ErasedState for S {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn ErasedState) -> bool {
        other.as_any().downcast_ref::<S>() == Some(self)
    }

    fn validate(&self) -> Result<(), String> {
        State::validate(self)
    }

    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// The live form of one record: the tool name and the typed state. Cloning shares the state,
/// so undo history costs no copies.
#[derive(Clone)]
pub(crate) struct Record {
    pub tool: &'static str,
    pub owns_children: bool,
    pub place: Place,
    pub state: Arc<dyn ErasedState>,
}

impl Record {
    pub fn new<S: State>(state: S) -> Self {
        Self {
            tool: S::TOOL,
            owns_children: S::OWNS_CHILDREN,
            place: S::PLACE,
            state: Arc::new(state),
        }
    }

    pub fn state<S: State>(&self) -> Option<&S> {
        self.state.as_any().downcast_ref()
    }

    pub fn equals(&self, other: &Record) -> bool {
        self.tool == other.tool
            && (Arc::ptr_eq(&self.state, &other.state) || self.state.equals(other.state.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_paths_of_valid_names() {
        let owner = InstanceId::new("band/piano").unwrap();
        assert_eq!(owner.name(), "piano");
        assert_eq!(owner.parent(), Some(InstanceId::new("band").unwrap()));
        assert_eq!(owner.parent().unwrap().parent(), None);
        let owned = owner.child("verse-a").unwrap();
        assert!(owned.is_inside(&owner));
        assert!(owned.is_inside(&owner.parent().unwrap()));
        assert!(!owner.is_inside(&owner));
        assert_eq!(owned.depth(), 2);

        for invalid in [
            "",
            "a//b",
            "/a",
            "a/",
            "Piano",
            "a.json",
            "a b",
            "a/instance",
            "..",
        ] {
            assert!(InstanceId::new(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_name_that_continues_another_is_not_inside_it() {
        let short = InstanceId::new("tone").unwrap();
        assert!(!InstanceId::new("tone-2").unwrap().is_inside(&short));
        let ids = ["tone", "tone-2", "tone/a", "tone/a/b", "tone0", "tonf"];
        let map: BTreeMap<InstanceId, ()> = ids
            .iter()
            .map(|id| (InstanceId::new(id).unwrap(), ()))
            .collect();
        let inside: Vec<&str> = short.inside(&map).map(|(id, ())| id.as_str()).collect();
        assert_eq!(inside, ["tone/a", "tone/a/b"]);
    }

    #[test]
    fn children_are_found_without_walking_what_they_own() {
        let ids = [
            "a",
            "a-2",
            "a/c",
            "a/c-2",
            "a/c-2/x",
            "a/c/x",
            "a/c/x/y",
            "a/c/z",
            "a/c0",
            "a/d",
            "a/orphaned/deep",
            "a0",
            "b",
        ];
        let map: BTreeMap<InstanceId, ()> = ids
            .iter()
            .map(|id| (InstanceId::new(id).unwrap(), ()))
            .collect();
        let children = |id: &str| -> Vec<&str> {
            let id = InstanceId::new(id).unwrap();
            id.children_in(&map).map(|(id, ())| id.as_str()).collect()
        };
        assert_eq!(children("a"), ["a/c", "a/c-2", "a/c0", "a/d"]);
        assert_eq!(children("a/c"), ["a/c/x", "a/c/z"]);
        assert_eq!(children("b"), Vec::<&str>::new());
    }

    #[test]
    fn ids_load_validated_from_json() {
        let id: InstanceId = serde_json::from_str("\"a/b\"").unwrap();
        assert_eq!(id.as_str(), "a/b");
        assert!(serde_json::from_str::<InstanceId>("\"a/../b\"").is_err());
    }
}
