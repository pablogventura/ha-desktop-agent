use crate::config::Config;
use crate::entity::{truncate_ha_state, Value};
use crate::ipc::{IpcMessage, IpcValue};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::w;
use windows::core::{Interface, Result as WinResult, HSTRING};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, MAX_PATH};
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, SwitchDesktop, DESKTOP_SWITCHDESKTOP,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetLastInputInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::LASTINPUTINFO;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

const AUMID: windows::core::PCWSTR = w!("ha-desktop-agent.toast");

pub struct SessionState {
    caffeine: AtomicBool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            caffeine: AtomicBool::new(false),
        }
    }
}

pub fn init_com() -> WinResult<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(AUMID) };
    Ok(())
}

pub fn collect_session(config: &Config, state: &SessionState) -> HashMap<String, IpcValue> {
    let mut values = HashMap::new();
    let caffeine = state.caffeine.load(Ordering::Relaxed);
    values.insert("caffeine".into(), IpcValue::Bool(caffeine));
    values.insert("suspend_inhibited".into(), IpcValue::Bool(caffeine));
    values.insert("session_type".into(), IpcValue::Text("windows".into()));
    values.insert(
        "desktop_environment".into(),
        IpcValue::Text("Windows".into()),
    );

    match idle_seconds() {
        Some(seconds) => {
            values.insert("idle_time".into(), IpcValue::Number(seconds));
            values.insert("user_active".into(), IpcValue::Bool(seconds < 60.0));
        }
        None => {
            values.insert("idle_time".into(), IpcValue::Null);
            values.insert("user_active".into(), IpcValue::Null);
        }
    }
    values.insert(
        "locked".into(),
        match workstation_locked() {
            Some(locked) => IpcValue::Bool(locked),
            None => IpcValue::Null,
        },
    );

    if config.sensors.audio {
        match audio_state() {
            Ok((volume, muted, sink)) => {
                values.insert("volume".into(), IpcValue::Number(volume));
                values.insert("muted".into(), IpcValue::Bool(muted));
                match sink {
                    Some(name) => values.insert("audio_sink".into(), IpcValue::Text(name)),
                    None => values.insert("audio_sink".into(), IpcValue::Null),
                };
            }
            Err(_) => {
                values.insert("volume".into(), IpcValue::Null);
                values.insert("muted".into(), IpcValue::Null);
                values.insert("audio_sink".into(), IpcValue::Null);
            }
        }
    }

    if config.sensors.mpris {
        match smtc_state() {
            Some((title, artist, playing)) => {
                values.insert(
                    "media_title".into(),
                    title.map(IpcValue::Text).unwrap_or(IpcValue::Null),
                );
                values.insert(
                    "media_artist".into(),
                    artist.map(IpcValue::Text).unwrap_or(IpcValue::Null),
                );
                values.insert("media_playing".into(), IpcValue::Bool(playing));
            }
            None => {
                values.insert("media_title".into(), IpcValue::Null);
                values.insert("media_artist".into(), IpcValue::Null);
                values.insert("media_playing".into(), IpcValue::Bool(false));
            }
        }
    }

    if config.sensors.active_window_title || config.sensors.active_application {
        let (app, title) = focused_window();
        if config.sensors.active_application {
            values.insert(
                "active_application".into(),
                app.map(IpcValue::Text).unwrap_or(IpcValue::Null),
            );
        }
        if config.sensors.active_window_title {
            values.insert(
                "active_window_title".into(),
                title.map(IpcValue::Text).unwrap_or(IpcValue::Null),
            );
        }
    }

    values
}

pub fn handle_rpc(state: &SessionState, msg: &IpcMessage) -> IpcMessage {
    let IpcMessage::Rpc {
        id,
        method,
        title,
        body,
        urgent,
        on,
        delta,
        action,
    } = msg
    else {
        return IpcMessage::Result {
            id: 0,
            ok: false,
            error: Some("expected rpc".into()),
        };
    };
    let result = match method.as_str() {
        "notify" => send_toast(title.as_deref(), body.as_deref(), *urgent),
        "lock" => lock_workstation(),
        "set_mute" => match on {
            Some(muted) => set_muted(*muted),
            None => Err(anyhow::anyhow!("missing on")),
        },
        "set_caffeine" => match on {
            Some(value) => {
                state.caffeine.store(*value, Ordering::Relaxed);
                set_caffeine(*value)
            }
            None => Err(anyhow::anyhow!("missing on")),
        },
        "bump_volume" => bump_volume(delta.unwrap_or(0)),
        "mpris" => smtc_action(action.as_deref().unwrap_or("")),
        other => Err(anyhow::anyhow!("unknown method {other}")),
    };
    match result {
        Ok(()) => IpcMessage::Result {
            id: *id,
            ok: true,
            error: None,
        },
        Err(err) => IpcMessage::Result {
            id: *id,
            ok: false,
            error: Some(err.to_string()),
        },
    }
}

