//! One VST 3 bundle, loaded: its binary and the factory that lists what is in it.
//!
//! A VST 3 plugin on macOS is a bundle, and the format says the host loads it through
//! `CFBundle` and calls `bundleEntry` with the `CFBundleRef`. Plugins use that reference to
//! find their own resources, so a null one is not good enough. The three symbols a bundle
//! exports are `bundleEntry`, `GetPluginFactory` and `bundleExit`.
//!
//! On Linux a bundle is a plain folder with one `.so` per architecture, such as
//! `Contents/x86_64-linux/piano.so`. The host loads it with `dlopen` and calls `ModuleEntry`
//! with the handle; the last symbol is `ModuleExit`.
//!
//! On Windows a bundle is the same kind of folder with a DLL named like the bundle, such as
//! `Contents/x86_64-win/piano.vst3`, or, from before bundles, that DLL alone as `piano.vst3`.
//! The host loads it with `LoadLibraryExW` and calls `InitDll`, which takes nothing and which a
//! plugin need not have; the last symbol is `ExitDll`.
//!
//! The `platform` modules at the end hold the three ways, and are all of this backend that
//! differs between them.
//!
//! A bundle is loaded once per process and never unloaded. Unloading runs the plugin's static
//! destructors and unregisters its Objective-C classes while views, timers and audio threads of
//! that plugin may still exist; every host this was written against keeps them. So
//! [`Module::load`] keeps what it loaded in a table of this thread and hands out the same
//! module again. That also makes a second plugin from one bundle free.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::c_char;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, TUID,
};

/// What every VST 3 binary exports, whatever the platform. Each platform's entry function is in
/// its `platform` module.
type GetPluginFactory = unsafe extern "C" fn() -> *mut IPluginFactory;

/// The class category of a plugin that makes sound. Everything else in a bundle, such as a
/// controller class, is not a plugin a project can name.
const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

/// One loaded bundle. Dropping it releases our reference to the factory and leaves the binary
/// where it is, see the module documentation.
pub(super) struct Module {
    factory: ComPtr<IPluginFactory>,
}

impl Module {
    /// Loads `bundle`, or gives back the one this process already loaded from that path.
    ///
    /// Loading runs the plugin's own code: its static initializers and `bundleEntry`. That is
    /// why a scan does this in a child process.
    pub(super) fn load(bundle: &Path) -> Result<Rc<Self>, String> {
        thread_local! {
            static LOADED: RefCell<BTreeMap<PathBuf, Rc<Module>>> = const {
                RefCell::new(BTreeMap::new())
            };
        }
        if let Some(module) = LOADED.with_borrow(|loaded| loaded.get(bundle).cloned()) {
            return Ok(module);
        }
        let module = Rc::new(Self::load_once(bundle)?);
        LOADED.with_borrow_mut(|loaded| loaded.insert(bundle.to_path_buf(), module.clone()));
        Ok(module)
    }

    fn load_once(bundle: &Path) -> Result<Self, String> {
        let fail = |message: &dyn std::fmt::Display| format!("{}: {message}", bundle.display());
        // SAFETY: `platform::open` gives the function the binary exports under the name the
        // format gives it, once the plugin's own entry function has said yes.
        let factory = unsafe {
            let get_factory = platform::open(bundle).map_err(|message| fail(&message))?;
            ComPtr::from_raw(get_factory()).ok_or_else(|| fail(&"the plugin has no factory"))?
        };
        Ok(Self { factory })
    }

    pub(super) fn factory(&self) -> &ComPtr<IPluginFactory> {
        &self.factory
    }

    /// Every plugin class in this bundle: its id, what its maker calls it and what it says it
    /// is. Classes that are not audio modules, such as a plugin's controller, are left out.
    pub(super) fn classes(&self) -> Vec<ClassInfo> {
        let factory2 = self.factory.cast::<IPluginFactory2>();
        let mut classes = Vec::new();
        // SAFETY: the factory came from the plugin and is alive. Every `info` is written by the
        // plugin before it is read, and a call that fails leaves it untouched, which is why it
        // starts zeroed.
        unsafe {
            let count = self.factory.countClasses();
            for index in 0..count {
                let Some(class) = (match &factory2 {
                    Some(factory2) => {
                        let mut info: PClassInfo2 = std::mem::zeroed();
                        (factory2.getClassInfo2(index, &mut info) == vst3::Steinberg::kResultOk)
                            .then(|| ClassInfo::of2(&info))
                    }
                    None => {
                        let mut info: PClassInfo = std::mem::zeroed();
                        (self.factory.getClassInfo(index, &mut info) == vst3::Steinberg::kResultOk)
                            .then(|| ClassInfo::of(&info))
                    }
                }) else {
                    continue;
                };
                if class.category == AUDIO_MODULE_CLASS {
                    classes.push(class);
                }
            }
        }
        classes
    }
}

