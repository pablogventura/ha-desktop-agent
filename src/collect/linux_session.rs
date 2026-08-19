use crate::config::Config;
use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use zbus::zvariant::OwnedFd;
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

pub struct LinuxSession {
    system: Option<Connection>,
    session: Option<Connection>,
    caffeine: CaffeineLock,
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
        }
    }

    pub async fn collect(&self, config: &Config, snapshot: &mut Snapshot) {
        self.collect_desktop(snapshot);
        self.collect_idle(config, snapshot).await;
        self.collect_inhibitors(snapshot).await;
        self.collect_focused_window(config, snapshot).await;
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
            "lock" => {
                let session_id = session_id(conn).await?;
                proxy.lock_session(&session_id).await?;
            }
            other => anyhow::bail!("unknown action {other}"),
        }
        Ok(())
    }
}

async fn session_id(conn: &Connection) -> anyhow::Result<String> {
    if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let session = session_proxy(conn)
        .await
        .ok_or_else(|| anyhow::anyhow!("could not resolve logind session"))?;
    Ok(session.id().await?)
}

async fn session_proxy<'a>(conn: &'a Connection) -> Option<Login1SessionProxy<'a>> {
    let manager = Login1ManagerProxy::new(conn).await.ok()?;
    let path = manager.get_session_by_pid(std::process::id()).await.ok()?;
    Login1SessionProxy::builder(conn)
        .path(path)
        .ok()?
        .build()
        .await
        .ok()
}

/// Home Assistant rejects entity states longer than 255 characters.
const HA_STATE_MAX_CHARS: usize = 255;

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

fn truncate_ha_state(value: &str) -> String {
    if value.chars().count() <= HA_STATE_MAX_CHARS {
        return value.to_string();
    }
    let keep = HA_STATE_MAX_CHARS.saturating_sub(3);
    let mut out: String = value.chars().take(keep).collect();
    out.push_str("...");
    out
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

    fn item(what: &str, who: &str, why: &str, mode: &str) -> (String, String, String, String, u32, u32) {
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
        assert_eq!(text.chars().count(), 255);
        assert!(text.ends_with("..."));
    }
}

