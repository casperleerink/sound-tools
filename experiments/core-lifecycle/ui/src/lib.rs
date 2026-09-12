//! GPUI bridge. Extensions observe this entity; every published edit notifies them.
use gpui::Context;
use sound_core::{Project, Result};

pub struct Session {
    project: Project,
    pub status: String,
    pub rms: f32,
}
impl Session {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            status: "Ready. Offline audio only.".into(),
            rms: 0.0,
        }
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn change(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut Project) -> Result<()>,
    ) {
        match action(&mut self.project) {
            Ok(()) => self.status = "Edit applied".into(),
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        match self.project.poll_files() {
            Ok(count) if count > 0 => {
                self.status = format!("Applied {count} file edit(s)");
                cx.notify();
            }
            Err(error) => {
                let message = error.to_string();
                if self.status != message {
                    self.status = message;
                    cx.notify();
                }
            }
            _ => {}
        }
        if self.project.playing() {
            let mut samples = [0.0; 256];
            self.project.render(&mut samples, 48_000.0);
            self.rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
            cx.notify();
        } else if self.rms != 0.0 {
            self.rms = 0.0;
            cx.notify();
        }
    }
}

use gpui::{AnyView, App, AppContext, Entity, Render};
use sound_core::{Instance, State, Tool};
use std::collections::BTreeMap;
type ViewFactory = Box<dyn Fn(Entity<Session>, &str, &mut App) -> Result<AnyView>>;
#[derive(Default)]
pub struct Views {
    factories: BTreeMap<&'static str, ViewFactory>,
}
impl Views {
    pub fn register<S: State, V: Render + 'static>(
        &mut self,
        tool: Tool<S>,
        create: fn(Entity<Session>, Instance<S>, &mut Context<V>) -> V,
    ) {
        self.factories.insert(
            tool.name(),
            Box::new(move |session, id, cx| {
                let instance = session.read(cx).project().resolve(tool, id)?;
                Ok(cx.new(|cx| create(session, instance, cx)).into())
            }),
        );
    }
    pub fn open(
        &self,
        tool: &str,
        session: Entity<Session>,
        id: &str,
        cx: &mut App,
    ) -> Result<AnyView> {
        self.factories
            .get(tool)
            .ok_or_else(|| sound_core::Error(format!("No view for {tool}")))?(
            session, id, cx
        )
    }
}
