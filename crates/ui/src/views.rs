//! Which view shows which tool. An extension registers a view per tool. The window, and any
//! view that hosts the view of another instance, asks for it without naming the extension's
//! types.
//!
//! The registry is a GPUI global, installed once before the window opens. So a nested view
//! reaches it from its own context and nothing has to be passed down.
//!
//! Provisional and small. Composing a workspace from many views is later work.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{AnyView, App, Context, Entity, Global, Render, Window, prelude::*};
use sound_core::{Instance, InstanceId, State};

use crate::session::Session;

type CreateView =
    Rc<dyn Fn(&Entity<Session>, &InstanceId, &mut Window, &mut App) -> Option<AnyView>>;

#[derive(Default)]
pub struct Views {
    by_tool: BTreeMap<&'static str, CreateView>,
}

impl Global for Views {}

impl Views {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes this the registry of the application. The window of the runtime takes a `Views`
    /// and calls this, so the runtime cannot forget it. Another host, or a test without that
    /// window, calls it once before its first view.
    pub fn install(self, cx: &mut App) {
        cx.set_global(self);
    }

    /// Registers the view of the tool with state `S`. `create` gets the session and the typed
    /// instance, and makes the view entity.
    pub fn register<S: State, V: Render>(
        &mut self,
        create: impl Fn(Entity<Session>, Instance<S>, &mut Window, &mut Context<V>) -> V + 'static,
    ) {
        self.by_tool.insert(
            S::TOOL,
            Rc::new(move |session, id, window, cx| {
                let instance = session.read(cx).project().resolve::<S>(id)?;
                let view = cx.new(|cx| create(session.clone(), instance, window, cx));
                Some(view.into())
            }),
        );
    }

    /// A new view of the instance, from the installed registry. `None` when the instance is
    /// gone or its tool has no view. Any view may call it, to host the view of an instance
    /// whose tool it does not know.
    pub fn view_of(
        session: &Entity<Session>,
        id: &InstanceId,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyView> {
        let tool = session.read(cx).project().tool_of(id)?;
        let create = cx.try_global::<Self>()?.by_tool.get(tool)?.clone();
        create(session, id, window, cx)
    }

    /// The first instance at the top of the project whose tool has a view. The window shows
    /// it in its main area.
    pub fn main_instance(session: &Entity<Session>, cx: &App) -> Option<InstanceId> {
        let views = cx.try_global::<Self>()?;
        let project = session.read(cx).project();
        let mut instances = project.instances();
        instances
            .find(|(id, tool)| id.parent().is_none() && views.by_tool.contains_key(tool))
            .map(|(id, _)| id.clone())
    }
}
