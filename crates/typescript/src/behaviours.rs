//! The behaviours of the tools of the project that own children: what each one declares about
//! its children, which its code in Bun works out from its record and its children.
//!
//! Nothing here waits for Bun. The declarations of an instance are kept by a key of what they
//! were worked out from. A record or a child that changes plays the last declaration at once
//! and asks Bun, one question at a time per instance; when Bun answers, [`take_declared`] names
//! the instance, so its behaviour runs again with the record it has and finds the answer. A
//! render waits for every answer first ([`wait_for_behaviours`]), so it plays what the records
//! say from its first block.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use serde::Deserialize;
use serde_json::Value;
use sound_core::{Assets, InstanceId};
use sound_notes::Note;

use crate::bun::{Bun, Request};

/// The declarations kept per instance, the newest ones: enough for an undo and a redo back and
/// forth, and for a few records an agent writes in turn.
const KEPT: usize = 16;

/// What the behaviour of an owner declares: how its children connect, and its note players.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Declaration {
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// The note players of the owner, by name.
    #[serde(default)]
    pub players: BTreeMap<String, Player>,
}

/// A note player of an owner: a `NotePlayer` processor with the patterns a launch picks from.
#[derive(Clone, Debug, PartialEq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Player {
    pub patterns: Vec<PlayerPattern>,
    /// The pattern it plays before any launch, by index.
    #[serde(default)]
    pub plays: Option<usize>,
}

/// Notes that loop: `length` in ticks, and note starts counted from the start of the pattern.
#[derive(Clone, Debug, PartialEq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlayerPattern {
    pub length: u64,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    pub from: FromPort,
    pub to: ToPort,
}

/// Where a connection starts: a child, or a player of the owner.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(untagged)]
pub(crate) enum FromPort {
    Child(ChildPort),
    Player(PlayerPort),
}

/// The `notes` output of a player of the owner, by its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlayerPort {
    pub player: String,
}

/// Where a connection goes: a child, or the owner's own sound.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToPort {
    Own(Own),
    Child(ChildPort),
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Own {
    Sound,
}

/// A port of a child, by the child's name and the port's.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChildPort {
    pub child: String,
    /// `audio` when left out, and `notes` for the input a player connects to.
    pub port: Option<String>,
}

/// What Bun answered: the declaration, or why there is none, as a sentence.
pub(crate) type Answer = Result<Declaration, String>;

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
}

struct Asked {
    owner: (Assets, InstanceId),
    key: u64,
    bun: u64,
}

fn store() -> MutexGuard<'static, Store> {
    STORE.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the behaviour of `instance` of `tool` declares for its `state` and `children`, which
/// hash to `key`: the answer of `bun` when it has one, else the last declaration while `bun`
/// works it out. Never waits.
pub(crate) fn declared(
    bun: &Bun,
    (assets, instance): (&Assets, &InstanceId),
    tool: &str,
    key: u64,
    (state, children): (&Value, &Value),
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
            state,
            children,
        };
        if !bun.send(&request) {
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

/// Bun answered `question`.
pub(crate) fn answered(question: u64, answer: Answer) {
    let mut store = store();
    answer_in(&mut store, question, answer);
    STORE.1.notify_all();
}

fn answer_in(store: &mut Store, question: u64, answer: Answer) {
    let Some(Asked { owner, key, bun }) = store.asked.remove(&question) else {
        return;
    };
    if let Some(kept) = store.owners.get_mut(&owner)
        && kept.bun == bun
    {
        kept.known.retain(|(known, _)| *known != key);
        kept.known.push_back((key, answer));
        if kept.known.len() > KEPT {
            kept.known.pop_front();
        }
        if kept.asking == Some(key) {
            kept.asking = None;
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
        answer_in(&mut store, question, Err(problem));
    }
    STORE.1.notify_all();
}

/// The instances of the project of `assets` whose behaviour Bun answered since the last call,
/// to run again: they find the answer.
pub fn take_declared(assets: &Assets) -> Vec<InstanceId> {
    let mut store = store();
    let (ours, others): (BTreeSet<_>, BTreeSet<_>) = std::mem::take(&mut store.ready)
        .into_iter()
        .partition(|(project, _)| project == assets);
    store.ready = others;
    ours.into_iter().map(|(_, instance)| instance).collect()
}

/// The players `instance` of the project of `assets` plays now, by name, with how many patterns
/// each has: for a launch, which never waits for Bun.
pub(crate) fn players(assets: &Assets, instance: &InstanceId) -> BTreeMap<String, usize> {
    let store = store();
    let owner = store.owners.get(&(assets.clone(), instance.clone()));
    let players = owner.into_iter().flat_map(|owner| &owner.last.players);
    (players.map(|(name, player)| (name.clone(), player.patterns.len()))).collect()
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