/// What a bundle says about one of its classes.
pub(super) struct ClassInfo {
    pub id: TUID,
    pub category: String,
    pub name: String,
    pub vendor: String,
    /// What the class says it is, such as `Instrument|Synth`. VST 3 calls these subcategories.
    pub subcategories: Vec<String>,
}

impl ClassInfo {
    fn of(info: &PClassInfo) -> Self {
        Self {
            id: info.cid,
            category: text(&info.category),
            name: text(&info.name),
            vendor: String::new(),
            subcategories: Vec::new(),
        }
    }

    fn of2(info: &PClassInfo2) -> Self {
        Self {
            id: info.cid,
            category: text(&info.category),
            name: text(&info.name),
            vendor: text(&info.vendor),
            subcategories: text(&info.subCategories)
                .split('|')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

/// A fixed C string field of the VST 3 API, up to its zero byte.
fn text(field: &[c_char]) -> String {
    let bytes: Vec<u8> = field
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| byte.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// macOS: the bundle through `CFBundle`, and `bundleEntry`.
#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;

    use super::GetPluginFactory;

    /// `bundleEntry` takes the `CFBundleRef` of the bundle it is in.
    type BundleEntry = unsafe extern "C" fn(*mut c_void) -> bool;

    /// Loads the binary of `bundle` and starts it. It stays loaded when it turns out not to be
    /// a plugin: see the module documentation on unloading.
    ///
    /// # Safety
    ///
    /// This runs the plugin's static initializers and `bundleEntry`.
    pub(super) unsafe fn open(bundle: &Path) -> Result<GetPluginFactory, &'static str> {
        let path = CString::new(bundle.as_os_str().as_encoded_bytes())
            .map_err(|_| "the bundle path is not a path")?;
        // SAFETY: every pointer below is checked for null before it is used, and each one is
        // released on the way out. `path` outlives the URL, which is copied by CFBundleCreate.
        unsafe {
            let url = CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                path.as_ptr().cast(),
                path.as_bytes().len() as isize,
                true,
            );
            if url.is_null() {
                return Err("the bundle path is not a path");
            }
            let handle = CFBundleCreate(std::ptr::null(), url);
            CFRelease(url.cast());
            if handle.is_null() {
                return Err("this is not a bundle");
            }
            if !CFBundleLoadExecutable(handle) {
                CFRelease(handle.cast());
                return Err("the bundle has no binary this machine can load");
            }
            let entry: Option<BundleEntry> =
                std::mem::transmute(function_in(handle, c"bundleEntry"));
            let get_factory: Option<GetPluginFactory> =
                std::mem::transmute(function_in(handle, c"GetPluginFactory"));
            let (Some(entry), Some(get_factory)) = (entry, get_factory) else {
                return Err("the binary is not a VST 3 plugin");
            };
            // The bundle stays: the plugin holds it from here on. Nothing releases it, because
            // nothing unloads a plugin.
            if !entry(handle) {
                return Err("the plugin refused to start");
            }
            Ok(get_factory)
        }
    }

    /// A symbol of a loaded bundle. Null when the bundle does not export it.
    ///
    /// # Safety
    ///
    /// `handle` must be a loaded `CFBundleRef`.
    unsafe fn function_in(handle: *mut c_void, name: &CStr) -> *mut c_void {
        // SAFETY: the caller keeps the contract, and the string is released before returning.
        unsafe {
            let key = CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8);
            if key.is_null() {
                return std::ptr::null_mut();
            }
            let function = CFBundleGetFunctionPointerForName(handle, key);
            CFRelease(key.cast());
            function
        }
    }

    const UTF8: u32 = 0x0800_0100;

    // The few CoreFoundation calls a bundle needs, declared here instead of taking a dependency.
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            length: isize,
            is_directory: bool,
        ) -> *mut c_void;
        fn CFBundleCreate(allocator: *const c_void, url: *mut c_void) -> *mut c_void;
        fn CFBundleLoadExecutable(bundle: *mut c_void) -> bool;
        fn CFBundleGetFunctionPointerForName(bundle: *mut c_void, name: *mut c_void)
        -> *mut c_void;
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            string: *const c_char,
            encoding: u32,
        ) -> *mut c_void;
    }
}

