//! Control Centre side effects: Windows media sessions (SMTC), master volume,
//! radio (Wi-Fi/Bluetooth), desktop wallpaper and app theme.
//!
//! Every entry point initializes COM on its own thread because Tauri command
//! handlers run on a pool and WinRT/Core Audio both need an apartment.

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::{fs, io::BufWriter};

use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, LPARAM, LUID, WPARAM, ERROR_NOT_ALL_ASSIGNED,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, TOKEN_PRIVILEGES,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_QUERY, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{
    ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_REBOOT, EWX_POWEROFF, SHTDN_REASON_MAJOR_OTHER,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, SystemParametersInfoW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER, WM_SETTINGCHANGE,
};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

#[derive(Debug, Clone, Serialize)]
pub struct ControlSnapshot {
    pub media: Option<MediaSnapshot>,
    pub volume: f64,
    pub muted: bool,
    pub volume_supported: bool,
    pub wifi: ToggleState,
    pub bluetooth: ToggleState,
    pub dark_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToggleState {
    pub supported: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_seconds: Option<u64>,
    pub duration_seconds: Option<u64>,
    pub artwork: Option<String>,
}

impl ControlSnapshot {
    pub fn read() -> Self {
        init_com();

        let (volume, muted, volume_supported) = match volume_state() {
            Ok((volume, muted)) => (volume, muted, true),
            Err(_) => (0.0, false, false),
        };
        // One radio enumeration feeds both toggles; WinRT access can be slow
        // and the panel polls this every couple of seconds.
        let radios = radios().ok();
        let toggle = |kind: RadioKind| -> ToggleState {
            let Some(list) = radios.as_ref() else {
                return ToggleState {
                    supported: false,
                    enabled: false,
                };
            };
            match list.iter().find(|radio| radio.Kind().ok() == Some(kind)) {
                Some(radio) => ToggleState {
                    supported: true,
                    enabled: radio.State().ok() == Some(RadioState::On),
                },
                None => ToggleState {
                    supported: false,
                    enabled: false,
                },
            }
        };

        Self {
            media: media_state(),
            volume,
            muted,
            volume_supported,
            wifi: toggle(RadioKind::WiFi),
            bluetooth: toggle(RadioKind::Bluetooth),
            dark_mode: dark_mode_enabled(),
        }
    }
}

fn init_com() {
    // RPC_E_CHANGED_MODE only means the thread already has an apartment.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

// ---------------------------------------------------------------------------
// Media (SMTC)
// ---------------------------------------------------------------------------

fn media_session() -> windows::core::Result<GlobalSystemMediaTransportControlsSession> {
    // Commands run on pooled threads, so the apartment has to be set up here
    // rather than relying on the snapshot read that happened earlier.
    init_com();
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    manager.GetCurrentSession()
}

fn media_state() -> Option<MediaSnapshot> {
    let session = media_session().ok()?;
    let properties = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let playback = session.GetPlaybackInfo().ok()?;
    let status = playback.PlaybackStatus().ok()?;
    let timeline = session.GetTimelineProperties().ok();
    let (position_seconds, duration_seconds) = timeline
        .and_then(|timeline| {
            let start = timeline.StartTime().ok()?.Duration;
            let end = timeline.EndTime().ok()?.Duration;
            let position = timeline.Position().ok()?.Duration;
            let duration = end.saturating_sub(start);
            if duration <= 0 {
                return None;
            }

            let position = position.saturating_sub(start).clamp(0, duration);
            Some((
                Some((position / 10_000_000) as u64),
                Some((duration / 10_000_000) as u64),
            ))
        })
        .unwrap_or((None, None));

    Some(MediaSnapshot {
        artwork: media_artwork(&properties),
        title: properties
            .Title()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        artist: properties
            .Artist()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        album: properties
            .AlbumTitle()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        playing: status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing,
        position_seconds,
        duration_seconds,
    })
}

fn media_artwork(
    properties: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<String> {
    let stream = properties
        .Thumbnail()
        .ok()?
        .OpenReadAsync()
        .ok()?
        .get()
        .ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 2_000_000 {
        return None;
    }
    let mime = stream.ContentType().ok()?.to_string();
    if !matches!(mime.as_str(), "image/png" | "image/jpeg" | "image/webp") {
        return None;
    }
    let input = stream.GetInputStreamAt(0).ok()?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&input).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.get().ok()?;
    if loaded != size as u32 {
        return None;
    }
    let mut data = vec![0; size as usize];
    reader.ReadBytes(&mut data).ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(data)))
}

pub fn media_toggle() -> Result<()> {
    let session = media_session().map_err(media_error)?;
    session
        .TryTogglePlayPauseAsync()
        .and_then(|operation| operation.get())
        .map_err(media_error)?;
    Ok(())
}

pub fn media_next() -> Result<()> {
    let session = media_session().map_err(media_error)?;
    session
        .TrySkipNextAsync()
        .and_then(|operation| operation.get())
        .map_err(media_error)?;
    Ok(())
}

