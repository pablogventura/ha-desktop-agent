use crate::config::Config;
use crate::entity::{truncate_ha_state, Value};
use crate::snapshot::Snapshot;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue};
use zbus::Connection;

pub type CaffeineLock = Arc<Mutex<Option<OwnedFd>>>;

pub fn new_caffeine_lock() -> CaffeineLock {
    Arc::new(Mutex::new(None))
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    fn inhibit(
        &self,
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedFd>;
    fn list_inhibitors(&self) -> zbus::Result<Vec<(String, String, String, String, u32, u32)>>;
    fn suspend(&self, interactive: bool) -> zbus::Result<()>;
    fn hibernate(&self, interactive: bool) -> zbus::Result<()>;
    #[zbus(name = "PowerOff")]
    fn power_off(&self, interactive: bool) -> zbus::Result<()>;
    fn reboot(&self, interactive: bool) -> zbus::Result<()>;
    fn lock_session(&self, session_id: &str) -> zbus::Result<()>;
    fn list_sessions(&self) -> zbus::Result<Vec<(String, u32, String, String, OwnedObjectPath)>>;
    fn get_session(&self, session_id: &str) -> zbus::Result<OwnedObjectPath>;
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(interface = "org.freedesktop.login1.Session")]
trait Login1Session {
    #[zbus(property, name = "Type")]
    fn session_type(&self) -> zbus::Result<String>;
    #[zbus(property, name = "IdleHint")]
    fn idle_hint(&self) -> zbus::Result<bool>;
    #[zbus(property, name = "IdleSinceHint")]
    fn idle_since_hint(&self) -> zbus::Result<u64>;
    #[zbus(property, name = "Id")]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property, name = "LockedHint")]
    fn locked_hint(&self) -> zbus::Result<bool>;
    #[zbus(property, name = "Class")]
    fn class(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.gnome.Mutter.IdleMonitor",
    default_service = "org.gnome.Mutter.IdleMonitor",
    default_path = "/org/gnome/Mutter/IdleMonitor/Core"
)]
trait MutterIdleMonitor {
    #[zbus(name = "GetIdletime")]
    fn get_idletime(&self) -> zbus::Result<u64>;
}

#[zbus::proxy(
    interface = "org.gnome.shell.extensions.FocusedWindow",
    default_service = "org.gnome.Shell",
    default_path = "/org/gnome/shell/extensions/FocusedWindow"
)]
trait FocusedWindow {
    #[zbus(name = "Get")]
    fn get(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.gnome.ScreenSaver",
    default_service = "org.gnome.ScreenSaver",
    default_path = "/org/gnome/ScreenSaver"
)]
trait GnomeScreenSaver {
    #[zbus(name = "GetActive")]
    fn get_active(&self) -> zbus::Result<bool>;
    fn lock(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.ScreenSaver",
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver"
)]
trait FreedesktopScreenSaver {
    fn lock(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.DBus",
    default_service = "org.freedesktop.DBus",
    default_path = "/org/freedesktop/DBus"
)]
trait FreedesktopDbus {
    fn list_names(&self) -> zbus::Result<Vec<String>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmDevice {
    #[zbus(property, name = "DeviceType")]
    fn device_type(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmWireless {
    #[zbus(property)]
    fn active_access_point(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmAccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;
    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;
}

#[zbus::proxy(interface = "org.mpris.MediaPlayer2.Player")]
trait MprisPlayer {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[String],
        hints: HashMap<String, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

pub struct LinuxSession {
    system: Option<Connection>,
    session: Option<Connection>,
    caffeine: CaffeineLock,
    mpris_owner: Mutex<Option<String>>,
}

impl LinuxSession {
    pub async fn connect(caffeine: CaffeineLock) -> Self {
        let system = match Connection::system().await {
            Ok(conn) => Some(conn),
            Err(err) => {
                warn!("system dbus unavailable: {err}");
                None
            }
        };
        let session = match Connection::session().await {
            Ok(conn) => Some(conn),
            Err(err) => {
                warn!("session dbus unavailable: {err}");
                None
            }
        };
        Self {
            system,
            session,
            caffeine,
            mpris_owner: Mutex::new(None),
        }
    }

    pub async fn collect(&self, config: &Config, snapshot: &mut Snapshot) {
        self.collect_desktop(snapshot);
        self.collect_idle(config, snapshot).await;
        self.collect_locked(snapshot).await;
        self.collect_inhibitors(snapshot).await;
        self.collect_focused_window(config, snapshot).await;
        if config.sensors.wifi {
            self.collect_wifi(snapshot).await;
        }
        if config.sensors.mpris {
            self.collect_mpris(snapshot).await;
        }
        let caffeine_on = self.caffeine.lock().await.is_some();
        snapshot.set("caffeine", Value::Bool(caffeine_on));
    }

    fn collect_desktop(&self, snapshot: &mut Snapshot) {
        if let Ok(de) = std::env::var("XDG_CURRENT_DESKTOP") {
            if !de.is_empty() {
                snapshot.set("desktop_environment", Value::Text(de));
            }
        }
        if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
            if !session.is_empty() {
                snapshot.set("session_type", Value::Text(session.to_ascii_lowercase()));
            }
        }
    }

    async fn collect_idle(&self, config: &Config, snapshot: &mut Snapshot) {
        if let Some(ms) = self.mutter_idle_ms().await {
            let seconds = (ms as f64) / 1000.0;
            snapshot.set("idle_time", Value::Number(seconds));
            snapshot.set(
                "user_active",
                Value::Bool(seconds < config.poll.idle_threshold_s as f64),
            );
            return;
        }
        if let Some((idle_hint, since_us)) = self.logind_idle().await {
            if let Some(since_us) = since_us {
                let now_us = unix_now_us();
                if now_us >= since_us {
                    let seconds = (now_us - since_us) as f64 / 1_000_000.0;
                    snapshot.set("idle_time", Value::Number(seconds));
                    snapshot.set(
                        "user_active",
                        Value::Bool(seconds < config.poll.idle_threshold_s as f64),
                    );
                    return;
                }
            }
            snapshot.set("user_active", Value::Bool(!idle_hint));
        }
    }

    async fn mutter_idle_ms(&self) -> Option<u64> {
        let conn = self.session.as_ref()?;
        let proxy = MutterIdleMonitorProxy::new(conn).await.ok()?;
        proxy.get_idletime().await.ok()
    }

    async fn logind_idle(&self) -> Option<(bool, Option<u64>)> {
        let conn = self.system.as_ref()?;
        let session = session_proxy(conn).await?;
        let hint = session.idle_hint().await.ok()?;
        let since = session.idle_since_hint().await.ok();
        Some((hint, since))
    }

    async fn collect_inhibitors(&self, snapshot: &mut Snapshot) {
        let Some(conn) = self.system.as_ref() else {
            return;
        };
        let Ok(proxy) = Login1ManagerProxy::new(conn).await else {
            return;
        };
        let Ok(list) = proxy.list_inhibitors().await else {
            debug!("ListInhibitors failed");
            return;
        };
        let reasons = sleep_block_reasons(&list);
        snapshot.set("suspend_inhibited", Value::Bool(!reasons.is_empty()));
        snapshot.set(
            "suspend_inhibit_reason",
            Value::Text(format_inhibit_reason(&reasons)),
        );
    }

    async fn collect_focused_window(&self, config: &Config, snapshot: &mut Snapshot) {
        if !config.sensors.active_application && !config.sensors.active_window_title {
            return;
        }
        let Some(conn) = self.session.as_ref() else {
            return;
        };
        let Ok(proxy) = FocusedWindowProxy::new(conn).await else {
            return;
        };
        let Ok(raw) = proxy.get().await else {
            return;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        if config.sensors.active_application {
            if let Some(class) = json
                .get("wm_class")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                snapshot.set("active_application", Value::Text(class.to_string()));
            }
        }
        if config.sensors.active_window_title {
            if let Some(title) = json
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                snapshot.set("active_window_title", Value::Text(title.to_string()));
            }
        }
    }

    pub async fn set_caffeine(&self, on: bool) -> anyhow::Result<()> {
        let mut slot = self.caffeine.lock().await;
        if on {
            if slot.is_some() {
                return Ok(());
            }
            let conn = self
                .system
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("system dbus unavailable"))?;
            let proxy = Login1ManagerProxy::new(conn).await?;
            let fd = proxy
                .inhibit("sleep:idle", "ha-desktop-agent", "Caffeine mode", "block")
                .await?;
            *slot = Some(fd);
        } else {
            *slot = None;
        }
        Ok(())
    }

    pub async fn power_action(&self, action: &str) -> anyhow::Result<()> {
        if action == "lock" {
            return self.lock_screen().await;
        }
        let conn = self
            .system
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("system dbus unavailable"))?;
        let proxy = Login1ManagerProxy::new(conn).await?;
        match action {
            "suspend" => proxy.suspend(false).await?,
            "hibernate" => proxy.hibernate(false).await?,
            "shutdown" => proxy.power_off(false).await?,
            "reboot" => proxy.reboot(false).await?,
            other => anyhow::bail!("unknown action {other}"),
        }
        Ok(())
    }

    pub async fn lock_screen(&self) -> anyhow::Result<()> {
        if let Some(conn) = self.session.as_ref() {
            if let Ok(proxy) = GnomeScreenSaverProxy::new(conn).await {
                if proxy.lock().await.is_ok() {
                    return Ok(());
                }
            }
            if let Ok(proxy) = FreedesktopScreenSaverProxy::new(conn).await {
                if proxy.lock().await.is_ok() {
                    return Ok(());
                }
            }
        }
        let conn = self
            .system
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("system dbus unavailable"))?;
        let proxy = Login1ManagerProxy::new(conn).await?;
        let session_id = graphical_session_id(conn).await?;
        proxy.lock_session(&session_id).await?;
        Ok(())
    }

