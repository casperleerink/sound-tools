//! The VST 3 side of a plugin's own window: `IPlugView` in a window of ours.
//!
//! What the format says, from `pluginterfaces/gui/iplugview.h`:
//!
//! - `setFrame` before `attached`, because "in this call the plug-in could call a
//!   IPlugFrame::resizeView ()".
//! - Only a view that was attached may be `removed()`.
//! - After `IPlugFrame::resizeView`, "in the same callstack, the host has to call
//!   IPlugView::onSize () if a resize is needed". [`PlugFrame::resizeView`] does that the way
//!   Steinberg's `editorhost.cpp` does: refuse a view that is not the frame's, refuse a request
//!   from inside another one (a plugin that answers `onSize` with the same request would run
//!   the host out of stack), skip a view that already has the size, and end on the view's own
//!   size. The window follows at the next poll, which is the one place that has the
//!   application.
//! - A `ViewRect` is in logical pixels on macOS and physical pixels on Windows.
//!
//! Every call is on the UI thread, `resizeView` included by the format. The frame still uses
//! atomics, so a plugin that calls from elsewhere cannot make this host unsound.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

use vst3::Steinberg::Vst::{IEditController, IEditControllerTrait, ViewType};
use vst3::Steinberg::{
    FIDString, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument, kResultFalse,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use crate::PluginProblem;
use crate::backend::PluginGui;
use crate::processor::not_ours;
use crate::window::{Key, KeyDirection, Keystroke, WindowSize};

/// The plugin's own window, from the VST 3 side, for a plugin that has an edit controller.
pub(super) struct Vst3Gui {
    /// Kept as a reference of its own, so the view cannot outlive the object it came from.
    controller: ComPtr<IEditController>,
    /// Outlives every view: a view is given a null frame before it is released.
    frame: ComWrapper<PlugFrame>,
    view: Option<ComPtr<IPlugView>>,
    /// Whether `attached` worked, so `removed` is called only then.
    attached: bool,
    plugin_id: String,
}

impl Vst3Gui {
    /// `None` for a plugin with no edit controller, which has no window.
    pub(super) fn new(
        controller: Option<&ComPtr<IEditController>>,
        plugin_id: &str,
    ) -> Option<Self> {
        Some(Self {
            controller: controller?.clone(),
            frame: ComWrapper::new(PlugFrame::default()),
            view: None,
            attached: false,
            plugin_id: plugin_id.to_string(),
        })
    }

    pub(super) fn take_wanted_size(&self) -> Option<WindowSize> {
        self.frame.take_wanted_size()
    }

    /// `None` says this plugin has no window.
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
    /// Offered without asking: the format's only way to ask is `createView`, which builds the
    /// plugin's whole interface (up to a second for some pianos) on the thread that draws.
    /// Nearly every instrument has a window; one that has none says so when it is opened.
    fn is_offered(&mut self) -> bool {
        true
    }

    fn create(&mut self) -> Result<(), PluginProblem> {
        if self.view.is_some() {
            return Ok(());
        }
        let view = self.make_view().ok_or_else(|| self.no_window())?;
        if !supports_our_view(&view) {
            return Err(self.no_window());
        }
        // The frame learns its view first, so a resize from inside `attached` is accepted.
        let frame = self
            .frame
            .as_com_ref::<IPlugFrame>()
            .ok_or_else(|| self.no_window())?;
        self.frame.holds(view.as_ptr());
        // SAFETY: the view came from the plugin and is alive. The frame outlives the view:
        // `destroy` gives the view a null frame before it is released.
        let result = unsafe { not_ours(|| view.setFrame(frame.as_ptr())) };
        if result != kResultOk {
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
        if result != kResultOk {
            return None;
        }
        window_size(&rect)
    }

    fn set_scale(&mut self, scale: f64) {
        let Some(support) = self
            .view
            .as_ref()
            .and_then(|view| view.cast::<IPlugViewContentScaleSupport>())
        else {
            return;
        };
        // SAFETY: the view came from the plugin and is alive. A view without this interface, or
        // that answers no, reads the scale itself, which the format allows.
        unsafe { not_ours(|| support.setContentScaleFactor(scale as f32)) };
    }

    unsafe fn set_parent(&mut self, parent: NonNull<c_void>) -> Result<(), PluginProblem> {
        let Some(view) = self.view.as_ref() else {
            return Err(self.no_window());
        };
        // SAFETY: the view is alive, and the caller says `parent` is a view of this platform
        // that lives until `destroy` has run, which is what `removed` needs.
        let result = unsafe { not_ours(|| view.attached(parent.as_ptr(), platform_type())) };
        if result != kResultOk {
            return Err(self.refused("attached", result));
        }
        self.attached = true;
        Ok(())
    }

    /// VST 3 has no separate show: a view attached to a parent on screen is on screen.
    fn show(&mut self) -> Result<(), PluginProblem> {
        Ok(())
    }

    fn can_resize(&mut self) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        // SAFETY: the view came from the plugin and is alive.
        let result = unsafe { not_ours(|| view.canResize()) };
        result == kResultOk
    }

    /// The window ends on what the view says its size is afterwards.
    fn resize(&mut self, wanted: WindowSize) -> Option<WindowSize> {
        let view = self.view.as_ref()?;
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: i32::try_from(wanted.width).ok()?,
            bottom: i32::try_from(wanted.height).ok()?,
        };
        // SAFETY: the view came from the plugin and is alive, and `rect` outlives the call.
        unsafe { not_ours(|| view.checkSizeConstraint(&mut rect)) };
        let offered = window_size(&rect)?;
        // Counts as answering a request, so a `resizeView` from inside this `onSize` is
        // refused, as `editorhost.cpp` refuses a nested one.
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
        result == kResultOk
    }

    /// The order of Steinberg's `editorhost.cpp` `closePlugView`: the frame first, because a
    /// plugin may ask for a resize from inside `removed`; then `removed`; then the release.
    fn destroy(&mut self) {
        let Some(view) = self.view.take() else {
            return;
        };
        let _wanted = self.frame.take_wanted_size();
        self.frame.holds(std::ptr::null_mut());
        // SAFETY: the view is alive and is not in anybody else's hands.
        unsafe {
            not_ours(|| view.setFrame(std::ptr::null_mut()));
            if std::mem::take(&mut self.attached) {
                not_ours(|| view.removed());
            }
        }
        drop(view);
    }
}

