//! The VST 3 side of a plugin's own window: `IPlugView` in a window of ours.
//!
//! The host machinery is in `host.rs` and `window.rs` and knows no format. What a plugin has to
//! do for a window is [`crate::backend::PluginGui`], and this is the VST 3 answer to it. The
//! CLAP answer is in `clap.rs`; neither format changed the machinery.
//!
//! What the format says, from `pluginterfaces/gui/iplugview.h`:
//!
//! - The view comes from the plugin's edit controller, `createView(ViewType::kEditor)`. A
//!   controller that has no window answers with nothing.
//! - `isPlatformTypeSupported(kPlatformTypeNSView)` is how a host asks whether the plugin can
//!   put its view in a Cocoa view of ours. On macOS the coordinates of a `ViewRect` are
//!   logical, so no scaling is needed, which is why nothing here sets one.
//! - `setFrame` before `attached`: "Note that in this call the plug-in could call a
//!   IPlugFrame::resizeView ()". So the frame is in place before the view has a parent.
//! - `attached(parent, kPlatformTypeNSView)` puts the plugin's view in ours. `removed()` takes
//!   it out again, and only a view that was attached may be removed.
//! - A plugin that wants another size calls `IPlugFrame::resizeView`, and then, in the words of
//!   the header, "Afterwards, in the same callstack, the host has to call IPlugView::onSize ()
//!   if a resize is needed". [`PlugFrame::resizeView`] does exactly that, and leaves the window
//!   itself to the next poll, which is the one place that has the application.
//!
//! Every call here belongs to the thread the user interface lives on, which is the thread
//! [`crate::Plugins`] lives on. The one call a plugin makes of its own accord is
//! `IPlugFrame::resizeView`, which VST 3 puts on that same thread; it is written down in an
//! atomic all the same, so a plugin that calls it from elsewhere cannot make this host unsound.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

use vst3::Steinberg::Vst::{IEditController, IEditControllerTrait, ViewType};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeNSView, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use crate::PluginProblem;
use crate::backend::PluginGui;
use crate::processor::not_ours;
use crate::window::WindowSize;

/// The plugin's own window, from the VST 3 side. One of these per loaded plugin that has an
/// edit controller; [`Vst3Gui::is_offered`] says whether that controller really has a window.
pub struct Vst3Gui {
    /// The half of the plugin that makes views. Kept as a reference of its own, so the view
    /// cannot outlive the object it came from.
    controller: ComPtr<IEditController>,
    /// What the plugin asks the host to resize it. It outlives every view: a view is given a
    /// null frame before it is released, and this goes with the plugin.
    frame: ComWrapper<PlugFrame>,
    /// The view while a window holds it. `None` says the plugin holds nothing for a window.
    view: Option<ComPtr<IPlugView>>,
    /// Whether `attached` was answered, so that `removed` is called for that and nothing else.
    attached: bool,
    plugin_id: String,
}

impl Vst3Gui {
    /// The window side of a plugin that has an edit controller. A plugin without one has no
    /// window at all and gets `None`.
    pub fn new(controller: Option<&ComPtr<IEditController>>, plugin_id: &str) -> Option<Self> {
        Some(Self {
            controller: controller?.clone(),
            frame: ComWrapper::new(PlugFrame::default()),
            view: None,
            attached: false,
            plugin_id: plugin_id.to_string(),
        })
    }

    /// A size the plugin asked for since the last call, for [`crate::backend::Requests`].
    pub fn take_wanted_size(&self) -> Option<WindowSize> {
        self.frame.take_wanted_size()
    }

    /// The plugin's editor view, made anew. `None` says this plugin has no window.
    fn make_view(&self) -> Option<ComPtr<IPlugView>> {
        // SAFETY: the controller came from the plugin and is alive, and `kEditor` is a static
        // C string. `createView` gives a view whose reference belongs to this host.
        unsafe {
            let view = not_ours(|| self.controller.createView(ViewType::kEditor));
            ComPtr::from_raw(view)
        }
    }

    fn no_window(&self) -> PluginProblem {
        PluginProblem::NoWindow {
            plugin_id: self.plugin_id.clone(),
        }
    }

    fn refused(&self, call: &str, result: tresult) -> PluginProblem {
        PluginProblem::WindowDidNotOpen {
            plugin_id: self.plugin_id.clone(),
            message: format!("the plugin answered {result} to {call}"),
        }
    }
}

impl PluginGui for Vst3Gui {
    /// Whether this plugin has a window a Cocoa view of ours can hold. The only way the format
    /// has of asking is to make a view and ask it, so one is made and let go of again. It is
    /// asked once, while the plugin loads, and the answer is what a card reads on every frame.
    fn is_offered(&mut self) -> bool {
        self.make_view().is_some_and(|view| supports_nsview(&view))
    }

    fn create(&mut self) -> Result<(), PluginProblem> {
        if self.view.is_some() {
            return Ok(());
        }
        let view = self.make_view().ok_or_else(|| self.no_window())?;
        if !supports_nsview(&view) {
            return Err(self.no_window());
        }
        // The frame goes in before the view has a parent, because a plugin may ask to be
        // resized from inside `attached`, which is the very next call.
        let frame = self
            .frame
            .as_com_ref::<IPlugFrame>()
            .ok_or_else(|| self.no_window())?;
        // SAFETY: the view came from the plugin and is alive. The frame belongs to this object,
        // which outlives the view: `destroy` gives the view a null frame before it is released.
        let result = unsafe { not_ours(|| view.setFrame(frame.as_ptr())) };
        if result != kResultOk && result != kResultTrue {
            return Err(self.refused("setFrame", result));
        }
        self.view = Some(view);
        Ok(())
    }

