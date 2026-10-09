//! The sound of other apps on this computer, which a `project.json` connection can start at:
//! one app by name, or every app but Sound Tools itself. On macOS 14.2 and later a Core Audio
//! process tap hears it, read through a private aggregate device like any input. Other systems
//! have no such tap, and a connection from an app is a problem there.

use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::device::DeviceError;
use crate::input::{InputDevice, InputId};

pub(crate) use tap::{TAP_NAME, Tap};

/// The sound of other apps, as `project.json` names it: `"all"`, or the name of one app.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum AppSound {
    /// Every app but Sound Tools itself, which would hear itself.
    All,
    /// Every process of the app with this name, as the Dock shows it, such as "Music", or of
    /// the program, such as "afplay". Letter case does not count.
    Named(String),
}

const ALL: &str = "all";

impl AppSound {
    /// The live input of the engine that plays it. One app is one input in every edit, so the
    /// tap of an app that is open stays heard when `project.json` changes around it.
    pub(crate) fn input(&self) -> InputId {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        // 0 is the device input.
        InputId(hasher.finish().max(1))
    }
}

impl TryFrom<String> for AppSound {
    type Error = &'static str;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        match name.as_str() {
            ALL => Ok(Self::All),
            name if name.trim().is_empty() => {
                Err(r#"name an app, such as "Music", or "all" for every app"#)
            }
            _ => Ok(Self::Named(name)),
        }
    }
}

impl From<AppSound> for String {
    fn from(app: AppSound) -> Self {
        match app {
            AppSound::All => ALL.to_string(),
            AppSound::Named(name) => name,
        }
    }
}

impl fmt::Display for AppSound {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => formatter.write_str("every app"),
            Self::Named(name) => write!(formatter, "{name:?}"),
        }
    }
}

impl InputDevice {
    /// A tap of the sound of `app`, read like an input. The first tap of this app on this
    /// computer is when macOS asks the composer whether it may record other apps; one it may
    /// not hears silence. Opening it can take a while: never on the thread that draws.
    pub fn of_apps(app: &AppSound) -> Result<Self, DeviceError> {
        let (tap, device) = tap::open(app)?;
        Self::new(device, Some(tap))
    }
}

/// The processes a tap of `app` would hear now, to see when they change: an app that started
/// or quit, or a helper that came to play its sound. Empty for every app, whose tap takes new
/// processes by itself, and on systems with no taps.
pub fn app_processes(app: &AppSound) -> Vec<u32> {
    match app {
        AppSound::All => Vec::new(),
        AppSound::Named(name) => tap::processes_named(name).unwrap_or_default(),
    }
}

#[cfg(not(target_os = "macos"))]
mod tap {
    use super::AppSound;
    use crate::device::DeviceError;

    pub(crate) const TAP_NAME: &str = "Sound Tools hears ";

    /// No system but macOS has taps, so there is never one.
    pub(crate) enum Tap {}

    pub(crate) fn open(_: &AppSound) -> Result<(Tap, cpal::Device), DeviceError> {
        Err(DeviceError::AppsOnlyOnMacos)
    }

    pub(crate) fn processes_named(_: &str) -> Result<Vec<u32>, DeviceError> {
        Err(DeviceError::AppsOnlyOnMacos)
    }
}

#[cfg(target_os = "macos")]
mod tap {
    use std::ffi::{CStr, c_void};
    use std::ptr::{NonNull, null};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use cpal::traits::HostTrait;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_audio::{
        AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
        AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress, AudioObjectPropertySelector, CATapDescription,
        CATapMuteBehavior, kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey,
        kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
        kAudioAggregateDeviceUIDKey, kAudioHardwarePropertyProcessObjectList,
        kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyBundleID,
        kAudioProcessPropertyPID, kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
    };
    use objc2_core_foundation::CFDictionary;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

    use super::AppSound;
    use crate::device::DeviceError;

    /// How the name of every device a tap is read through begins, so the device menus can
    /// leave them out.
    pub(crate) const TAP_NAME: &str = "Sound Tools hears ";

    /// Makes the id of each aggregate device of this process its own.
    static TAPS: AtomicU32 = AtomicU32::new(0);

    /// A process tap and the private aggregate device it is read through. Dropping it removes
    /// both, after the stream that reads them stopped.
    pub(crate) struct Tap {
        tap: AudioObjectID,
        /// 0 until it is made.
        aggregate: AudioObjectID,
    }

