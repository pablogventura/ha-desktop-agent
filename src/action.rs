use crate::config::{CommandSpec, Config};
use std::time::Duration;
use tokio::process::Command;
use tracing::{info, warn};

#[cfg(target_os = "linux")]
use crate::collect::linux_session::LinuxSession;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub enum IncomingCommand {
    Switch { id: String, on: bool },
    Press { id: String },
}

impl IncomingCommand {
    pub fn parse(entity_id: &str, payload: &str) -> Option<Self> {
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
            "PRESS" | "PRESSED" | "" => Some(Self::Press {
                id: entity_id.to_string(),
            }),
            _ => None,
        }
    }
}

pub struct ActionRouter<'a> {
    config: &'a Config,
    #[cfg(target_os = "linux")]
    session: Option<&'a LinuxSession>,
}

impl<'a> ActionRouter<'a> {
    pub fn new(
        config: &'a Config,
        #[cfg(target_os = "linux")] session: Option<&'a LinuxSession>,
    ) -> Self {
        Self {
            config,
            #[cfg(target_os = "linux")]
            session,
        }
    }

    pub async fn handle(&self, command: IncomingCommand) -> anyhow::Result<()> {
        match command {
            IncomingCommand::Switch { id, on } => self.handle_switch(&id, on).await,
            IncomingCommand::Press { id } => self.handle_press(&id).await,
        }
    }

    async fn handle_switch(&self, id: &str, on: bool) -> anyhow::Result<()> {
        if id != "caffeine" {
            warn!("ignored switch for unknown entity {id}");
            return Ok(());
        }
        if !self.config.action_enabled("caffeine") {
            anyhow::bail!("caffeine is disabled in config");
        }
        #[cfg(target_os = "linux")]
        if let Some(session) = self.session {
            session.set_caffeine(on).await?;
            info!(on, "caffeine updated");
            return Ok(());
        }
        anyhow::bail!("caffeine is not supported on this platform");
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
                anyhow::bail!("power action '{id}' is not supported on this platform");
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
        assert!(IncomingCommand::parse("caffeine", "explode").is_none());
    }
}