    async fn collect_locked(&self, snapshot: &mut Snapshot) {
        if let Some(conn) = self.session.as_ref() {
            if let Ok(proxy) = GnomeScreenSaverProxy::new(conn).await {
                if let Ok(active) = proxy.get_active().await {
                    snapshot.set("locked", Value::Bool(active));
                    return;
                }
            }
        }
        let Some(conn) = self.system.as_ref() else {
            return;
        };
        if let Some(session) = session_proxy(conn).await {
            if let Ok(locked) = session.locked_hint().await {
                snapshot.set("locked", Value::Bool(locked));
            }
        }
    }

    async fn collect_wifi(&self, snapshot: &mut Snapshot) {
        let Some(conn) = self.system.as_ref() else {
            snapshot.set("wifi_ssid", Value::Unavailable);
            snapshot.set("wifi_signal", Value::Unavailable);
            return;
        };
        let Some((ssid, strength)) = wifi_active(conn).await else {
            snapshot.set("wifi_ssid", Value::Unavailable);
            snapshot.set("wifi_signal", Value::Unavailable);
            return;
        };
        if ssid.is_empty() {
            snapshot.set("wifi_ssid", Value::Unavailable);
        } else {
            snapshot.set("wifi_ssid", Value::Text(truncate_ha_state(&ssid)));
        }
        let dbm = f64::from(strength) - 100.0;
        snapshot.set("wifi_signal", Value::Number(dbm));
    }

