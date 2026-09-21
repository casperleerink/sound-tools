//! The plugin's own window.
//!
//! A hosted plugin draws its own interface. CLAP offers two ways: the plugin embeds its view
//! in a window of ours, or it makes a floating window of its own. We ask for the floating one.
//! It is the way CLAP says every plugin must support, and it means the plugin owns its window
//! whole: its size, its resizing and its layout are the plugin's, and there is nothing of ours
//! to keep in step. Nothing of a plugin's window is saved: where it was and whether it was
//! open are not part of the piece.
//!
//! Every call here belongs to the main thread, which is where [`crate::Plugins`] lives. The one
//! call that does not is `clap_host_gui.closed`, which a plugin may make from any thread when
//! its window goes; it only sets a flag that the next poll reads.

use std::ffi::CString;

use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiError, PluginGui};
use clack_host::prelude::*;

use crate::PluginProblem;
use crate::host::SoundToolsHost;

/// The windowing API of this machine, asked for as a floating window.
fn configuration() -> Option<GuiConfiguration<'static>> {
    Some(GuiConfiguration {
        api_type: GuiApiType::default_for_current_platform()?,
        is_floating: true,
    })
}

/// The plugin's window, from the host's side: whether the plugin's resources for it exist.
///
/// `created` is the whole state. A window that is open was created and shown; one that is
/// closed was destroyed. There is no hidden window, so a plugin that keeps its own state
/// keeps it in itself and not in a window we hold on to.
#[derive(Default)]
pub(crate) struct PluginWindow {
    created: bool,
}

impl PluginWindow {
    pub fn is_open(&self) -> bool {
        self.created
    }

    /// Whether this plugin has a window at all. A plugin without one is ordinary: it has no
    /// interface of its own, and the card in the rack says so instead of offering to open it.
    pub fn is_offered(instance: &mut PluginInstance<SoundToolsHost>) -> bool {
        let Some(gui) = gui_of(instance) else {
            return false;
        };
        let Some(configuration) = configuration() else {
            return false;
        };
        gui.is_api_supported(&instance.plugin_handle(), configuration)
    }

    /// Opens the plugin's window, or brings the one that is open forward. CLAP's order for a
    /// floating window: create, suggest a title, show.
    pub fn open(
        &mut self,
        instance: &mut PluginInstance<SoundToolsHost>,
        plugin_id: &str,
        title: &str,
    ) -> Result<(), PluginProblem> {
        let no_window = || PluginProblem::NoWindow {
            plugin_id: plugin_id.to_string(),
        };
        let failed = |error: GuiError| PluginProblem::WindowDidNotOpen {
            plugin_id: plugin_id.to_string(),
            message: error.to_string(),
        };
        let gui = gui_of(instance).ok_or_else(no_window)?;
        let configuration = configuration().ok_or_else(no_window)?;
        if self.created {
            // Already there. `show` on a window that is shown is what brings it forward.
            return gui.show(&instance.plugin_handle()).map_err(failed);
        }
        if !gui.is_api_supported(&instance.plugin_handle(), configuration) {
            return Err(no_window());
        }
        gui.create(&instance.plugin_handle(), configuration)
            .map_err(failed)?;
        // From here the plugin holds resources, so every way out goes through `close`.
        self.created = true;
        if let Ok(title) = CString::new(title) {
            gui.suggest_title(&instance.plugin_handle(), &title);
        }
        if let Err(error) = gui.show(&instance.plugin_handle()) {
            self.close(instance);
            return Err(failed(error));
        }
        Ok(())
    }

    /// Frees the plugin's window. It is also how the host acknowledges a window the plugin
    /// closed by itself, which CLAP asks for. Nothing of the plugin's sound or state is
    /// touched: a plugin goes on playing with no window.
    pub fn close(&mut self, instance: &mut PluginInstance<SoundToolsHost>) {
        if !std::mem::take(&mut self.created) {
            return;
        }
        if let Some(gui) = gui_of(instance) {
            gui.destroy(&instance.plugin_handle());
        }
    }
}

fn gui_of(instance: &mut PluginInstance<SoundToolsHost>) -> Option<PluginGui> {
    instance.plugin_shared_handle().get_extension::<PluginGui>()
}
