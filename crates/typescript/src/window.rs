//! The tools of `extensions/` while the window is open: a save of a file defines the tools
//! again and draws every card again, the pickers offer the tools, and the cards play their
//! live controls and triggers and follow their watches.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Task, WeakEntity};
use sound_core::{InstanceId, Problem};
use sound_hum::{Hum, HumUpdate};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Session, Views};

use crate::bun::{Bun, Event, Loaded, Request};
use crate::card::TypeScriptCard;
use crate::tools::{Control, ToolInfo, ToolKind};
use crate::tree::Node;
use crate::{Extensions, FOLDER, problems_of};

/// How often the cards look at their watches: often enough for a step light or a meter.
const WATCH_INTERVAL: Duration = Duration::from_millis(33);

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    bun: Arc<Bun>,
    session: WeakEntity<Session>,
    /// The tools of the last load, shared with the pickers.
    tools: Rc<RefCell<Vec<ToolInfo>>>,
    /// Goes up when the tools do, so the pickers fill themselves again.
    generation: Rc<Cell<u64>>,
    cards: HashMap<u64, Card>,
    next_card: u64,
    /// Where each live control was put last, by instance: the processor keeps it on the audio
    /// side, and a control on the card shows it.
    played: HashMap<InstanceId, BTreeMap<String, f32>>,
    _tasks: [Task<()>; 2],
}

struct Card {
    id: InstanceId,
    /// The name of its tool, which a card keeps for its life.
    tool: String,
    /// The last tree, or why there is none. `None` until the first one arrives.
    tree: Option<Result<Rc<Node>, String>>,
    /// The watches the last tree was drawn with, which native elements read too.
    watches: BTreeMap<String, f32>,
    /// A render was asked for and its tree has not come yet.
    asked: bool,
    /// The record or a watch changed while a render was out, so one more is due when it comes.
    stale: bool,
}