    async fn collect_mpris(&self, snapshot: &mut Snapshot) {
        let Some(conn) = self.session.as_ref() else {
            return;
        };
        let owner = match pick_mpris_owner(conn).await {
            Some(owner) => owner,
            None => {
                *self.mpris_owner.lock().await = None;
                snapshot.set("media_title", Value::Unavailable);
                snapshot.set("media_artist", Value::Unavailable);
                snapshot.set("media_playing", Value::Bool(false));
                return;
            }
        };
        let Ok(proxy) = mpris_proxy(conn, &owner).await else {
            return;
        };
        let playing = proxy
            .playback_status()
            .await
            .ok()
            .map(|status| status.eq_ignore_ascii_case("Playing"))
            .unwrap_or(false);
        snapshot.set("media_playing", Value::Bool(playing));
        if let Ok(meta) = proxy.metadata().await {
            snapshot.set(
                "media_title",
                mpris_string(&meta, "xesam:title")
                    .map(|text| Value::Text(truncate_ha_state(&text)))
                    .unwrap_or(Value::Unavailable),
            );
            snapshot.set(
                "media_artist",
                mpris_artists(&meta)
                    .map(|text| Value::Text(truncate_ha_state(&text)))
                    .unwrap_or(Value::Unavailable),
            );
        }
        snapshot.set_attr("mpris_player", serde_json::json!(owner));
        *self.mpris_owner.lock().await = Some(owner);
    }

