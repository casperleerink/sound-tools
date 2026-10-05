//! Windows: loading a plugin's DLL so that the DLLs it imports are found beside it.
//!
//! A plain `LoadLibraryW` looks for the DLLs a plugin imports beside this program and in the
//! system folders, not beside the plugin, so a plugin that ships a helper DLL next to its own
//! would not load. `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` adds the plugin's own folder to the
//! search, and `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS` keeps the usual safe folders. The first needs
//! an absolute path.

use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr::NonNull;

/// A DLL this process loaded. Dropping it lets go of this load; [`Self::keep`] keeps the DLL
/// for the rest of the process.
pub(crate) struct Library(NonNull<c_void>);

impl Library {
    /// Loads `binary`, looking for the DLLs it imports in its own folder as well.
    ///
    /// # Safety
    ///
    /// This runs the DLL's initializers, which are the plugin's own code.
    pub(crate) unsafe fn load(binary: &Path) -> Result<Self, String> {
        let absolute = std::path::absolute(binary).map_err(|error| error.to_string())?;
        let mut wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err("the path of the plugin is not a path".to_string());
        }
        wide.push(0);
        // SAFETY: `wide` ends in its only zero and outlives the call. The caller agreed to run
        // the DLL's own code.
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        // The system says why, such as a DLL the plugin imports that is not there.
        NonNull::new(module)
            .map(Self)
            .ok_or_else(|| std::io::Error::last_os_error().to_string())
    }

    /// The address of `name` in the DLL. Null when the DLL does not export it.
    pub(crate) fn symbol(&self, name: &CStr) -> *mut c_void {
        // SAFETY: the module is loaded for as long as `self` lives, and `name` is a C string.
        unsafe { GetProcAddress(self.0.as_ptr(), name.as_ptr()) }
    }

    /// Keeps the DLL loaded until the process ends, which is what nothing unloading a plugin
    /// means.
    pub(crate) fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: the module came from `LoadLibraryExW` and this is its one release. One that
        // fails leaves the DLL loaded, which costs nothing but memory.
        unsafe { FreeLibrary(self.0.as_ptr()) };
    }
}

const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(file: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
