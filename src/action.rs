use crate::config::{CommandSpec, Config};
use crate::entity::truncate_ha_state;
use crate::update::UpdateController;
use serde_json::{Map, Value as JsonValue};
use std::time::Duration;
use tokio::process::Command;
use tracing::{info, warn};

#[cfg(target_os = "linux")]
use crate::collect::linux_session::LinuxSession;
#[cfg(target_os = "windows")]
use crate::collect::windows::SessionHub;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_NOTIFY_PAYLOAD_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationUrgency {
    Normal,
    Critical,
}

impl NotificationUrgency {
    pub fn dbus_byte(self) -> u8 {
        match self {
            Self::Normal => 1,
            Self::Critical => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub enum IncomingCommand {
    Switch {
        id: String,
        on: bool,
    },
    Press {
        id: String,
    },
    Notify {
        title: Option<String>,
        body: Option<String>,
        urgency: NotificationUrgency,
    },
}

impl IncomingCommand {
    pub fn parse(entity_id: &str, payload: &str) -> Option<Self> {
        if payload.len() > MAX_NOTIFY_PAYLOAD_BYTES {
            return None;
        }
        match entity_id {
            "notify_message" => return parse_notify_payload(payload, NotificationUrgency::Normal),
            "notify_urgent" => return parse_notify_payload(payload, NotificationUrgency::Critical),
            _ => {}
        }
        let payload = payload.trim();
        let upper = payload.to_ascii_uppercase();
        match upper.as_str() {
            "ON" | "TRUE" | "1" => Some(Self::Switch {
                id: entity_id.to_string(),
                on: true,
            }),
            "OFF" | "FALSE" | "0" => Some(Self::Switch {
                id: entity_id.to_string(),
                on: false,
            }),
            "PRESS" | "PRESSED" | "LOCK" | "" => Some(Self::Press {
                id: entity_id.to_string(),
            }),
            _ => None,
        }
    }
}

fn parse_notify_payload(payload: &str, urgency: NotificationUrgency) -> Option<IncomingCommand> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Some(IncomingCommand::Notify {
            title: None,
            body: None,
            urgency,
        });
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let value: JsonValue = serde_json::from_str(trimmed).ok()?;
        let JsonValue::Object(object) = value else {
            return None;
        };
        return Some(IncomingCommand::Notify {
            title: json_text(&object, "title"),
            body: json_text(&object, "body").or_else(|| json_text(&object, "message")),
            urgency,
        });
    }
    Some(IncomingCommand::Notify {
        title: None,
        body: Some(trimmed.to_string()),
        urgency,
    })
}

fn json_text(object: &Map<String, JsonValue>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

pub struct ActionRouter<'a> {
    config: &'a Config,
    #[cfg(target_os = "linux")]
    session: Option<&'a LinuxSession>,
    #[cfg(target_os = "windows")]
    hub: Option<&'a SessionHub>,
    update: Option<&'a UpdateController>,
}

impl<'a> ActionRouter<'a> {
    pub fn new(
        config: &'a Config,
        #[cfg(target_os = "linux")] session: Option<&'a LinuxSession>,
        #[cfg(target_os = "windows")] hub: Option<&'a SessionHub>,
        update: Option<&'a UpdateController>,
    ) -> Self {
        Self {
            config,
            #[cfg(target_os = "linux")]
            session,
            #[cfg(target_os = "windows")]
            hub,
            update,
        }
    }

    pub async fn handle(&self, command: IncomingCommand) -> anyhow::Result<()> {
        match command {
            IncomingCommand::Switch { id, on } => self.handle_switch(&id, on).await,
            IncomingCommand::Press { id } => self.handle_press(&id).await,
            IncomingCommand::Notify {
                title,
                body,
                urgency,
            } => self.handle_notify(title, body, urgency).await,
        }
    }

