use crate::config::Config;
use serde_json::{Map, Number, Value as JsonValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Sensor,
    BinarySensor,
    Switch,
    Button,
}

impl EntityKind {
    pub fn platform(self) -> &'static str {
        match self {
            Self::Sensor => "sensor",
            Self::BinarySensor => "binary_sensor",
            Self::Switch => "switch",
            Self::Button => "button",
        }
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
    if config.actions.caffeine && !config.is_disabled("caffeine") {
        entities.push(EntityMeta::switch(
            "caffeine",
            "Caffeine",
            Some("mdi:coffee"),
        ));
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
        EntityMeta::sensor(
            "uptime",
            "Uptime",
            Some("duration"),
            Some("s"),
            Some("total_increasing"),
            Some(0),
            1.0,
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
        assert!(ids.contains(&"discord_running".into()));
        assert!(ids.contains(&"caffeine".into()));
        assert!(ids.contains(&"lock".into()));
        assert!(!ids.contains(&"shutdown".into()));
        assert!(!ids.contains(&"hibernate".into()));
        assert!(!ids.contains(&"active_window_title".into()));
        assert!(ids.contains(&"agent_version".into()));
        assert!(ids.contains(&"dram_power".into()));
    }
}