/// What the plugin asks of the window its view is in. One per plugin.
#[derive(Default)]
pub(super) struct PlugFrame {
    /// A size the plugin asked for, packed so the width and height are always one pair. Zero
    /// means none.
    wanted: AtomicU64,
    view: AtomicPtr<IPlugView>,
    answering: AtomicBool,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl PlugFrame {
    fn take_wanted_size(&self) -> Option<WindowSize> {
        unpack(self.wanted.swap(0, Ordering::AcqRel))
    }

    fn holds(&self, view: *mut IPlugView) {
        self.view.store(view, Ordering::Release);
    }

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
        match result == kResultOk {
            true => window_size(&rect),
            false => None,
        }
    }
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() || view.is_null() || view != self.view.load(Ordering::Acquire) {
            return kInvalidArgument;
        }
        // SAFETY: the caller gives one rectangle that lives for this call.
        let Some(wanted) = window_size(unsafe { &*new_size }) else {
            return kInvalidArgument;
        };
        // A request from inside the answer to another one.
        if self.answering.swap(true, Ordering::AcqRel) {
            return kResultFalse;
        }
        // SAFETY: the view is the frame's, which `Vst3Gui` holds for as long as the frame names
        // it. The rectangle is the caller's own.
        let settled = unsafe {
            let Some(view) = ComRef::from_raw(view) else {
                self.answering.store(false, Ordering::Release);
                return kInvalidArgument;
            };
            if Self::size_of(view) != Some(wanted) {
                not_ours(|| view.onSize(new_size));
            }
            self.answering.store(false, Ordering::Release);
            Self::size_of(view).unwrap_or(wanted)
        };
        self.wanted.store(pack(settled), Ordering::Release);
        kResultOk
    }
}

/// A key as `IPlugView::onKeyDown` takes it (`keycodes.h`): the character it types, a virtual
/// key code for a key that types none, and the modifiers. A space is both, as `keycodes.h` has
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vst3Key {
    character: u16,
    code: i16,
    modifiers: i16,
}

