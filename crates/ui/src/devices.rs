//! What a composer can put in a device slot of a rack, and what to call what is in one.
//!
//! The track panel is a rack of slots. It knows no instrument and no plugin, so it has to be
//! told. This registry is that telling: a list of offers, and a name per tool. Whoever makes
//! the window fills it, as it fills [`crate::Views`], and installs it as a GPUI global.
//!
//! Provisional and small, like the view registry. There are two kinds of slot, the instrument
//! of a track and an effect after it, and an offer is made for one of them.

use std::collections::{BTreeMap, BTreeSet};
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
        match &self.needs {
            Some(needs) => extension_is_enabled(project, needs),
            None => true,
        }
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
type Generation = Rc<dyn Fn() -> u64>;
type DescribeInstance = Rc<dyn Fn(&Project, &InstanceId) -> Option<DeviceLabel>>;

/// What a rack says about the instance in a slot: what to call it, and which offer it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLabel {
    /// The [`DeviceOffer::key`] of the offer that would write this record, so a picker can
    /// mark what is there and leave it alone when it is picked again.
    pub key: SharedString,
    pub name: SharedString,
}

/// Which slot of a rack an offer is for. A rack asks for one kind at a time: the picker on the
/// instrument card offers instruments, and the control that adds one offers effects.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Instrument,
    Effect,
}

#[derive(Default)]
pub struct Devices {
    instruments: Vec<ListOffers>,
    effects: Vec<ListOffers>,
    notes: Vec<ListNotes>,
    generations: Vec<Generation>,
    describe: BTreeMap<&'static str, DescribeInstance>,
    /// The tools whose view hides controls behind the expand icon of its card.
    expands: BTreeSet<&'static str>,
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

    /// Adds a source of effects, asked for like [`Self::instruments`].
    pub fn effects(&mut self, list: impl Fn() -> Vec<DeviceOffer> + 'static) {
        self.effects.push(Rc::new(list));
    }

    /// Adds a source of quiet lines a picker shows under its offers: what a source of offers
    /// is still doing, and what it has to say about what it offers. Asked when a picker is
    /// filled, like the offers.
    pub fn notes(&mut self, list: impl Fn() -> Vec<SharedString> + 'static) {
        self.notes.push(Rc::new(list));
    }

    /// Adds a source of the number behind [`Self::offers_generation`]: a count that goes up
    /// whenever what this source offers, or what it has to say under its offers, changes.
    pub fn offers_change(&mut self, generation: impl Fn() -> u64 + 'static) {
        self.generations.push(Rc::new(generation));
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

    /// Says that the view of the tool with state `S` hides controls, so its card gets the
    /// expand icon. The view reads [`Session::is_expanded`] and shows them when it is set.
    pub fn expands<S: State>(&mut self) {
        self.expands.insert(S::TOOL);
    }

    /// Whether the card of what is in `id` has controls behind its expand icon.
    pub fn has_hidden(session: &Entity<Session>, id: &InstanceId, cx: &App) -> bool {
        let project = session.read(cx).project();
        let (Some(tool), Some(devices)) = (project.tool_of(id), cx.try_global::<Self>()) else {
            return false;
        };
        devices.expands.contains(tool)
    }

    /// Everything on offer for one kind of slot, from the installed registry.
    pub fn offered(slot: Slot, cx: &App) -> Vec<DeviceOffer> {
        let Some(devices) = cx.try_global::<Self>() else {
            return Vec::new();
        };
        let sources = match slot {
            Slot::Instrument => &devices.instruments,
            Slot::Effect => &devices.effects,
        };
        sources.iter().flat_map(|list| list()).collect()
    }

    /// A number that changes when the offers do. A view builds its menus once, because a
    /// source may have to look at the machine, and builds them again when this changes; the
    /// plugin host looks for the plugins of this Mac on a thread of its own, so a menu built
    /// while that runs holds a part of the list and the line that says so.
    ///
    /// It reads a counter per source and nothing else, so a poll may ask on every frame.
    pub fn offers_generation(cx: &App) -> u64 {
        let Some(devices) = cx.try_global::<Self>() else {
            return 0;
        };
        let sources = devices.generations.iter();
        sources.fold(0, |total, generation| total.wrapping_add(generation()))
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

/// Whether `project.json` lists this extension under `extensions`, which is what decides
/// whether the project can load a record of its tools.
pub fn extension_is_enabled(project: &Project, extension: &str) -> bool {
    let enabled = &project.project_file().extensions;
    enabled.iter().any(|it| it == extension)
}

/// The one edit that brings an extension within reach. Every control that offers something a
/// project has not enabled says this and nothing else: enabling an extension while a project
/// runs is refused, so it is a file edit and a reopen.
pub fn enable_extension(extension: &str) -> String {
    format!("Add \"{extension}\" to \"extensions\" in project.json and open the project again.")
}
