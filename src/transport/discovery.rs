use crate::config::Config;
use crate::entity::{enabled_entities, EntityKind, EntityMeta};
use serde_json::{json, Map, Value};
use std::path::Path;

pub fn resolve_device_id(config: &Config) -> String {
    if let Some(id) = &config.device.id {
        return sanitize_id(id);
    }
    #[cfg(windows)]
    if let Some(id) = windows_machine_guid() {
        return id;
    }
    if let Ok(machine) = std::fs::read_to_string("/etc/machine-id") {
        let trimmed = machine.trim();
        if trimmed.len() >= 12 {
            return sanitize_id(&trimmed[..12]);
        }
        if !trimmed.is_empty() {
            return sanitize_id(trimmed);
        }
    }
    sanitize_id(&config.device.name)
}

#[cfg(windows)]
fn windows_machine_guid() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ,
        REG_VALUE_TYPE,
    };
    unsafe {
        let mut key = Default::default();
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            0,
            KEY_READ,
            &mut key,
        )
        .ok()
        .ok()?;
        let mut data = [0u16; 64];
        let mut size = (data.len() * 2) as u32;
        let mut kind = REG_VALUE_TYPE(0);
        let status = RegQueryValueExW(
            key,
            w!("MachineGuid"),
            None,
            Some(&mut kind as *mut _),
            Some(data.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        status.ok().ok()?;
        if kind != REG_SZ {
            return None;
        }
        let guid = String::from_utf16_lossy(&data);
        let hex: String = guid.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
        if hex.len() >= 12 {
            Some(sanitize_id(&hex[..12]))
        } else if hex.is_empty() {
            None
        } else {
            Some(sanitize_id(&hex))
        }
    }
}

pub fn sanitize_id(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "desktop".into()
    } else {
        out
    }
}

pub fn device_slug(name: &str) -> String {
    sanitize_id(name)
}

pub fn availability_topic(config: &Config, device_id: &str) -> String {
    format!("{}/{device_id}/availability", config.mqtt.topic_prefix)
}

pub fn state_topic(config: &Config, device_id: &str) -> String {
    format!("{}/{device_id}/state", config.mqtt.topic_prefix)
}

pub fn command_topic(config: &Config, device_id: &str, entity_id: &str) -> String {
    format!(
        "{}/{device_id}/command/{entity_id}",
        config.mqtt.topic_prefix
    )
}

pub fn command_filter(config: &Config, device_id: &str) -> String {
    format!("{}/{device_id}/command/+", config.mqtt.topic_prefix)
}

pub fn discovery_topic(config: &Config, device_id: &str) -> String {
    format!("{}/device/{device_id}/config", config.mqtt.discovery_prefix)
}

pub fn parse_command_entity(topic: &str, device_id: &str, prefix: &str) -> Option<String> {
    let expected = format!("{prefix}/{device_id}/command/");
    topic.strip_prefix(&expected).map(ToOwned::to_owned)
}

pub fn discovery_payload(config: &Config, device_id: &str) -> Value {
    let entities = enabled_entities(config);
    discovery_payload_with_entities(config, device_id, &entities)
}

pub fn discovery_payload_with_entities(
    config: &Config,
    device_id: &str,
    entities: &[EntityMeta],
) -> Value {
    let slug = device_slug(&config.device.name);
    let state = state_topic(config, device_id);
    let availability = availability_topic(config, device_id);
    let mut cmps = Map::new();
    for entity in entities {
        cmps.insert(
            entity.id.clone(),
            component(config, device_id, &slug, entity),
        );
    }
    json!({
        "dev": {
            "ids": [device_id],
            "name": config.device.name,
            "mf": "ha-desktop-agent",
            "mdl": "desktop",
            "sw": env!("CARGO_PKG_VERSION"),
        },
        "o": {
            "name": "ha-desktop-agent",
            "sw": env!("CARGO_PKG_VERSION"),
        },
        "cmps": cmps,
        "state_topic": state,
        "availability": [{
            "topic": availability,
            "payload_available": "online",
            "payload_not_available": "offline",
        }],
        "qos": 1,
    })
}

fn mqtt_bool_template(entity_id: &str) -> String {
    format!(
        "{{% if value_json.{entity_id} is boolean %}}{{{{ 'ON' if value_json.{entity_id} else 'OFF' }}}}{{% endif %}}"
    )
}

