//! What Rune does not give for plugin windows: the displays, whether the app is active, floating
//! above the main window, and handing the keyboard back.

use plugin_host::Display;

#[cfg(target_os = "macos")]
pub(crate) use mac::*;
#[cfg(target_os = "windows")]
pub(crate) use win::*;

/// The main window of the app, which a plugin's window floats above.
#[cfg(not(target_os = "windows"))]
pub(crate) type Owner = ();

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod other {
    use super::*;

    pub(crate) fn displays() -> Vec<Display> {
        Vec::new()
    }

    pub(crate) fn app_is_active() -> bool {
        true
    }

    pub(crate) fn owner() -> Owner {}

    pub(crate) fn float(_: &rune::raw_window_handle::RawWindowHandle, (): Owner) {}

    pub(crate) fn keyboard() -> Option<usize> {
        None
    }

    pub(crate) fn give_keyboard_to(_: usize) {}
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) use other::*;

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use rune::raw_window_handle::RawWindowHandle;

    use super::*;

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayBounds(display: u32) -> Rect;
        fn CGDisplayCopyDisplayMode(display: u32) -> *mut c_void;
        fn CGDisplayModeGetWidth(mode: *mut c_void) -> usize;
        fn CGDisplayModeGetPixelWidth(mode: *mut c_void) -> usize;
        fn CGDisplayModeRelease(mode: *mut c_void);
    }

    /// As winit counts them, which Rune's places are in: points times the display's scale.
    pub(crate) fn displays() -> Vec<Display> {
        let (mut ids, mut count) = ([0_u32; 16], 0_u32);
        // SAFETY: the list has room for the 16 it is told.
        if unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) } != 0 {
            return Vec::new();
        }
        // SAFETY: no arguments.
        let main = unsafe { CGMainDisplayID() };
        let mut ids = ids[..count as usize].to_vec();
        ids.sort_by_key(|id| *id != main);
        ids.into_iter().map(display).collect()
    }

    fn display(id: u32) -> Display {
        // SAFETY: an id the system gave; a mode it copies is released once read.
        let (bounds, scale) = unsafe {
            let mode = CGDisplayCopyDisplayMode(id);
            let scale = match mode.is_null() {
                true => 1.0,
                false => {
                    let scale = CGDisplayModeGetPixelWidth(mode) as f64
                        / CGDisplayModeGetWidth(mode).max(1) as f64;
                    CGDisplayModeRelease(mode);
                    scale
                }
            };
            (CGDisplayBounds(id), scale)
        };
        let pixels = |points: f64| (points * scale).round();
        Display {
            id: Some(u64::from(id)),
            position: (pixels(bounds.x) as i32, pixels(bounds.y) as i32),
            size: (pixels(bounds.width) as u32, pixels(bounds.height) as u32),
            scale,
        }
    }

    fn app() -> *mut AnyObject {
        // SAFETY: AppKit's shared application, on the main thread.
        unsafe { msg_send![class!(NSApplication), sharedApplication] }
    }

    pub(crate) fn app_is_active() -> bool {
        // SAFETY: a getter of the shared application.
        unsafe { msg_send![app(), isActive] }
    }

    pub(crate) fn owner() -> Owner {}

    /// Above the main window, and hidden while another app is active, as GPUI's floating
    /// panel was: a plugin window over a browser is the classic annoyance.
    pub(crate) fn float(handle: &RawWindowHandle, (): Owner) {
        let RawWindowHandle::AppKit(handle) = handle else {
            return;
        };
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        // SAFETY: a live view of ours, on the main thread; a view in no window gives nil.
        unsafe {
            let window: *mut AnyObject = msg_send![view, window];
            if let Some(window) = window.as_ref() {
                // NSFloatingWindowLevel.
                let _: () = msg_send![window, setLevel: 3_isize];
                let _: () = msg_send![window, setHidesOnDeactivate: true];
            }
        }
    }

    /// The window that has the keyboard.
    pub(crate) fn keyboard() -> Option<usize> {
        // SAFETY: a getter of the shared application.
        let window: *mut AnyObject = unsafe { msg_send![app(), keyWindow] };
        (!window.is_null()).then_some(window as usize)
    }

    pub(crate) fn give_keyboard_to(window: usize) {
        let window = window as *mut AnyObject;
        // SAFETY: the key window of a moment ago, on the same turn of the main thread.
        unsafe {
            let _: () = msg_send![window, makeKeyWindow];
        }
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;

    use rune::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GWLP_HWNDPARENT, GetAncestor, GetForegroundWindow, MONITORINFOF_PRIMARY,
        SetForegroundWindow, SetWindowLongPtrW,
    };
    use windows_sys::core::BOOL;

    use super::*;

    pub(crate) type Owner = Option<usize>;

    pub(crate) fn displays() -> Vec<Display> {
        let mut displays: Vec<(bool, Display)> = Vec::new();
        // SAFETY: the callback gets the list for as long as the call runs.
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(each),
                &mut displays as *mut _ as LPARAM,
            );
        }
        displays.sort_by_key(|(primary, _)| !primary);
        displays.into_iter().map(|(_, display)| display).collect()
    }

    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `displays` passes its list.
        let list = unsafe { &mut *(list as *mut Vec<(bool, Display)>) };
        // SAFETY: a plain C struct, for which all zeros is a value.
        let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        let (mut x, mut y) = (96, 96);
        // SAFETY: a monitor the system gave, and a buffer of the size it is told.
        let known = unsafe {
            GetMonitorInfoW(
                monitor,
                (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
            ) != 0
                && GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0
        };
        if known {
            let rect = info.monitorInfo.rcMonitor;
            // The device's name, such as \\.\DISPLAY2, outlives a handle, which is new every run.
            let name = info.szDevice.iter().take_while(|unit| **unit != 0);
            let id = name.fold(0xcbf2_9ce4_8422_2325_u64, |hash, unit| {
                (hash ^ u64::from(*unit)).wrapping_mul(0x100_0000_01b3)
            });
            let display = Display {
                id: Some(id),
                position: (rect.left, rect.top),
                size: (
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                ),
                scale: f64::from(x) / 96.0,
            };
            list.push((
                info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                display,
            ));
        }
        1
    }

    pub(crate) fn app_is_active() -> bool {
        // SAFETY: no arguments.
        !unsafe { GetActiveWindow() }.is_null()
    }

    /// The root owner of the active window: the main window, also while a plugin's is active.
    pub(crate) fn owner() -> Owner {
        // SAFETY: takes and gives window handles; a null one is no window.
        let owner = unsafe { GetAncestor(GetActiveWindow(), GA_ROOTOWNER) };
        (!owner.is_null()).then_some(owner as usize)
    }

    /// Owned by the main window, which keeps it above itself and hides it with itself.
    pub(crate) fn float(handle: &RawWindowHandle, owner: Owner) {
        let (RawWindowHandle::Win32(handle), Some(owner)) = (handle, owner) else {
            return;
        };
        // SAFETY: both are open windows of this thread. For a window that is not a child,
        // `GWLP_HWNDPARENT` sets its owner; a failure leaves a plain window beside the main one.
        unsafe {
            SetWindowLongPtrW(
                handle.hwnd.get() as *mut c_void,
                GWLP_HWNDPARENT,
                owner as isize,
            )
        };
    }

    pub(crate) fn keyboard() -> Option<usize> {
        // SAFETY: no arguments.
        let window = unsafe { GetForegroundWindow() };
        (!window.is_null()).then_some(window as usize)
    }

    pub(crate) fn give_keyboard_to(window: usize) {
        // SAFETY: a window handle; one that went is refused.
        unsafe { SetForegroundWindow(window as *mut c_void) };
    }
}
