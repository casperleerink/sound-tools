//! The tools and cards of `extensions/` while the window is open: a save of a file defines the
//! tools again and draws every card again, and the effect picker offers the tools.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::{App, AppContext, Context, Entity, Task, WeakEntity};
use sound_core::{InstanceId, Problem};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Session, Views};

use crate::bun::{Bun, Event, Loaded, Request};
use crate::card::TypeScriptCard;
use crate::tools::ToolInfo;
use crate::tree::Node;
use crate::{Extensions, FOLDER, problems_of};

/// The tools and cards of the project in the window. One per window.
pub(crate) struct Live {
    bun: Arc<Bun>,
    session: WeakEntity<Session>,
    /// The tools of the last load, shared with the effect picker.
    tools: Rc<RefCell<Vec<ToolInfo>>>,
    /// Goes up when the tools do, so the picker fills itself again.
    generation: Rc<Cell<u64>>,
    /// Every tool that has a card in `extensions/`.
    cards_of: BTreeSet<String>,
    cards: HashMap<u64, Card>,
    next_card: u64,
    _events: Task<()>,
}

struct Card {
    id: InstanceId,
    /// The last tree, or why there is none. `None` until the first one arrives.
    tree: Option<Result<Rc<Node>, String>>,
    /// A render was asked for and its tree has not come yet.
    asked: bool,
    /// The record changed while a render was out, so one more is due when it comes.
    stale: bool,
}

/// Starts the live part of `extensions` for the window of `session`: a card host that stands
/// in for every card of `views`, and the tools in the effect picker of `devices`. Nothing when
/// the project has no running tools.
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
    let tools = Rc::new(RefCell::new(loaded.tools.clone()));
    let generation = Rc::new(Cell::new(0));
    let live = cx.new(|cx: &mut Context<Live>| {
        let events = bun.events();
        let task = cx.spawn(async move |live, cx| {
            while let Ok(event) = events.recv().await {
                if live.update(cx, |live, cx| live.heard(event, cx)).is_err() {
                    break;
                }
            }
        });
        Live {
            bun: bun.clone(),
            session: session.downgrade(),
            tools: tools.clone(),
            generation: generation.clone(),
            cards_of: loaded.cards.clone(),
            cards: HashMap::new(),
            next_card: 0,
            _events: task,
        }
    });

    views.set_card_host(move |session, id, frame, built_in, _, cx| {
        let (live, session, id) = (live.clone(), session.clone(), id.clone());
        let card = cx.new(|cx| TypeScriptCard::new(live, session, id, frame, built_in, cx));
        Some(card.into())
    });
    devices.effects({
        let tools = tools.clone();
        move || tools.borrow().iter().map(offer).collect()
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

/// What the effect picker offers for a tool: its record at its defaults.
fn offer(info: &ToolInfo) -> DeviceOffer {
    let tool = info.name.clone();
    DeviceOffer::new(
        key(&tool),
        info.title.clone(),
        OfferGroup::Project,
        move |project, slot, changes| {
            project.set_json(changes, slot.clone(), &tool, serde_json::json!({}))
        },
    )
}

impl Live {
    pub(crate) fn has_card(&self, tool: &str) -> bool {
        self.cards_of.contains(tool)
    }

    /// The last tree of a card. `None` until it first drew.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<Rc<Node>, String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// A new card of the instance, which draws at once.
    pub(crate) fn add(&mut self, id: InstanceId, cx: &mut Context<Self>) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        let entry = Card {
            id,
            tree: None,
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

    /// Asks for the tree of the card again, from the record as it is now. While one is out,
    /// one more is asked for when it comes, so a drag that changes the record sixty times a
    /// second keeps one request in flight and not sixty.
    pub(crate) fn render(&mut self, card: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Some(entry) = self.cards.get_mut(&card) else {
            return;
        };
        let project = session.read(cx).project();
        let Some(tool) = project.tool_of(&entry.id) else {
            return;
        };
        if !self.cards_of.contains(tool) {
            return;
        }
        if entry.asked {
            entry.stale = true;
            return;
        }
        let state = project
            .state_json(&entry.id)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        entry.asked = true;
        self.bun.send(&Request::Render { card, tool, state });
    }

    pub(crate) fn click(&mut self, card: u64, handler: usize) {
        self.bun.send(&Request::Event { card, handler });
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
        }
        cx.notify();
    }

    /// A file of `extensions/` was saved: every tool whose definition changed is defined
    /// again, and every card draws again.
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
            session.edit(cx, |project| {
                project.set_problems_in(&format!("{FOLDER}/"), problems);
                Ok(())
            });
        });
        let kept: Vec<ToolInfo> = (before.into_iter())
            .filter(|info| !loaded.tools.iter().any(|tool| tool.name == info.name))
            .chain(loaded.tools.iter().cloned())
            .collect();
        *self.tools.borrow_mut() = kept;
        self.generation.set(self.generation.get() + 1);
        self.cards_of = loaded.cards;
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
