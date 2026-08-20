use crate::config::Config;
use serde_json::{Map, Number, Value as JsonValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Sensor,
    BinarySensor,
    Switch,
    Button,
    Notify,
}

impl EntityKind {
    pub fn platform(self) -> &'static str {
        match self {
            Self::Sensor => "sensor",
            Self::BinarySensor => "binary_sensor",
            Self::Switch => "switch",
            Self::Button => "button",
            Self::Notify => "notify",
        }
    }

    pub fn publishes_state(self) -> bool {
        !matches!(self, Self::Button | Self::Notify)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Text(String),
    Unavailable,
}

impl Value {
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::Number(n) if n.is_finite() => {
                JsonValue::Number(Number::from_f64(*n).unwrap_or_else(|| Number::from(0)))
            }
            Self::Number(_) => JsonValue::Null,
            Self::Bool(v) => JsonValue::Bool(*v),
            Self::Text(v) => JsonValue::String(v.clone()),
            Self::Unavailable => JsonValue::Null,
        }
    }

    pub fn changed_enough(&self, previous: &Value, hysteresis: f64) -> bool {
        match (previous, self) {
            (Self::Unavailable, Self::Unavailable) => false,
            (Self::Number(old), Self::Number(new)) => (old - new).abs() >= hysteresis,
            (old, new) => old != new,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntityMeta {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub device_class: Option<&'static str>,
    pub unit: Option<&'static str>,
    pub state_class: Option<&'static str>,
    pub precision: Option<u8>,
    pub entity_category: Option<&'static str>,
    pub icon: Option<&'static str>,
    pub hysteresis: f64,
}

impl EntityMeta {
    fn sensor(
        id: &str,
        name: &str,
        device_class: Option<&'static str>,
        unit: Option<&'static str>,
        state_class: Option<&'static str>,
        precision: Option<u8>,
        hysteresis: f64,
    ) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Sensor,
            name: name.into(),
            device_class,
            unit,
            state_class,
            precision,
            entity_category: None,
            icon: None,
            hysteresis,
        }
    }

    fn diagnostic_sensor(
        id: &str,
        name: &str,
        device_class: Option<&'static str>,
        unit: Option<&'static str>,
        precision: Option<u8>,
    ) -> Self {
        let mut meta = Self::sensor(
            id,
            name,
            device_class,
            unit,
            Some("measurement"),
            precision,
            0.0,
        );
        meta.entity_category = Some("diagnostic");
        meta
    }

    fn binary(id: &str, name: &str, device_class: Option<&'static str>) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::BinarySensor,
            name: name.into(),
            device_class,
            unit: None,
            state_class: None,
            precision: None,
            entity_category: None,
            icon: None,
            hysteresis: 0.0,
        }
    }

    fn switch(id: &str, name: &str, icon: Option<&'static str>) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Switch,
            name: name.into(),
            device_class: None,
            unit: None,
            state_class: None,
            precision: None,
            entity_category: None,
            icon,
            hysteresis: 0.0,
        }
    }

    fn button(
        id: &str,
        name: &str,
        device_class: Option<&'static str>,
        icon: Option<&'static str>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Button,
            name: name.into(),
            device_class,
            unit: None,
            state_class: None,
            precision: None,
            entity_category: None,
            icon,
            hysteresis: 0.0,
        }
    }

    fn notify(id: &str, name: &str, icon: Option<&'static str>) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Notify,
            name: name.into(),
            device_class: None,
            unit: None,
            state_class: None,
            precision: None,
            entity_category: None,
            icon,
            hysteresis: 0.0,
        }
    }
}

