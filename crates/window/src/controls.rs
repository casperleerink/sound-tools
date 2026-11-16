//! What an agent sees and does in the window: every control with its meaning (role, label,
//! value or state, and the actions it takes), and `--act`, which runs those actions by label or
//! key through the same road a pointer takes.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::InstanceId;
use sound_typescript::next::Drawn;

use crate::app::State;
use crate::card::{self, Field, Kind};
use crate::lives::live_name;
use crate::tree::Node;

/// One control of the window, as `--tree` lists it under `controls`.
#[derive(Serialize)]
pub(crate) struct Control {
    /// Its key in the window: the tool's own key in the main view, `t01/synth:cutoff` in a card.
    pub(crate) key: String,
    pub(crate) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) checked: Option<bool>,
    pub(crate) actions: Vec<String>,
    #[serde(skip)]
    pub(crate) target: Target,
}

/// What runs an action of a control.
pub(crate) enum Target {
    /// The tool's code, for an element of its view.
    View {
        instance: InstanceId,
        key: String,
    },
    /// The window, for a field of a card without a view: `option` is the choice a segment sets.
    Field {
        instance: InstanceId,
        field: Field,
        option: Option<Value>,
    },
    Play,
}

/// Every control the window shows now, the instances whose views it shows, and the meters.
pub(crate) fn collect(state: &State) -> Found {
    let mut found = Found::default();
    found.controls.push(Control {
        key: "play".into(),
        label: if state.playing { "Stop" } else { "Play" }.into(),
        role: Some("button".into()),
        value: None,
        checked: Some(state.playing),
        actions: vec!["press".into()],
        target: Target::Play,
    });
    if let Some(main) = state.main() {
        found.instance(state, main);
    }
    found
}

#[derive(Default)]
pub(crate) struct Found {
    pub(crate) controls: Vec<Control>,
    pub(crate) shown: BTreeSet<InstanceId>,
    /// The meters the views bind, by live name, such as `meter:song/t01/mix`.
    pub(crate) meters: BTreeSet<String>,
}

impl Found {
    fn instance(&mut self, state: &State, id: &InstanceId) {
        self.shown.insert(id.clone());
        let prefix = state.prefix_of(id);
        match state.drawn(id) {
            Some(Drawn::View { root, memos }) => {
                if let Ok(root) = Node::deserialize(&**root) {
                    self.node(state, id, &prefix, memos, &root);
                }
            }
            Some(Drawn::Failed(_)) => {}
            None if state.pending(id) => {}
            Some(Drawn::None) | None => self.fields(state, id, &prefix),
        }
    }

    fn node(
        &mut self,
        state: &State,
        id: &InstanceId,
        prefix: &str,
        memos: &HashMap<String, ((u64, u64), Rc<Value>)>,
        node: &Node,
    ) {
        let b = match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => b,
            Node::Memo { key } => {
                let inner =
                    (memos.get(key)).and_then(|(_, inner)| Node::deserialize(&**inner).ok());
                if let Some(inner) = inner {
                    self.node(state, id, prefix, memos, &inner);
                }
                return;
            }
            Node::Child { path } => {
                let child = id.child(path).ok();
                if let Some(child) = child.filter(|child| state.project.tool_of(child).is_some()) {
                    self.instance(state, &child);
                }
                return;
            }
            Node::Text(_) | Node::Shapes(_) => return,
        };
        if let Some(level) = &b.level {
            self.meters.insert(live_name(id, level));
        }
        let press = b.press.then(|| "press".to_string());
        let drag = b.drag.is_some().then(|| "drag".to_string());
        let actions: Vec<String> = (press.into_iter())
            .chain(b.actions.iter().cloned())
            .chain(drag)
            .collect();
        if let (Some(key), false) = (&b.key, actions.is_empty()) {
            self.controls.push(Control {
                key: format!("{prefix}{key}"),
                label: b.label.clone().unwrap_or_default(),
                role: b.role.clone(),
                value: b.value.clone(),
                checked: b.checked,
                actions,
                target: Target::View {
                    instance: id.clone(),
                    key: key.clone(),
                },
            });
        }
        for child in &b.children {
            self.node(state, id, prefix, memos, child);
        }
    }

    fn fields(&mut self, state: &State, id: &InstanceId, prefix: &str) {
        let title = card::title(state, id);
        for field in card::fields(state, id) {
            let key = format!("{prefix}{}", field.name);
            let label = format!("{title} {}", field.label);
            let target = |option| Target::Field {
                instance: id.clone(),
                field: field.clone(),
                option,
            };
            match &field.kind {
                Kind::Number { .. } => self.controls.push(Control {
                    key,
                    label,
                    role: Some("slider".into()),
                    value: Some(field.value.clone()),
                    checked: None,
                    actions: ["set", "increment", "decrement"].map(String::from).to_vec(),
                    target: target(None),
                }),
                Kind::Switch => self.controls.push(Control {
                    key,
                    label,
                    role: Some("switch".into()),
                    value: None,
                    checked: field.value.as_bool(),
                    actions: ["press", "toggle"].map(String::from).to_vec(),
                    target: target(None),
                }),
                Kind::Choice(options) => {
                    for option in options.clone() {
                        let word = option
                            .as_str()
                            .map_or_else(|| option.to_string(), str::to_string);
                        self.controls.push(Control {
                            key: format!("{key}={word}"),
                            label: format!("{label} {word}"),
                            role: Some("button".into()),
                            value: None,
                            checked: Some(option == field.value),
                            actions: vec!["press".into()],
                            target: target(Some(option)),
                        });
                    }
                }
            }
        }
    }
}

/// One step of `--act`: an action of the control with this label or key, with a number for
/// `set`; or `undo` or `redo` alone.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Step {
    label: Option<String>,
    key: Option<String>,
    action: String,
    value: Option<Value>,
}

pub(crate) fn read_steps(path: &Path) -> Result<Vec<Step>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| {
        format!("{} is not a list of steps such as {{ \"label\": \"Mute Track 2\", \"action\": \"toggle\" }}", path.display())
    })
}

impl Step {
    /// Runs the step on `state`, as a pointer or a key would, and waits for what it changed to
    /// show: the next step acts on the view as it is then.
    pub(crate) fn run(&self, state: &mut State) -> Result<()> {
        let done = self.act(state);
        state.settle();
        done?;
        match state.take_failed_action() {
            Some(why) => bail!("{} did not {}: {why}", self.named(), self.action),
            None => Ok(()),
        }
    }

    fn named(&self) -> &str {
        (self.label.as_ref())
            .or(self.key.as_ref())
            .map_or("", String::as_str)
    }

    fn act(&self, state: &mut State) -> Result<()> {
        if self.label.is_none() && self.key.is_none() {
            return match self.action.as_str() {
                "undo" => state.undo(),
                "redo" => state.redo(),
                other => bail!("{other} needs a control: give its label or key"),
            };
        }
        let found = collect(state).controls.into_iter().find(|control| {
            Some(&control.key) == self.key.as_ref() || Some(&control.label) == self.label.as_ref()
        });
        let named = self.named();
        let Some(control) = found else {
            let controls = collect(state).controls;
            let labels: Vec<String> = controls
                .iter()
                .map(|control| format!("{} ({})", control.label, control.key))
                .collect();
            bail!(
                "no control is called {named}; the window has: {}",
                labels.join(", ")
            );
        };
        if !control.actions.contains(&self.action) {
            bail!(
                "{named} takes {}, not {}",
                control.actions.join(", "),
                self.action
            );
        }
        state.perform(&control.target, &self.action, self.value.clone())
    }
}