fn component(config: &Config, device_id: &str, slug: &str, entity: &EntityMeta) -> Value {
    let mut map = Map::new();
    map.insert("p".into(), Value::String(entity.kind.platform().into()));
    map.insert("name".into(), Value::String(entity.name.clone()));
    map.insert(
        "unique_id".into(),
        Value::String(format!("{device_id}_{}", entity.id)),
    );
    map.insert(
        "default_entity_id".into(),
        Value::String(format!("{}_{}", slug, entity.id)),
    );
    if let Some(class) = entity.device_class {
        map.insert("device_class".into(), Value::String(class.into()));
    }
    if let Some(unit) = entity.unit {
        map.insert("unit_of_measurement".into(), Value::String(unit.into()));
    }
    if let Some(state_class) = entity.state_class {
        map.insert("state_class".into(), Value::String(state_class.into()));
    }
    if let Some(precision) = entity.precision {
        map.insert("suggested_display_precision".into(), json!(precision));
    }
    if let Some(category) = entity.entity_category {
        map.insert("entity_category".into(), Value::String(category.into()));
    }
    if let Some(icon) = entity.icon {
        map.insert("icon".into(), Value::String(icon.into()));
    }
    match entity.kind {
        EntityKind::Sensor => {
            map.insert(
                "state_topic".into(),
                Value::String(state_topic(config, device_id)),
            );
            map.insert(
                "value_template".into(),
                Value::String(format!("{{{{ value_json.{} }}}}", entity.id)),
            );
        }
        EntityKind::BinarySensor => {
            map.insert(
                "state_topic".into(),
                Value::String(state_topic(config, device_id)),
            );
            map.insert(
                "value_template".into(),
                Value::String(mqtt_bool_template(&entity.id)),
            );
        }
        EntityKind::Switch => {
            map.insert(
                "state_topic".into(),
                Value::String(state_topic(config, device_id)),
            );
            map.insert(
                "command_topic".into(),
                Value::String(command_topic(config, device_id, &entity.id)),
            );
            map.insert(
                "value_template".into(),
                Value::String(mqtt_bool_template(&entity.id)),
            );
            map.insert("payload_on".into(), Value::String("ON".into()));
            map.insert("payload_off".into(), Value::String("OFF".into()));
        }
        EntityKind::Button => {
            map.insert(
                "command_topic".into(),
                Value::String(command_topic(config, device_id, &entity.id)),
            );
            map.insert("payload_press".into(), Value::String("PRESS".into()));
        }
        EntityKind::Notify => {
            map.insert(
                "command_topic".into(),
                Value::String(command_topic(config, device_id, &entity.id)),
            );
            map.insert("retain".into(), Value::Bool(false));
        }
    }
    if entity.kind.publishes_state() {
        map.insert(
            "json_attributes_topic".into(),
            Value::String(state_topic(config, device_id)),
        );
        map.insert(
            "json_attributes_template".into(),
            Value::String("{{ value_json.attrs | tojson }}".into()),
        );
    }
    Value::Object(map)
}

pub fn warn_if_world_readable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o077;
            if mode != 0 {
                tracing::warn!(
                    path = %path.display(),
                    "config file is readable by group/other; chmod 600 is recommended"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn discovery_is_a_single_device() {
        let mut config = Config::default();
        config.mqtt.host = "localhost".into();
        let payload = discovery_payload(&config, "abc123def456");
        assert_eq!(payload["dev"]["name"], "desktop");
        assert!(payload["cmps"]["cpu_usage"].is_object());
        assert_eq!(payload["cmps"]["cpu_usage"]["p"], "sensor");
        assert_eq!(payload["cmps"]["caffeine"]["p"], "switch");
        assert_eq!(payload["cmps"]["lock"]["p"], "button");
        assert!(payload["cmps"]["ac_power"]["value_template"]
            .as_str()
            .unwrap()
            .contains("is boolean"));
        assert_eq!(payload["cmps"]["notify_message"]["p"], "notify");
        assert_eq!(payload["cmps"]["notify_urgent"]["p"], "notify");
        assert!(payload["cmps"].get("notify").is_none());
        assert!(payload["cmps"]["shutdown"]["p"] == "button");
        assert_eq!(
            payload["cmps"]["cpu_usage"]["default_entity_id"],
            "desktop_cpu_usage"
        );
        assert_eq!(
            payload["cmps"]["agent_version"]["entity_category"],
            "diagnostic"
        );
    }

    #[test]
    fn sanitizes_ids() {
        assert_eq!(sanitize_id("Pablo PC"), "pablo_pc");
        assert_eq!(
            parse_command_entity("ha-desktop/abc/command/lock", "abc", "ha-desktop").as_deref(),
            Some("lock")
        );
    }
}
