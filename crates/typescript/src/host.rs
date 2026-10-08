//! The Bun process that runs the cards of the project, and what it said last: which tools have
//! a card, and the tree of every card on screen. One JSON message per line, both ways; the
//! other side is `host.ts`.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;

use gpui::{App, AppContext, Context, Entity, Task, WeakEntity};
use serde::{Deserialize, Serialize};
use smol::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use sound_core::InstanceId;
use sound_ui::Session;

use crate::tree::Node;

/// What the runtime asks.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request<'a> {
    Render {
        card: u64,
        tool: &'a str,
        state: serde_json::Value,
    },
    Event {
        card: u64,
        handler: usize,
    },
    Drop {
        card: u64,
    },
}

/// What the host says.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Answer {
    /// The files of `ui/` loaded again.
    Loaded {
        tools: BTreeSet<String>,
        errors: Vec<String>,
    },
    /// A card drew, or failed to.
    Tree {
        card: u64,
        #[serde(default)]
        tree: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
    },
    /// A click changed the record of a card.
    Edit {
        card: u64,
        label: String,
        state: serde_json::Value,
    },
}

struct Card {
    id: InstanceId,
    /// The last tree, or why there is none. `None` until the first one arrives.
    tree: Option<Result<Rc<Node>, String>>,
}

pub(crate) struct Host {
    session: WeakEntity<Session>,
    requests: smol::channel::Sender<String>,
    /// The tools the files of `ui/` give a card.
    tools: BTreeSet<String>,
    cards: HashMap<u64, Card>,
    next_card: u64,
    _tasks: [Task<()>; 2],
}

impl Host {
    /// Starts `bun` on `host_script` in the folder `ui`.
    pub(crate) fn start(
        session: &Entity<Session>,
        bun: &Path,
        host_script: &Path,
        ui: &Path,
        cx: &mut App,
    ) -> std::io::Result<Entity<Self>> {
        let mut child = smol::process::Command::new(bun)
            .arg(host_script)
            .current_dir(ui)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(std::io::Error::other("bun started without pipes"));
        };
        let (requests, waiting) = smol::channel::unbounded::<String>();
        Ok(cx.new(|cx: &mut Context<Self>| {
            let writing = cx.background_spawn(async move {
                // The child goes with the task, and with it the process, when the host goes.
                let _child = child;
                while let Ok(line) = waiting.recv().await {
                    if let Err(error) = stdin.write_all(line.as_bytes()).await {
                        eprintln!("error: the TypeScript host stopped: {error}");
                        break;
                    }
                }
            });
            let reading = cx.spawn(async move |host, cx| {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = smol::stream::StreamExt::next(&mut lines).await {
                    let answer = match line.map(|line| serde_json::from_str::<Answer>(&line)) {
                        Ok(Ok(answer)) => answer,
                        Ok(Err(error)) => {
                            eprintln!("error: the TypeScript host said something unknown: {error}");
                            continue;
                        }
                        Err(error) => {
                            eprintln!("error: the TypeScript host stopped: {error}");
                            break;
                        }
                    };
                    if host
                        .update(cx, |host, cx| host.answered(answer, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self {
                session: session.downgrade(),
                requests,
                tools: BTreeSet::new(),
                cards: HashMap::new(),
                next_card: 0,
                _tasks: [writing, reading],
            }
        }))
    }

    /// Whether a file of `ui/` gives `tool` a card.
    pub(crate) fn has_card(&self, tool: &str) -> bool {
        self.tools.contains(tool)
    }

    /// The last tree of a card. `None` until it first drew.
    pub(crate) fn tree(&self, card: u64) -> Option<&Result<Rc<Node>, String>> {
        self.cards.get(&card)?.tree.as_ref()
    }

    /// A new card of the instance, which draws at once.
    pub(crate) fn add(&mut self, id: InstanceId, cx: &mut Context<Self>) -> u64 {
        let card = self.next_card;
        self.next_card += 1;
        self.cards.insert(card, Card { id, tree: None });
        self.render(card, cx);
        card
    }

    pub(crate) fn remove(&mut self, card: u64) {
        self.cards.remove(&card);
        self.send(&Request::Drop { card });
    }

    /// Asks for the tree of the card again, from the record as it is now.
    pub(crate) fn render(&mut self, card: u64, cx: &mut Context<Self>) {
        let (Some(entry), Some(session)) = (self.cards.get(&card), self.session.upgrade()) else {
            return;
        };
        let project = session.read(cx).project();
        let Some(tool) = project.tool_of(&entry.id) else {
            return;
        };
        if !self.tools.contains(tool) {
            return;
        }
        let state = project
            .state_json(&entry.id)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        self.send(&Request::Render { card, tool, state });
    }

    pub(crate) fn click(&mut self, card: u64, handler: usize) {
        self.send(&Request::Event { card, handler });
    }

    fn send(&self, request: &Request) {
        let line = match serde_json::to_string(request) {
            Ok(json) => json + "\n",
            Err(error) => return eprintln!("error: {error}"),
        };
        // Closed only when the host stopped, which was reported then.
        self.requests.try_send(line).ok();
    }

    fn answered(&mut self, answer: Answer, cx: &mut Context<Self>) {
        match answer {
            Answer::Loaded { tools, errors } => {
                self.tools = tools;
                if let Some(session) = self.session.upgrade() {
                    for error in errors {
                        session.update(cx, |session, cx| session.report(&error, cx));
                    }
                }
                let cards: Vec<u64> = self.cards.keys().copied().collect();
                for card in cards {
                    self.render(card, cx);
                }
            }
            Answer::Tree { card, tree, error } => {
                let Some(entry) = self.cards.get_mut(&card) else {
                    return;
                };
                entry.tree = Some(match (tree, error) {
                    (Some(tree), _) => serde_json::from_value(tree)
                        .map(Rc::new)
                        .map_err(|error| error.to_string()),
                    (None, error) => Err(error.unwrap_or_default()),
                });
            }
            Answer::Edit { card, label, state } => {
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
}