    fn size(&mut self) -> Option<WindowSize> {
        let view = self.view.as_ref()?;
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: the view came from the plugin and is alive, and `rect` outlives the call.
        let result = unsafe { not_ours(|| view.getSize(&mut rect)) };
        if result != kResultOk && result != kResultTrue {
            return None;
        }
        window_size(&rect)
    }

    unsafe fn set_parent(&mut self, parent: NonNull<c_void>) -> Result<(), PluginProblem> {
        let Some(view) = self.view.as_ref() else {
            return Err(self.no_window());
        };
        // SAFETY: the view is alive, and the caller says `parent` is an `NSView` that lives
        // until `destroy` has run, which is what `removed` needs.
        let result = unsafe { not_ours(|| view.attached(parent.as_ptr(), kPlatformTypeNSView)) };
        if result != kResultOk && result != kResultTrue {
            return Err(self.refused("attached", result));
        }
        self.attached = true;
        Ok(())
    }

    /// VST 3 has no separate show: a view is on screen as soon as it is attached to a parent
    /// that is on screen, which is the window this host has just made for it.
    fn show(&mut self) -> Result<(), PluginProblem> {
        Ok(())
    }

    fn destroy(&mut self) {
        let Some(view) = self.view.take() else {
            return;
        };
        // A size the plugin asked for while it was in a window is nothing to anybody now.
        let _wanted = self.frame.take_wanted_size();
        // SAFETY: the view is alive and is not in anybody else's hands. `removed` is called for
        // an `attached` that was answered and for nothing else, which is what the format asks.
        unsafe {
            if std::mem::take(&mut self.attached) {
                not_ours(|| view.removed());
            }
            // The plugin lets go of this host's frame before the view that holds it goes, so
            // nothing of the plugin's can reach an object of ours that is not there any more.
            not_ours(|| view.setFrame(std::ptr::null_mut()));
        }
        // Dropping the pointer releases the view, which is the last thing the plugin holds.
        drop(view);
    }
}

/// What the plugin asks the host to do with the window its view is in.
///
/// It is one object per plugin and lives as long as the plugin does. A view is given a null
/// frame before it is released, so no plugin holds this after the host has let go of it.
#[derive(Default)]
pub struct PlugFrame {
    /// A size the plugin asked for, packed into one number. Zero means none. Packed and not two
    /// fields, so a width and a height are always the pair the plugin asked for.
    wanted: AtomicU64,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl PlugFrame {
    /// The size the plugin last asked for, once.
    fn take_wanted_size(&self) -> Option<WindowSize> {
        unpack(self.wanted.swap(0, Ordering::AcqRel))
    }
}

impl IPlugFrameTrait for PlugFrame {
    /// The plugin wants its window to be another size.
    ///
    /// The format asks the host to answer `onSize` in the same callstack, so that the plugin
    /// resizes the view it made; the window around it is given the size at the next poll, which
    /// is the one place that has the application. See the module documentation.
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller gives one rectangle that lives for this call.
        let Some(wanted) = window_size(unsafe { &*new_size }) else {
            return kInvalidArgument;
        };
        // SAFETY: the caller gives the view it is asking about, alive for this call. The
        // rectangle is the caller's own and is only read by the plugin it came from.
        unsafe {
            if let Some(view) = ComRef::from_raw(view) {
                not_ours(|| view.onSize(new_size));
            }
        }
        self.wanted.store(pack(wanted), Ordering::Release);
        kResultOk
    }
}

/// Whether a view can live in a Cocoa view of ours, which is the only kind this host makes.
fn supports_nsview(view: &ComPtr<IPlugView>) -> bool {
    // SAFETY: the view came from the plugin and is alive, and the type is a static C string.
    let result = unsafe { not_ours(|| view.isPlatformTypeSupported(kPlatformTypeNSView)) };
    result == kResultOk || result == kResultTrue
}

/// A window size out of a view rectangle. `None` says the rectangle is empty, which is no size
/// for a window.
fn window_size(rect: &ViewRect) -> Option<WindowSize> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(WindowSize {
        width: width as u32,
        height: height as u32,
    })
}

/// A size as one number, so that the width and the height a plugin asked for are read together.
fn pack(size: WindowSize) -> u64 {
    (u64::from(size.width) << 32) | u64::from(size.height)
}

fn unpack(packed: u64) -> Option<WindowSize> {
    if packed == 0 {
        return None;
    }
    Some(WindowSize {
        width: (packed >> 32) as u32,
        height: packed as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_the_plugin_asked_for_comes_back_as_it_was_asked() {
        let size = WindowSize {
            width: 1234,
            height: 567,
        };
        assert_eq!(unpack(pack(size)), Some(size));
        assert_eq!(unpack(0), None);
    }

    #[test]
    fn a_view_rectangle_is_read_as_its_width_and_height_and_an_empty_one_is_no_size() {
        let rect = ViewRect {
            left: 10,
            top: 20,
            right: 330,
            bottom: 260,
        };
        assert_eq!(
            window_size(&rect),
            Some(WindowSize {
                width: 320,
                height: 240
            })
        );
        let empty = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        assert_eq!(window_size(&empty), None);
        let backwards = ViewRect {
            left: 10,
            top: 10,
            right: 0,
            bottom: 0,
        };
        assert_eq!(window_size(&backwards), None);
    }
}
