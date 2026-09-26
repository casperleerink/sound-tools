//! The VST 3 side of a plugin's own window: `IPlugView` in a window of ours.
//!
//! The host machinery is in `host.rs` and `window.rs` and knows no format. What a plugin has to
//! do for a window is [`crate::backend::PluginGui`], and this is the VST 3 answer to it. The
//! CLAP answer is in `clap.rs`; neither format changed the machinery.
//!
//! What the format says, from `pluginterfaces/gui/iplugview.h`:
//!
//! - The view comes from the plugin's edit controller, `createView(ViewType::kEditor)`. A
//!   controller that has no window answers with nothing, and then the card stops offering one.
//!   No view is ever made outside the composer's open: asking a plugin whether it has a window
//!   means building its whole interface, which is up to a second, see ARCHITECTURE.md.
//! - `isPlatformTypeSupported(kPlatformTypeNSView)` is how a host asks whether the plugin can
//!   put its view in a Cocoa view of ours. On macOS the coordinates of a `ViewRect` are
//!   logical, so no scaling is needed, which is why nothing here sets one.
//! - `setFrame` before `attached`: "Note that in this call the plug-in could call a
//!   IPlugFrame::resizeView ()". So the frame is in place before the view has a parent.
//! - `attached(parent, kPlatformTypeNSView)` puts the plugin's view in ours. `removed()` takes
//!   it out again, and only a view that was attached may be removed.
//! - A plugin that wants another size calls `IPlugFrame::resizeView`, and then, in the words of
//!   the header, "Afterwards, in the same callstack, the host has to call IPlugView::onSize ()
//!   if a resize is needed (size was changed)". [`PlugFrame::resizeView`] does exactly that, and
//!   leaves the window itself to the next poll, which is the one place that has the application.
//!   The shape of it is Steinberg's own `editorhost.cpp`: refuse a request for a view that is
//!   not the one this frame holds, refuse a request made from inside another one, say nothing
//!   to a view that already has the size that was asked for, and read the view's own size
//!   afterwards, because that is what the window has to end on whatever happened in between.
//!   Without the guard a plugin that answers `onSize` with the same request runs the host out
//!   of stack.
//!
//! Every call here belongs to the thread the user interface lives on, which is the thread
//! [`crate::Plugins`] lives on. The one call a plugin makes of its own accord is
//! `IPlugFrame::resizeView`, which VST 3 puts on that same thread; it is written down in an
//! atomic all the same, so a plugin that calls it from elsewhere cannot make this host unsound.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

use vst3::Steinberg::Vst::{IEditController, IEditControllerTrait, ViewType};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeNSView, kResultFalse, kResultOk, kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use crate::PluginProblem;
use gpui::Keystroke;

