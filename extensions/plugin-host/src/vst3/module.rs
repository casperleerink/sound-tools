//! One VST 3 bundle, loaded: its binary and the factory that lists what is in it.
//!
//! A VST 3 plugin on macOS is a bundle, and the format says the host loads it through
//! `CFBundle` and calls `bundleEntry` with the `CFBundleRef`. Plugins use that reference to
//! find their own resources, so a null one is not good enough. The three symbols a bundle
//! exports are `bundleEntry`, `GetPluginFactory` and `bundleExit`.
//!
//! A bundle is loaded once per process and never unloaded. Unloading runs the plugin's static
//! destructors and unregisters its Objective-C classes while views, timers and audio threads of
//! that plugin may still exist; every host this was written against keeps them. So
//! [`Module::load`] keeps what it loaded in a table of this thread and hands out the same
//! module again. That also makes a second plugin from one bundle free.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo,
    PClassInfo2, TUID,
};

/// What a VST 3 bundle exports. `bundleEntry` takes the `CFBundleRef` of the bundle it is in.
type BundleEntry = unsafe extern "C" fn(*mut c_void) -> bool;
type GetPluginFactory = unsafe extern "C" fn() -> *mut IPluginFactory;

/// The class category of a plugin that makes sound. Everything else in a bundle, such as a
/// controller class, is not a plugin a project can name.
const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

/// One loaded bundle. Dropping it releases our reference to the factory and leaves the binary
/// where it is, see the module documentation.
pub struct Module {
    factory: ComPtr<IPluginFactory>,
}

impl Module {
    /// Loads `bundle`, or gives back the one this process already loaded from that path.
    ///
    /// Loading runs the plugin's own code: its static initializers and `bundleEntry`. That is
    /// why a scan does this in a child process.
    pub fn load(bundle: &Path) -> Result<Rc<Self>, String> {
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
        let path = CString::new(bundle.as_os_str().as_encoded_bytes())
            .map_err(|error| format!("{}: {error}", bundle.display()))?;
        let fail = |message: &str| format!("{}: {message}", bundle.display());
        // SAFETY: every pointer below is checked for null before it is used, and each one is
        // released on the way out. `path` outlives the URL, which is copied by CFBundleCreate.
        let factory = unsafe {
            let url = core_foundation::CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                path.as_ptr().cast(),
                path.as_bytes().len() as isize,
                true,
            );
            if url.is_null() {
                return Err(fail("the bundle path is not a path"));
            }
            let handle = core_foundation::CFBundleCreate(std::ptr::null(), url);
            core_foundation::CFRelease(url.cast());
            if handle.is_null() {
                return Err(fail("this is not a bundle"));
            }
            if !core_foundation::CFBundleLoadExecutable(handle) {
                core_foundation::CFRelease(handle.cast());
                return Err(fail("the bundle has no binary this machine can load"));
            }
            let entry: Option<BundleEntry> =
                std::mem::transmute(function_in(handle, c"bundleEntry"));
            let get_factory: Option<GetPluginFactory> =
                std::mem::transmute(function_in(handle, c"GetPluginFactory"));
            let (Some(entry), Some(get_factory)) = (entry, get_factory) else {
                // Left loaded: see the module documentation on unloading.
                return Err(fail("the binary is not a VST 3 plugin"));
            };
            if !entry(handle.cast()) {
                return Err(fail("the plugin refused to start"));
            }
            // The bundle reference stays: the plugin holds it from here on. Nothing releases
            // it, because nothing unloads a plugin.
            ComPtr::from_raw(get_factory()).ok_or_else(|| fail("the plugin has no factory"))?
        };
        Ok(Self { factory })
    }

    pub fn factory(&self) -> &ComPtr<IPluginFactory> {
        &self.factory
    }

    /// Every plugin class in this bundle: its id, what its maker calls it and what it says it
    /// is. Classes that are not audio modules, such as a plugin's controller, are left out.
    pub fn classes(&self) -> Vec<ClassInfo> {
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
pub struct ClassInfo {
    pub id: TUID,
    pub category: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
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
            version: String::new(),
            subcategories: Vec::new(),
        }
    }

    fn of2(info: &PClassInfo2) -> Self {
        Self {
            id: info.cid,
            category: text(&info.category),
            name: text(&info.name),
            vendor: text(&info.vendor),
            version: text(&info.version),
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
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// A symbol of a loaded bundle. `None` when the bundle does not export it.
///
/// # Safety
///
/// `handle` must be a loaded `CFBundleRef`.
unsafe fn function_in(handle: *mut c_void, name: &std::ffi::CStr) -> *mut c_void {
    // SAFETY: the caller keeps the contract, and the string is released before returning.
    unsafe {
        let key = core_foundation::CFStringCreateWithCString(
            std::ptr::null(),
            name.as_ptr(),
            core_foundation::UTF8,
        );
        if key.is_null() {
            return std::ptr::null_mut();
        }
        let function = core_foundation::CFBundleGetFunctionPointerForName(handle, key);
        core_foundation::CFRelease(key.cast());
        function
    }
}

/// The few CoreFoundation calls a bundle needs. Declared here instead of taking a dependency:
/// this is the whole platform surface of the VST 3 backend.
mod core_foundation {
    use std::ffi::{c_char, c_void};

    pub const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub fn CFRelease(value: *const c_void);
        pub fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            length: isize,
            is_directory: bool,
        ) -> *mut c_void;
        pub fn CFBundleCreate(allocator: *const c_void, url: *mut c_void) -> *mut c_void;
        pub fn CFBundleLoadExecutable(bundle: *mut c_void) -> bool;
        pub fn CFBundleGetFunctionPointerForName(
            bundle: *mut c_void,
            name: *mut c_void,
        ) -> *mut c_void;
        pub fn CFStringCreateWithCString(
            allocator: *const c_void,
            string: *const c_char,
            encoding: u32,
        ) -> *mut c_void;
    }
}