/// Linux: the `.so` of this machine's architecture in the bundle, through `dlopen`, and
/// `ModuleEntry` with the handle `dlopen` gave.
#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::path::Path;

    use super::GetPluginFactory;

    /// `ModuleEntry` takes the `dlopen` handle of the library it is in.
    type ModuleEntry = unsafe extern "C" fn(*mut c_void) -> bool;

    /// Loads the binary of `bundle` and starts it. It stays loaded when it turns out not to be
    /// a plugin: see the module documentation on unloading.
    ///
    /// # Safety
    ///
    /// This runs the plugin's static initializers and `ModuleEntry`.
    pub(super) unsafe fn open(bundle: &Path) -> Result<GetPluginFactory, &'static str> {
        let mut file = bundle.file_stem().ok_or("this is not a bundle")?.to_owned();
        file.push(".so");
        let binary = crate::scan::binary_folder(bundle).join(file);
        if !binary.is_file() {
            return Err("the bundle has no binary this machine can load");
        }
        let path = CString::new(binary.into_os_string().into_encoded_bytes())
            .map_err(|_| "the bundle path is not a path")?;
        // SAFETY: `path` outlives the call, and each symbol is checked for null before it is
        // turned into a function.
        unsafe {
            let handle = dlopen(path.as_ptr(), RTLD_NOW | RTLD_LOCAL);
            if handle.is_null() {
                return Err("the bundle has no binary this machine can load");
            }
            let entry: Option<ModuleEntry> =
                std::mem::transmute(dlsym(handle, c"ModuleEntry".as_ptr()));
            let get_factory: Option<GetPluginFactory> =
                std::mem::transmute(dlsym(handle, c"GetPluginFactory".as_ptr()));
            let (Some(entry), Some(get_factory)) = (entry, get_factory) else {
                return Err("the binary is not a VST 3 plugin");
            };
            // The handle stays: the plugin holds it from here on. Nothing closes it, because
            // nothing unloads a plugin.
            if !entry(handle) {
                return Err("the plugin refused to start");
            }
            Ok(get_factory)
        }
    }

    const RTLD_NOW: c_int = 2;
    const RTLD_LOCAL: c_int = 0;

    // From the C library, which every Rust program on Linux links already.
    unsafe extern "C" {
        fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
}

/// Windows: the DLL of this machine's architecture in the bundle, or the bundle itself when it
/// is the older single file, loaded with its own folder in the DLL search (see `library.rs`),
/// and `InitDll` when the DLL has one.
#[cfg(target_os = "windows")]
mod platform {
    use std::path::Path;

    use super::GetPluginFactory;
    use crate::library::Library;

    /// `InitDll` takes nothing. Steinberg's own module loader calls it only when it is there.
    type InitDll = unsafe extern "C" fn() -> bool;

    /// Loads the binary of `bundle` and starts it. It stays loaded when it turns out not to be
    /// a plugin: see the module documentation on unloading.
    ///
    /// # Safety
    ///
    /// This runs the plugin's static initializers and `InitDll`.
    pub(super) unsafe fn open(bundle: &Path) -> Result<GetPluginFactory, String> {
        let binary = match bundle.is_dir() {
            true => {
                let file = bundle.file_name().ok_or("this is not a bundle")?;
                crate::scan::binary_folder(bundle).join(file)
            }
            false => bundle.to_path_buf(),
        };
        if !binary.is_file() {
            return Err("the bundle has no binary this machine can load".to_string());
        }
        // SAFETY: the caller agreed to run the plugin's code. Each symbol is checked for null
        // before it is turned into a function.
        unsafe {
            let library = Library::load(&binary)
                .map_err(|error| format!("the binary did not load: {error}"))?;
            let get_factory: Option<GetPluginFactory> =
                std::mem::transmute(library.symbol(c"GetPluginFactory"));
            let init: Option<InitDll> = std::mem::transmute(library.symbol(c"InitDll"));
            // The DLL stays, whatever comes next: nothing unloads a plugin.
            library.keep();
            let Some(get_factory) = get_factory else {
                return Err("the binary is not a VST 3 plugin".to_string());
            };
            if let Some(init) = init
                && !init()
            {
                return Err("the plugin refused to start".to_string());
            }
            Ok(get_factory)
        }
    }
}
