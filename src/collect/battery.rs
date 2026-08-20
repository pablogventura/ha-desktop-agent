//! Chassis type (SMBIOS) and system battery / AC from sysfs.
//! Does not recurse into `device` symlinks.

use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::fs;
use std::path::Path;

pub fn collect_chassis(chassis_type_path: &Path, snapshot: &mut Snapshot) {
    snapshot.set(
        "chassis",
        match read_trimmed(chassis_type_path).and_then(|raw| map_chassis_type(&raw)) {
            Some(kind) => Value::Text(kind.to_string()),
            None => Value::Unavailable,
        },
    );
}

pub fn collect_power_supply(power_root: &Path, snapshot: &mut Snapshot) {
    let supplies = match list_supply_dirs(power_root) {
        Some(list) => list,
        None => {
            set_battery_unavailable(snapshot);
            snapshot.set("ac_power", infer_ac_without_adapter(snapshot));
            return;
        }
    };

    let mut batteries = Vec::new();
    let mut ac_online = None;
    for dir in supplies {
        let kind = match read_trimmed(&dir.join("type")) {
            Some(value) => value,
            None => continue,
        };
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if is_ac_adapter(&kind) {
            if let Some(online) = read_trimmed(&dir.join("online")) {
                ac_online = Some(online == "1");
            }
            continue;
        }
        let scope = read_trimmed(&dir.join("scope"));
        if is_system_battery(&name, &kind, scope.as_deref()) {
            batteries.push(dir);
        }
    }

    match ac_online {
        Some(on) => snapshot.set("ac_power", Value::Bool(on)),
        None => snapshot.set("ac_power", infer_ac_without_adapter(snapshot)),
    }

    batteries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let Some(bat) = batteries.first() else {
        snapshot.set("battery_present", Value::Bool(false));
        snapshot.set("battery_percent", Value::Unavailable);
        snapshot.set("battery_charging", Value::Unavailable);
        snapshot.set("battery_status", Value::Unavailable);
        snapshot.set("battery_health", Value::Unavailable);
        snapshot.set("battery_cycles", Value::Unavailable);
        return;
    };

    snapshot.set("battery_present", Value::Bool(true));
    let status_raw = read_trimmed(&bat.join("status"));
    let status = normalize_battery_status(status_raw.as_deref());
    snapshot.set("battery_status", Value::Text(status.to_string()));
    snapshot.set(
        "battery_charging",
        Value::Bool(status_raw.as_deref() == Some("Charging")),
    );
    match read_trimmed(&bat.join("capacity")).and_then(|v| v.parse::<f64>().ok()) {
        Some(percent) => snapshot.set("battery_percent", Value::Number(percent)),
        None => snapshot.set("battery_percent", Value::Unavailable),
    }
    match battery_health_percent(bat) {
        Some(health) => snapshot.set("battery_health", Value::Number(health)),
        None => snapshot.set("battery_health", Value::Unavailable),
    }
    match read_trimmed(&bat.join("cycle_count")).and_then(|v| v.parse::<f64>().ok()) {
        Some(cycles) => snapshot.set("battery_cycles", Value::Number(cycles)),
        None => snapshot.set("battery_cycles", Value::Unavailable),
    }
}

pub fn map_chassis_type(contents: &str) -> Option<&'static str> {
    let code: u32 = contents.trim().parse().ok()?;
    Some(super::win32_parse::map_chassis_code(code))
}

pub fn is_system_battery(name: &str, kind: &str, scope: Option<&str>) -> bool {
    if !kind.eq_ignore_ascii_case("Battery") {
        return false;
    }
    if let Some(scope) = scope {
        return scope.eq_ignore_ascii_case("System");
    }
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("hidpp") || lower.starts_with("mouse") || lower.contains("ps-controller") {
        return false;
    }
    lower.starts_with("bat") || lower.starts_with("cmb")
}

fn infer_ac_without_adapter(snapshot: &Snapshot) -> Value {
    match snapshot.get("chassis") {
        Some(Value::Text(kind)) if super::win32_parse::chassis_is_mains_powered(kind) => {
            Value::Bool(true)
        }
        _ => Value::Unavailable,
    }
}

fn is_ac_adapter(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("Mains") || kind.eq_ignore_ascii_case("USB")
}

fn normalize_battery_status(raw: Option<&str>) -> &'static str {
    match raw {
        Some("Charging") => "charging",
        Some("Discharging") => "discharging",
        Some("Full") => "full",
        Some("Not charging") | Some("Not Charging") => "not_charging",
        _ => "unknown",
    }
}

