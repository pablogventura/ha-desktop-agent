//! OS-independent parsers for Windows firmware/power blobs.

#![allow(dead_code)]

use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::collections::BTreeSet;

/// `MIB_TCPTABLE_OWNER_PID` as returned by `GetExtendedTcpTable`.
pub fn listen_ports_from_tcp_owner_pid_table(buf: &[u8]) -> BTreeSet<u16> {
    let mut ports = BTreeSet::new();
    if buf.len() < 4 {
        return ports;
    }
    let count = u32::from_le_bytes(buf[0..4].try_into().unwrap_or([0; 4])) as usize;
    let mut offset = 4usize;
    for _ in 0..count {
        if offset + 24 > buf.len() {
            break;
        }
        let state = u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
        let port_nbo = u32::from_le_bytes(buf[offset + 8..offset + 12].try_into().unwrap());
        // MIB_TCP_STATE_LISTEN == 2
        if state == 2 {
            let port = u16::from_be((port_nbo & 0xffff) as u16);
            if port != 0 {
                ports.insert(port);
            }
        }
        offset += 24;
    }
    ports
}

pub fn map_chassis_code(code: u32) -> &'static str {
    match code {
        3 | 4 | 6 | 7 | 24 | 34 | 35 => "desktop",
        8 | 9 | 10 | 14 => "laptop",
        31 | 32 => "convertible",
        30 => "tablet",
        13 => "all_in_one",
        17 | 23 => "server",
        _ => "other",
    }
}

pub fn chassis_is_mains_powered(kind: &str) -> bool {
    matches!(kind, "desktop" | "server" | "all_in_one")
}

pub fn map_chassis_code_u8(code: u8) -> &'static str {
    map_chassis_code(u32::from(code & 0x7f))
}

/// `GetSystemFirmwareTable('RSMB')` payload: 8-byte header then SMBIOS structures.
pub fn chassis_from_raw_smbios(buf: &[u8]) -> Option<&'static str> {
    if buf.len() < 8 {
        return None;
    }
    let table_len = u32::from_le_bytes(buf[4..8].try_into().ok()?) as usize;
    let table = buf.get(8..8 + table_len).or_else(|| buf.get(8..))?;
    chassis_from_smbios_table(table)
}

pub fn chassis_from_smbios_table(table: &[u8]) -> Option<&'static str> {
    let mut offset = 0usize;
    while offset + 4 <= table.len() {
        let kind = table[offset];
        let length = table[offset + 1] as usize;
        if length < 4 {
            break;
        }
        if kind == 127 {
            break;
        }
        if kind == 3 && length > 5 {
            return Some(map_chassis_code_u8(table[offset + 5]));
        }
        let mut next = offset + length;
        while next + 1 < table.len() {
            if table[next] == 0 && table[next + 1] == 0 {
                next += 2;
                break;
            }
            next += 1;
        }
        if next <= offset {
            break;
        }
        offset = next;
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemPowerStatus {
    pub ac_line: u8,
    pub battery_flag: u8,
    pub battery_percent: u8,
}

impl SystemPowerStatus {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 3 {
            return None;
        }
        Some(Self {
            ac_line: bytes[0],
            battery_flag: bytes[1],
            battery_percent: bytes[2],
        })
    }
}

pub fn apply_system_power_status(
    snapshot: &mut Snapshot,
    status: SystemPowerStatus,
    chassis: Option<&str>,
) {
    let no_battery = status.battery_flag & 128 != 0 || status.battery_percent == 255;
    if no_battery {
        snapshot.set("battery_present", Value::Bool(false));
        snapshot.set("battery_percent", Value::Unavailable);
        snapshot.set("battery_charging", Value::Unavailable);
        snapshot.set("battery_status", Value::Unavailable);
        snapshot.set("battery_health", Value::Unavailable);
        snapshot.set("battery_cycles", Value::Unavailable);
    } else {
        snapshot.set("battery_present", Value::Bool(true));
        snapshot.set(
            "battery_percent",
            Value::Number(f64::from(status.battery_percent.min(100))),
        );
        let charging = status.battery_flag & 8 != 0;
        snapshot.set("battery_charging", Value::Bool(charging));
        let text = if charging {
            "charging"
        } else if status.battery_flag & 1 != 0 {
            "discharging"
        } else if status.battery_percent >= 99 {
            "full"
        } else {
            "unknown"
        };
        snapshot.set("battery_status", Value::Text(text.into()));
        snapshot.set("battery_health", Value::Unavailable);
        snapshot.set("battery_cycles", Value::Unavailable);
    }

    match status.ac_line {
        0 => snapshot.set("ac_power", Value::Bool(false)),
        1 => snapshot.set("ac_power", Value::Bool(true)),
        _ if no_battery && chassis.is_some_and(chassis_is_mains_powered) => {
            snapshot.set("ac_power", Value::Bool(true));
        }
        _ => snapshot.set("ac_power", Value::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_smbios_desktop_chassis() {
        let mut table = vec![0u8; 16];
        table[0] = 3;
        table[1] = 9;
        table[5] = 3;
        table[9] = 0;
        table[10] = 0;
        assert_eq!(chassis_from_smbios_table(&table), Some("desktop"));
    }

    #[test]
    fn parses_raw_smbios_header() {
        let mut raw = vec![0u8; 8 + 12];
        raw[4..8].copy_from_slice(&12u32.to_le_bytes());
        raw[8] = 3;
        raw[9] = 9;
        raw[13] = 9;
        raw[17] = 0;
        raw[18] = 0;
        assert_eq!(chassis_from_raw_smbios(&raw), Some("laptop"));
    }

    #[test]
    fn desktop_without_battery_is_on_ac() {
        let mut snapshot = Snapshot::default();
        apply_system_power_status(
            &mut snapshot,
            SystemPowerStatus {
                ac_line: 255,
                battery_flag: 128,
                battery_percent: 255,
            },
            Some("desktop"),
        );
        assert_eq!(snapshot.get("battery_present"), Some(&Value::Bool(false)));
        assert_eq!(snapshot.get("ac_power"), Some(&Value::Bool(true)));
    }

    #[test]
    fn tcp_owner_pid_listen_row() {
        let mut buf = vec![0u8; 4 + 24];
        buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        buf[12..16].copy_from_slice(&u32::from(u16::to_be(22)).to_le_bytes());
        assert!(listen_ports_from_tcp_owner_pid_table(&buf).contains(&22));
    }

    #[test]
    fn laptop_charging() {
        let mut snapshot = Snapshot::default();
        apply_system_power_status(
            &mut snapshot,
            SystemPowerStatus {
                ac_line: 1,
                battery_flag: 8,
                battery_percent: 80,
            },
            Some("laptop"),
        );
        assert_eq!(snapshot.get("battery_present"), Some(&Value::Bool(true)));
        assert_eq!(snapshot.get("battery_charging"), Some(&Value::Bool(true)));
        assert_eq!(snapshot.get("ac_power"), Some(&Value::Bool(true)));
        assert_eq!(snapshot.number("battery_percent"), Some(80.0));
    }
}
