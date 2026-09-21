//! What a composer can put in a device slot of a rack, and what to call what is in one.
//!
//! The track panel is a rack of slots. It knows no instrument and no plugin, so it has to be
//! told. This registry is that telling: a list of offers, and a name per tool. Whoever makes
//! the window fills it, as it fills [`crate::Views`], and installs it as a GPUI global.
//!
//! Provisional and small, like the view registry. Today there is one kind of slot, the
//! instrument of a track.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{App, Entity, Global, SharedString};
use sound_core::{Changes, InstanceId, Project, ProjectError, State};

use crate::session::Session;

/// One thing a composer can choose for a slot: a built-in instrument, or a plugin this
/// machine has.
#[derive(Clone)]
pub struct DeviceOffer {
    /// Tells one offer from another in a menu and in a test. Unique in the list.
    pub key: SharedString,
    /// What the composer reads.
    pub name: SharedString,
    /// A quiet second line, such as who made the plugin.
    pub detail: Option<SharedString>,
    /// The extension whose tool this offer writes. A project that does not enable it cannot
    /// load what the offer writes, so a picker shows the offer and does not take it.
    pub needs: Option<SharedString>,
    write: Rc<dyn Fn(&Project, &InstanceId, &mut Changes) -> Result<(), ProjectError>>,
}

impl DeviceOffer {
    pub fn new(
        key: impl Into<SharedString>,
        name: impl Into<SharedString>,
        write: impl Fn(&Project, &InstanceId, &mut Changes) -> Result<(), ProjectError> + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            detail: None,
            needs: None,
            write: Rc::new(write),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The extension this offer needs the project to enable.
    pub fn needs(mut self, extension: impl Into<SharedString>) -> Self {
        self.needs = Some(extension.into());
        self
    }

    /// Whether this project could load what the offer writes. Enabling an extension while a
    /// project runs is refused, so an offer a project has not enabled stays out of reach until
    /// the composer edits `project.json` and opens the project again.
    pub fn is_enabled_in(&self, project: &Project) -> bool {
        let Some(needs) = &self.needs else {
            return true;
        };
        let enabled = &project.project_file().extensions;
        enabled.iter().any(|extension| extension == needs.as_ref())
    }

    /// Puts the record of this offer into `slot`. The caller commits the group, so choosing an
    /// instrument is one undo step. Only call it for an offer [`Self::is_enabled_in`] takes.
    pub fn write(
        &self,
        project: &Project,
        slot: &InstanceId,
        changes: &mut Changes,
    ) -> Result<(), ProjectError> {
        (self.write)(project, slot, changes)
    }
}

type ListOffers = Rc<dyn Fn() -> Vec<DeviceOffer>>;
type ListNotes = Rc<dyn Fn() -> Vec<SharedString>>;
type DescribeInstance = Rc<dyn Fn(&Project, &InstanceId) -> Option<DeviceLabel>>;

/// What a rack says about the instance in a slot: what to call it, and which offer it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLabel {
    /// The [`DeviceOffer::key`] of the offer that would write this record, so a picker can
    /// mark what is there and leave it alone when it is picked again.
    pub key: SharedString,
    pub name: SharedString,
}

#[derive(Default)]
pub struct Devices {
    instruments: Vec<ListOffers>,
    notes: Vec<ListNotes>,
    describe: BTreeMap<&'static str, DescribeInstance>,
}

impl Global for Devices {}

impl Devices {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes this the registry of the application, as [`crate::Views::install`] does for views.
    pub fn install(self, cx: &mut App) {
        cx.set_global(self);
    }

    /// Adds a source of instruments. It is asked every time a picker is filled, not while the
    /// window opens, so a source that has to look at this machine pays for it when a composer
    /// opens a track panel and not before.
    pub fn instruments(&mut self, list: impl Fn() -> Vec<DeviceOffer> + 'static) {
        self.instruments.push(Rc::new(list));
    }

    /// Adds a source of quiet lines a picker shows under its offers: what a source of offers
    /// is still doing, and what it has to say about what it offers. Asked when a picker is
    /// filled, like the offers.
    pub fn notes(&mut self, list: impl Fn() -> Vec<SharedString> + 'static) {
        self.notes.push(Rc::new(list));
    }

    /// Registers what a rack says about an instance of the tool with state `S`.
    pub fn describe<S: State>(&mut self, describe: impl Fn(&S) -> DeviceLabel + 'static) {
        self.describe.insert(
            S::TOOL,
            Rc::new(move |project, id| {
                let instance = project.resolve::<S>(id)?;
                Some(describe(project.state(&instance)?))
            }),
        );
    }

    /// Every instrument on offer, from the installed registry.
    pub fn offered(cx: &App) -> Vec<DeviceOffer> {
        let Some(devices) = cx.try_global::<Self>() else {
            return Vec::new();
        };
        devices.instruments.iter().flat_map(|list| list()).collect()
    }

    /// Every quiet line under the offers, from the installed registry.
    pub fn offer_notes(cx: &App) -> Vec<SharedString> {
        let Some(devices) = cx.try_global::<Self>() else {
            return Vec::new();
        };
        devices.notes.iter().flat_map(|list| list()).collect()
    }

    /// What a rack says about what is in `id`. `None` when the instance is gone or its tool
    /// registered nothing, and then the caller shows the tool name or that the slot is empty.
    pub fn label_of(session: &Entity<Session>, id: &InstanceId, cx: &App) -> Option<DeviceLabel> {
        let project = session.read(cx).project();
        let tool = project.tool_of(id)?;
        let describe = cx.try_global::<Self>()?.describe.get(tool)?.clone();
        describe(project, id)
    }
}
