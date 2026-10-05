//! What the host needs of a plugin, whatever its format.
//!
//! `host.rs` holds the table, the saving rule, the problems and the windows and knows no
//! format. One backend per format fills these in: `clap.rs` and `vst3/`. A format that cannot
//! do something says so here rather than in the table: [`LoadedPlugin::gui`] is `None` for a
//! plugin that can have no window at all, and the card then says so.

use std::ffi::c_void;
use std::ptr::NonNull;

use gpui::Keystroke;
use sound_core::PrepareConfig;

use crate::PluginProblem;
use crate::parameters::Parameter;
use crate::processor::Started;
use crate::window::WindowSize;

/// One plugin that is loaded, seen from the thread the project lives on.
pub(crate) trait LoadedPlugin {
    /// Main-thread work the plugin asked for since the last call, and what it wants of its
    /// window. Whoever polls acts on it.
    fn poll(&mut self) -> Requests;

    /// The plugin's own state as opaque bytes, for its state asset.
    fn save_state(&mut self) -> Result<Vec<u8>, String>;

    /// The plugin's own window, when it offers one.
    fn gui(&mut self) -> Option<&mut dyn PluginGui>;

    /// The value the parameter `id` has now. `None` when the plugin cannot say, which a CLAP
    /// plugin does for an id it does not have.
    fn value(&mut self, id: u32) -> Option<f64>;

    /// The plugin's own text for `value` of the parameter `id`, such as `1.2 kHz`.
    fn text(&mut self, id: u32, value: f64) -> Option<String>;

    /// Every parameter a host may set, as the plugin lists them now.
    fn parameters(&mut self) -> Vec<Parameter>;

    /// Sends a value to a parameter. It reaches the processor at the start of a block, and
    /// whatever does not fit on the way waits here for the next poll, so the last value sent
    /// always arrives.
    fn send(&mut self, change: ParameterChange);

    /// Shows the plugin's own window the value an automation lane plays, or the record value
    /// again once the lane lets go. Only where the format needs the host for it: a VST 3
    /// controller hears nothing of what the processor is given. `false` where the format does
    /// not, and then the window follows the processor by itself.
    fn show(&mut self, _change: ParameterChange) -> bool {
        false
    }

    /// Whether every value sent has been played by the processor, so that what [`Self::value`]
    /// says now is the plugin's own and not a value still on its way. A plugin whose block
    /// failed plays nothing again, so this stays false for it, which only means nothing of it
    /// is read.
    fn sent_values_played(&mut self) -> bool;

    /// Where the composer's hand is in the plugin's own window, since the last call.
    fn hand(&mut self) -> Hand;

    /// Lets the plugin go, when the engine has given its audio side back. `false` says it has
    /// not, and the caller keeps the plugin and asks again at the next poll. Dropping a plugin
    /// whose audio side is still in the engine would leave the two ends in different hands.
    fn released(&mut self) -> bool;

    /// Deactivates the plugin and activates it again, and gives the new audio side, which
    /// knows the plugin's latency and buses as they are now. It is how both formats let a
    /// plugin change its latency, and how VST 3 lets one change its buses: CLAP's
    /// `request_restart`, VST 3's `kLatencyChanged` and `kIoChanged`.
    ///
    /// `pins` are given to the plugin while it is inactive, as when it loads, so a pin that
    /// changed while it waited is not glided to.
    ///
    /// Only once the engine has given the audio side back, as [`Self::released`]: `None` says
    /// it has not, and the caller asks again at the next poll. An error leaves the plugin
    /// inactive, and it plays nothing until its record changes.
    fn restart(
        &mut self,
        config: PrepareConfig,
        pins: &[ParameterChange],
    ) -> Option<Result<Box<dyn Started>, PluginProblem>>;
}

