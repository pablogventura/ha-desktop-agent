use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

fn default_listeners() -> Vec<PortListener> {
    vec![
        PortListener {
            id: "ssh".into(),
            port: 22,
        },
        PortListener {
            id: "vnc".into(),
            port: 5900,
        },
        PortListener {
            id: "rdp".into(),
            port: 3389,
        },
    ]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub device: DeviceConfig,
    pub mqtt: MqttConfig,
    pub poll: PollConfig,
    pub sensors: SensorsConfig,
    pub processes: Vec<ProcessMonitor>,
    #[serde(default = "default_listeners")]
    pub listeners: Vec<PortListener>,
    pub actions: ActionsConfig,
    pub commands: Vec<CommandSpec>,
    pub power: PowerConfig,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub update: UpdateConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig::default(),
            mqtt: MqttConfig::default(),
            poll: PollConfig::default(),
            sensors: SensorsConfig::default(),
            processes: vec![
                ProcessMonitor {
                    id: "discord".into(),
                    match_name: "discord".into(),
                },
                ProcessMonitor {
                    id: "ollama".into(),
                    match_name: "ollama".into(),
                },
            ],
            listeners: default_listeners(),
            actions: ActionsConfig::default(),
            commands: Vec::new(),
            power: PowerConfig::default(),
            notify: NotifyConfig::default(),
            update: UpdateConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DeviceConfig {
    pub name: String,
    pub id: Option<String>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            name: "desktop".into(),
            id: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: bool,
    pub insecure_skip_verify: bool,
    pub ca: Option<PathBuf>,
    pub discovery_prefix: String,
    pub topic_prefix: String,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "homeassistant.local".into(),
            port: 1883,
            username: None,
            password: None,
            tls: false,
            insecure_skip_verify: false,
            ca: None,
            discovery_prefix: "homeassistant".into(),
            topic_prefix: "ha-desktop".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PollConfig {
    pub fast_ms: u64,
    pub slow_ms: u64,
    pub idle_threshold_s: u64,
    pub force_publish_s: u64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            fast_ms: 2000,
            slow_ms: 10_000,
            idle_threshold_s: 60,
            force_publish_s: 60,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SensorsConfig {
    pub gpu: bool,
    pub estimated_power: bool,
    pub active_application: bool,
    #[serde(default = "default_true")]
    pub active_window_title: bool,
    #[serde(default = "default_true")]
    pub tailscale: bool,
    #[serde(default = "default_true")]
    pub wireguard: bool,
    #[serde(default = "default_true")]
    pub lan_ip: bool,
    #[serde(default = "default_true")]
    pub disk: bool,
    #[serde(default = "default_true")]
    pub battery: bool,
    #[serde(default = "default_true")]
    pub wifi: bool,
    #[serde(default = "default_true")]
    pub online: bool,
    #[serde(default = "default_true")]
    pub audio: bool,
    #[serde(default = "default_true")]
    pub mpris: bool,
    #[serde(default = "default_true")]
    pub dnd: bool,
    /// Entity ids excluded from discovery and collection.
    pub disabled: Vec<String>,
}

impl Default for SensorsConfig {
    fn default() -> Self {
        Self {
            gpu: true,
            estimated_power: true,
            active_application: true,
            active_window_title: true,
            tailscale: true,
            wireguard: true,
            lan_ip: true,
            disk: true,
            battery: true,
            wifi: true,
            online: true,
            audio: true,
            mpris: true,
            dnd: true,
            disabled: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessMonitor {
    pub id: String,
    #[serde(rename = "match")]
    pub match_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PortListener {
    pub id: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ActionsConfig {
    pub lock: bool,
    pub suspend: bool,
    #[serde(default = "default_true")]
    pub hibernate: bool,
    #[serde(default = "default_true")]
    pub shutdown: bool,
    #[serde(default = "default_true")]
    pub reboot: bool,
    pub caffeine: bool,
    #[serde(default = "default_true")]
    pub mute: bool,
    #[serde(default = "default_true")]
    pub volume: bool,
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default = "default_true")]
    pub dnd: bool,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            lock: true,
            suspend: true,
            hibernate: true,
            shutdown: true,
            reboot: true,
            caffeine: true,
            mute: true,
            volume: true,
            notify: true,
            dnd: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandSpec {
    pub id: String,
    pub name: Option<String>,
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub title: String,
    pub body: String,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            title: "Desktop".into(),
            body: "Notification from ha-desktop-agent".into(),
        }
    }
}

fn default_github_repo() -> String {
    "pablogventura/ha-desktop-agent".into()
}

fn default_check_interval_hours() -> u64 {
    24
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct UpdateConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto: bool,
    #[serde(default = "default_check_interval_hours")]
    pub check_interval_hours: u64,
    #[serde(default = "default_github_repo")]
    pub github_repo: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto: true,
            check_interval_hours: default_check_interval_hours(),
            github_repo: default_github_repo(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PowerConfig {
    pub idle_w: f64,
    pub coefficients: HashMap<String, f64>,
    pub log_csv: Option<PathBuf>,
}

impl Default for PowerConfig {
    fn default() -> Self {
        let mut coefficients = HashMap::new();
        coefficients.insert("cpu_package_w".into(), 1.0);
        coefficients.insert("dram_w".into(), 1.0);
        coefficients.insert("gpu_w".into(), 1.0);
        coefficients.insert("cpu_usage".into(), 0.0);
        coefficients.insert("gpu_usage".into(), 0.0);
        Self {
            idle_w: 30.0,
            coefficients,
            log_csv: None,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let path = resolve_config_path(path)?;
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| anyhow::anyhow!("failed to read {}: {err}", path.display()))?;
        let mut config: Config = serde_yaml::from_str(&raw)
            .map_err(|err| anyhow::anyhow!("invalid config {}: {err}", path.display()))?;
        if let Ok(password) = env::var("HA_DESKTOP_MQTT_PASSWORD") {
            if !password.is_empty() {
                config.mqtt.password = Some(password);
            }
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.device.name.trim().is_empty() {
            anyhow::bail!("device.name must not be empty");
        }
        if self.mqtt.host.trim().is_empty() {
            anyhow::bail!("mqtt.host must not be empty");
        }
        if self.poll.fast_ms == 0 {
            anyhow::bail!("poll.fast_ms must be > 0");
        }
        if self.mqtt.insecure_skip_verify && !self.mqtt.tls {
            anyhow::bail!("mqtt.insecure_skip_verify requires mqtt.tls");
        }
        for process in &self.processes {
            validate_id(&process.id, "processes.id")?;
            if process.match_name.trim().is_empty() {
                anyhow::bail!("process '{}' has empty match", process.id);
            }
        }
        for listener in &self.listeners {
            validate_id(&listener.id, "listeners.id")?;
            if listener.port == 0 {
                anyhow::bail!("listener '{}' port must be 1-65535", listener.id);
            }
        }
        for command in &self.commands {
            validate_id(&command.id, "commands.id")?;
            if command.argv.is_empty() {
                anyhow::bail!("command '{}' has empty argv", command.id);
            }
            if command.argv[0].trim().is_empty() {
                anyhow::bail!("command '{}' has empty program", command.id);
            }
        }
        crate::update::validate_github_repo(&self.update.github_repo)?;
        if self.update.check_interval_hours == 0 {
            anyhow::bail!("update.check_interval_hours must be > 0");
        }
        Ok(())
    }

    pub fn is_disabled(&self, entity_id: &str) -> bool {
        self.sensors.disabled.iter().any(|id| id == entity_id)
    }

    pub fn action_enabled(&self, entity_id: &str) -> bool {
        match entity_id {
            "lock" => self.actions.lock,
            "suspend" => self.actions.suspend,
            "hibernate" => self.actions.hibernate,
            "shutdown" => self.actions.shutdown,
            "reboot" => self.actions.reboot,
            "caffeine" => self.actions.caffeine,
            "mute" => self.actions.mute,
            "volume_up" | "volume_down" => self.actions.volume,
            "notify" | "notify_message" | "notify_urgent" => self.actions.notify,
            "do_not_disturb" => self.actions.dnd,
            "media_play_pause" | "media_next" | "media_previous" => self.sensors.mpris,
            "update_auto" | "apply_update" => self.update.enabled,
            _ => self.commands.iter().any(|command| command.id == entity_id),
        }
    }
}

pub fn resolve_config_path(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg).join("ha-desktop-agent/config.yaml");
        if path.exists() {
            return Ok(path);
        }
    }
    if let Ok(home) = env::var("HOME") {
        let path = PathBuf::from(home).join(".config/ha-desktop-agent/config.yaml");
        if path.exists() {
            return Ok(path);
        }
    }
    #[cfg(windows)]
    {
        if let Ok(program_data) = env::var("ProgramData") {
            let path = PathBuf::from(program_data).join("ha-desktop-agent/config.yaml");
            if path.exists() {
                return Ok(path);
            }
        }
    }
    let local = PathBuf::from("config.yaml");
    if local.exists() {
        return Ok(local);
    }
    #[cfg(windows)]
    anyhow::bail!(
        "no config file found; pass --config or create %ProgramData%\\ha-desktop-agent\\config.yaml"
    );
    #[cfg(not(windows))]
    anyhow::bail!(
        "no config file found; pass --config or create $XDG_CONFIG_HOME/ha-desktop-agent/config.yaml"
    );
}

fn validate_id(id: &str, field: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        anyhow::bail!("{field} '{id}' must match [A-Za-z0-9_-]+");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_defaults() {
        let yaml = r#"
device:
  name: desktop
mqtt:
  host: localhost
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.device.name, "desktop");
        assert!(config.actions.lock);
        assert!(config.actions.shutdown);
        assert!(config.sensors.active_window_title);
        assert!(config.sensors.tailscale);
        assert_eq!(config.listeners.len(), 3);
        assert_eq!(config.listeners[0].id, "ssh");
    }
}