pub fn media_previous() -> Result<()> {
    let session = media_session().map_err(media_error)?;
    session
        .TrySkipPreviousAsync()
        .and_then(|operation| operation.get())
        .map_err(media_error)?;
    Ok(())
}

fn media_error(err: windows::core::Error) -> anyhow::Error {
    if err.code().0 == 0x8007_0434u32 as i32 {
        anyhow!("No app is currently reporting media playback")
    } else {
        anyhow!("Media control failed: {err}")
    }
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

fn endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

fn volume_state() -> windows::core::Result<(f64, bool)> {
    let endpoint = endpoint_volume()?;
    unsafe {
        let level = endpoint.GetMasterVolumeLevelScalar()? as f64;
        let muted = endpoint.GetMute()?.as_bool();
        Ok((level, muted))
    }
}

pub fn set_volume(level: f64) -> Result<()> {
    init_com();
    let level = level.clamp(0.0, 1.0) as f32;
    let endpoint = endpoint_volume().map_err(|err| anyhow!("Audio endpoint: {err}"))?;
    unsafe {
        endpoint
            .SetMasterVolumeLevelScalar(level, std::ptr::null())
            .map_err(|err| anyhow!("Set volume: {err}"))?;
    }
    Ok(())
}

pub fn set_muted(muted: bool) -> Result<()> {
    init_com();
    let endpoint = endpoint_volume().map_err(|err| anyhow!("Audio endpoint: {err}"))?;
    unsafe {
        endpoint
            .SetMute(muted, std::ptr::null())
            .map_err(|err| anyhow!("Mute: {err}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Radios
// ---------------------------------------------------------------------------

fn radios() -> Result<Vec<Radio>> {
    init_com();
    let access = Radio::RequestAccessAsync()
        .map_err(|err| anyhow!("Radio access: {err}"))?
        .get()
        .map_err(|err| anyhow!("Radio access: {err}"))?;
    if access != RadioAccessStatus::Allowed {
        return Err(anyhow!("Windows denied radio access ({access:?})"));
    }

    let list = Radio::GetRadiosAsync()
        .map_err(|err| anyhow!("List radios: {err}"))?
        .get()
        .map_err(|err| anyhow!("List radios: {err}"))?;

    let mut result = Vec::new();
    for index in 0..list
        .Size()
        .map_err(|err| anyhow!("Read radio count: {err}"))?
    {
        if let Ok(radio) = list.GetAt(index) {
            result.push(radio);
        }
    }
    Ok(result)
}

pub fn set_radio(kind: RadioKind, enabled: bool) -> Result<()> {
    let list = radios()?;
    let mut found = false;

    for radio in list {
        if radio.Kind().ok() != Some(kind) {
            continue;
        }
        found = true;
        let target = if enabled {
            RadioState::On
        } else {
            RadioState::Off
        };
        let status = radio
            .SetStateAsync(target)
            .map_err(|err| anyhow!("Change the radio state: {err}"))?
            .get()
            .map_err(|err| anyhow!("Change the radio state: {err}"))?;
        if status != RadioAccessStatus::Allowed {
            return Err(anyhow!(
                "Windows refused the radio change ({status:?}); run Murmur without restrictions"
            ));
        }
    }

    if !found {
        return Err(anyhow!("No matching radio was found"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows app theme
// ---------------------------------------------------------------------------

fn dark_mode_enabled() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(PERSONALIZE_KEY, KEY_READ)
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|light| light == 0)
        .unwrap_or(false)
}

pub fn set_dark_mode(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(PERSONALIZE_KEY)
        .map_err(|err| anyhow!("Open the personalization key: {err}"))?;
    let _ = key.set_value("AppsUseLightTheme", &u32::from(!enabled));
    let _ = key.set_value("SystemUsesLightTheme", &u32::from(!enabled));
    drop(key);

    // Tell already-running apps that the immersive colour set changed.
    let setting: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(setting.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5_000,
            None,
        );
    }
    Ok(())
}

pub fn set_wallpaper(data_dir: &Path, preset: &str) -> Result<()> {
    let filename = wallpaper_filename(preset)?;
    let wallpaper_dir = data_dir.join("wallpapers");
    fs::create_dir_all(&wallpaper_dir)
        .map_err(|err| anyhow!("Create the wallpaper directory: {err}"))?;
    let path = wallpaper_dir.join(filename);

    if !path.exists() {
        match preset {
            "terraces" => fs::write(
                &path,
                include_bytes!("../../public/images/terraced-fields.jpg"),
            )
            .map_err(|err| anyhow!("Save the Terraced fields wallpaper: {err}"))?,
            "midnight" => write_gradient_wallpaper(&path, [48, 63, 96], [20, 25, 34])?,
            "forest" => write_gradient_wallpaper(&path, [71, 98, 85], [23, 36, 31])?,
            "dusk" => write_gradient_wallpaper(&path, [118, 90, 105], [37, 36, 50])?,
            _ => unreachable!("wallpaper preset was validated"),
        }
    }

    set_wallpaper_file(&path)
}

pub fn set_wallpaper_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(anyhow!("Wallpaper file does not exist"));
    }
    let wallpaper: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wallpaper.as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .map_err(|err| anyhow!("Set the Windows wallpaper: {err}"))?;
    Ok(())
}

fn wallpaper_filename(preset: &str) -> Result<&'static str> {
    match preset {
        "terraces" => Ok("terraced-fields.jpg"),
        "midnight" => Ok("midnight.png"),
        "forest" => Ok("forest.png"),
        "dusk" => Ok("dusk.png"),
        other => Err(anyhow!("Unknown wallpaper preset: {other}")),
    }
}

fn write_gradient_wallpaper(path: &Path, start: [u8; 3], end: [u8; 3]) -> Result<()> {
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT * 3);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let amount = (x as f32 / (WIDTH - 1) as f32 + y as f32 / (HEIGHT - 1) as f32) / 2.0;
            for channel in 0..3 {
                pixels.push(
                    (start[channel] as f32 * (1.0 - amount) + end[channel] as f32 * amount) as u8,
                );
            }
        }
    }

    let file = fs::File::create(path).map_err(|err| anyhow!("Create wallpaper image: {err}"))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), WIDTH as u32, HEIGHT as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&pixels))
        .map_err(|err| anyhow!("Encode wallpaper image: {err}"))?;
    Ok(())
}

