//! The behaviours of the tools of the project that own children: how they connect, which its
//! code in Bun works out from its record and its children.
//!
//! Nothing here waits for Bun. The declarations of an instance are kept by a key of what they
//! were worked out from. A record or a child that changes plays the last declaration at once
//! and asks Bun, one question at a time per instance; when Bun answers, [`take_declared`] names
//! the instance, so its behaviour runs again with the record it has and finds the answer. A
//! render waits for every answer first ([`wait_for_behaviours`]), so it plays what the records
//! say from its first block.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use serde::Deserialize;
use sound_core::{Assets, InstanceId};

use crate::bun::{Bun, Request};

/// The declarations kept per instance, the newest ones: enough for an undo and a redo back and
/// forth, and for a few records an agent writes in turn.
const KEPT: usize = 16;

/// What the behaviour of an owner declares: how its children connect.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Declaration {
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    pub from: End,
    pub to: End,
}

/// An end of a connection: a port of the child `child`, or of the owner itself without one,
/// under the key of the kind of what it carries. `{ child }` alone is the child's `audio`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct End {
    pub child: Option<String>,
    pub port: Option<String>,
    pub notes: Option<String>,
    pub number: Option<String>,
    pub bang: Option<String>,
}

/// The key an [`End`] names its port under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Port,
    Notes,
    Number,
    Bang,
}

impl End {
    fn keys(&self) -> [(Key, &Option<String>); 4] {
        [
            (Key::Port, &self.port),
            (Key::Notes, &self.notes),
            (Key::Number, &self.number),
            (Key::Bang, &self.bang),
        ]
    }

    /// The key it names its port under and the name, or `None` when it names no port or more
    /// than one.
    pub(crate) fn port(&self) -> Option<(Key, &str)> {
        let mut named =
            (self.keys().into_iter()).filter_map(|(key, name)| Some((key, name.as_deref()?)));
        match (named.next(), named.next(), &self.child) {
            (Some(port), None, _) => Some(port),
            // `audio`, the name of a sound input and output alike.
            (None, None, Some(_)) => Some((Key::Port, sound_notes::AUDIO_OUTPUT)),
            _ => None,
        }
    }
}

/// As the behaviour wrote it, such as `{ child: "lfo", number: "rate" }`.
impl fmt::Display for End {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let child = self.child.as_ref().map(|child| ("child", child));
        let keys =
            (self.keys().into_iter()).filter_map(|(key, name)| Some((key.word(), name.as_ref()?)));
        let fields: Vec<String> = (child.into_iter().chain(keys))
            .map(|(key, name)| format!("{key}: {name:?}"))
            .collect();
        write!(formatter, "{{ {} }}", fields.join(", "))
    }
}

impl Key {
    fn word(self) -> &'static str {
        match self {
            Key::Port => "port",
            Key::Notes => "notes",
            Key::Number => "number",
            Key::Bang => "bang",
        }
    }
}

/// What Bun answered: the declaration, or why there is none, as a sentence.
pub(crate) type Answer = Result<Declaration, String>;

/// What a behaviour read when it answered: the fields of its own state, and the fields of any
/// child's state; `None` when it read them all. The names and tools of the children always
/// count. Its answer holds for any record and children that agree on these, as a behaviour is a
/// pure function of what it reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reads {
    pub state: Option<Vec<String>>,
    pub children: Option<Vec<String>>,
}

/// What an instance plays, and what is wrong when it is not what its record says.
pub(crate) struct Declared {
    pub declaration: Declaration,
    pub problem: Option<String>,
}

/// The behaviours of every project of the process. One lock, so a thread that hears Bun and the
/// project agree on what is asked.
static STORE: (Mutex<Store>, Condvar) = (Mutex::new(Store::new()), Condvar::new());

struct Store {
    owners: BTreeMap<(Assets, InstanceId), Owner>,
    /// What is asked and not answered, by the number of the question.
    asked: BTreeMap<u64, Asked>,
    next_question: u64,
    /// The instances that got an answer since [`take_declared`] last looked.
    ready: BTreeSet<(Assets, InstanceId)>,
    /// The Buns that stopped, by their number, with why.
    gone: BTreeMap<u64, String>,
}

impl Store {
    const fn new() -> Self {
        Self {
            owners: BTreeMap::new(),
            asked: BTreeMap::new(),
            next_question: 0,
            ready: BTreeSet::new(),
            gone: BTreeMap::new(),
        }
    }
}

struct Owner {
    /// The opening of the project it was declared in. What another one declared plays nothing
    /// here.
    opening: u64,
    /// The number of the Bun the answers came from: another one runs other code.
    bun: u64,
    /// By key, the oldest first.
    known: VecDeque<(u64, Answer)>,
    /// What it played last that Bun worked out.
    last: Declaration,
    /// The key it asked for and has no answer for yet.
    asking: Option<u64>,
    /// The key of its record and children as they are now.
    wanted: u64,
    /// What its last answer read, which the next keys are made of.
    reads: Option<Reads>,
}

struct Asked {
    owner: (Assets, InstanceId),
    key: u64,
    bun: u64,
}

fn store() -> MutexGuard<'static, Store> {
    STORE.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the last answer of the behaviour of `instance` read, if it has one.
pub(crate) fn reads_of(assets: &Assets, instance: &InstanceId) -> Option<Reads> {
    let store = store();
    let owner = store.owners.get(&(assets.clone(), instance.clone()))?;
    owner.reads.clone()
}

