//! The tools of `extensions/` while the window is open: a save of a file defines the tools
//! again and draws every card again, the pickers offer the tools, and the cards play their
//! live controls and triggers and follow their watches.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Context, Entity, Task, WeakEntity};
use sound_core::{InstanceId, Problem, Project};
use sound_hum::{Hum, HumUpdate};
use sound_ui::{DeviceLabel, DeviceOffer, Devices, OfferGroup, Session, Views};

use crate::bun::{Bun, Event, Loaded, Looped, Request};
use crate::card::TypeScriptCard;
use crate::tools::{Control, ToolInfo, ToolKind};
use crate::tree::Node;
use crate::{Extensions, FOLDER};

/// How often the cards look at their watches and the control loops run: often enough for a
/// step light, a meter or a moving drawing.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

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
    /// When the control loops last ran.
    last_frame: Instant,
    _tasks: [Task<()>; 2],
}

struct Card {
    id: InstanceId,
    /// The name of its tool, which a card keeps for its life.
    tool: String,
    /// Drawn as a page, the whole window, and not as a card.
    page: bool,
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
    let Some((bun, loaded)) = extensions.running else {
        return;
    };
    let tools = Rc::new(RefCell::new(loaded.tools));
    let generation = Rc::new(Cell::new(0));
    let live = cx.new(|cx: &mut Context<Live>| {
        let events = bun.events();
        let hearing = cx.spawn(async move |live, cx| {
            while let Ok(event) = events.recv().await {
                if live.update(cx, |live, cx| live.heard(event, cx)).is_err() {
                    return;
                }
            }
            // Bun is gone: what plays was built and plays on, but nothing new can be built.
            live.update(cx, |live, cx| live.stopped(cx)).ok();
        });
        let framing = cx.spawn(async move |live, cx| {
            loop {
                cx.background_executor().timer(FRAME_INTERVAL).await;
                if live.update(cx, |live, cx| live.frame(cx)).is_err() {
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
            last_frame: Instant::now(),
            _tasks: [hearing, framing],
        }
    });

    views.set_other_cards({
        let (tools, live) = (tools.clone(), live.clone());
        move |session, id, frame, _, cx| {
            let tool = session.read(cx).project().tool_of(id)?;
            if !tools.borrow().iter().any(|info| info.name == tool) {
                return None;
            }
            let (live, session, id) = (live.clone(), session.clone(), id.clone());
            let card = cx.new(|cx| TypeScriptCard::new(live, session, id, Some(frame), cx));
            Some(card.into())
        }
    });
    views.set_other_views(
        {
            let tools = tools.clone();
            move |tool| {
                tools
                    .borrow()
                    .iter()
                    .any(|info| info.name == tool && info.page)
            }
        },
        {
            let tools = tools.clone();
            move |session, id, _, cx| {
                let tool = session.read(cx).project().tool_of(id)?;
                if !tools
                    .borrow()
                    .iter()
                    .any(|info| info.name == tool && info.page)
                {
                    return None;
                }
                let (live, session, id) = (live.clone(), session.clone(), id.clone());
                let page = cx.new(|cx| TypeScriptCard::new(live, session, id, None, cx));
                Some(page.into())
            }
        },
    );
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

/// The `state` of the record of `id`.
pub(crate) fn state_of(project: &Project, id: &InstanceId) -> Option<serde_json::Value> {
    serde_json::from_str(&project.state_json(id)?).ok()
}

/// What a picker offers of the tools of `kind`: each one's record at its defaults.
fn offers(tools: &[ToolInfo], kind: impl Fn(ToolKind) -> bool) -> Vec<DeviceOffer> {
    let tools = tools.iter().filter(|info| kind(info.kind));
    tools
        .map(|info| {
            let tool = info.name.clone();
            let write = move |project: &Project, slot: &InstanceId, changes: &mut _| {
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

    /// The tool of a card, as it last loaded.
    pub(crate) fn info(&self, card: u64) -> Option<ToolInfo> {
        let tool = &self.cards.get(&card)?.tool;
        let tools = self.tools.borrow();
        tools.iter().find(|info| info.name == *tool).cloned()
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
    pub(crate) fn add(
        &mut self,
        id: InstanceId,
        tool: String,
        page: bool,
        cx: &mut Context<Self>,
    ) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        let entry = Card {
            id,
            tool,
            page,
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
        let (Some(tool), Some(state)) = (project.tool_of(&entry.id), state_of(project, &entry.id))
        else {
            return;
        };
        entry.asked = true;
        self.bun.send(&Request::Render {
            card,
            instance: entry.id.as_str(),
            tool,
            state,
            watches: &entry.watches,
            page: entry.page,
        });
    }

    /// A click on an element of the card, or a press or a drag on a canvas at `x` and `y`
    /// across and down, 0 to 1.
    pub(crate) fn event(&self, card: u64, handler: usize, at: Option<(f32, f32)>) {
        let (x, y) = (at.map(|at| at.0), at.map(|at| at.1));
        self.bun.send(&Request::Event {
            card,
            handler,
            x,
            y,
        });
    }

    /// Moves the live control `name` of instance `id` to `value`, or fires the trigger `name`
    /// when `value` is `None`. Not an edit: nothing is saved.
    pub(crate) fn control(
        &mut self,
        id: &InstanceId,
        name: &str,
        value: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let tools = self.tools.borrow();
        let tool = session.read(cx).project().tool_of(id);
        let Some(info) = tools.iter().find(|info| Some(info.name.as_str()) == tool) else {
            return;
        };
        let update = match (info.control(name), value) {
            (Some((index, Control::Live { .. })), Some(value)) => HumUpdate::Live { index, value },
            (Some((index, Control::Trigger { .. })), None) => HumUpdate::Trigger { index },
            _ => return eprintln!("error: {} has no control {name} that takes that", info.name),
        };
        let processor = info.processor();
        drop(tools);
        if let Some(value) = value {
            self.played
                .entry(id.clone())
                .or_default()
                .insert(name.to_string(), value);
            // A pad or a knob of the card shows where it is, also while nothing else redraws.
            cx.notify();
        }
        session.update(cx, |session, cx| {
            let sent = session.background(cx, |project| project.send::<Hum>(id, processor, update));
            if let Err(error) = sent {
                session.report(error, cx);
            }
        });
    }

    fn stopped(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let message = "The tools of extensions/ stopped running: what plays goes on as it is, but cards, control loops and new sounds wait until the project opens again";
        session.update(cx, |session, cx| session.report(message, cx));
    }

    /// One frame of the window: the control loops run, and a card whose watches moved draws
    /// again.
    fn frame(&mut self, cx: &mut Context<Self>) {
        self.run_loops(cx);
        self.look_at_watches(cx);
    }

    /// Runs the control loop of every instance whose tool has one, cards or not.
    fn run_loops(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();
        let tools = self.tools.borrow();
        let project = session.read(cx).project();
        let looped: Vec<Looped> = (project.instances())
            .filter(|(_, tool)| tools.iter().any(|info| info.name == *tool && info.tick))
            .map(|(id, tool)| Looped {
                instance: id.as_str(),
                tool,
                state: state_of(project, id).unwrap_or_default(),
                watches: (project.watches(id).into_iter())
                    .map(|(name, watch)| (name, watch.get()))
                    .collect(),
            })
            .collect();
        if !looped.is_empty() {
            self.bun.send(&Request::Frame {
                dt,
                instances: looped,
            });
        }
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
            Event::Control {
                instance,
                name,
                value,
            } => match InstanceId::new(&instance) {
                Ok(id) => self.control(&id, &name, value, cx),
                Err(error) => eprintln!("error: {error}"),
            },
        }
        cx.notify();
    }

    /// A file of `extensions/` was saved: every tool is defined again, and every card draws
    /// again.
    fn loaded(&mut self, mut loaded: Loaded, cx: &mut Context<Self>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        // Every tool, changed or not: its file was saved, so its `sound` may make other Hum.
        let mut problems = crate::define(&self.bun, &mut loaded, |tool| {
            session
                .update(cx, |session, cx| {
                    session.background(cx, |project| project.define_json_tool(tool))
                })
                .map_err(|error| error.to_string())
        });
        let gone: Vec<ToolInfo> = (self.tools.borrow().iter())
            .filter(|info| !loaded.tools.iter().any(|tool| tool.name == info.name))
            .cloned()
            .collect();
        for info in &gone {
            problems.push(Problem {
                path: format!("{FOLDER}/{}", info.file),
                message: format!(
                    "the tool {} is gone from the code; its records play as they did until the project opens again",
                    info.name
                ),
            });
        }
        session.update(cx, |session, cx| {
            session.background(cx, |project| {
                project.set_problems_in(&format!("{FOLDER}/"), problems);
            });
        });
        *self.tools.borrow_mut() = gone.into_iter().chain(loaded.tools).collect();
        self.generation.set(self.generation.get() + 1);
        let cards: Vec<u64> = self.cards.keys().copied().collect();
        for card in cards {
            self.render(card, cx);
        }
    }
}