fn with_shutdown_privilege(action: impl FnOnce() -> Result<()>) -> Result<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().map_err(|_| anyhow!("Power action lock poisoned"))?;
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)?;
    }
    let result = (|| -> Result<()> {
        let mut luid = LUID::default();
        unsafe { LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid)? };
        let requested = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut previous = TOKEN_PRIVILEGES::default();
        let mut returned = 0;
        unsafe {
            AdjustTokenPrivileges(
                token, false, Some(&requested),
                std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                Some(&mut previous), Some(&mut returned),
            )?;
            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err(anyhow!("This Windows account cannot change the system power state"));
            }
        }
        let result = action();
        let restored = unsafe { AdjustTokenPrivileges(token, false, Some(&previous), 0, None, None) };
        if let Err(err) = restored {
            return Err(anyhow!("Could not restore the Windows shutdown privilege: {err}; action result: {result:?}"));
        }
        result
    })();
    unsafe { CloseHandle(token)? };
    result
}

pub fn power_action(action: &str) -> Result<()> {
    let execute = || match action {
        "lock" => unsafe { LockWorkStation().map_err(|err| anyhow!("Lock Windows: {err}")) },
        "sleep" => {
            let suspended = unsafe { SetSuspendState(false, false, false) };
            if suspended.0 == 0 {
                return Err(anyhow!("Windows could not enter sleep mode"));
            }
            Ok(())
        }
        "sign_out" => unsafe {
            ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_MAJOR_OTHER)
                .map_err(|err| anyhow!("Sign out of Windows: {err}"))
        },
        "restart" => unsafe {
            ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OTHER)
                .map_err(|err| anyhow!("Restart Windows: {err}"))
        },
        "shutdown" => unsafe {
            ExitWindowsEx(EWX_POWEROFF, SHTDN_REASON_MAJOR_OTHER)
                .map_err(|err| anyhow!("Shut down Windows: {err}"))
        },
        other => Err(anyhow!("Unknown power action: {other}")),
    };
    if matches!(action, "sleep" | "restart" | "shutdown") {
        with_shutdown_privilege(execute)
    } else {
        execute()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shutdown_privilege_enabled() -> bool {
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::Security::{PrivilegeCheck, PRIVILEGE_SET};
        let mut token = HANDLE::default();
        let mut luid = LUID::default();
        unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).unwrap();
            LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid).unwrap();
            let mut privileges = PRIVILEGE_SET {
                PrivilegeCount: 1,
                Control: 1,
                Privilege: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
            };
            let mut enabled = BOOL::default();
            let result = PrivilegeCheck(token, &mut privileges, &mut enabled);
            CloseHandle(token).unwrap();
            result.unwrap();
            enabled.as_bool()
        }
    }

    #[test]
    fn shutdown_privilege_is_restored_after_an_action_error() {
        let before = shutdown_privilege_enabled();
        let mut called = false;
        let error = with_shutdown_privilege(|| {
            called = true;
            assert!(shutdown_privilege_enabled());
            Err(anyhow!("simulated power action failure"))
        }).unwrap_err();
        assert_eq!(shutdown_privilege_enabled(), before);
        if called {
            assert_eq!(error.to_string(), "simulated power action failure");
        } else {
            assert!(error.to_string().contains("account cannot change"), "{error:#}");
        }
        assert!(power_action("invalid").is_err());
    }

    #[test]
    fn wallpaper_presets_resolve_only_to_bundled_files() {
        assert_eq!(
            wallpaper_filename("terraces").unwrap(),
            "terraced-fields.jpg"
        );
        assert_eq!(wallpaper_filename("forest").unwrap(), "forest.png");
        assert!(wallpaper_filename("C:\\custom.jpg").is_err());
    }
}