/// What a plugin asked for since the last poll. All of it may be asked for from another
/// thread, so a backend only notes it and the poll on the main thread acts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Requests {
    /// The plugin asked to be deactivated and activated again, which this host does, and then
    /// reads its latency and its buses again: CLAP's `request_restart`, and VST 3's
    /// `kLatencyChanged` and `kIoChanged`.
    pub restart: bool,
    /// The plugin asked to be unloaded and loaded again: VST 3's `kReloadComponent`. The host
    /// saves it and loads its record again.
    pub reload: bool,
    /// The plugin says its own state changed and the host should save it.
    pub state_is_dirty: bool,
    /// The plugin moved its sustain pedal to no parameter at all, so the pedal no longer
    /// reaches it. VST 3 only: there the pedal goes through a mapping the plugin can change,
    /// and a mapping that moved to another parameter is simply followed.
    pub pedal_unmapped: bool,
    /// The plugin closed its own window, by its title bar or by losing it. CLAP only: VST 3
    /// has no such call, because there the host owns the window and the plugin only fills it.
    pub window_closed: bool,
    /// A size the plugin asked its window to be.
    pub window_size: Option<WindowSize>,
    /// The plugin changed which parameters it has, or what they are called, so the host reads
    /// the list again: CLAP's `rescan`, VST 3's `kParamIDMappingChanged` and
    /// `kParamTitlesChanged`.
    pub parameters_changed: bool,
}

/// One value of one parameter, on its way into a plugin's processor or out of it, in the
/// format's own units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ParameterChange {
    pub id: u32,
    pub value: f64,
}

/// Where the composer's hand is in a plugin's own window, as far as the format says.
///
/// One for the whole plugin, asked once a poll, though VST 3 says it per parameter: a hand that
/// lets go and takes a knob again between two polls is one turn, and so one undo step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hand {
    /// On a knob: VST 3's `beginEdit` without its `endEdit` yet.
    Held,
    /// It let go since the host last asked: VST 3's `endEdit`.
    LetGo,
    /// The plugin does not say: CLAP, and a plugin moving a parameter itself.
    Unknown,
}

/// What a plugin's own window needs from the plugin. One window of the application holds one
/// plugin's view, see `window.rs`.
pub(crate) trait PluginGui {
    /// Whether this plugin can put its view in a window of ours on this platform.
    fn is_offered(&mut self) -> bool;

    /// The plugin makes what it needs for a window. Never called twice without a
    /// [`Self::destroy`] in between: CLAP's `gui.create`, and a VST 3 controller making an
    /// `IPlugView`.
    fn create(&mut self) -> Result<(), PluginProblem>;

    /// How big the plugin wants its window, if it says.
    fn size(&mut self) -> Option<WindowSize>;

    /// Tells the plugin how many physical pixels make one logical pixel of its window. Only on
    /// Windows, where both formats count physical pixels; never on macOS, where CLAP forbids it
    /// and sizes are logical. A plugin that reads the scale from the system itself may ignore
    /// it, which both formats allow. CLAP's `set_scale`, VST 3's `setContentScaleFactor`.
    fn set_scale(&mut self, scale: f64);

    /// Puts the plugin's view inside `view`.
    ///
    /// # Safety
    ///
    /// `view` must be a view of this platform, an `NSView` or an `HWND`, that stays alive
    /// until [`Self::destroy`] has run.
    unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), PluginProblem>;

    /// Shows the plugin's view. VST 3 has no such call: a view is on screen as soon as it is
    /// attached, so its backend answers this with nothing.
    fn show(&mut self) -> Result<(), PluginProblem>;

    /// Whether the composer may resize the window by dragging its edge. Asked once the view is
    /// made: CLAP's `can_resize`, VST 3's `canResize`.
    fn can_resize(&mut self) -> bool;

    /// The composer dragged the window to `wanted`. The plugin makes a size it takes of it and
    /// takes that size, and this gives the size the view is now, for the window to end on.
    /// CLAP's `adjust_size` and `set_size`, VST 3's `checkSizeConstraint` and `onSize`.
    fn resize(&mut self, wanted: WindowSize) -> Option<WindowSize>;

    /// A key the window got while the plugin's own view did not have the keyboard. `true` says
    /// the plugin used it. Only VST 3 has a call for this (`IPlugView::onKeyDown` and
    /// `onKeyUp`); a CLAP plugin takes the keyboard in its own view, through the system.
    fn key(&mut self, _keystroke: &Keystroke, _direction: KeyDirection) -> bool {
        false
    }

    /// Frees everything the plugin made for its window. Its sound and its state are untouched.
    fn destroy(&mut self);
}

/// Whether a key went down or came up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyDirection {
    Down,
    Up,
}

/// A plugin that loaded: the two ends of it, and what to report about it while it plays.
pub(crate) struct Opening {
    /// The control side, for the table.
    pub plugin: Box<dyn LoadedPlugin>,
    /// The audio side, for the engine.
    pub started: Box<dyn Started>,
    /// What stays true about this plugin while it plays, such as a plugin the sustain pedal
    /// cannot reach. None of these stops it from sounding.
    pub notes: Vec<PluginProblem>,
}
