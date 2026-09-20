//! Which view shows which tool. An extension registers a view per tool, and the window asks
//! for the view of an instance without naming the extension's types.
//!
//! Provisional and small. Composing a workspace from many views is later work.

use std::collections::BTreeMap;

use gpui::{AnyView, App, Context, Entity, Render, Window, prelude::*};
use sound_core::{Instance, InstanceId, State};

use crate::session::Session;

type CreateView =
    Box<dyn Fn(&Entity<Session>, &InstanceId, &mut Window, &mut App) -> Option<AnyView>>;

#[derive(Default)]
pub struct Views {
    by_tool: BTreeMap<&'static str, CreateView>,
}

impl Views {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the view of the tool with state `S`. `create` gets the session and the typed
    /// instance, and makes the view entity.
    pub fn register<S: State, V: Render>(
        &mut self,
        create: impl Fn(Entity<Session>, Instance<S>, &mut Window, &mut Context<V>) -> V + 'static,
    ) {
        self.by_tool.insert(
            S::TOOL,
            Box::new(move |session, id, window, cx| {
                let instance = session.read(cx).project().resolve::<S>(id)?;
                let view = cx.new(|cx| create(session.clone(), instance, window, cx));
                Some(view.into())
            }),
        );
    }

    /// A new view of the instance. `None` when the instance is gone or its tool has no view.
    pub fn view_of(
        &self,
        session: &Entity<Session>,
        id: &InstanceId,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyView> {
        let tool = session.read(cx).project().tool_of(id)?;
        self.by_tool.get(tool)?(session, id, window, cx)
    }

    /// The first instance at the top of the project whose tool has a view. The window shows
    /// it in its main area.
    pub fn main_instance(&self, session: &Entity<Session>, cx: &App) -> Option<InstanceId> {
        let project = session.read(cx).project();
        let mut instances = project.instances();
        instances
            .find(|(id, tool)| id.parent().is_none() && self.by_tool.contains_key(tool))
            .map(|(id, _)| id.clone())
    }
}
