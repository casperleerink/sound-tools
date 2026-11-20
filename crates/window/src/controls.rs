//! What an agent sees and does in the window: every control with its meaning (role, label,
//! value or state, and the actions it takes), and `--act`, which runs those actions by label or
//! key through the same road a pointer takes.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::InstanceId;

use crate::app::State;
use crate::tree::{DragSpec, Node, Opens, Press, Within};

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
    /// Whether what it opens is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) expanded: Option<bool>,
    pub(crate) actions: Vec<String>,
    /// Where it is placed in the plane around it, in its units: [x, y, w, h], or [x, y].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) place: Option<Vec<f32>>,
    #[serde(skip)]
    pub(crate) target: Target,
    /// The key of what opens the menu around it, or its own.
    #[serde(skip)]
    menu: Option<String>,
    /// How it drags, and the point of it that lands on the grid.
    #[serde(skip)]
    drag: Option<(DragSpec, [f32; 2])>,
}

/// What runs an action of a control.
pub(crate) enum Target {
    /// The tool's code, for an element of its view, as a press on it goes.
    View(Press),
    Play,
}

/// Every control the window shows now, and the instances whose views it shows.
pub(crate) fn collect(state: &State) -> Found {
    let mut found = Found::default();
    found.controls.push(Control {
        key: "play".into(),
        label: if state.playing { "Stop" } else { "Play" }.into(),
        role: Some("button".into()),
        value: None,
        checked: Some(state.playing),
        expanded: None,
        actions: vec!["press".into()],
        place: None,
        target: Target::Play,
        menu: None,
        drag: None,
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
}

impl Found {
    fn instance(&mut self, state: &State, id: &InstanceId) {
        self.shown.insert(id.clone());
        let prefix = state.prefix_of(id);
        if let Some(root) = state.root(id) {
            self.node(state, id, &prefix, &root, &Around::default());
        }
    }

    fn node(&mut self, state: &State, id: &InstanceId, prefix: &str, node: &Node, around: &Around) {
        let within = around.within.as_ref();
        let press = |key: &str, pressed: bool, opens: bool| {
            Target::View(Press {
                instance: id.clone(),
                element: key.to_string(),
                pressed,
                opens,
                within: within.cloned(),
            })
        };
        let b = match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => b,
            Node::Memo { key } => {
                if let Some((_, inner)) = state.memo_of(id, key) {
                    self.node(state, id, prefix, &inner, around);
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
            Node::Field(f) => {
                self.controls.push(Control {
                    key: format!("{prefix}{}", f.key),
                    label: f.label.clone(),
                    role: Some("textbox".into()),
                    value: Some(Value::String(f.value.clone())),
                    checked: None,
                    expanded: None,
                    actions: vec!["commit".into()],
                    place: None,
                    target: press(&f.key, false, false),
                    menu: around.menu.clone(),
                    drag: None,
                });
                return;
            }
            Node::Text(_) | Node::Shapes(_) | Node::Icon(_) => return,
        };
        let pressed = b.press.then(|| "press".to_string());
        let drag = b.drag.is_some().then(|| "drag".to_string());
        let enter = b.enter.then(|| "enter".to_string());
        let opens = b.overlay_of(Opens::Press).is_some();
        let open = opens.then(|| "open".to_string());
        let menu = b.opens_menu().then(|| "menu".to_string());
        let actions: Vec<String> = (open.into_iter())
            .chain(menu)
            .chain(pressed)
            .chain(b.actions.iter().cloned())
            .chain(drag)
            .chain(enter)
            .collect();
        let value = |num: &sound_typescript::live::Num| state.lives.value(state, id, num);
        let place = (b.place.as_ref()).map(|place| place.numbers().iter().map(value).collect());
        let window_key = b.key.as_ref().map(|key| format!("{prefix}{key}"));
        let around = Around {
            within: around.within.clone(),
            menu: (window_key.clone().filter(|_| b.opens_menu())).or(around.menu.clone()),
            placed: (b.place.as_ref())
                .map(|place| place.origin().each_ref().map(value))
                .or(around.placed),
        };
        // A slider a lane moves takes no action, but an agent reads what it plays: its value.
        let listed = !actions.is_empty() || b.role.as_deref() == Some("slider");
        if let (Some(key), Some(window_key), true) = (&b.key, &window_key, listed) {
            let target = press(key, b.press, opens);
            self.controls.push(Control {
                expanded: (opens || b.opens_menu()).then(|| state.open.contains(window_key)),
                key: window_key.clone(),
                label: b.label.clone().unwrap_or_default(),
                role: b.role.clone(),
                value: (b.value.as_ref()).map(|value| state.lives.said(state, id, value)),
                checked: b.checked,
                actions,
                place,
                target,
                menu: around.menu.clone(),
                drag: (b.drag.clone()).map(|drag| {
                    let from = drag.from.or(around.placed).unwrap_or_default();
                    (drag, from)
                }),
            });
        }
        for child in &b.children {
            let overlay = match child {
                Node::Row(o) | Node::Col(o) | Node::Plain(o) => o.overlay.map(|spec| (o, spec)),
                _ => None,
            };
            match (overlay, &b.key) {
                // What a closed overlay shows is not there to act on, as for a pointer.
                (Some((o, spec)), Some(key)) if spec.open != Opens::Hover => {
                    let opener = format!("{prefix}{key}");
                    if state.open.contains(&opener) {
                        let inside = Around {
                            within: Some(Within {
                                opener,
                                picks: o.picks(),
                            }),
                            ..Around::default()
                        };
                        self.node(state, id, prefix, child, &inside);
                    }
                }
                _ => self.node(state, id, prefix, child, &around),
            }
        }
    }
}

/// What is around an element of a view: the overlay it is in, what opens the menu around it,
/// and where the placed element nearest around it is, in units.
#[derive(Default)]
struct Around {
    within: Option<Within>,
    menu: Option<String>,
    placed: Option<[f32; 2]>,
}

/// One step of `--act`: an action of the control with this label or key, with a number for
/// `set`, a pointer for `press` and `drag`; `view` of a plane by its key; or `undo` or `redo`
/// alone.
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
        state.open_menu();
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
        if let (Some(key), "view") = (&self.key, self.action.as_str()) {
            return view(state, key, self.value.as_ref());
        }
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
        if control.actions.is_empty() {
            bail!(
                "a lane of the track moves {named}, so it takes no change until the lane is taken out"
            );
        }
        if !control.actions.contains(&self.action) {
            bail!(
                "{named} takes {}, not {}",
                control.actions.join(", "),
                self.action
            );
        }
        if self.action == "menu" {
            state.menu_at(control.menu.as_deref().unwrap_or_default(), None);
            return Ok(());
        }
        let value = match (&control.drag, self.action.as_str()) {
            (Some((drag, from)), "drag") => Some(landed(drag, *from, self.value.as_ref())),
            _ => self.value.clone(),
        };
        let right = value.as_ref().is_some_and(|value| value["right"] == true);
        if !(right && !control.actions.contains(&"press".to_string())) {
            state.perform(&control.target, &self.action, value)?;
        }
        if let (true, Some(menu)) = (right, &control.menu) {
            state.menu_at(menu, None);
        }
        Ok(())
    }
}

/// A drag of `value`, a part of a pointer, as the window ends it: on the grid unless Cmd is held,
/// then in its bounds.
fn landed(drag: &DragSpec, from: [f32; 2], value: Option<&Value>) -> Value {
    let mut value = value.cloned().unwrap_or_else(|| serde_json::json!({}));
    let number = |name: &str| value[name].as_f64().unwrap_or(0.0) as f32;
    let free = value["cmd"] == true;
    let [dx, dy] = drag.landed([number("dx"), number("dy")], from, free);
    value["dx"] = dx.into();
    value["dy"] = dy.into();
    value
}

/// Shows a part of the plane `key`, as `{ "left", "top", "zoom" }` say.
fn view(state: &mut State, key: &str, value: Option<&Value>) -> Result<()> {
    let number = |name: &str| {
        value
            .and_then(|value| value[name].as_f64())
            .map(|n| n as f32)
    };
    state
        .planes
        .set_view(key, number("left"), number("top"), number("zoom"))
        .map_err(anyhow::Error::msg)?;
    state.lives.move_planes(state, &[key.to_string()], true);
    state.handled();
    Ok(())
}