fn battery_health_percent(bat: &Path) -> Option<f64> {
    let ratio = |now: &str, design: &str| -> Option<f64> {
        let now: f64 = read_trimmed(&bat.join(now))?.parse().ok()?;
        let design: f64 = read_trimmed(&bat.join(design))?.parse().ok()?;
        if design <= 0.0 {
            return None;
        }
        Some((now / design) * 100.0)
    };
    ratio("charge_full", "charge_full_design")
        .or_else(|| ratio("energy_full", "energy_full_design"))
}

fn set_battery_unavailable(snapshot: &mut Snapshot) {
    snapshot.set("battery_present", Value::Unavailable);
    snapshot.set("battery_percent", Value::Unavailable);
    snapshot.set("battery_charging", Value::Unavailable);
    snapshot.set("battery_status", Value::Unavailable);
    snapshot.set("battery_health", Value::Unavailable);
    snapshot.set("battery_cycles", Value::Unavailable);
}

fn list_supply_dirs(root: &Path) -> Option<Vec<std::path::PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root).ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() || file_type.is_symlink() {
            dirs.push(entry.path());
        }
    }
    Some(dirs)
}

fn read_trimmed(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn maps_chassis_codes() {
        assert_eq!(map_chassis_type("3\n"), Some("desktop"));
        assert_eq!(map_chassis_type("9"), Some("laptop"));
        assert_eq!(map_chassis_type("31"), Some("convertible"));
        assert_eq!(map_chassis_type("not-a-number"), None);
    }

    #[test]
    fn rejects_peripheral_batteries() {
        assert!(!is_system_battery(
            "hidpp_battery_18",
            "Battery",
            Some("Device")
        ));
        assert!(!is_system_battery("hidpp_battery_18", "Battery", None));
        assert!(is_system_battery("BAT0", "Battery", Some("System")));
        assert!(is_system_battery("BAT0", "Battery", None));
        assert!(is_system_battery("CMB1", "Battery", None));
    }

    #[test]
    fn hidpp_only_fixture_has_no_system_battery() {
        let mut snapshot = Snapshot::default();
        collect_power_supply(&fixture_root("power_supply/hidpp_only"), &mut snapshot);
        assert_eq!(snapshot.get("battery_present"), Some(&Value::Bool(false)));
        assert_eq!(snapshot.get("battery_percent"), Some(&Value::Unavailable));
        assert_eq!(snapshot.get("ac_power"), Some(&Value::Unavailable));
    }

    #[test]
    fn desktop_without_mains_sysfs_is_on_ac() {
        let mut snapshot = Snapshot::default();
        collect_chassis(
            &fixture_root("dmi/desktop").join("chassis_type"),
            &mut snapshot,
        );
        collect_power_supply(&fixture_root("power_supply/hidpp_only"), &mut snapshot);
        assert_eq!(snapshot.get("ac_power"), Some(&Value::Bool(true)));
        assert_eq!(snapshot.get("battery_present"), Some(&Value::Bool(false)));
    }

    #[test]
    fn laptop_fixture_reads_charge_and_ac() {
        let mut snapshot = Snapshot::default();
        collect_power_supply(&fixture_root("power_supply/laptop"), &mut snapshot);
        collect_chassis(
            &fixture_root("dmi/laptop").join("chassis_type"),
            &mut snapshot,
        );
        assert_eq!(snapshot.get("chassis"), Some(&Value::Text("laptop".into())));
        assert_eq!(snapshot.get("battery_present"), Some(&Value::Bool(true)));
        assert_eq!(snapshot.get("battery_percent"), Some(&Value::Number(87.0)));
        assert_eq!(snapshot.get("battery_charging"), Some(&Value::Bool(true)));
        assert_eq!(
            snapshot.get("battery_status"),
            Some(&Value::Text("charging".into()))
        );
        assert_eq!(snapshot.get("ac_power"), Some(&Value::Bool(true)));
        let health = snapshot.number("battery_health").unwrap();
        assert!((health - 95.0).abs() < 0.01);
        assert_eq!(snapshot.get("battery_cycles"), Some(&Value::Number(120.0)));
    }

    #[test]
    fn desktop_dmi_fixture() {
        let mut snapshot = Snapshot::default();
        collect_chassis(
            &fixture_root("dmi/desktop").join("chassis_type"),
            &mut snapshot,
        );
        assert_eq!(
            snapshot.get("chassis"),
            Some(&Value::Text("desktop".into()))
        );
    }
}
