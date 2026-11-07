//! What the host needs of a plugin. One backend per format fills it in: `clap.rs` and `vst3/`.

use std::ffi::c_void;
use std::ptr::NonNull;

use gpui::Keystroke;
use sound_core::PrepareConfig;

use crate::PluginProblem;
use crate::parameters::Parameter;
use crate::processor::Started;
use crate::window::WindowSize;

/// One loaded plugin, seen from the main thread.
pub(crate) trait LoadedPlugin {
    fn poll(&mut self) -> Requests;

    fn save_state(&mut self) -> Result<Vec<u8>, String>;

    /// `None` for a plugin that can have no window at all.
    fn gui(&mut self) -> Option<&mut dyn PluginGui>;

    /// `None` when the plugin cannot say, as a CLAP plugin for an id it does not have.
    fn value(&mut self, id: u32) -> Option<f64>;

    fn text(&mut self, id: u32, value: f64) -> Option<String>;

    fn parameters(&mut self) -> Vec<Parameter>;

    /// Reaches the processor at the start of a block. What does not fit on the way waits for
    /// the next poll, so the last value sent always arrives.
    fn send(&mut self, change: ParameterChange);

    /// Shows the plugin's own window a value a lane plays. A VST 3 controller hears nothing of
    /// what the processor is given. `false` where the window follows the processor by itself.
    fn show(&mut self, _change: ParameterChange) -> bool {
        false
    }

    /// Whether every value sent has been played, so what [`Self::value`] says is the plugin's
    /// own. Stays false for a plugin whose block failed, so nothing of it is read.
    fn sent_values_played(&mut self) -> bool;

    /// Since the last call.
    fn hand(&mut self) -> Hand;

    /// Lets the plugin go once the engine has given its audio side back; `false` until then.
    /// Dropping it earlier would leave the two ends of one plugin in different hands.
    fn released(&mut self) -> bool;

    /// Deactivates the plugin and activates it again, and gives the new audio side, with the
    /// latency and buses as they are now. `None` until the engine has given the audio side back.
    ///
    /// `pins` are given while it is inactive, so a pin that changed while it waited is not
    /// glided to.
    fn restart(
        &mut self,
        config: PrepareConfig,
        pins: &[ParameterChange],
    ) -> Option<Result<Box<dyn Started>, PluginProblem>>;
}

/// What a plugin asked for since the last poll. It may ask from any thread, so a backend only
/// notes it and the poll acts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Requests {
    /// CLAP's `request_restart`, VST 3's `kLatencyChanged` and `kIoChanged`.
    pub restart: bool,
    /// VST 3's `kReloadComponent`.
    pub reload: bool,
    pub state_is_dirty: bool,
    /// VST 3 only: the pedal goes through a mapping the plugin can change, and one that moved
    /// to another parameter is simply followed.
    pub pedal_unmapped: bool,
    /// CLAP only: in VST 3 the host owns the window and the plugin only fills it.
    pub window_closed: bool,
    pub window_size: Option<WindowSize>,
    /// CLAP's `rescan`, VST 3's `kParamIDMappingChanged` and `kParamTitlesChanged`.
    pub parameters_changed: bool,
}

/// In the format's own units.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ParameterChange {
    pub id: u32,
    pub value: f64,
}

/// Where the composer's hand is in a plugin's own window. One for the whole plugin, though
/// VST 3 says it per parameter: a hand that lets go and takes a knob again between two polls is
/// one turn, and so one undo step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Hand {
    /// VST 3's `beginEdit` without its `endEdit` yet.
    Held,
    /// VST 3's `endEdit`.
    LetGo,
    /// The plugin does not say: CLAP, and a plugin moving a parameter itself.
    Unknown,
}

pub(crate) trait PluginGui {
    /// Whether this plugin can put its view in a window of ours on this platform.
    fn is_offered(&mut self) -> bool;

    /// Never called twice without a [`Self::destroy`] in between.
    fn create(&mut self) -> Result<(), PluginProblem>;

    fn size(&mut self) -> Option<WindowSize>;

    /// Physical pixels per logical pixel. Only on Windows, where both formats count physical
    /// pixels; never on macOS, where CLAP forbids it. A plugin may ignore it.
    fn set_scale(&mut self, scale: f64);

    /// # Safety
    ///
    /// `view` must be a view of this platform, an `NSView` or an `HWND`, that stays alive
    /// until [`Self::destroy`] has run.
    unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), PluginProblem>;

    /// VST 3 has no such call: a view is on screen as soon as it is attached.
    fn show(&mut self) -> Result<(), PluginProblem>;

    /// Asked once the view is made.
    fn can_resize(&mut self) -> bool;

    /// The composer dragged the window to `wanted`. Gives the size the plugin took of it, for
    /// the window to end on.
    fn resize(&mut self, wanted: WindowSize) -> Option<WindowSize>;

    /// A key the window got while the plugin's own view did not have the keyboard. `true` says
    /// the plugin used it. Only VST 3 has a call for this; a CLAP plugin takes the keyboard in
    /// its own view, through the system.
    fn key(&mut self, _keystroke: &Keystroke, _direction: KeyDirection) -> bool {
        false
    }

    fn destroy(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyDirection {
    Down,
    Up,
}

pub(crate) struct Opening {
    pub plugin: Box<dyn LoadedPlugin>,
    /// The audio side, for the engine.
    pub started: Box<dyn Started>,
    /// What stays true while it plays, such as a sustain pedal that cannot reach it.
    pub notes: Vec<PluginProblem>,
}
