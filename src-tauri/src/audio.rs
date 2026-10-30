//! Sound: the output devices, the master level, and every app's level in the volume mixer,
//! for the Sound view and the volume keys. Windows only for now; macOS has no per-app levels
//! without a virtual audio driver.

use serde::Serialize;

/// An output device, as Windows' sound settings list it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    /// "Speakers", "Headphones".
    pub name: String,
    /// The hardware behind it: "Realtek High Definition Audio".
    pub detail: String,
}

/// One app's audio, all its sessions together (a browser plays from several processes).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAudio {
    /// What the setters take: the program's path in lowercase, or `system` for system sounds.
    pub key: String,
    pub name: String,
    pub icon: String,
    /// 0–1, the mixer's level for the app.
    pub volume: f32,
    pub muted: bool,
    /// 0–1, how loud it is right now.
    pub peak: f32,
    /// Playing, rather than open and quiet.
    pub active: bool,
}

/// The default output device and what's on it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub device: Option<Device>,
    pub devices: Vec<Device>,
    pub volume: f32,
    pub muted: bool,
    pub peak: f32,
    pub apps: Vec<AppAudio>,
}

/// Where a level change lands: the default device, or one app on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Master,
    App(String),
}

/// A level after a change, for the volume overlay.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Level {
    /// `master`, or the app's key.
    pub key: String,
    pub name: String,
    pub icon: String,
    pub volume: f32,
    pub muted: bool,
}

#[cfg(windows)]
const SYSTEM_KEY: &str = "system";
pub const MASTER_KEY: &str = "master";

impl Target {
    pub fn from_key(key: &str) -> Target {
        if key == MASTER_KEY {
            Target::Master
        } else {
            Target::App(key.to_string())
        }
    }
}

#[cfg(windows)]
pub use win::{set_default_device, set_muted, set_volume, snapshot, step};

#[cfg(not(windows))]
mod other {
    use super::*;

    pub fn snapshot() -> Option<Sound> {
        None
    }
    pub fn set_volume(_target: &Target, _volume: f32) -> Option<Level> {
        None
    }
    pub fn set_muted(_target: &Target, _muted: bool) -> Option<Level> {
        None
    }
    pub fn step(_target: &Target, _delta: f32) -> Option<Level> {
        None
    }
    pub fn set_default_device(_id: &str) -> Result<(), String> {
        Err("Switching the output device isn't available here".into())
    }
}

#[cfg(not(windows))]
pub use other::{set_default_device, set_muted, set_volume, snapshot, step};