/// What the behaviour of `instance` of `tool` declares for its record and its children: the
/// answer of `bun` when it has one, else the last declaration while `bun` works it out. `key`
/// is of all of them, and `read` of what its last answer read (see [`reads_of`]), which an
/// answer is kept under: a change of a field it does not read asks nothing. Before it asks,
/// `tell` tells `bun` those records. Never waits.
pub(crate) fn declared(
    bun: &Bun,
    (assets, instance): (&Assets, &InstanceId),
    tool: &str,
    (key, read): (u64, Option<u64>),
    tell: impl FnOnce() -> bool,
) -> Declared {
    let mut store = store();
    let Store {
        owners,
        asked,
        next_question,
        gone,
        ..
    } = &mut *store;
    let owner_key = (assets.clone(), instance.clone());
    let new = || Owner {
        opening: bun.opening(),
        bun: bun.number(),
        known: VecDeque::new(),
        last: Declaration::default(),
        asking: None,
        wanted: key,
        reads: None,
    };
    let owner = owners.entry(owner_key.clone()).or_insert_with(new);
    if owner.opening != bun.opening() {
        *owner = new();
    } else if owner.bun != bun.number() {
        owner.bun = bun.number();
        owner.known.clear();
        owner.asking = None;
    }
    let last = |problem: String| Declared {
        declaration: owner.last.clone(),
        problem: Some(problem),
    };
    owner.wanted = key;
    // An answer asked before it was known what it reads is kept under the key of all: it holds
    // under the key of what it read too.
    if let Some(read) = read
        && !owner.known.iter().any(|(known, _)| *known == read)
        && let Some((_, answer)) = owner.known.iter().find(|(known, _)| *known == key)
    {
        let answer = answer.clone();
        owner.known.push_back((read, answer));
        if owner.known.len() > KEPT {
            owner.known.pop_front();
        }
    }
    let key = read.unwrap_or(key);
    owner.wanted = key;
    match owner.known.iter().find(|(known, _)| *known == key) {
        Some((_, Ok(declaration))) => {
            owner.last = declaration.clone();
            return Declared {
                declaration: declaration.clone(),
                problem: None,
            };
        }
        Some((_, Err(problem))) => return last(problem.clone()),
        None => {}
    }
    if let Some(why) = gone.get(&bun.number()) {
        return last(format!(
            "its behaviour did not run: the TypeScript host {why}"
        ));
    }
    if owner.asking.is_none() {
        let question = *next_question;
        let request = Request::Behaviour {
            question,
            tool,
            instance,
        };
        if !tell() || !bun.send(&request) {
            return last("its behaviour did not run: the TypeScript host does not read".into());
        }
        *next_question += 1;
        owner.asking = Some(key);
        let bun = bun.number();
        let owner = owner_key;
        asked.insert(question, Asked { owner, key, bun });
    }
    Declared {
        declaration: owner.last.clone(),
        problem: None,
    }
}

/// Bun answered `question`, having read `reads`.
pub(crate) fn answered(question: u64, answer: Answer, reads: Option<Reads>) {
    let mut store = store();
    answer_in(&mut store, question, answer, reads);
    STORE.1.notify_all();
}

fn answer_in(store: &mut Store, question: u64, answer: Answer, reads: Option<Reads>) {
    let Some(Asked { owner, key, bun }) = store.asked.remove(&question) else {
        return;
    };
    if let Some(kept) = store.owners.get_mut(&owner)
        && kept.bun == bun
    {
        // It plays this already: no need to run its behaviour again.
        let playing =
            kept.wanted == key && matches!(&answer, Ok(declared) if *declared == kept.last);
        if reads.is_some() {
            kept.reads = reads;
        }
        kept.known.retain(|(known, _)| *known != key);
        kept.known.push_back((key, answer));
        if kept.known.len() > KEPT {
            kept.known.pop_front();
        }
        if kept.asking == Some(key) {
            kept.asking = None;
        }
        if playing {
            return;
        }
    }
    store.ready.insert(owner);
}

/// Bun `number` hung or stopped, `why`: what it was asked has no answer.
pub(crate) fn ended(number: u64, why: &str) {
    let mut store = store();
    store.gone.insert(number, why.to_string());
    let questions: Vec<u64> = (store.asked.iter())
        .filter(|(_, asked)| asked.bun == number)
        .map(|(question, _)| *question)
        .collect();
    for question in questions {
        let problem = format!("its behaviour did not run: the TypeScript host {why}");
        answer_in(&mut store, question, Err(problem), None);
    }
    STORE.1.notify_all();
}

/// Lets go of what is kept for the owners of `project` that are gone.
pub(crate) fn forget_gone(project: &sound_core::Project) {
    let assets = project.assets();
    (store().owners).retain(|(of, id), _| of != assets || project.tool_of(id).is_some());
}

pub fn take_declared(assets: &Assets) -> Vec<InstanceId> {
    let mut store = store();
    let (ours, others): (BTreeSet<_>, BTreeSet<_>) = std::mem::take(&mut store.ready)
        .into_iter()
        .partition(|(project, _)| project == assets);
    store.ready = others;
    ours.into_iter().map(|(_, instance)| instance).collect()
}

/// How many questions the behaviours of this process asked Bun, for tests and measurements.
pub fn asked_behaviours() -> u64 {
    store().next_question
}

/// Waits until Bun answered everything it was asked, or stopped. For a render, which plays what
/// the records say from its first block, and for tests.
pub fn wait_for_behaviours() {
    let (lock, done) = &STORE;
    let mut store = lock.lock().unwrap_or_else(PoisonError::into_inner);
    while !store.asked.is_empty() {
        store = done.wait(store).unwrap_or_else(PoisonError::into_inner);
    }
}