pub fn enabled_entities(config: &Config) -> Vec<EntityMeta> {
    let mut entities = Vec::new();
    for meta in static_entities() {
        if !entity_enabled(config, &meta) {
            continue;
        }
        entities.push(meta);
    }
    for process in &config.processes {
        let id = format!("{}_running", process.id);
        if config.is_disabled(&id) {
            continue;
        }
        entities.push(EntityMeta::binary(
            &id,
            &format!("{} running", title_case(&process.id)),
            Some("running"),
        ));
    }
    for listener in &config.listeners {
        let id = format!("{}_listening", listener.id);
        if config.is_disabled(&id) {
            continue;
        }
        entities.push(EntityMeta::binary(
            &id,
            &format!("{} listening", title_case(&listener.id)),
            Some("running"),
        ));
    }
    if config.actions.caffeine && !config.is_disabled("caffeine") {
        entities.push(EntityMeta::switch(
            "caffeine",
            "Caffeine",
            Some("mdi:coffee"),
        ));
    }
    if config.sensors.audio && config.actions.mute && !config.is_disabled("mute") {
        entities.push(EntityMeta::switch("mute", "Mute", Some("mdi:volume-off")));
    }
    if config.sensors.dnd
        && config.actions.dnd
        && !cfg!(target_os = "windows")
        && !config.is_disabled("do_not_disturb")
    {
        entities.push(EntityMeta::switch(
            "do_not_disturb",
            "Do not disturb",
            Some("mdi:minus-circle"),
        ));
    }
    if config.sensors.audio && config.actions.volume {
        for (id, name, icon) in [
            ("volume_up", "Volume up", Some("mdi:volume-plus")),
            ("volume_down", "Volume down", Some("mdi:volume-minus")),
        ] {
            if !config.is_disabled(id) {
                entities.push(EntityMeta::button(id, name, None, icon));
            }
        }
    }
    if config.actions.notify {
        for (id, name, icon) in [
            ("notify_message", "Notification", Some("mdi:bell")),
            (
                "notify_urgent",
                "Urgent notification",
                Some("mdi:bell-alert"),
            ),
        ] {
            if !config.is_disabled(id) {
                entities.push(EntityMeta::notify(id, name, icon));
            }
        }
    }
    if config.sensors.mpris {
        for (id, name, icon) in [
            (
                "media_play_pause",
                "Media play pause",
                Some("mdi:play-pause"),
            ),
            ("media_next", "Media next", Some("mdi:skip-next")),
            (
                "media_previous",
                "Media previous",
                Some("mdi:skip-previous"),
            ),
        ] {
            if !config.is_disabled(id) {
                entities.push(EntityMeta::button(id, name, None, icon));
            }
        }
    }
    for (id, name, class, icon, enabled) in [
        ("lock", "Lock", None, Some("mdi:lock"), config.actions.lock),
        (
            "suspend",
            "Suspend",
            None,
            Some("mdi:power-sleep"),
            config.actions.suspend,
        ),
        (
            "hibernate",
            "Hibernate",
            None,
            Some("mdi:power-sleep"),
            config.actions.hibernate,
        ),
        (
            "shutdown",
            "Shutdown",
            None,
            Some("mdi:power"),
            config.actions.shutdown,
        ),
        (
            "reboot",
            "Restart",
            Some("restart"),
            Some("mdi:restart"),
            config.actions.reboot,
        ),
    ] {
        if enabled && !config.is_disabled(id) {
            entities.push(EntityMeta::button(id, name, class, icon));
        }
    }
    for command in &config.commands {
        if config.is_disabled(&command.id) {
            continue;
        }
        let name = command
            .name
            .clone()
            .unwrap_or_else(|| title_case(&command.id));
        entities.push(EntityMeta::button(
            &command.id,
            &name,
            None,
            Some("mdi:play"),
        ));
    }
    if config.update.enabled {
        if !config.is_disabled("update_available") {
            entities.push(EntityMeta::binary(
                "update_available",
                "Update available",
                Some("update"),
            ));
        }
        if !config.is_disabled("update_latest_version") {
            entities.push(EntityMeta::diagnostic_sensor(
                "update_latest_version",
                "Latest version",
                None,
                None,
                None,
            ));
        }
        if !config.is_disabled("update_auto") {
            entities.push(EntityMeta::switch(
                "update_auto",
                "Auto update",
                Some("mdi:update"),
            ));
        }
        if !config.is_disabled("apply_update") {
            entities.push(EntityMeta::button(
                "apply_update",
                "Apply update",
                None,
                Some("mdi:download"),
            ));
        }
    }
    entities
}

fn entity_enabled(config: &Config, meta: &EntityMeta) -> bool {
    if config.is_disabled(&meta.id) {
        return false;
    }
    if meta.id.starts_with("gpu_") && !config.sensors.gpu {
        return false;
    }
    match meta.id.as_str() {
        "estimated_power" => config.sensors.estimated_power,
        "active_application" => config.sensors.active_application,
        "active_window_title" => config.sensors.active_window_title,
        "tailscale_running" | "tailscale_ip" => config.sensors.tailscale,
        "wireguard_running" | "wireguard_ip" => config.sensors.wireguard,
        "lan_ip" | "lan_rx" | "lan_tx" => config.sensors.lan_ip,
        "disk_root_used" | "disk_root_free" | "disk_root_usage" | "disk_home_used"
        | "disk_home_free" | "disk_home_usage" => config.sensors.disk,
        "wifi_ssid" | "wifi_signal" => config.sensors.wifi,
        "online" => config.sensors.online,
        "volume" | "muted" | "audio_sink" => config.sensors.audio,
        "media_title" | "media_artist" | "media_playing" => config.sensors.mpris,
        "battery_present" | "battery_percent" | "battery_charging" | "battery_status"
        | "battery_health" | "battery_cycles" | "ac_power" => config.sensors.battery,
        "update_available" | "update_latest_version" | "update_auto" | "apply_update" => {
            config.update.enabled
        }
        _ => true,
    }
}

