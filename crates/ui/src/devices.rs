//! What a composer can put in a device slot of a rack, and what to call what is in one.
//!
//! The track panel is a rack of slots. It knows no instrument and no plugin, so it has to be
//! told. This registry is that telling: a list of offers, and a name per tool. Whoever makes
//! the window fills it, as it fills [`crate::Views`], and installs it as a GPUI global.
//!
//! Provisional and small, like the view registry. There are two kinds of slot, the instrument
//! of a track and an effect after it, and an offer is made for one of them.

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
    /// Where a picker lists it.
    pub group: OfferGroup,
    /// The icon a picker shows beside the name, from `crates/ui/assets/icons`.
    pub icon: SharedString,
    /// The extension whose tool this offer writes. A project that does not enable it cannot
    /// load what the offer writes, so a picker shows the offer and does not take it.
    pub needs: Option<Needs>,
    write: Rc<dyn Fn(&Project, &InstanceId, &mut Changes) -> Result<(), ProjectError>>,
}

impl DeviceOffer {
    /// An offer with the plug icon, which is what a plugin shows. A built-in device names its
    /// own with [`Self::icon`].
    pub fn new(
        key: impl Into<SharedString>,
        name: impl Into<SharedString>,
        group: OfferGroup,
        write: impl Fn(&Project, &InstanceId, &mut Changes) -> Result<(), ProjectError> + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            detail: None,
            group,
            icon: "device-plugin".into(),
            needs: None,
            write: Rc::new(write),
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = icon.into();
        self
    }

    pub fn with_detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The extension this offer needs the project to enable, and why the offer is off in a
    /// project that does not, in words for a composer: `This project does not load plugins.`
    pub fn needs(
        mut self,
        extension: impl Into<SharedString>,
        reason: impl Into<SharedString>,
    ) -> Self {
        self.needs = Some(Needs {
            extension: extension.into(),
            reason: reason.into(),
        });
        self
    }

    /// Whether this project could load what the offer writes. Enabling an extension while a
    /// project runs is refused, so an offer a project has not enabled stays out of reach until
    /// the composer edits `project.json` and opens the project again.
    pub fn is_enabled_in(&self, project: &Project) -> bool {
        match &self.needs {
            Some(needs) => extension_is_enabled(project, &needs.extension),
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

/// The groups of a picker, in the order it lists them. Built-in instruments are one group;
/// built-in effects are grouped by what they do to the sound. Plugins come last.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OfferGroup {
    BuiltIn,
    Tone,
    Dynamics,
    Space,
    Mix,
    /// The tools a project wrote for itself.
    Project,
    Plugins,
}

impl OfferGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "Built-in",
            Self::Tone => "Tone",
            Self::Dynamics => "Dynamics",
            Self::Space => "Space",
            Self::Mix => "Mix",
            Self::Project => "This project",
            Self::Plugins => "Plugins",
        }
    }
}

/// What an offer needs of the project. The file edit that enables an extension is for the
/// agent docs; a composer reads the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needs {
    pub extension: SharedString,
    pub reason: SharedString,
}

type ListOffers = Rc<dyn Fn() -> Vec<DeviceOffer>>;
type ListNotes = Rc<dyn Fn() -> Vec<SharedString>>;
type Generation = Rc<dyn Fn() -> u64>;
type DescribeInstance = Rc<dyn Fn(&Project, &InstanceId) -> Option<DeviceLabel>>;

/// What the lanes of a track need to know of the numbers of a device whose numbers are known
/// only as it runs, such as a plugin, where the name of a number in the record says nothing to
/// a person: `parameters.12.value`. Registered per tool with [`Devices::numbers`]. A built-in
/// device registers none: a lane names its numbers by their field, the unit at the end of the
/// field reads a value out, and its behaviour names every number a lane can take.
///
/// Each may call into a plugin, so they are asked when something changes, never while drawing.
pub trait Numbers<S> {
    /// What a person reads for the number `field`: `Cutoff`.
    fn name(&self, project: &Project, id: &InstanceId, state: &S, field: &str) -> Option<String>;

    /// What a person reads for `value` of the number `field`: the plugin's own `-6 dB`.
    fn text(
        &self,
        project: &Project,
        id: &InstanceId,
        state: &S,
        field: &str,
        value: f32,
    ) -> Option<String>;

    /// Every number a lane can be added for, in the device's own order, also one the device
    /// takes only once its record says so, such as a parameter of a plugin that is not pinned.
    fn lane_numbers(&self, project: &Project, id: &InstanceId, state: &S) -> Vec<LaneNumber>;

    /// The record that lets a lane move `field`, which it does not yet, with the value the lane
    /// starts at: what the device plays now. `None` when the record already does, or cannot.
    fn take(&self, project: &Project, id: &InstanceId, state: &S, field: &str) -> Option<(S, f32)>;
}

/// A number a lane can be added for, see [`Numbers::lane_numbers`].
#[derive(Clone, Debug, PartialEq)]
pub struct LaneNumber {
    /// What a lane names it by: `parameters.12.value`.
    pub field: SharedString,
    pub name: SharedString,
}

/// [`Numbers`] of one tool, with the state looked up.
trait ToolNumbers {
    fn name(&self, project: &Project, id: &InstanceId, field: &str) -> Option<String>;
    fn text(&self, project: &Project, id: &InstanceId, field: &str, value: f32) -> Option<String>;
    fn lane_numbers(&self, project: &Project, id: &InstanceId) -> Vec<LaneNumber>;
    fn take(
        &self,
        project: &Project,
        id: &InstanceId,
        field: &str,
        changes: &mut Changes,
    ) -> Option<f32>;
}