use crate::backend::{KeyDirection, PluginGui};
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
    /// A plugin with an edit controller is offered a window, without being asked.
    ///
    /// The format's only way of asking is `createView`, which builds the plugin's whole
    /// interface: up to a second for one of the pianos this was measured on, on the thread that
    /// draws, for every load in every mode, including the ones that can open no window at all.
    /// Nearly every instrument has a window, so the card offers one and a plugin that turns out
    /// to have none says so once and is not offered one again this session. See ARCHITECTURE.md.
    fn is_offered(&mut self) -> bool {
        true
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
        // resized from inside `attached`, which is the very next call. The frame is told which
        // view it belongs to first, so a request that names another one is refused.
        let frame = self
            .frame
            .as_com_ref::<IPlugFrame>()
            .ok_or_else(|| self.no_window())?;
        self.frame.holds(view.as_ptr());
        // SAFETY: the view came from the plugin and is alive. The frame belongs to this object,
        // which outlives the view: `destroy` gives the view a null frame before it is released.
        let result = unsafe { not_ours(|| view.setFrame(frame.as_ptr())) };
        if result != kResultOk && result != kResultTrue {
            self.frame.holds(std::ptr::null_mut());
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

    fn can_resize(&mut self) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        // SAFETY: the view came from the plugin and is alive.
        let result = unsafe { not_ours(|| view.canResize()) };
        result == kResultOk || result == kResultTrue
    }

    /// `checkSizeConstraint` makes a size the view takes of the one the composer dragged to,
    /// and `onSize` gives it that size, unless it has it already. What the view says it is
    /// afterwards is what the window ends on. A plugin that asks for another size from inside
    /// `onSize` is answered by the frame as any request is.
    fn resize(&mut self, wanted: WindowSize) -> Option<WindowSize> {
        let view = self.view.as_ref()?;
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: i32::try_from(wanted.width).ok()?,
            bottom: i32::try_from(wanted.height).ok()?,
        };
        // SAFETY: the view came from the plugin and is alive, and `rect` outlives the call. A
        // view that does not constrain leaves the rectangle as it was, which is then the size.
        unsafe { not_ours(|| view.checkSizeConstraint(&mut rect)) };
        let offered = window_size(&rect)?;
        // The frame counts this as a request being answered, so a plugin that asks for another
        // size from inside this `onSize` is refused, as a request from inside the answer to one
        // of its own is: a nested `onSize` is what `editorhost.cpp` guards against. The view's
        // own size afterwards is what the window ends on either way.
        if self.frame.answering.swap(true, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: as above.
        let size = unsafe {
            let view = view.as_com_ref();
            if PlugFrame::size_of(view) != Some(offered) {
                not_ours(|| view.onSize(&mut rect));
            }
            PlugFrame::size_of(view)
        };
        self.frame.answering.store(false, Ordering::Release);
        size
    }

    fn key(&mut self, keystroke: &Keystroke, direction: KeyDirection) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        let Vst3Key {
            character,
            code,
            modifiers,
        } = vst3_key(keystroke);
        // SAFETY: the view came from the plugin and is alive.
        let result = unsafe {
            not_ours(|| match direction {
                KeyDirection::Down => view.onKeyDown(character, code, modifiers),
                KeyDirection::Up => view.onKeyUp(character, code, modifiers),
            })
        };
        result == kResultOk || result == kResultTrue
    }

    /// The order is the one Steinberg's `editorhost.cpp` takes in `closePlugView`: the frame
    /// goes first, then `removed`, then the release. The frame first, because a plugin is
    /// allowed to ask for a resize from inside `removed` and there must be nothing of ours left
    /// for it to ask; `removed` only for an `attached` that was answered, which is what the
    /// format says the call is the other half of.
    fn destroy(&mut self) {
        let Some(view) = self.view.take() else {
            return;
        };
        // A size the plugin asked for while it was in a window is nothing to anybody now.
        let _wanted = self.frame.take_wanted_size();
        self.frame.holds(std::ptr::null_mut());
        // SAFETY: the view is alive and is not in anybody else's hands.
        unsafe {
            // The plugin lets go of this host's frame before anything else, so nothing of the
            // plugin's can reach an object of ours while the view is being taken apart.
            not_ours(|| view.setFrame(std::ptr::null_mut()));
            if std::mem::take(&mut self.attached) {
                not_ours(|| view.removed());
            }
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
    /// The view this frame was given to. A request that names another view is refused, as
    /// `editorhost.cpp` refuses one, so this host never calls into a view it does not own.
    view: AtomicPtr<IPlugView>,
    /// Whether a request is being answered. A plugin that asks again from inside `onSize` is
    /// refused instead of being let into this call a second time.
    answering: AtomicBool,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl PlugFrame {
    /// The size the plugin last asked for, once.
    fn take_wanted_size(&self) -> Option<WindowSize> {
        unpack(self.wanted.swap(0, Ordering::AcqRel))
    }

    /// Says which view this frame belongs to, or none while it belongs to no view.
    fn holds(&self, view: *mut IPlugView) {
        self.view.store(view, Ordering::Release);
    }

    /// The size of a view, when it says.
    ///
    /// # Safety
    ///
    /// `view` must be alive.
    unsafe fn size_of(view: ComRef<'_, IPlugView>) -> Option<WindowSize> {
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: the caller keeps the contract, and the rectangle outlives the call.
        let result = unsafe { not_ours(|| view.getSize(&mut rect)) };
        match result == kResultOk || result == kResultTrue {
            true => window_size(&rect),
            false => None,
        }
    }
}

impl IPlugFrameTrait for PlugFrame {
    /// The plugin wants its window to be another size.
    ///
    /// The format asks the host to answer `onSize` in the same callstack, so that the plugin
    /// resizes the view it made; the window around it is given the size at the next poll, which
    /// is the one place that has the application. See the module documentation.
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() || view.is_null() || view != self.view.load(Ordering::Acquire) {
            return kInvalidArgument;
        }
        // SAFETY: the caller gives one rectangle that lives for this call.
        let Some(wanted) = window_size(unsafe { &*new_size }) else {
            return kInvalidArgument;
        };
        // A request made from inside the answer to another one. Refused, as `editorhost.cpp`
        // refuses it; the view's own size below is what the window ends on, so a plugin that
        // insists on another size still gets it.
        if self.answering.swap(true, Ordering::AcqRel) {
            return kResultFalse;
        }
        // SAFETY: the view is the one this frame was given to, which `Vst3Gui` holds a
        // reference to for as long as the frame names it, so it is alive. The rectangle is the
        // caller's own and is only read by the plugin it came from.
        let settled = unsafe {
            let Some(view) = ComRef::from_raw(view) else {
                self.answering.store(false, Ordering::Release);
                return kInvalidArgument;
            };
            // Nothing is said to a view that already has the size that was asked for, which is
            // the check `editorhost.cpp` makes before it touches anything.
            if Self::size_of(view) != Some(wanted) {
                not_ours(|| view.onSize(new_size));
            }
            self.answering.store(false, Ordering::Release);
            // What the view really is now. A plugin may have asked for another size from
            // inside `onSize`, and then this is that size and not the one this call carried.
            Self::size_of(view).unwrap_or(wanted)
        };
        self.wanted.store(pack(settled), Ordering::Release);
        kResultOk
    }
}

/// A key as `IPlugView::onKeyDown` takes it, read from `pluginterfaces/gui/iplugview.h` and
/// `keycodes.h`: the character the key types, a virtual key code for a key that types none,
/// and the modifiers. A space is both, `' '` and `KEY_SPACE`, as `keycodes.h` converts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vst3Key {
    character: u16,
    code: i16,
    modifiers: i16,
}