/// Starts the live part of `extensions` for the window of `session`: the cards of its tools in
/// `views`, and its tools in the pickers of `devices`. Nothing when the project has no running
/// tools.
pub fn start_window(
    extensions: Extensions,
    session: &Entity<Session>,
    views: &mut Views,
    devices: &mut Devices,
    cx: &mut App,
) {
    let (Some(bun), Some(loaded)) = (extensions.bun, extensions.loaded) else {
        return;
    };
    let tools = Rc::new(RefCell::new(loaded.tools));
    let generation = Rc::new(Cell::new(0));
    let live = cx.new(|cx: &mut Context<Live>| {
        let events = bun.events();
        let hearing = cx.spawn(async move |live, cx| {
            while let Ok(event) = events.recv().await {
                if live.update(cx, |live, cx| live.heard(event, cx)).is_err() {
                    break;
                }
            }
        });
        let watching = cx.spawn(async move |live, cx| {
            loop {
                cx.background_executor().timer(WATCH_INTERVAL).await;
                if live
                    .update(cx, |live, cx| live.look_at_watches(cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Live {
            bun: bun.clone(),
            session: session.downgrade(),
            tools: tools.clone(),
            generation: generation.clone(),
            cards: HashMap::new(),
            next_card: 0,
            played: HashMap::new(),
            _tasks: [hearing, watching],
        }
    });

    views.set_other_cards({
        let tools = tools.clone();
        move |session, id, frame, _, cx| {
            let tool = session.read(cx).project().tool_of(id)?;
            if !tools.borrow().iter().any(|info| info.name == tool) {
                return None;
            }
            let (live, session, id) = (live.clone(), session.clone(), id.clone());
            let card = cx.new(|cx| TypeScriptCard::new(live, session, id, frame, cx));
            Some(card.into())
        }
    });
    devices.effects({
        let tools = tools.clone();
        move || offers(&tools.borrow(), |kind| kind == ToolKind::Effect)
    });
    devices.instruments({
        let tools = tools.clone();
        move || offers(&tools.borrow(), |kind| kind != ToolKind::Effect)
    });
    devices.offers_change(move || generation.get());
    devices.describe_others(move |project, id| {
        let tool = project.tool_of(id)?;
        let tools = tools.borrow();
        let info = tools.iter().find(|info| info.name == tool)?;
        Some(DeviceLabel {
            key: key(&info.name).into(),
            name: info.title.clone().into(),
        })
    });
}

fn key(tool: &str) -> String {
    format!("{FOLDER}/{tool}")
}

/// What a picker offers of the tools of `kind`: each one's record at its defaults.
fn offers(tools: &[ToolInfo], kind: impl Fn(ToolKind) -> bool) -> Vec<DeviceOffer> {
    let tools = tools.iter().filter(|info| kind(info.kind));
    tools
        .map(|info| {
            let tool = info.name.clone();
            let write = move |project: &sound_core::Project, slot: &InstanceId, changes: &mut _| {
                project.set_json(changes, slot.clone(), &tool, serde_json::json!({}))
            };
            DeviceOffer::new(
                key(&info.name),
                info.title.clone(),
                OfferGroup::Project,
                write,
            )
        })
        .collect()
}

impl Live {
    /// The last tree of a card. `None` until it first drew.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<Rc<Node>, String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// The watches a card last drew with.
    pub(crate) fn watches(&self, card: u64) -> Option<&BTreeMap<String, f32>> {
        self.cards.get(&card).map(|card| &card.watches)
    }

    /// Where the live control `name` of the card's instance is, and its range: where it was
    /// put last, or its default.
    pub(crate) fn live_value(&self, card: u64, name: &str) -> Option<(f32, f32, f32)> {
        let entry = self.cards.get(&card)?;
        let tools = self.tools.borrow();
        let info = tools.iter().find(|info| info.name == entry.tool)?;
        let Some((
            _,
            Control::Live {
                min, max, default, ..
            },
        )) = info.control(name)
        else {
            return None;
        };
        let played = self
            .played
            .get(&entry.id)
            .and_then(|played| played.get(name));
        Some((played.copied().unwrap_or(*default), *min, *max))
    }

    /// A new card of the instance, which draws at once.
    pub(crate) fn add(&mut self, id: InstanceId, tool: String, cx: &mut Context<Self>) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        let entry = Card {
            id,
            tool,
            tree: None,
            watches: BTreeMap::new(),
            asked: false,
            stale: false,
        };
        self.cards.insert(card, entry);
        self.render(card, cx);
        card
    }

    pub(crate) fn remove(&mut self, card: u64) {
        self.cards.remove(&card);
        self.bun.send(&Request::Drop { card });
    }

    /// Asks for the tree of the card again, from the record and the watches as they are now.
    /// While one is out, one more is asked for when it comes, so a drag that changes the record
    /// sixty times a second keeps one request in flight and not sixty.
    pub(crate) fn render(&mut self, card: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        if entry.asked {
            entry.stale = true;
            return;
        }
        let project = session.read(cx).project();
        let Some(tool) = project.tool_of(&entry.id) else {
            return;
        };
        let state = project
            .state_json(&entry.id)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        entry.asked = true;
        let watches = &entry.watches;
        self.bun.send(&Request::Render {
            card,
            tool,
            state,
            watches,
        });
    }

    pub(crate) fn click(&mut self, card: u64, handler: usize) {
        self.bun.send(&Request::Event { card, handler });
    }

    /// Moves the live control `name` of the card's instance to `value`, or fires the trigger
    /// `name` when `value` is `None`. Not an edit: nothing is saved.
    pub(crate) fn control(&mut self, card: u64, name: &str, value: Option<f32>, cx: &mut App) {
        let (Some(entry), Some(session)) = (self.cards.get(&card), self.session.upgrade()) else {
            return;
        };
        let id = entry.id.clone();
        let tools = self.tools.borrow();
        let Some(info) = tools.iter().find(|info| info.name == entry.tool) else {
            return;
        };
        let update = match (info.control(name), value) {
            (Some((index, Control::Live { .. })), Some(value)) => HumUpdate::Live { index, value },
            (Some((index, Control::Trigger { .. })), None) => HumUpdate::Trigger { index },
            _ => return eprintln!("error: {} has no control {name} that takes that", info.name),
        };
        let processor = info.processor();
        if let (HumUpdate::Live { .. }, Some(value)) = (&update, value) {
            self.played
                .entry(id.clone())
                .or_default()
                .insert(name.to_string(), value);
        }
        drop(tools);
        session.update(cx, |session, cx| {
            let sent =
                session.background(cx, |project| project.send::<Hum>(&id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    /// Reads the watches of every card; a card whose watches moved draws again.
    fn look_at_watches(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let mut moved = Vec::new();
        for (card, entry) in &mut self.cards {
            let watches = session.read(cx).project().watches(&entry.id);
            let watches: BTreeMap<String, f32> = watches
                .into_iter()
                .map(|(name, watch)| (name, watch.get()))
                .collect();
            if watches != entry.watches {
                entry.watches = watches;
                moved.push(*card);
            }
        }
        for card in &moved {
            self.render(*card, cx);
        }
        if !moved.is_empty() {
            cx.notify();
        }
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Loaded(loaded) => self.loaded(loaded, cx),
            Event::Tree { card, tree, error } => {
                let Some(entry) = self.cards.get_mut(&card) else {
                    return;
                };
                entry.asked = false;
                entry.tree = Some(match (tree, error) {
                    (Some(tree), _) => serde_json::from_value(tree)
                        .map(Rc::new)
                        .map_err(|error| error.to_string()),
                    (None, error) => Err(error.unwrap_or_default()),
                });
                if std::mem::take(&mut entry.stale) {
                    self.render(card, cx);
                }
            }
            Event::Edit { card, label, state } => {
                let (Some(entry), Some(session)) = (self.cards.get(&card), self.session.upgrade())
                else {
                    return;
                };
                let id = entry.id.clone();
                session.update(cx, |session, cx| {
                    session.edit(cx, |project| {
                        let mut edit = project.begin(&label);
                        project.update_json(&mut edit, &id, |old| *old = state)?;
                        project.finish(edit)
                    })
                });
            }
            Event::Control { card, name, value } => self.control(card, &name, value, cx),
        }
        cx.notify();
    }

    /// A file of `extensions/` was saved: every tool is defined again, and every card draws
    /// again.
    fn loaded(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let started = Instant::now();
        let before = self.tools.borrow().clone();
        let mut problems = problems_of(&self.bun, &loaded);
        for info in &before {
            if !loaded.tools.iter().any(|tool| tool.name == info.name) {
                problems.push(Problem {
                    path: format!("{FOLDER}/{}", info.file),
                    message: format!(
                        "the tool {} is gone from the code; its records play as they did until the project opens again",
                        info.name
                    ),
                });
            }
        }
        // Every tool, changed or not: its file was saved, so its `sound` may make other Hum.
        for info in &loaded.tools {
            let tool = info.json_tool(&self.bun);
            session.update(cx, |session, cx| {
                session.edit(cx, |project| project.define_json_tool(tool));
            });
        }
        session.update(cx, |session, cx| {
            session.background(cx, |project| {
                project.set_problems_in(&format!("{FOLDER}/"), problems);
            });
        });
        let kept: Vec<ToolInfo> = (before.into_iter())
            .filter(|info| !loaded.tools.iter().any(|tool| tool.name == info.name))
            .chain(loaded.tools.iter().cloned())
            .collect();
        *self.tools.borrow_mut() = kept;
        self.generation.set(self.generation.get() + 1);
        let cards: Vec<u64> = self.cards.keys().copied().collect();
        for card in cards {
            self.render(card, cx);
        }
        if std::env::var_os("SOUND_TOOLS_TIMING").is_some() {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
            let at = now.map_or(0, |now| now.as_millis());
            eprintln!(
                "timing: runtime defined the tools again in {:?}, done at {at}",
                started.elapsed()
            );
        }
    }
}