struct Typed<S, N>(N, std::marker::PhantomData<S>);

impl<S: State, N: Numbers<S>> ToolNumbers for Typed<S, N> {
    fn name(&self, project: &Project, id: &InstanceId, field: &str) -> Option<String> {
        let state = project.state(&project.resolve::<S>(id)?)?;
        self.0.name(project, id, state, field)
    }

    fn text(&self, project: &Project, id: &InstanceId, field: &str, value: f32) -> Option<String> {
        let state = project.state(&project.resolve::<S>(id)?)?;
        self.0.text(project, id, state, field, value)
    }

    fn lane_numbers(&self, project: &Project, id: &InstanceId) -> Vec<LaneNumber> {
        let state = project.resolve::<S>(id).and_then(|it| project.state(&it));
        let numbers = state.map(|state| self.0.lane_numbers(project, id, state));
        numbers.unwrap_or_default()
    }

    fn take(
        &self,
        project: &Project,
        id: &InstanceId,
        field: &str,
        changes: &mut Changes,
    ) -> Option<f32> {
        let instance = project.resolve::<S>(id)?;
        let (record, value) = self.0.take(project, id, project.state(&instance)?, field)?;
        changes.set(&instance, record);
        Some(value)
    }
}

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
    /// See [`Self::describe_others`].
    describe_others: Option<DescribeInstance>,
    numbers: BTreeMap<&'static str, Rc<dyn ToolNumbers>>,
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

    /// Registers what a rack says about an instance of a tool that registered nothing itself,
    /// such as a tool the project wrote, which comes and goes while the project is open.
    pub fn describe_others(
        &mut self,
        describe: impl Fn(&Project, &InstanceId) -> Option<DeviceLabel> + 'static,
    ) {
        self.describe_others = Some(Rc::new(describe));
    }

    /// Registers what the lanes of a track need to know of the numbers of an instance of the
    /// tool with state `S`, see [`Numbers`].
    pub fn numbers<S: State>(&mut self, numbers: impl Numbers<S> + 'static) {
        let typed = Typed(numbers, std::marker::PhantomData);
        self.numbers.insert(S::TOOL, Rc::new(typed));
    }

    /// Registers a built-in device with state `S`: what a rack calls it, and its offer, which
    /// is the device at its defaults. [`OfferGroup::BuiltIn`] is the group of the instruments;
    /// every other group is one of effects. A project that does not enable `extension` shows
    /// the offer and does not take it, and `reason` says why. A picker lists the offers of a
    /// group in the order they were registered.
    pub fn built_in<S: State + Default>(
        &mut self,
        name: &'static str,
        group: OfferGroup,
        icon: &'static str,
        extension: &'static str,
        reason: &'static str,
    ) {
        self.describe::<S>(move |_| DeviceLabel {
            key: S::TOOL.into(),
            name: name.into(),
        });
        let offer = DeviceOffer::new(S::TOOL, name, group, |_, slot, changes| {
            changes.create(slot.clone(), S::default());
            Ok(())
        })
        .icon(icon)
        .needs(extension, reason);
        let offers = match group {
            OfferGroup::BuiltIn => &mut self.instruments,
            _ => &mut self.effects,
        };
        offers.push(Rc::new(move || vec![offer.clone()]));
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
        let devices = cx.try_global::<Self>()?;
        let describe = devices
            .describe
            .get(tool)
            .or(devices.describe_others.as_ref());
        describe?.clone()(project, id)
    }

    /// The [`Numbers`] of the tool of `id`, when it registered them.
    fn numbers_of(project: &Project, id: &InstanceId, cx: &App) -> Option<Rc<dyn ToolNumbers>> {
        let devices = cx.try_global::<Self>()?;
        devices.numbers.get(project.tool_of(id)?).cloned()
    }

    /// What a person reads for the number `field` of `id`, when its tool names it, see
    /// [`Numbers::name`].
    pub fn number_name(
        project: &Project,
        id: &InstanceId,
        field: &str,
        cx: &App,
    ) -> Option<String> {
        Self::numbers_of(project, id, cx)?.name(project, id, field)
    }

    /// What a person reads for `value` of the number `field` of `id`, when its tool says, see
    /// [`Numbers::text`].
    pub fn number_text(
        project: &Project,
        id: &InstanceId,
        field: &str,
        value: f32,
        cx: &App,
    ) -> Option<String> {
        Self::numbers_of(project, id, cx)?.text(project, id, field, value)
    }

    /// Every number of `id` a lane can be added for, when its tool says, see
    /// [`Numbers::lane_numbers`]. `None` when it does not, and its behaviour names them.
    pub fn lane_numbers(project: &Project, id: &InstanceId, cx: &App) -> Option<Vec<LaneNumber>> {
        Some(Self::numbers_of(project, id, cx)?.lane_numbers(project, id))
    }

    /// Puts the record of `id` that lets a lane move `field` into `changes`, and gives the
    /// value the lane starts at, see [`Numbers::take`].
    pub fn take_number(
        project: &Project,
        id: &InstanceId,
        field: &str,
        changes: &mut Changes,
        cx: &App,
    ) -> Option<f32> {
        Self::numbers_of(project, id, cx)?.take(project, id, field, changes)
    }
}

/// Whether `project.json` lists this extension under `extensions`, which is what decides
/// whether the project can load a record of its tools.
pub fn extension_is_enabled(project: &Project, extension: &str) -> bool {
    let enabled = &project.project_file().extensions;
    enabled.iter().any(|it| it == extension)
}