    async fn handle_switch(&self, id: &str, on: bool) -> anyhow::Result<()> {
        match id {
            "caffeine" => {
                if !self.config.action_enabled("caffeine") {
                    anyhow::bail!("caffeine is disabled in config");
                }
                #[cfg(target_os = "linux")]
                if let Some(session) = self.session {
                    session.set_caffeine(on).await?;
                    info!(on, "caffeine updated");
                    return Ok(());
                }
                #[cfg(target_os = "windows")]
                if let Some(hub) = self.hub {
                    let command = IncomingCommand::Switch {
                        id: "caffeine".into(),
                        on,
                    };
                    if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                        hub.call(rpc).await?;
                        info!(on, "caffeine updated");
                        return Ok(());
                    }
                }
                anyhow::bail!("caffeine is not supported on this platform");
            }
            "mute" => {
                if !self.config.action_enabled("mute") {
                    anyhow::bail!("mute is disabled in config");
                }
                #[cfg(target_os = "linux")]
                {
                    crate::collect::audio::set_muted(on).await?;
                    info!(on, "mute updated");
                    return Ok(());
                }
                #[cfg(target_os = "windows")]
                if let Some(hub) = self.hub {
                    let command = IncomingCommand::Switch {
                        id: "mute".into(),
                        on,
                    };
                    if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                        hub.call(rpc).await?;
                        info!(on, "mute updated");
                        return Ok(());
                    }
                }
                #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                anyhow::bail!("mute is not supported on this platform");
                #[cfg(target_os = "windows")]
                anyhow::bail!("mute requires the session helper");
            }
            "do_not_disturb" => {
                if !self.config.action_enabled("do_not_disturb") {
                    anyhow::bail!("do_not_disturb is disabled in config");
                }
                #[cfg(target_os = "linux")]
                {
                    crate::collect::dnd::set_dnd(on).await?;
                    info!(on, "do_not_disturb updated");
                    return Ok(());
                }
                #[cfg(not(target_os = "linux"))]
                anyhow::bail!("do_not_disturb is not supported on this platform");
            }
            "lock" => {
                if !on {
                    return Ok(());
                }
                if !self.config.action_enabled("lock") {
                    anyhow::bail!("lock is disabled in config");
                }
                #[cfg(target_os = "linux")]
                if let Some(session) = self.session {
                    session.lock_screen().await?;
                    info!("lock requested");
                    return Ok(());
                }
                #[cfg(target_os = "windows")]
                if let Some(hub) = self.hub {
                    let command = IncomingCommand::Press { id: "lock".into() };
                    if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                        hub.call(rpc).await?;
                        info!("lock requested");
                        return Ok(());
                    }
                }
                anyhow::bail!("lock is not supported on this platform");
            }
            "update_auto" => {
                if !self.config.action_enabled("update_auto") {
                    anyhow::bail!("update is disabled in config");
                }
                let Some(update) = self.update else {
                    anyhow::bail!("update controller unavailable");
                };
                update.set_auto(on);
                info!(on, "update_auto updated");
                Ok(())
            }
            other => {
                warn!("ignored switch for unknown entity {other}");
                Ok(())
            }
        }
    }

    async fn handle_notify(
        &self,
        title: Option<String>,
        body: Option<String>,
        urgency: NotificationUrgency,
    ) -> anyhow::Result<()> {
        if !self.config.action_enabled("notify") {
            anyhow::bail!("notify is disabled in config");
        }
        let title = truncate_ha_state(title.as_deref().unwrap_or(&self.config.notify.title));
        let body = truncate_ha_state(body.as_deref().unwrap_or(&self.config.notify.body));
        #[cfg(target_os = "linux")]
        if let Some(session) = self.session {
            session
                .send_notify(&title, &body, urgency.dbus_byte())
                .await?;
            info!(?urgency, "desktop notification sent");
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        if let Some(hub) = self.hub {
            let command = IncomingCommand::Notify {
                title: Some(title.clone()),
                body: Some(body.clone()),
                urgency,
            };
            if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                hub.call(rpc).await?;
                info!(?urgency, "desktop notification sent");
                return Ok(());
            }
        }
        anyhow::bail!("notify is not supported on this platform");
    }

    async fn handle_press(&self, id: &str) -> anyhow::Result<()> {
        if let Some(spec) = self.config.commands.iter().find(|command| command.id == id) {
            return run_command(spec).await;
        }
        if !self.config.action_enabled(id) {
            anyhow::bail!("action '{id}' is disabled in config");
        }
        match id {
            "lock" | "suspend" | "hibernate" | "shutdown" | "reboot" => {
                #[cfg(target_os = "linux")]
                if let Some(session) = self.session {
                    session.power_action(id).await?;
                    info!(action = id, "power action requested");
                    return Ok(());
                }
                #[cfg(target_os = "windows")]
                {
                    if id == "lock" {
                        if let Some(hub) = self.hub {
                            let command = IncomingCommand::Press { id: "lock".into() };
                            if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                                hub.call(rpc).await?;
                                info!(action = id, "power action requested");
                                return Ok(());
                            }
                        }
                    } else {
                        crate::collect::windows::power_action(id)?;
                        info!(action = id, "power action requested");
                        return Ok(());
                    }
                }
                anyhow::bail!("power action '{id}' is not supported on this platform");
            }
            "volume_up" => {
                #[cfg(target_os = "linux")]
                {
                    crate::collect::audio::bump_volume(5).await?;
                    return Ok(());
                }
                #[cfg(not(target_os = "linux"))]
                {
                    #[cfg(target_os = "windows")]
                    if let Some(hub) = self.hub {
                        let command = IncomingCommand::Press {
                            id: "volume_up".into(),
                        };
                        if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                            hub.call(rpc).await?;
                            return Ok(());
                        }
                    }
                    anyhow::bail!("volume is not supported on this platform");
                }
            }
            "volume_down" => {
                #[cfg(target_os = "linux")]
                {
                    crate::collect::audio::bump_volume(-5).await?;
                    return Ok(());
                }
                #[cfg(not(target_os = "linux"))]
                {
                    #[cfg(target_os = "windows")]
                    if let Some(hub) = self.hub {
                        let command = IncomingCommand::Press {
                            id: "volume_down".into(),
                        };
                        if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                            hub.call(rpc).await?;
                            return Ok(());
                        }
                    }
                    anyhow::bail!("volume is not supported on this platform");
                }
            }
            "media_play_pause" | "media_next" | "media_previous" => {
                #[cfg(target_os = "linux")]
                if let Some(session) = self.session {
                    session.mpris_action(id).await?;
                    return Ok(());
                }
                #[cfg(target_os = "windows")]
                if let Some(hub) = self.hub {
                    let command = IncomingCommand::Press { id: id.to_string() };
                    if let Some(rpc) = crate::collect::windows::rpc_from_command(&command) {
                        hub.call(rpc).await?;
                        return Ok(());
                    }
                }
                anyhow::bail!("mpris is not supported on this platform");
            }
            "apply_update" => {
                if !self.config.action_enabled("apply_update") {
                    anyhow::bail!("update is disabled in config");
                }
                let Some(update) = self.update else {
                    anyhow::bail!("update controller unavailable");
                };
                update.check_and_apply().await?;
                info!("apply_update requested");
                Ok(())
            }
            other => {
                warn!("ignored press for unknown entity {other}");
                Ok(())
            }
        }
    }
}

