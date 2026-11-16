//! The least of a CLAP host that opens a plugin's editor, in the order plugin-host uses
//! (`extensions/plugin-host/src/clap.rs`). The plugin is never activated: no audio.

use std::ffi::{CString, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui};
use clack_host::prelude::*;

pub struct Host;

impl HostHandlers for Host {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Shared) {
        builder.register::<HostGui>();
    }
}

#[derive(Default)]
pub struct Shared {
    callback_requested: AtomicBool,
    size_wanted: AtomicU64,
}

impl SharedHandler<'_> for Shared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}
    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        self.size_wanted.store(new_size.pack_to_u64(), Ordering::Release);
        Ok(())
    }
    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }
    fn closed(&self, _was_destroyed: bool) {
        crate::log("CLAP plugin says its window closed");
    }
}

pub struct MainThread;
impl MainThreadHandler<'_> for MainThread {}

pub struct Plugin {
    instance: PluginInstance<Host>,
    created: bool,
}

fn configuration() -> GuiConfiguration<'static> {
    GuiConfiguration { api_type: GuiApiType::COCOA, is_floating: false }
}

impl Plugin {
    pub fn load(bundle: &Path) -> Result<Self, String> {
        // SAFETY: loading a plugin runs its code; this is a spike on the author's machine.
        let entry = unsafe { PluginEntry::load(bundle) }.map_err(|e| e.to_string())?;
        let factory = entry.get_plugin_factory().ok_or("no plugin factory")?;
        let descriptor = factory.plugin_descriptors().next().ok_or("no plugin in the bundle")?;
        let id = CString::from(descriptor.id().ok_or("no id")?);
        crate::log(format!("CLAP plugin {id:?}"));
        let info = HostInfo::new("Rune plugin window spike", "Sound Tools", "https://example.invalid", "0.0.0").map_err(|e| e.to_string())?;
        let instance = PluginInstance::<Host>::new(|_| Shared::default(), |_| MainThread, &entry, &id, &info).map_err(|e| e.to_string())?;
        Ok(Self { instance, created: false })
    }

    fn gui(&mut self) -> Result<PluginGui, String> {
        self.instance.plugin_shared_handle().get_extension().ok_or_else(|| "the plugin has no gui".into())
    }

    pub fn create(&mut self) -> Result<(), String> {
        if self.created {
            return Ok(());
        }
        let gui = self.gui()?;
        if !gui.is_api_supported(&self.instance.plugin_handle(), configuration()) {
            return Err("no embedded Cocoa window".into());
        }
        gui.create(&self.instance.plugin_handle(), configuration()).map_err(|e| e.to_string())?;
        self.created = true;
        Ok(())
    }

    pub fn size(&mut self) -> Option<(u32, u32)> {
        let gui = self.gui().ok()?;
        let size = gui.get_size(&self.instance.plugin_handle())?;
        Some((size.width, size.height))
    }

    pub fn can_resize(&mut self) -> bool {
        self.gui().is_ok_and(|gui| gui.can_resize(&self.instance.plugin_handle()))
    }

    /// # Safety
    /// `view` outlives the editor: [`Self::destroy`] runs before it goes.
    pub unsafe fn set_parent(&mut self, view: NonNull<c_void>) -> Result<(), String> {
        let gui = self.gui()?;
        // SAFETY: as the caller promised.
        unsafe {
            let parent = clack_extensions::gui::Window::from_cocoa_nsview(view.as_ptr());
            gui.set_parent(&self.instance.plugin_handle(), parent)
        }
        .map_err(|e| e.to_string())
    }

    pub fn show(&mut self) -> Result<(), String> {
        let gui = self.gui()?;
        gui.show(&self.instance.plugin_handle()).map_err(|e| e.to_string())
    }

    pub fn resize(&mut self, wanted: (u32, u32)) -> Option<(u32, u32)> {
        let gui = self.gui().ok()?;
        let plugin = self.instance.plugin_handle();
        let offered = GuiSize { width: wanted.0, height: wanted.1 };
        let adjusted = gui.adjust_size(&plugin, offered).unwrap_or(offered);
        if let Err(error) = gui.set_size(&plugin, adjusted) {
            crate::log(format!("CLAP refused {}x{}: {error}", adjusted.width, adjusted.height));
        }
        let size = gui.get_size(&plugin)?;
        Some((size.width, size.height))
    }

    pub fn destroy(&mut self) {
        if std::mem::take(&mut self.created) && let Ok(gui) = self.gui() {
            gui.destroy(&self.instance.plugin_handle());
        }
    }

    /// Runs the main-thread callback the plugin asked for and gives the size it asked for.
    pub fn poll(&mut self) -> Option<(u32, u32)> {
        if self.instance.access_shared_handler(|s| s.callback_requested.swap(false, Ordering::AcqRel)) {
            self.instance.call_on_main_thread_callback();
        }
        let size = self.instance.access_shared_handler(|s| s.size_wanted.swap(0, Ordering::AcqRel));
        (size != 0).then(|| {
            let size = GuiSize::unpack_from_u64(size);
            (size.width, size.height)
        })
    }
}
