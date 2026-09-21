//! What the host needs of a plugin, whatever its format.
//!
//! `host.rs` holds the table, the saving rule, the problems and the windows and knows no
//! format. One backend per format fills these in: `clap.rs` and `vst3/`. A format that cannot
//! do something says so here rather than in the table; a VST 3 plugin has no window before step
//! 5b, so its [`LoadedPlugin::gui`] is `None` and the card says the plugin has no window.

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::PluginProblem;
use crate::processor::Started;
use crate::window::WindowSize;

/// One plugin that is loaded, seen from the thread the project lives on.
pub trait LoadedPlugin {
    /// Main-thread work the plugin asked for since the last call, and what it wants of its
    /// window. Whoever polls acts on it.
    fn poll(&mut self) -> Requests;

    /// The plugin's own state as opaque bytes, for its state asset.
    fn save_state(&mut self) -> Result<Vec<u8>, String>;

    /// The plugin's own window, when it offers one.
    fn gui(&mut self) -> Option<&mut dyn PluginGui>;

    /// Lets the plugin go, when the engine has given its audio side back. `false` says it has
    /// not, and the caller keeps the plugin and asks again at the next poll. Dropping a plugin
    /// whose audio side is still in the engine would leave the two ends in different hands.
    fn released(&mut self) -> bool;
}

/// What a plugin asked for since the last poll. All of it may be asked for from another
/// thread, so a backend only notes it and the poll on the main thread acts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requests {
    /// The plugin asked to be deactivated and activated again. This build does not.
    pub restart: bool,
    /// The plugin says its own state changed and the host should save it.
    pub state_is_dirty: bool,
    /// The plugin closed its own window, by its title bar or by losing it.
    pub window_closed: bool,
    /// A size the plugin asked its window to be.
    pub window_size: Option<WindowSize>,
}

/// What a plugin's own window needs from the plugin. One window of the application holds one
/// plugin's view, see `window.rs`.
pub trait PluginGui {
    /// Whether this plugin can put its view in a window of ours on this platform.
    fn is_offered(&mut self) -> bool;

    /// The plugin makes what it needs for a window. Never called twice without a
    /// [`Self::destroy`] in between.
    fn create(&mut self) -> Result<(), PluginProblem>;

    /// How big the plugin wants its window, if it says.
    fn size(&mut self) -> Option<WindowSize>;

    /// Puts the plugin's view inside `view`.
    ///
    /// # Safety
    ///
    /// `view` must be an `NSView` that stays alive until [`Self::destroy`] has run.
    unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), PluginProblem>;

    /// Shows the plugin's view.
    fn show(&mut self) -> Result<(), PluginProblem>;

    /// Frees everything the plugin made for its window. Its sound and its state are untouched.
    fn destroy(&mut self);
}

/// A plugin that loaded: the two ends of it, and what to report about it while it plays.
pub struct Opening {
    /// The control side, for the table.
    pub plugin: Box<dyn LoadedPlugin>,
    /// The audio side, for the engine.
    pub started: Box<dyn Started>,
    /// What stays true about this plugin while it plays, such as a plugin the sustain pedal
    /// cannot reach. None of these stops it from sounding.
    pub notes: Vec<PluginProblem>,
}