    impl Drop for Tap {
        fn drop(&mut self) {
            if self.aggregate != 0 {
                // SAFETY: this process made the device and removes it once, here.
                let status = unsafe { AudioHardwareDestroyAggregateDevice(self.aggregate) };
                if let Err(error) = check(status, "remove the device of a tap") {
                    eprintln!("error: {error}");
                }
            }
            // SAFETY: this process made the tap and removes it once, here.
            let status = unsafe { AudioHardwareDestroyProcessTap(self.tap) };
            if let Err(error) = check(status, "remove a tap") {
                eprintln!("error: {error}");
            }
        }
    }

    fn check(status: i32, what: &'static str) -> Result<(), DeviceError> {
        match status {
            0 => Ok(()),
            status => Err(DeviceError::CoreAudio { what, status }),
        }
    }

    fn address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    /// A property of `object` that holds one `T`, asked with a process id when `pid` is given.
    /// `T` is a plain value or a pointer, all of whose bit patterns are valid.
    fn read<T: Default>(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        pid: Option<&i32>,
    ) -> Result<T, DeviceError> {
        let address = address(selector);
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        let (qualifier_size, qualifier) = match pid {
            Some(pid) => (size_of::<i32>() as u32, pid as *const i32 as *const c_void),
            None => (0, null()),
        };
        // SAFETY: `size` is the room of `value`, which Core Audio fills with one `T`, and the
        // qualifier, when there is one, is the `i32` this selector takes.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&address),
                qualifier_size,
                qualifier,
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast(),
            )
        };
        check(status, "read a property of Core Audio")?;
        Ok(value)
    }

    /// Every process that has used sound since it started, as process objects of Core Audio.
    fn process_objects() -> Result<Vec<AudioObjectID>, DeviceError> {
        let address = address(kAudioHardwarePropertyProcessObjectList);
        let system = kAudioObjectSystemObject as AudioObjectID;
        let mut size = 0u32;
        // SAFETY: asks for the size of the list only.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                system,
                NonNull::from(&address),
                0,
                null(),
                NonNull::from(&mut size),
            )
        };
        check(status, "list the processes that play sound")?;
        let mut objects = vec![0; size as usize / size_of::<AudioObjectID>()];
        let mut size = size_of_val(objects.as_slice()) as u32;
        // SAFETY: `size` is the room of `objects`, which Core Audio fills with process objects.
        let status = unsafe {
            AudioObjectGetPropertyData(
                system,
                NonNull::from(&address),
                0,
                null(),
                NonNull::from(&mut size),
                NonNull::from(objects.as_mut_slice()).cast(),
            )
        };
        check(status, "list the processes that play sound")?;
        objects.truncate(size as usize / size_of::<AudioObjectID>());
        Ok(objects)
    }

    /// The name of the app a program belongs to: the outermost `.app` in its path, so a helper
    /// of Chrome is "Google Chrome", or else the name of the program, such as "afplay".
    pub(super) fn app_name(path: &str) -> Option<&str> {
        let components = path.split('/').filter(|component| !component.is_empty());
        let mut last = None;
        for component in components {
            if let Some(app) = component.strip_suffix(".app") {
                return Some(app);
            }
            last = Some(component);
        }
        last
    }

    fn program_path(pid: i32) -> Option<String> {
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer has the room it says.
        let length =
            unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        path.truncate(usize::try_from(length).ok().filter(|length| *length > 0)?);
        String::from_utf8(path).ok()
    }

    fn bundle_id(process: AudioObjectID) -> Option<String> {
        let id: Option<NonNull<NSString>> =
            read(process, kAudioProcessPropertyBundleID, None).ok()?;
        // SAFETY: Core Audio gives the string under the create rule, so it is ours to release.
        let id = unsafe { Retained::from_raw(id?.as_ptr()) }?;
        Some(id.to_string())
    }

    /// The process objects of every process of the app `name`, Sound Tools left out.
    pub(crate) fn processes_named(name: &str) -> Result<Vec<AudioObjectID>, DeviceError> {
        let own = std::process::id() as i32;
        let wanted = name.to_lowercase();
        let is_wanted = |name: Option<&str>| name.is_some_and(|name| name.to_lowercase() == wanted);
        let mut found = Vec::new();
        for process in process_objects()? {
            // A process that ended since the list was made has nothing to read.
            let Ok(pid) = read::<i32>(process, kAudioProcessPropertyPID, None) else {
                continue;
            };
            if pid == own {
                continue;
            }
            let path = program_path(pid);
            if is_wanted(path.as_deref().and_then(app_name))
                || is_wanted(bundle_id(process).as_deref())
            {
                found.push(process);
            }
        }
        Ok(found)
    }

    /// The process object of Sound Tools itself, which a tap of every app leaves out.
    fn own_process() -> Option<AudioObjectID> {
        let pid = std::process::id() as i32;
        let system = kAudioObjectSystemObject as AudioObjectID;
        let selector = kAudioHardwarePropertyTranslatePIDToProcessObject;
        read::<AudioObjectID>(system, selector, Some(&pid))
            .ok()
            .filter(|object| *object != 0)
    }

    fn key(name: &CStr) -> Retained<NSString> {
        NSString::from_str(&name.to_string_lossy())
    }

    /// A tap of `app` and the device of cpal to read it with.
    pub(crate) fn open(app: &AppSound) -> Result<(Tap, cpal::Device), DeviceError> {
        if !objc2::available!(macos = 14.2) {
            return Err(DeviceError::AppsNeedNewerMacos);
        }
        let processes = match app {
            AppSound::All => own_process().into_iter().collect(),
            AppSound::Named(name) => match processes_named(name)? {
                found if found.is_empty() => return Err(DeviceError::NoApp(name.clone())),
                found => found,
            },
        };
        let numbers: Vec<Retained<NSNumber>> =
            processes.into_iter().map(NSNumber::new_u32).collect();
        let numbers: Vec<&NSNumber> = numbers.iter().map(|number| &**number).collect();
        let numbers = NSArray::from_slice(&numbers);
        // SAFETY: a new description from a list of process objects, as the methods take.
        let description = unsafe {
            match app {
                AppSound::All => CATapDescription::initStereoGlobalTapButExcludeProcesses(
                    CATapDescription::alloc(),
                    &numbers,
                ),
                AppSound::Named(_) => CATapDescription::initStereoMixdownOfProcesses(
                    CATapDescription::alloc(),
                    &numbers,
                ),
            }
        };
        let name = NSString::from_str(&format!("{TAP_NAME}{app}"));
        // SAFETY: setters of plain values on a description this function owns.
        unsafe {
            description.setName(&name);
            description.setPrivate(true);
            // The app goes on playing on its own device as well.
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
        }
        let mut tap = Tap {
            tap: 0,
            aggregate: 0,
        };
        // SAFETY: a valid description, and room for the id of the tap.
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap.tap) };
        check(status, "make a tap of other apps")?;

        // SAFETY: a getter of the description.
        let tap_uid = unsafe { description.UUID().UUIDString() };
        let yes = NSNumber::new_bool(true);
        let sub_tap_values: [&AnyObject; 2] = [&tap_uid, &yes];
        let sub_tap = NSDictionary::from_slices(
            &[
                &*key(kAudioSubTapUIDKey),
                &*key(kAudioSubTapDriftCompensationKey),
            ],
            &sub_tap_values,
        );
        let taps = NSArray::from_slice(&[&*sub_tap]);
        let uid = format!(
            "sound-tools.tap.{}.{}",
            std::process::id(),
            TAPS.fetch_add(1, Ordering::Relaxed)
        );
        let uid = NSString::from_str(&uid);
        let values: [&AnyObject; 5] = [&name, &uid, &yes, &yes, &taps];
        let description = NSDictionary::from_slices(
            &[
                &*key(kAudioAggregateDeviceNameKey),
                &*key(kAudioAggregateDeviceUIDKey),
                &*key(kAudioAggregateDeviceIsPrivateKey),
                &*key(kAudioAggregateDeviceTapAutoStartKey),
                &*key(kAudioAggregateDeviceTapListKey),
            ],
            &values,
        );
        // SAFETY: an NSDictionary is a CFDictionary: the two types are toll-free bridged.
        let dictionary = unsafe { &*(Retained::as_ptr(&description) as *const CFDictionary) };
        // SAFETY: the keys Core Audio documents for an aggregate device, and room for its id.
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(dictionary, NonNull::from(&mut tap.aggregate))
        };
        check(status, "make a device to read a tap through")?;

        // The device may take a moment to be listed.
        let id = cpal::DeviceId::new(cpal::HostId::CoreAudio, uid.to_string());
        let host = cpal::default_host();
        for _ in 0..20 {
            if let Some(device) = host.device_by_id(&id) {
                return Ok((tap, device));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(DeviceError::TapNotListed)
    }

    #[cfg(test)]
    mod tests {
        use super::app_name;

        #[test]
        fn a_process_belongs_to_the_outermost_app_or_else_is_its_program() {
            let helper = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/1/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
            assert_eq!(app_name(helper), Some("Google Chrome"));
            assert_eq!(
                app_name("/System/Applications/Music.app/Contents/MacOS/Music"),
                Some("Music")
            );
            assert_eq!(app_name("/usr/bin/afplay"), Some("afplay"));
            assert_eq!(app_name(""), None);
        }
    }
}