    pub async fn send_notify(&self, title: &str, body: &str, urgency: u8) -> anyhow::Result<()> {
        let conn = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session dbus unavailable"))?;
        let proxy = NotificationsProxy::new(conn).await?;
        let mut hints: HashMap<String, zbus::zvariant::Value<'_>> = HashMap::new();
        hints.insert("urgency".into(), zbus::zvariant::Value::U8(urgency));
        let expire_timeout = if urgency >= 2 { -1 } else { 5000 };
        proxy
            .notify(
                "ha-desktop-agent",
                0,
                "computer",
                title,
                body,
                &[],
                hints,
                expire_timeout,
            )
            .await?;
        Ok(())
    }

    pub async fn mpris_action(&self, action: &str) -> anyhow::Result<()> {
        let conn = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("session dbus unavailable"))?;
        let owner = {
            let cached = self.mpris_owner.lock().await.clone();
            match cached {
                Some(owner) => owner,
                None => pick_mpris_owner(conn)
                    .await
                    .ok_or_else(|| anyhow::anyhow!("no MPRIS player"))?,
            }
        };
        let proxy = mpris_proxy(conn, &owner).await?;
        match action {
            "media_play_pause" => proxy.play_pause().await?,
            "media_next" => proxy.next().await?,
            "media_previous" => proxy.previous().await?,
            other => anyhow::bail!("unknown mpris action {other}"),
        }
        Ok(())
    }
}

const NM_DEVICE_WIFI: u32 = 2;
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

async fn wifi_active(conn: &Connection) -> Option<(String, u8)> {
    let nm = NetworkManagerProxy::new(conn).await.ok()?;
    let devices = nm.devices().await.ok()?;
    for path in devices {
        let device = NmDeviceProxy::builder(conn)
            .path(&path)
            .ok()?
            .build()
            .await
            .ok()?;
        if device.device_type().await.ok()? != NM_DEVICE_WIFI {
            continue;
        }
        let wireless = NmWirelessProxy::builder(conn)
            .path(&path)
            .ok()?
            .build()
            .await
            .ok()?;
        let ap_path = wireless.active_access_point().await.ok()?;
        if ap_path.as_str() == "/" {
            continue;
        }
        let ap = NmAccessPointProxy::builder(conn)
            .path(&ap_path)
            .ok()?
            .build()
            .await
            .ok()?;
        let ssid = String::from_utf8_lossy(&ap.ssid().await.ok()?).into_owned();
        let strength = ap.strength().await.ok()?;
        return Some((ssid, strength));
    }
    None
}

async fn pick_mpris_owner(conn: &Connection) -> Option<String> {
    let dbus = FreedesktopDbusProxy::new(conn).await.ok()?;
    let names = dbus.list_names().await.ok()?;
    names
        .into_iter()
        .find(|name| name.starts_with(MPRIS_PREFIX) && !name.contains("playerctld"))
}

async fn mpris_proxy<'a>(
    conn: &'a Connection,
    owner: &str,
) -> anyhow::Result<MprisPlayerProxy<'a>> {
    Ok(MprisPlayerProxy::builder(conn)
        .destination(owner.to_owned())?
        .path("/org/mpris/MediaPlayer2")?
        .build()
        .await?)
}

fn owned_to_string(value: &OwnedValue) -> Option<String> {
    if let Ok(text) = <&str>::try_from(value) {
        return Some(text.to_string());
    }
    String::try_from(value.clone()).ok()
}

fn mpris_string(meta: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(owned_to_string)
        .filter(|text| !text.is_empty())
}