fn vst3_key(keystroke: &Keystroke) -> Vst3Key {
    use vst3::Steinberg::KeyModifier_::{kAlternateKey, kCommandKey, kControlKey, kShiftKey};
    use vst3::Steinberg::VirtualKeyCodes_::*;
    let (code, text) = match &keystroke.key {
        Key::Text(text) => (0, text.as_str()),
        Key::Space => (KEY_SPACE, " "),
        Key::Enter => (KEY_RETURN, ""),
        Key::Tab => (KEY_TAB, ""),
        Key::Backspace => (KEY_BACK, ""),
        Key::Delete => (KEY_DELETE, ""),
        Key::Escape => (KEY_ESCAPE, ""),
        Key::Insert => (KEY_INSERT, ""),
        Key::Home => (KEY_HOME, ""),
        Key::End => (KEY_END, ""),
        Key::PageUp => (KEY_PAGEUP, ""),
        Key::PageDown => (KEY_PAGEDOWN, ""),
        Key::Left => (KEY_LEFT, ""),
        Key::Right => (KEY_RIGHT, ""),
        Key::Up => (KEY_UP, ""),
        Key::Down => (KEY_DOWN, ""),
        // F1 to F12 follow each other in `keycodes.h`.
        Key::Function(number @ 1..=12) => (
            KEY_F1 + vst3::Steinberg::VirtualKeyCodes::from(*number) - 1,
            "",
        ),
        Key::Function(_) => (0, ""),
    };
    // Only one UTF-16 unit fits.
    let mut units = text.encode_utf16();
    let character = match (units.next(), units.next()) {
        (Some(unit), None) if !char::from_u32(u32::from(unit)).is_some_and(char::is_control) => {
            unit
        }
        _ => 0,
    };
    let held = keystroke.modifiers;
    // `keycodes.h`: `kCommandKey` is the command key on macOS and the control key on Windows,
    // and `kControlKey` the other one: the control key on macOS, the Windows key on Windows.
    let modifiers = [
        (held.shift, kShiftKey),
        (held.alt, kAlternateKey),
        (held.command, kCommandKey),
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

/// Linux has no plugin windows yet: it asks for an `NSView`, which no plugin there makes, so
/// opening one finds that the plugin has no window.
fn platform_type() -> FIDString {
    if cfg!(target_os = "windows") {
        vst3::Steinberg::kPlatformTypeHWND
    } else {
        vst3::Steinberg::kPlatformTypeNSView
    }
}

fn supports_our_view(view: &ComPtr<IPlugView>) -> bool {
    // SAFETY: the view came from the plugin and is alive, and the type is a static C string.
    let result = unsafe { not_ours(|| view.isPlatformTypeSupported(platform_type())) };
    result == kResultOk
}

/// `None` for an empty rectangle.
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
    use crate::window::Modifiers;

    /// The repository's VST 3 test plugin, loaded, logging every call it gets.
    fn loaded(folder: &Path, log: &Path) -> Opening {
        // SAFETY: nextest runs one test per process and no thread but this one exists yet.
        unsafe { std::env::set_var(test_plugin_support::LOG_VARIABLE, log) };
        let bundle = test_vst3_plugin::install_into(&folder.join("plugins"));
        let found = ScannedPlugin {
            format: PluginFormat::Vst3,
            id: test_vst3_plugin::PLUGIN_ID.to_string(),
            name: test_vst3_plugin::PLUGIN_NAME.to_string(),
            vendor: "Sound Tools".to_string(),
            features: vec!["Instrument".to_string()],
            path: bundle,
        };
        let config = sound_core::PrepareConfig {
            sample_rate: 48_000,
            offline: false,
        };
        super::super::load(&found, None, config, &Default::default())
            .expect("the test plugin loads")
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

    fn calls(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split(' ').next().map(str::to_string))
            .filter(|call| call.starts_with("gui_"))
            .collect()
    }

    /// One live byte, not an `NSView`: the test plugin only checks that it is not null, so these
    /// run with no display.
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

    /// The test view grows with the scale, which shows the call reached it.
    #[test]
    fn a_view_is_told_the_scale_and_its_size_follows() {
        let folder = tempfile::tempdir().expect("a folder");
        let log = folder.path().join("calls.txt");
        let mut opening = loaded(folder.path(), &log);
        let gui = opening.plugin.gui().expect("the plugin has a window");
        gui.create().expect("the view is made");
        gui.set_scale(1.5);
        assert!(
            calls(&log).contains(&"gui_set_scale[1.5]".to_string()),
            "{:?}",
            calls(&log)
        );
        assert_eq!(
            gui.size(),
            Some(WindowSize {
                width: 480,
                height: 360
            })
        );
        gui.destroy();
    }

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

    /// `iplugview.h` allows a resize from inside `attached`.
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

    fn key(key: Key, modifiers: Modifiers) -> Vst3Key {
        vst3_key(&Keystroke { key, modifiers })
    }

    fn text(text: &str) -> Key {
        Key::Text(text.to_string())
    }

    #[test]
    fn a_key_reaches_a_vst3_view_as_its_character_its_code_and_its_modifiers() {
        use vst3::Steinberg::KeyModifier_::{kAlternateKey, kCommandKey, kControlKey, kShiftKey};
        use vst3::Steinberg::VirtualKeyCodes_::{
            KEY_BACK, KEY_F12, KEY_LEFT, KEY_RETURN, KEY_SPACE,
        };
        let plain = |character: char, code, modifiers| Vst3Key {
            character: character as u16,
            code: code as i16,
            modifiers: modifiers as i16,
        };
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..none
        };
        assert_eq!(key(text("a"), none), plain('a', 0, 0));
        assert_eq!(key(text("A"), shift), plain('A', 0, kShiftKey));
        let command = Modifiers {
            command: true,
            ..none
        };
        assert_eq!(key(text("c"), command), plain('c', 0, kCommandKey));
        let control_alt = Modifiers {
            control: true,
            alt: true,
            ..none
        };
        assert_eq!(
            key(text("x"), control_alt),
            plain('x', 0, kControlKey | kAlternateKey)
        );
        assert_eq!(key(Key::Space, none), plain(' ', KEY_SPACE, 0));
        assert_eq!(key(Key::Enter, none), plain('\0', KEY_RETURN, 0));
        assert_eq!(key(Key::Backspace, none), plain('\0', KEY_BACK, 0));
        assert_eq!(key(Key::Left, shift), plain('\0', KEY_LEFT, kShiftKey));
        assert_eq!(key(Key::Function(12), none), plain('\0', KEY_F12, 0));
        // A character outside the first plane does not fit in one UTF-16 unit.
        assert_eq!(key(text("😀"), none), plain('\0', 0, 0));
        // A function key that `keycodes.h` has no code for types nothing either.
        assert_eq!(key(Key::Function(20), none), plain('\0', 0, 0));
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