#[cfg(windows)]
mod win {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::{
        AudioSessionStateActive, AudioSessionStateExpired, DEVICE_STATE_ACTIVE, ERole,
        IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator, eCommunications, eConsole, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        STGM_READ,
    };
    use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, PCWSTR, PWSTR};

    // Device names, from the property store (FunctionDiscovery's keys, without its feature).
    const DEVICE_DESC: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 2,
    };
    const INTERFACE_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x026e516e_b814_414b_83cd_856d6fef4822),
        pid: 2,
    };

    /// App names by program path, read once from the program's version info.
    static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    fn init() {
        // SAFETY: COM on this thread; already initialized (in any mode) is fine.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn enumerator() -> Option<IMMDeviceEnumerator> {
        init();
        // SAFETY: a plain COM object creation.
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    fn default_device(devices: &IMMDeviceEnumerator) -> Option<IMMDevice> {
        // SAFETY: a call on a live enumerator.
        unsafe { devices.GetDefaultAudioEndpoint(eRender, eConsole).ok() }
    }

    fn take_string(text: PWSTR) -> Option<String> {
        if text.is_null() {
            return None;
        }
        // SAFETY: a string COM allocated for us, read once and freed.
        unsafe {
            let value = text.to_string().ok();
            CoTaskMemFree(Some(text.0 as *const _));
            value
        }
    }

    fn device_info(device: &IMMDevice) -> Option<Device> {
        // SAFETY: calls on a live device and the property store it opens.
        unsafe {
            let id = take_string(device.GetId().ok()?)?;
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let read = |key: &PROPERTYKEY| {
                let value = store.GetValue(key).ok()?;
                take_string(PropVariantToStringAlloc(&value).ok()?)
            };
            let name = read(&DEVICE_DESC).unwrap_or_else(|| "Output".to_string());
            let detail = read(&INTERFACE_NAME).unwrap_or_default();
            Some(Device { id, name, detail })
        }
    }

    fn device_list(devices: &IMMDeviceEnumerator) -> Vec<Device> {
        // SAFETY: calls on a live enumerator and the collection it returns.
        unsafe {
            let Ok(collection) = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            let count = collection.GetCount().unwrap_or(0);
            (0..count)
                .filter_map(|index| collection.Item(index).ok())
                .filter_map(|device| device_info(&device))
                .collect()
        }
    }

    /// A session on the default device, with the app it belongs to.
    struct Session {
        key: String,
        path: String,
        display: String,
        volume: ISimpleAudioVolume,
        meter: Option<IAudioMeterInformation>,
        active: bool,
    }

    fn sessions(device: &IMMDevice) -> Vec<Session> {
        let mut found = Vec::new();
        // SAFETY: WASAPI calls on objects the device hands back, released as they drop.
        unsafe {
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                return found;
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                return found;
            };
            for index in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(index) else {
                    continue;
                };
                let state = control.GetState().unwrap_or(AudioSessionStateExpired);
                if state == AudioSessionStateExpired {
                    continue;
                }
                let Ok(control) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
                    continue;
                };
                let meter = control.cast::<IAudioMeterInformation>().ok();
                let system = control.IsSystemSoundsSession() == HRESULT(0);
                let (key, path) = if system {
                    (SYSTEM_KEY.to_string(), String::new())
                } else {
                    let pid = control.GetProcessId().unwrap_or(0);
                    let Some(path) = (pid != 0).then(|| crate::util::process_path(pid)).flatten()
                    else {
                        continue;
                    };
                    (path.to_lowercase(), path)
                };
                let display = control
                    .GetDisplayName()
                    .ok()
                    .and_then(take_string)
                    .filter(|name| !name.is_empty() && !name.starts_with('@'))
                    .unwrap_or_default();
                found.push(Session {
                    key,
                    path,
                    display,
                    volume,
                    meter,
                    active: state == AudioSessionStateActive,
                });
            }
        }
        found
    }

    fn app_name(session: &Session) -> String {
        if session.key == SYSTEM_KEY {
            return "System sounds".to_string();
        }
        if !session.display.is_empty() {
            return session.display.clone();
        }
        let mut names = NAMES.lock().unwrap_or_else(|poison| poison.into_inner());
        let names = names.get_or_insert_with(HashMap::new);
        names
            .entry(session.key.clone())
            .or_insert_with(|| {
                file_description(&session.path).unwrap_or_else(|| stem(&session.path))
            })
            .clone()
    }

    fn app_icon(session: &Session) -> String {
        if session.key == SYSTEM_KEY {
            "glyph:speaker".to_string()
        } else {
            format!("app:exe:{}", session.path)
        }
    }

    /// "C:\…\Discord.exe" → "Discord".
    fn stem(path: &str) -> String {
        let file = path.rsplit('\\').next().unwrap_or(path);
        file.strip_suffix(".exe")
            .or_else(|| file.strip_suffix(".EXE"))
            .unwrap_or(file)
            .to_string()
    }

    /// The program's own name for itself ("Google Chrome"), from its version info.
    fn file_description(path: &str) -> Option<String> {
        use windows::Win32::Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
        };
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        // SAFETY: a buffer sized by the first call, and pointers into it from VerQueryValue,
        // read only while the buffer lives.
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut block = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, block.as_mut_ptr() as _).ok()?;
            let mut pointer = std::ptr::null_mut();
            let mut length = 0u32;
            let translation: Vec<u16> = "\\VarFileInfo\\Translation"
                .encode_utf16()
                .chain([0])
                .collect();
            let (language, page) = if VerQueryValueW(
                block.as_ptr() as _,
                PCWSTR(translation.as_ptr()),
                &mut pointer,
                &mut length,
            )
            .as_bool()
                && length >= 4
            {
                let pair = pointer as *const u16;
                (*pair, *pair.add(1))
            } else {
                (0x0409, 0x04b0)
            };
            let query = format!("\\StringFileInfo\\{language:04x}{page:04x}\\FileDescription");
            let query: Vec<u16> = query.encode_utf16().chain([0]).collect();
            if !VerQueryValueW(
                block.as_ptr() as _,
                PCWSTR(query.as_ptr()),
                &mut pointer,
                &mut length,
            )
            .as_bool()
                || length == 0
            {
                return None;
            }
            let text = std::slice::from_raw_parts(pointer as *const u16, length as usize);
            let text = String::from_utf16_lossy(text);
            let text = text.trim_end_matches('\0').trim();
            (!text.is_empty()).then(|| text.to_string())
        }
    }

    fn endpoint_volume(device: &IMMDevice) -> Option<IAudioEndpointVolume> {
        // SAFETY: a call on a live device.
        unsafe {
            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .ok()
        }
    }

    fn peak(meter: Option<&IAudioMeterInformation>) -> f32 {
        // SAFETY: a call on a live meter.
        meter
            .and_then(|meter| unsafe { meter.GetPeakValue().ok() })
            .unwrap_or(0.0)
    }

    /// The default device, every output device, and the apps on the default one: playing
    /// apps first, then by name, with system sounds last.
    pub fn snapshot() -> Option<Sound> {
        let devices = enumerator()?;
        let device = default_device(&devices);
        let list = device_list(&devices);
        let Some(device) = device else {
            return Some(Sound {
                device: None,
                devices: list,
                volume: 0.0,
                muted: false,
                peak: 0.0,
                apps: Vec::new(),
            });
        };
        let endpoint = endpoint_volume(&device);
        // SAFETY: calls on a live endpoint volume.
        let (volume, muted) = endpoint
            .as_ref()
            .map(|endpoint| unsafe {
                (
                    endpoint.GetMasterVolumeLevelScalar().unwrap_or(0.0),
                    endpoint
                        .GetMute()
                        .map(|mute| mute.as_bool())
                        .unwrap_or(false),
                )
            })
            .unwrap_or((0.0, false));
        // SAFETY: a call on a live device.
        let meter = unsafe {
            device
                .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
                .ok()
        };

        let mut apps: Vec<AppAudio> = Vec::new();
        for session in sessions(&device) {
            // SAFETY: calls on a live session volume.
            let (level, mute) = unsafe {
                (
                    session.volume.GetMasterVolume().unwrap_or(0.0),
                    session
                        .volume
                        .GetMute()
                        .map(|mute| mute.as_bool())
                        .unwrap_or(false),
                )
            };
            let level_peak = peak(session.meter.as_ref());
            if let Some(app) = apps.iter_mut().find(|app| app.key == session.key) {
                app.peak = app.peak.max(level_peak);
                app.active |= session.active;
                app.muted &= mute;
                continue;
            }
            apps.push(AppAudio {
                key: session.key.clone(),
                name: app_name(&session),
                icon: app_icon(&session),
                volume: level,
                muted: mute,
                peak: level_peak,
                active: session.active,
            });
        }
        apps.sort_by(|a, b| {
            (a.key == SYSTEM_KEY)
                .cmp(&(b.key == SYSTEM_KEY))
                .then(b.active.cmp(&a.active))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Some(Sound {
            device: device_info(&device),
            devices: list,
            volume,
            muted,
            peak: peak(meter.as_ref()),
            apps,
        })
    }

    /// Changes `target`'s level and mute with `change`, which gets the current pair and
    /// returns the new one. Every session of an app lands on the same level.
    fn change(target: &Target, change: impl Fn(f32, bool) -> (f32, bool)) -> Option<Level> {
        let devices = enumerator()?;
        let device = default_device(&devices)?;
        match target {
            Target::Master => {
                let endpoint = endpoint_volume(&device)?;
                // SAFETY: calls on a live endpoint volume.
                unsafe {
                    let level = endpoint.GetMasterVolumeLevelScalar().ok()?;
                    let muted = endpoint.GetMute().ok()?.as_bool();
                    let (next, mute) = change(level, muted);
                    let next = next.clamp(0.0, 1.0);
                    if next != level {
                        endpoint
                            .SetMasterVolumeLevelScalar(next, std::ptr::null())
                            .ok()?;
                    }
                    if mute != muted {
                        endpoint.SetMute(mute, std::ptr::null()).ok()?;
                    }
                    Some(Level {
                        key: MASTER_KEY.to_string(),
                        name: device_info(&device)
                            .map(|info| info.name)
                            .unwrap_or_else(|| "Master".to_string()),
                        icon: "glyph:speaker".to_string(),
                        volume: next,
                        muted: mute,
                    })
                }
            }
            Target::App(key) => {
                let found: Vec<Session> = sessions(&device)
                    .into_iter()
                    .filter(|session| &session.key == key)
                    .collect();
                let first = found.first()?;
                // SAFETY: calls on live session volumes.
                unsafe {
                    let level = first.volume.GetMasterVolume().ok()?;
                    let muted = found
                        .iter()
                        .all(|session| session.volume.GetMute().is_ok_and(|mute| mute.as_bool()));
                    let (next, mute) = change(level, muted);
                    let next = next.clamp(0.0, 1.0);
                    for session in &found {
                        let _ = session.volume.SetMasterVolume(next, std::ptr::null());
                        let _ = session.volume.SetMute(mute, std::ptr::null());
                    }
                    Some(Level {
                        key: key.clone(),
                        name: app_name(first),
                        icon: app_icon(first),
                        volume: next,
                        muted: mute,
                    })
                }
            }
        }
    }

    pub fn set_volume(target: &Target, volume: f32) -> Option<Level> {
        // Turning a muted level up unmutes it, as Windows' own slider does.
        change(target, |_, muted| (volume, muted && volume <= 0.0))
    }

    pub fn set_muted(target: &Target, muted: bool) -> Option<Level> {
        change(target, |level, _| (level, muted))
    }

    /// Moves `target` by `delta` (0–1), snapped to whole percents; going up unmutes.
    pub fn step(target: &Target, delta: f32) -> Option<Level> {
        change(target, |level, muted| {
            let next = ((level + delta) * 100.0).round() / 100.0;
            (next, muted && delta <= 0.0)
        })
    }

    // Windows has no public way to change the default device; this is the interface its own
    // sound settings use, as EarTrumpet and SoundSwitch do. Only SetDefaultEndpoint is
    // called; the slots before it only keep the vtable in order.
    #[allow(non_snake_case)]
    mod policy {
        use super::*;
        #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
        pub unsafe trait IPolicyConfig: IUnknown {
            fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
            fn GetDeviceFormat(
                &self,
                device: PCWSTR,
                default: i32,
                format: *mut *mut core::ffi::c_void,
            ) -> HRESULT;
            fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
            fn SetDeviceFormat(
                &self,
                device: PCWSTR,
                endpoint: *mut core::ffi::c_void,
                mix: *mut core::ffi::c_void,
            ) -> HRESULT;
            fn GetProcessingPeriod(
                &self,
                device: PCWSTR,
                default: i32,
                period: *mut i64,
                minimum: *mut i64,
            ) -> HRESULT;
            fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
            fn GetShareMode(&self, device: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
            fn SetShareMode(&self, device: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
            fn GetPropertyValue(
                &self,
                device: PCWSTR,
                fx: i32,
                key: *const PROPERTYKEY,
                value: *mut core::ffi::c_void,
            ) -> HRESULT;
            fn SetPropertyValue(
                &self,
                device: PCWSTR,
                fx: i32,
                key: *const PROPERTYKEY,
                value: *mut core::ffi::c_void,
            ) -> HRESULT;
            fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
            fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
        }

        const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

        /// Makes `id` the default output for everything: media, games and calls.
        pub fn set_default_device(id: &str) -> Result<(), String> {
            init();
            let wide: Vec<u16> = id.encode_utf16().chain([0]).collect();
            // SAFETY: a COM object made and called on this thread; the id outlives the calls.
            unsafe {
                let policy: IPolicyConfig =
                    CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                        .map_err(|err| err.message())?;
                for role in [eConsole, eMultimedia, eCommunications] {
                    policy
                        .SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role)
                        .ok()
                        .map_err(|err| err.message())?;
                }
            }
            Ok(())
        }
    }
    pub use policy::set_default_device;

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn stem_drops_the_folder_and_extension() {
            assert_eq!(stem(r"C:\Apps\Discord.exe"), "Discord");
            assert_eq!(stem(r"C:\Apps\GAME.EXE"), "GAME");
            assert_eq!(stem("tool"), "tool");
        }

        /// Prints this computer's mix: `cargo test prints_the_mix -- --ignored --nocapture`.
        #[test]
        #[ignore]
        fn prints_the_mix() {
            println!("{:#?}", snapshot());
        }
    }
}