fn mpris_artists(meta: &HashMap<String, OwnedValue>) -> Option<String> {
    let value = meta.get("xesam:artist")?;
    if let Ok(list) = <Vec<String>>::try_from(value.clone()) {
        let joined = list.join(", ");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        owned_to_string(value).filter(|text| !text.is_empty())
    }
}

async fn graphical_session_id(conn: &Connection) -> anyhow::Result<String> {
    let manager = Login1ManagerProxy::new(conn).await?;
    let uid = unsafe { libc::getuid() };
    let sessions = manager.list_sessions().await?;
    let mut fallback = None;
    for (id, session_uid, _user, seat, path) in sessions {
        if session_uid != uid {
            continue;
        }
        let Ok(builder) = Login1SessionProxy::builder(conn).path(path) else {
            continue;
        };
        let Ok(session) = builder.build().await else {
            continue;
        };
        let class = session.class().await.unwrap_or_default();
        if class == "manager" {
            continue;
        }
        if !seat.is_empty() && seat != "-" {
            return Ok(id);
        }
        if class == "user" {
            fallback = Some(id);
        }
    }
    if let Some(id) = fallback {
        return Ok(id);
    }
    if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    anyhow::bail!("could not resolve graphical logind session")
}

async fn session_proxy<'a>(conn: &'a Connection) -> Option<Login1SessionProxy<'a>> {
    let manager = Login1ManagerProxy::new(conn).await.ok()?;
    let id = graphical_session_id(conn).await.ok()?;
    let path = manager.get_session(&id).await.ok()?;
    Login1SessionProxy::builder(conn)
        .path(path)
        .ok()?
        .build()
        .await
        .ok()
}

fn what_includes_sleep_or_idle(what: &str) -> bool {
    what.split(':')
        .any(|part| part == "sleep" || part == "idle")
}

fn is_blocking_mode(mode: &str) -> bool {
    mode == "block" || mode == "block-weak"
}

fn sleep_block_reasons(list: &[(String, String, String, String, u32, u32)]) -> Vec<String> {
    let mut reasons = Vec::new();
    for (what, who, why, mode, _, _) in list {
        if !what_includes_sleep_or_idle(what) || !is_blocking_mode(mode) {
            continue;
        }
        let line = if why.is_empty() {
            who.clone()
        } else if who.is_empty() {
            why.clone()
        } else {
            format!("{who}: {why}")
        };
        if !line.is_empty() && !reasons.iter().any(|existing| existing == &line) {
            reasons.push(line);
        }
    }
    reasons
}

fn format_inhibit_reason(reasons: &[String]) -> String {
    if reasons.is_empty() {
        return "none".into();
    }
    truncate_ha_state(&reasons.join("; "))
}

fn unix_now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> (String, String, String, String, u32, u32) {
        (what.into(), who.into(), why.into(), mode.into(), 0, 0)
    }

    #[test]
    fn ignores_delay_inhibitors() {
        let list = vec![
            item("sleep", "UPower", "Pause device polling", "delay"),
            item("sleep", "Cursor", "Cursor Agent trabajando", "block"),
            item("sleep", "pablo", "user session inhibited", "block-weak"),
        ];
        let reasons = sleep_block_reasons(&list);
        assert_eq!(
            reasons,
            vec![
                "Cursor: Cursor Agent trabajando".to_string(),
                "pablo: user session inhibited".to_string(),
            ]
        );
        let text = format_inhibit_reason(&reasons);
        assert!(text.chars().count() <= 255);
        assert!(text.contains("Cursor"));
    }

    #[test]
    fn none_when_only_delay() {
        let list = vec![item("sleep", "UPower", "Pause", "delay")];
        assert!(sleep_block_reasons(&list).is_empty());
        assert_eq!(format_inhibit_reason(&[]), "none");
    }

    #[test]
    fn truncates_long_reason() {
        let long = "x".repeat(400);
        let text = format_inhibit_reason(&[long]);
        assert_eq!(text.chars().count(), crate::entity::HA_STATE_MAX_CHARS);
        assert!(text.ends_with("..."));
    }
}
