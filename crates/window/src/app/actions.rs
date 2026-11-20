//! What a pointer, a key or an agent does to the views and the transport.

use std::collections::BTreeSet;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::Deserialize as _;
use serde_json::{Value, json};
use sound_core::InstanceId;
use sound_typescript::next::Drawn;

use crate::controls::Target;
use crate::tree;

use super::{DID_NOTHING, State};

impl State {
    /// An action on an element of a view: its tool's code runs it.
    pub(crate) fn act_on_view(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) -> Result<()> {
        if action != "commit" {
            self.fields.get_mut().pressed = Some(format!("{}{key}", self.prefix_of(instance)));
        }
        let tools = (self.tools.as_mut()).context("the project has no code to run this")?;
        tools
            .act(instance, key, action, value)
            .map_err(|why| anyhow!(why))
    }

    /// [`Self::act_on_view`] by a pointer: what goes wrong is said in the corner.
    pub(crate) fn pointed(
        &mut self,
        instance: &InstanceId,
        key: &str,
        action: &str,
        value: Option<Value>,
    ) {
        if self.act_on_view(instance, key, action, value).is_err() {
            self.notice = Some(DID_NOTHING.to_string());
        }
        self.handled();
    }

    /// Why the last actions on views did nothing.
    pub(crate) fn take_failed_action(&mut self) -> Option<String> {
        let failed = self.tools.as_mut()?.take_failed_actions();
        failed.into_iter().last()
    }

    pub(crate) fn perform(
        &mut self,
        target: &Target,
        action: &str,
        value: Option<Value>,
    ) -> Result<()> {
        match target {
            Target::Chrome(act) => act.run(self),
            Target::Dialog(says) => self.answer_dialog(*says),
            Target::View(press) => {
                let value = match action {
                    "drag" => value.or_else(|| Some(json!({ "dx": 0, "dy": 0 }))),
                    _ => value,
                };
                self.try_press(press, action, value)?;
            }
        }
        Ok(())
    }

    /// Runs the action `<tool>.<action>` of the main view's tool, else of the first shown
    /// instance of that tool, as its key does.
    pub(crate) fn act_by_name(&mut self, name: &str) -> Result<()> {
        let (tool, action) = name
            .rsplit_once('.')
            .context("an action is named <tool>.<action>")?;
        let main = self.main().cloned();
        let shown = main.iter().chain(&self.shown);
        let instance = shown
            .into_iter()
            .find(|id| self.project.tool_of(id) == Some(tool))
            .cloned()
            .with_context(|| format!("no {tool} shows, so nothing runs {name}"))?;
        let ids: Vec<String> = (self.keys.keymap.groups())
            .filter(|group| group.name == tool)
            .flat_map(|group| group.actions.iter().map(|action| action.name.clone()))
            .collect();
        if !ids.iter().any(|id| id == action) {
            bail!("{tool} has no action {action}; it has: {}", ids.join(", "));
        }
        if !self.run_actions(&instance, &[action.to_string()]) {
            bail!("the code of the project does not run now, so {name} did nothing");
        }
        Ok(())
    }

    /// A press on the element `key` of the view of `instance`, which has `onEnter`, paints it
    /// and what it enters until let go: one step of undo.
    pub(crate) fn paint(&mut self, instance: &InstanceId, key: &str) {
        self.painting = Some((instance.clone(), BTreeSet::from([key.to_string()])));
        if let Some(tools) = &mut self.tools {
            tools.hold(&mut self.project, true);
        }
        self.pointed(instance, key, "enter", None);
    }

    /// The pointer of a paint is over the element at `path`, Rune's path of keys: the first
    /// time it is over one that has `onEnter`, or is inside one, that one is painted.
    pub(crate) fn paint_over(&mut self, path: &str) {
        let Some((instance, entered)) = &self.painting else {
            return;
        };
        let Some(Drawn::View { root, memos }) = self.drawn(instance) else {
            return;
        };
        let Ok(root) = tree::Node::deserialize(&**root) else {
            return;
        };
        let mut keys = Vec::new();
        tree::enterable(&root, memos, &mut keys);
        let (prefix, path) = (self.prefix_of(instance), format!("/{path}/"));
        let found = (keys.into_iter())
            .filter(|key| !entered.contains(key))
            .find(|key| path.contains(&format!("/{prefix}{key}/")));
        let Some(key) = found else {
            return;
        };
        let instance = instance.clone();
        if let Some((_, entered)) = &mut self.painting {
            entered.insert(key.clone());
        }
        self.pointed(&instance, &key, "enter", None);
    }

    /// The press of a paint is let go.
    pub(crate) fn painted(&mut self) {
        self.painting = None;
        if let Some(tools) = &mut self.tools {
            tools.hold(&mut self.project, false);
        }
        self.handled();
    }

    /// Opens the overlay that the element `key` opens, inside the one it is in, or closes it
    /// and those inside it.
    pub(crate) fn toggle_overlay(&mut self, key: &str, within: Option<&str>) {
        match self.open.iter().position(|open| open == key) {
            Some(at) => self.open.truncate(at),
            None => {
                let kept =
                    within.and_then(|within| self.open.iter().position(|open| open == within));
                self.open.truncate(kept.map_or(0, |at| at + 1));
                self.open.push(key.to_string());
            }
        }
        self.handled();
    }

    /// Closes the overlay the element `key` opened, and those inside it.
    pub(crate) fn close_overlay(&mut self, key: &str) {
        if let Some(at) = self.open.iter().position(|open| open == key) {
            self.open.truncate(at);
        }
        self.handled();
    }

    /// `action` on an element of a view, as a pointer, a key or an agent does it: `open` opens
    /// or closes what it opens, a pick from a menu closes the menu, and a typed value closes the
    /// overlay it was typed in.
    pub(crate) fn press(&mut self, press: &tree::Press, action: &str, value: Option<Value>) {
        if self.try_press(press, action, value).is_err() {
            self.notice = Some(DID_NOTHING.to_string());
        }
        self.handled();
    }

    fn try_press(&mut self, press: &tree::Press, action: &str, value: Option<Value>) -> Result<()> {
        let key = format!("{}{}", self.prefix_of(&press.instance), press.element);
        let within = press.within.as_ref();
        if press.opens && (action == "open" || (action == "press" && !press.pressed)) {
            self.toggle_overlay(&key, within.map(|within| within.opener.as_str()));
            return Ok(());
        }
        let closes = match action {
            "press" => within.filter(|within| within.picks),
            "commit" => within,
            _ => None,
        };
        if let Some(within) = closes {
            let opener = within.opener.clone();
            self.close_overlay(&opener);
        }
        if press.opens && action == "press" {
            self.toggle_overlay(&key, within.map(|within| within.opener.as_str()));
        }
        self.act_on_view(&press.instance, &press.element, action, value)
    }

    pub(super) fn play_or_stop(&mut self) {
        match self.playing {
            true => self.project.engine().stop(),
            false => self.project.engine().play(),
        }
    }

    pub(crate) fn undo(&mut self) -> Result<()> {
        self.project.undo()?;
        Ok(())
    }

    pub(crate) fn redo(&mut self) -> Result<()> {
        self.project.redo()?;
        Ok(())
    }
}