fn idle_seconds() -> Option<f64> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() != true {
        return None;
    }
    let now = unsafe { GetTickCount() };
    let idle = now.wrapping_sub(info.dwTime);
    Some(f64::from(idle) / 1000.0)
}

fn workstation_locked() -> Option<bool> {
    unsafe {
        let desktop = OpenInputDesktop(0, false, DESKTOP_SWITCHDESKTOP).ok()?;
        let switched = SwitchDesktop(desktop).is_ok();
        let _ = CloseDesktop(desktop);
        Some(!switched)
    }
}

fn audio_endpoint() -> WinResult<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

fn audio_state() -> anyhow::Result<(f64, bool, Option<String>)> {
    unsafe {
        let volume = audio_endpoint()?;
        let mut scalar = 0f32;
        volume.GetMasterVolumeLevelScalar(&mut scalar)?;
        let mut muted = windows::Win32::Foundation::BOOL(0);
        volume.GetMute(&mut muted)?;
        Ok((
            (f64::from(scalar) * 100.0).clamp(0.0, 100.0),
            muted.as_bool(),
            Some("Default".into()),
        ))
    }
}

fn set_muted(muted: bool) -> anyhow::Result<()> {
    unsafe {
        audio_endpoint()?.SetMute(muted, std::ptr::null())?;
    }
    Ok(())
}

fn bump_volume(delta: i32) -> anyhow::Result<()> {
    unsafe {
        let endpoint = audio_endpoint()?;
        let mut scalar = 0f32;
        endpoint.GetMasterVolumeLevelScalar(&mut scalar)?;
        let next = (scalar + delta as f32 / 100.0).clamp(0.0, 1.0);
        endpoint.SetMasterVolumeLevelScalar(next, std::ptr::null())?;
    }
    Ok(())
}

fn set_caffeine(on: bool) -> anyhow::Result<()> {
    let state = if on {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe {
        SetThreadExecutionState(state);
    }
    Ok(())
}

fn lock_workstation() -> anyhow::Result<()> {
    unsafe {
        LockWorkStation().map_err(|err| anyhow::anyhow!("{err}"))?;
    }
    Ok(())
}

fn send_toast(title: Option<&str>, body: Option<&str>, urgent: bool) -> anyhow::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    let title = truncate_ha_state(title.unwrap_or("ha-desktop-agent"));
    let body = truncate_ha_state(body.unwrap_or(""));
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(&title),
        xml_escape(&body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    if urgent {
        let _ = toast.SetPriority(windows::UI::Notifications::ToastNotificationPriority::High);
    }
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
        "ha-desktop-agent.toast",
    ))?;
    notifier.Show(&toast)?;
    Ok(())
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn focused_window() -> (Option<String>, Option<String>) {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return (None, None);
        }
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = if n > 0 {
            Some(truncate_ha_state(&String::from_utf16_lossy(
                &buf[..n as usize],
            )))
        } else {
            None
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let app = process_base_name(pid);
        (app, title)
    }
}

fn process_base_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut size = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    ok.ok()?;
    let path = String::from_utf16_lossy(&buf[..size as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|stem| truncate_ha_state(&stem.to_string_lossy()))
}

fn smtc_state() -> Option<(Option<String>, Option<String>, bool)> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .get()
        .ok()?;
    let session = manager.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props
        .Title()
        .ok()
        .map(|s| truncate_ha_state(&s.to_string()));
    let artist = props
        .Artist()
        .ok()
        .map(|s| truncate_ha_state(&s.to_string()));
    let playing = session
        .GetPlaybackInfo()
        .ok()
        .and_then(|info| info.PlaybackStatus().ok())
        .map(|status| {
            status == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
        })
        .unwrap_or(false);
    Some((title, artist, playing))
}

fn smtc_action(action: &str) -> anyhow::Result<()> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    let session = manager.GetCurrentSession()?;
    let controls = session.GetPlaybackInfo()?.Controls()?;
    match action {
        "media_play_pause" => {
            let _ = controls.TryTogglePlayPauseAsync()?.get()?;
        }
        "media_next" => {
            let _ = controls.TrySkipNextAsync()?.get()?;
        }
        "media_previous" => {
            let _ = controls.TrySkipPreviousAsync()?.get()?;
        }
        other => anyhow::bail!("unknown mpris action {other}"),
    }
    Ok(())
}

#[allow(dead_code)]
fn _hwnd(_h: HWND) {}

pub fn snapshot_message(config: &Config, state: &SessionState) -> IpcMessage {
    IpcMessage::Snapshot {
        values: collect_session(config, state),
    }
}

pub fn run_session_loop(config: crate::config::Config) -> anyhow::Result<()> {
    super::pipe::run_session_client(config)
}