fn static_entities() -> Vec<EntityMeta> {
    vec![
        EntityMeta::sensor(
            "operating_system",
            "Operating system",
            None,
            None,
            None,
            None,
            0.0,
        ),
        EntityMeta::sensor("os_version", "OS version", None, None, None, None, 0.0),
        EntityMeta::sensor("hostname", "Hostname", None, None, None, None, 0.0),
        EntityMeta::sensor("chassis", "Chassis", None, None, None, None, 0.0),
        EntityMeta::sensor(
            "uptime",
            "Uptime",
            Some("duration"),
            Some("h"),
            Some("total_increasing"),
            Some(1),
            0.01,
        ),
        EntityMeta::sensor(
            "cpu_usage",
            "CPU usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "cpu_frequency",
            "CPU frequency",
            Some("frequency"),
            Some("MHz"),
            Some("measurement"),
            Some(0),
            10.0,
        ),
        EntityMeta::sensor(
            "cpu_temperature",
            "CPU temperature",
            Some("temperature"),
            Some("°C"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "cpu_power",
            "CPU power",
            Some("power"),
            Some("W"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "dram_power",
            "DRAM power",
            Some("power"),
            Some("W"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "ram_total",
            "RAM total",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "ram_used",
            "RAM used",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "ram_available",
            "RAM available",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "ram_usage",
            "RAM usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "swap_total",
            "Swap total",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "swap_used",
            "Swap used",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "swap_usage",
            "Swap usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "gpu_usage",
            "GPU usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "gpu_memory_used",
            "GPU memory used",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "gpu_temperature",
            "GPU temperature",
            Some("temperature"),
            Some("°C"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "gpu_power",
            "GPU power",
            Some("power"),
            Some("W"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor("gpu_driver", "GPU driver", None, None, None, None, 0.0),
        EntityMeta::sensor(
            "gpu_fan",
            "GPU fan",
            None,
            Some("%"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::sensor(
            "gpu_power_limit",
            "GPU power limit",
            Some("power"),
            Some("W"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::sensor(
            "idle_time",
            "Idle time",
            Some("duration"),
            Some("s"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::sensor("session_type", "Session type", None, None, None, None, 0.0),
        EntityMeta::sensor(
            "desktop_environment",
            "Desktop environment",
            None,
            None,
            None,
            None,
            0.0,
        ),
        EntityMeta::sensor(
            "active_application",
            "Active application",
            None,
            None,
            None,
            None,
            0.0,
        ),
        EntityMeta::sensor(
            "active_window_title",
            "Active window title",
            None,
            None,
            None,
            None,
            0.0,
        ),
        EntityMeta::sensor(
            "suspend_inhibit_reason",
            "Suspend inhibit reason",
            None,
            None,
            None,
            None,
            0.0,
        ),
        EntityMeta::sensor(
            "estimated_power",
            "Estimated power",
            Some("power"),
            Some("W"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        {
            let mut meta =
                EntityMeta::diagnostic_sensor("agent_version", "Agent version", None, None, None);
            meta.state_class = None;
            meta
        },
        EntityMeta::diagnostic_sensor(
            "agent_uptime",
            "Agent uptime",
            Some("duration"),
            Some("s"),
            Some(0),
        ),
        EntityMeta::binary("user_active", "User active", Some("occupancy")),
        EntityMeta::binary("suspend_inhibited", "Suspend inhibited", None),
        EntityMeta::binary("tailscale_running", "Tailscale running", Some("running")),
        EntityMeta::sensor("tailscale_ip", "Tailscale IP", None, None, None, None, 0.0),
        EntityMeta::sensor("lan_ip", "LAN IP", None, None, None, None, 0.0),
        EntityMeta::binary("wireguard_running", "WireGuard running", Some("running")),
        EntityMeta::sensor("wireguard_ip", "WireGuard IP", None, None, None, None, 0.0),
        EntityMeta::sensor(
            "disk_root_used",
            "Disk root used",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "disk_root_free",
            "Disk root free",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "disk_root_usage",
            "Disk root usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::sensor(
            "disk_home_used",
            "Disk home used",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "disk_home_free",
            "Disk home free",
            Some("data_size"),
            Some("GB"),
            Some("measurement"),
            Some(2),
            0.01,
        ),
        EntityMeta::sensor(
            "disk_home_usage",
            "Disk home usage",
            None,
            Some("%"),
            Some("measurement"),
            Some(1),
            0.5,
        ),
        EntityMeta::binary("battery_present", "Battery present", None),
        EntityMeta::sensor(
            "battery_percent",
            "Battery",
            Some("battery"),
            Some("%"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::binary(
            "battery_charging",
            "Battery charging",
            Some("battery_charging"),
        ),
        EntityMeta::binary("ac_power", "AC power", Some("plug")),
        {
            let mut meta =
                EntityMeta::diagnostic_sensor("battery_status", "Battery status", None, None, None);
            meta.state_class = None;
            meta
        },
        EntityMeta::diagnostic_sensor("battery_health", "Battery health", None, Some("%"), Some(0)),
        EntityMeta::diagnostic_sensor("battery_cycles", "Battery cycles", None, None, Some(0)),
        EntityMeta::sensor(
            "lan_rx",
            "LAN receive",
            Some("data_rate"),
            Some("kB/s"),
            Some("measurement"),
            Some(1),
            1.0,
        ),
        EntityMeta::sensor(
            "lan_tx",
            "LAN transmit",
            Some("data_rate"),
            Some("kB/s"),
            Some("measurement"),
            Some(1),
            1.0,
        ),
        EntityMeta::binary("locked", "Locked", Some("lock")),
        EntityMeta::sensor(
            "volume",
            "Volume",
            None,
            Some("%"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::binary("muted", "Muted", None),
        EntityMeta::sensor("audio_sink", "Audio sink", None, None, None, None, 0.0),
        EntityMeta::sensor("wifi_ssid", "WiFi SSID", None, None, None, None, 0.0),
        EntityMeta::sensor(
            "wifi_signal",
            "WiFi signal",
            Some("signal_strength"),
            Some("dBm"),
            Some("measurement"),
            Some(0),
            1.0,
        ),
        EntityMeta::binary("online", "Online", Some("connectivity")),
        EntityMeta::sensor("media_title", "Media title", None, None, None, None, 0.0),
        EntityMeta::sensor("media_artist", "Media artist", None, None, None, None, 0.0),
        EntityMeta::binary("media_playing", "Media playing", Some("running")),
    ]
}

fn title_case(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn snapshot_json(values: &Map<String, JsonValue>) -> String {
    JsonValue::Object(values.clone()).to_string()
}

/// Home Assistant rejects entity states longer than 255 characters.
pub const HA_STATE_MAX_CHARS: usize = 255;

pub fn truncate_ha_state(value: &str) -> String {
    if value.chars().count() <= HA_STATE_MAX_CHARS {
        return value.to_string();
    }
    let keep = HA_STATE_MAX_CHARS.saturating_sub(3);
    let mut out: String = value.chars().take(keep).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_includes_core_sensors() {
        let config = Config::default();
        let ids: Vec<_> = enabled_entities(&config)
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert!(ids.contains(&"cpu_usage".into()));
        assert!(ids.contains(&"hostname".into()));
        assert!(ids.contains(&"chassis".into()));
        assert!(ids.contains(&"battery_present".into()));
        assert!(ids.contains(&"battery_percent".into()));
        assert!(ids.contains(&"ac_power".into()));
        assert!(ids.contains(&"discord_running".into()));
        assert!(ids.contains(&"caffeine".into()));
        assert!(ids.contains(&"lock".into()));
        assert!(ids.contains(&"shutdown".into()));
        assert!(ids.contains(&"hibernate".into()));
        assert!(ids.contains(&"reboot".into()));
        assert!(ids.contains(&"media_play_pause".into()));
        assert!(ids.contains(&"active_window_title".into()));
        assert!(ids.contains(&"agent_version".into()));
        assert!(ids.contains(&"dram_power".into()));
        assert!(ids.contains(&"tailscale_running".into()));
        assert!(ids.contains(&"tailscale_ip".into()));
        assert!(ids.contains(&"lan_ip".into()));
        assert!(ids.contains(&"wireguard_running".into()));
        assert!(ids.contains(&"ssh_listening".into()));
        assert!(ids.contains(&"disk_root_free".into()));
        assert!(ids.contains(&"mute".into()));
        assert!(ids.contains(&"do_not_disturb".into()));
        assert!(ids.contains(&"notify_message".into()));
        assert!(ids.contains(&"notify_urgent".into()));
        assert!(!ids.contains(&"notify".into()));
        assert!(!ids.contains(&"http_alt_listening".into()));
        assert!(ids.contains(&"update_available".into()));
        assert!(ids.contains(&"update_latest_version".into()));
        assert!(ids.contains(&"update_auto".into()));
        assert!(ids.contains(&"apply_update".into()));
    }

    #[test]
    fn truncates_long_ha_state() {
        let text = truncate_ha_state(&"x".repeat(400));
        assert_eq!(text.chars().count(), HA_STATE_MAX_CHARS);
        assert!(text.ends_with("..."));
    }
}