async fn run_command(spec: &CommandSpec) -> anyhow::Result<()> {
    let program = &spec.argv[0];
    let args = &spec.argv[1..];
    info!(
        command = spec.id.as_str(),
        program, "running allowed command"
    );
    let mut child = Command::new(program);
    child.args(args);
    child.kill_on_drop(true);
    child.stdin(std::process::Stdio::null());
    let run = tokio::time::timeout(COMMAND_TIMEOUT, child.status());
    match run.await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => anyhow::bail!("command '{}' exited with {status}", spec.id),
        Ok(Err(err)) => anyhow::bail!("command '{}' failed to spawn: {err}", spec.id),
        Err(_) => anyhow::bail!("command '{}' timed out", spec.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_switch_and_press_payloads() {
        match IncomingCommand::parse("caffeine", "ON").unwrap() {
            IncomingCommand::Switch { on, .. } => assert!(on),
            _ => panic!("expected switch"),
        }
        match IncomingCommand::parse("suspend", "PRESS").unwrap() {
            IncomingCommand::Press { id } => assert_eq!(id, "suspend"),
            _ => panic!("expected press"),
        }
        match IncomingCommand::parse("lock", "LOCK").unwrap() {
            IncomingCommand::Press { id } => assert_eq!(id, "lock"),
            _ => panic!("expected press"),
        }
        assert!(IncomingCommand::parse("caffeine", "explode").is_none());
    }

    #[test]
    fn parses_notify_plain_text_and_json() {
        match IncomingCommand::parse("notify_message", "Hello").unwrap() {
            IncomingCommand::Notify {
                title,
                body,
                urgency,
            } => {
                assert!(title.is_none());
                assert_eq!(body.as_deref(), Some("Hello"));
                assert_eq!(urgency, NotificationUrgency::Normal);
            }
            _ => panic!("expected notify"),
        }
        match IncomingCommand::parse(
            "notify_urgent",
            r#"{"title":"Alarm","body":"Door open","bypass_dnd":true}"#,
        )
        .unwrap()
        {
            IncomingCommand::Notify {
                title,
                body,
                urgency,
            } => {
                assert_eq!(title.as_deref(), Some("Alarm"));
                assert_eq!(body.as_deref(), Some("Door open"));
                assert_eq!(urgency, NotificationUrgency::Critical);
            }
            _ => panic!("expected notify"),
        }
        match IncomingCommand::parse("notify_message", r#"{"message":"Ping"}"#).unwrap() {
            IncomingCommand::Notify { body, urgency, .. } => {
                assert_eq!(body.as_deref(), Some("Ping"));
                assert_eq!(urgency, NotificationUrgency::Normal);
            }
            _ => panic!("expected notify"),
        }
        match IncomingCommand::parse("notify_message", "  ").unwrap() {
            IncomingCommand::Notify { title, body, .. } => {
                assert!(title.is_none());
                assert!(body.is_none());
            }
            _ => panic!("expected notify"),
        }
        assert!(IncomingCommand::parse("notify_message", "[1]").is_none());
        assert!(IncomingCommand::parse("notify_message", "{").is_none());
        let oversized = "x".repeat(MAX_NOTIFY_PAYLOAD_BYTES + 1);
        assert!(IncomingCommand::parse("notify_message", &oversized).is_none());
    }
}