fn vst3_key(keystroke: &Keystroke) -> Vst3Key {
    use vst3::Steinberg::KeyModifier_::{kAlternateKey, kCommandKey, kControlKey, kShiftKey};
    use vst3::Steinberg::VirtualKeyCodes_::*;
    let code = match keystroke.key.as_str() {
        "backspace" => KEY_BACK,
        "tab" => KEY_TAB,
        "enter" => KEY_RETURN,
        "escape" => KEY_ESCAPE,
        "space" => KEY_SPACE,
        "delete" => KEY_DELETE,
        "insert" => KEY_INSERT,
        "home" => KEY_HOME,
        "end" => KEY_END,
        "pageup" => KEY_PAGEUP,
        "pagedown" => KEY_PAGEDOWN,
        "left" => KEY_LEFT,
        "right" => KEY_RIGHT,
        "up" => KEY_UP,
        "down" => KEY_DOWN,
        "f1" => KEY_F1,
        "f2" => KEY_F2,
        "f3" => KEY_F3,
        "f4" => KEY_F4,
        "f5" => KEY_F5,
        "f6" => KEY_F6,
        "f7" => KEY_F7,
        "f8" => KEY_F8,
        "f9" => KEY_F9,
        "f10" => KEY_F10,
        "f11" => KEY_F11,
        "f12" => KEY_F12,
        _ => 0,
    };
    // The character typed, which has the shift in it already. A key held with the command
    // key types nothing, so its own name is the character then. Only one UTF-16 unit fits.
    let typed = keystroke
        .key_char
        .as_deref()
        .filter(|typed| !typed.is_empty());
    let text = match (code, typed) {
        (KEY_SPACE, _) => " ",
        (0, Some(typed)) => typed,
        (0, None) => keystroke.key.as_str(),
        _ => "",
    };
    let mut units = text.encode_utf16();
    let character = match (units.next(), units.next()) {
        (Some(unit), None) if !char::from_u32(u32::from(unit)).is_some_and(char::is_control) => {
            unit
        }
        _ => 0,
    };
    let held = &keystroke.modifiers;
    let modifiers = [
        (held.shift, kShiftKey),
        (held.alt, kAlternateKey),
        // `keycodes.h`: `kCommandKey` is the Mac's command key, `kControlKey` its control key.
        (held.platform, kCommandKey),
        (held.control, kControlKey),
    ]
    .into_iter()
    .filter(|(down, _)| *down)
    .fold(0, |all, (_, modifier)| all | modifier);
    Vst3Key {
        character,
        code: code as i16,
        modifiers: modifiers as i16,
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

    use std::path::Path;

    use crate::PluginFormat;
    use crate::backend::Opening;
    use crate::scan::ScannedPlugin;

    /// The repository's VST 3 test plugin, loaded, with a log of every call it gets.
    ///
    /// These drive the backend directly, because a window of GPUI's test platform has no
    /// `NSView` to give a plugin: through the host, `attached` and `removed` never run, and
    /// they are where a real plugin does most of what it does for a window.
    fn loaded(folder: &Path, log: &Path) -> Opening {
        // SAFETY: nextest runs one test per process and no thread but this one exists yet.
        unsafe { std::env::set_var(test_plugin_support::LOG_VARIABLE, log) };
        let bundle = test_vst3_plugin::install_into(&folder.join("plugins"));
        let found = ScannedPlugin {
            format: PluginFormat::Vst3,
            id: test_vst3_plugin::PLUGIN_ID.to_string(),
            name: test_vst3_plugin::PLUGIN_NAME.to_string(),
            vendor: "Sound Tools".to_string(),
            version: "0.1.0".to_string(),
            features: vec!["Instrument".to_string()],
            path: bundle,
        };
        let config = sound_core::PrepareConfig {
            sample_rate: 48_000,
            offline: false,
        };
        super::super::load(&found, None, config).expect("the test plugin loads")
    }

    /// Makes the plugin misbehave in one way. Same rules as `loaded`.
    fn tell_the_plugin(variable: &str, value: Option<&str>) {
        // SAFETY: as `loaded`.
        unsafe {
            match value {
                Some(value) => std::env::set_var(variable, value),
                None => std::env::remove_var(variable),
            }
        }
    }

    /// Every call the plugin wrote down, in order.
    fn calls(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split(' ').next().map(str::to_string))
            .filter(|call| call.starts_with("gui_"))
            .collect()
    }

    /// A parent the plugin is given and never touches. It is one live byte of ours, not an
    /// `NSView`: the test plugin only ever checks that it is not null, which is what lets these
    /// run with no display. A real view would use it, and that is checked by hand.
    struct Parent(*mut u8);

    impl Parent {
        fn new() -> Self {
            Self(Box::into_raw(Box::new(0_u8)))
        }

        fn pointer(&self) -> NonNull<c_void> {
            NonNull::new(self.0.cast()).expect("a parent")
        }
    }

    impl Drop for Parent {
        fn drop(&mut self) {
            // SAFETY: made by `new` and given to nobody who keeps it.
            drop(unsafe { Box::from_raw(self.0) });
        }
    }

    /// A view that is attached is taken apart in the order the SDK example takes it: the frame
    /// first, so a plugin cannot ask for a resize in the middle of its own removal, then
    /// `removed`, then the release.
    #[test]
    fn a_view_that_was_attached_is_removed_after_its_frame_goes_and_before_it_is_released() {
        let folder = tempfile::tempdir().expect("a folder");
        let log = folder.path().join("calls.txt");
        let mut opening = loaded(folder.path(), &log);
        let parent = Parent::new();
        let gui = opening.plugin.gui().expect("the plugin has a window");
        gui.create().expect("the view is made");
        // SAFETY: the parent is one live byte and the test plugin never touches it, see above.
        unsafe { gui.set_parent(parent.pointer()) }.expect("the view attaches");
        gui.show().expect("the view is shown");
        gui.destroy();
        assert_eq!(
            calls(&log),
            [
                "gui_create",
                "gui_is_api_supported",
                "gui_set_frame",
                "gui_set_parent",
                "gui_clear_frame",
                "gui_removed",
                "gui_destroy",
            ]
        );
    }

    /// A plugin that refuses its parent. The host says so, and `removed` is the other half of
    /// an `attached` that worked, so it is not called for one that did not.
    #[test]
    fn a_view_that_refused_its_parent_is_not_removed() {
        let folder = tempfile::tempdir().expect("a folder");
        let log = folder.path().join("calls.txt");
        tell_the_plugin(test_plugin_support::ATTACH_FAILS_VARIABLE, Some("1"));
        let mut opening = loaded(folder.path(), &log);
        let parent = Parent::new();
        let gui = opening.plugin.gui().expect("the plugin has a window");
        gui.create().expect("the view is made");
        // SAFETY: as above.
        let refused = unsafe { gui.set_parent(parent.pointer()) };
        let problem = refused.expect_err("the plugin refuses its parent");
        assert!(problem.to_string().contains("attached"), "{problem}");
        gui.destroy();
        let calls = calls(&log);
        assert!(!calls.contains(&"gui_removed".to_string()), "{calls:?}");
        assert_eq!(calls.last().map(String::as_str), Some("gui_destroy"));
        tell_the_plugin(test_plugin_support::ATTACH_FAILS_VARIABLE, None);
    }

    /// `iplugview.h` says a plugin may ask for a resize from inside `attached`. The frame takes
    /// it, and the size waits for whoever polls, which is the only one that has a window.
    #[test]
    fn a_view_may_ask_for_a_resize_from_inside_attached() {
        let folder = tempfile::tempdir().expect("a folder");
        let log = folder.path().join("calls.txt");
        tell_the_plugin(
            test_plugin_support::RESIZE_IN_ATTACHED_VARIABLE,
            Some("900x700"),
        );
        let mut opening = loaded(folder.path(), &log);
        let parent = Parent::new();
        let gui = opening.plugin.gui().expect("the plugin has a window");
        gui.create().expect("the view is made");
        // SAFETY: as above.
        unsafe { gui.set_parent(parent.pointer()) }.expect("the view attaches");
        let calls = calls(&log);
        assert!(
            calls.contains(&"gui_request_resize".to_string()),
            "{calls:?}"
        );
        assert_eq!(
            gui.size(),
            Some(WindowSize {
                width: 900,
                height: 700
            })
        );
        gui.destroy();
        tell_the_plugin(test_plugin_support::RESIZE_IN_ATTACHED_VARIABLE, None);
    }

    fn key(text: &str) -> Vst3Key {
        vst3_key(&Keystroke::parse(text).expect("a keystroke"))
    }

    /// What the host tells a plugin about a key, in the words of `keycodes.h`: the character a
    /// key types, a virtual code for one that types none, and the modifiers. A space is both.
    #[test]
    fn a_key_reaches_a_vst3_view_as_its_character_its_code_and_its_modifiers() {
        use vst3::Steinberg::KeyModifier_::{kAlternateKey, kCommandKey, kControlKey, kShiftKey};
        use vst3::Steinberg::VirtualKeyCodes_::{KEY_BACK, KEY_LEFT, KEY_RETURN, KEY_SPACE};
        let plain = |character: char, code, modifiers| Vst3Key {
            character: character as u16,
            code: code as i16,
            modifiers: modifiers as i16,
        };
        assert_eq!(key("a"), plain('a', 0, 0));
        // `parse` gives no character for a shifted key; the window gives the one it typed.
        let mut shifted = Keystroke::parse("shift-a").expect("a keystroke");
        shifted.key_char = Some("A".to_string());
        assert_eq!(vst3_key(&shifted), plain('A', 0, kShiftKey));
        assert_eq!(key("cmd-c"), plain('c', 0, kCommandKey));
        assert_eq!(
            key("ctrl-alt-x"),
            plain('x', 0, kControlKey | kAlternateKey)
        );
        assert_eq!(key("space"), plain(' ', KEY_SPACE, 0));
        assert_eq!(key("enter"), plain('\0', KEY_RETURN, 0));
        assert_eq!(key("backspace"), plain('\0', KEY_BACK, 0));
        assert_eq!(key("shift-left"), plain('\0', KEY_LEFT, kShiftKey));
        // A character outside the first plane does not fit in one UTF-16 unit.
        let mut emoji = Keystroke::parse("a").expect("a keystroke");
        emoji.key_char = Some("😀".to_string());
        assert_eq!(vst3_key(&emoji), plain('\0', 0, 0));
        // A named key that `keycodes.h` has no code for types nothing either.
        assert_eq!(key("f20"), plain('\0', 0, 0));
    }

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
