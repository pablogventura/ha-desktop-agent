use crate::collect::iface_filter::{is_tailscale, is_wireguard, skip_lan_iface};
use crate::config::Config;
use crate::entity::{truncate_ha_state, Value};
use crate::snapshot::Snapshot;
use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct IfaceView {
    pub name: String,
    pub ipv4: Vec<Ipv4Addr>,
    pub up: bool,
}

#[cfg(target_os = "linux")]
pub fn collect_net(
    config: &Config,
    snapshot: &mut Snapshot,
    proc_root: &Path,
    sys_class_net: &Path,
) -> Option<String> {
    let ifaces = live_ifaces(sys_class_net);
    let listening = listening_ports_from_proc(proc_root);
    let default_ifaces = parse_default_routes(
        &read_to_string_lossy(&proc_root.join("net/route")).unwrap_or_default(),
    );
    let tailscaled = super::processes::ident_contains(proc_root, "tailscaled");
    let lan_iface = apply_net(
        config,
        snapshot,
        &ifaces,
        &listening,
        &default_ifaces,
        tailscaled,
    );
    lan_iface
}

#[cfg(target_os = "linux")]
fn live_ifaces(sys_class_net: &Path) -> Vec<IfaceView> {
    let mut ifaces = Vec::new();
    let Ok(addrs) = if_addrs::get_if_addrs() else {
        return ifaces;
    };
    let mut by_name: Vec<String> = addrs.iter().map(|iface| iface.name.clone()).collect();
    by_name.sort();
    by_name.dedup();
    for name in by_name {
        let ipv4: Vec<Ipv4Addr> = addrs
            .iter()
            .filter(|iface| iface.name == name)
            .filter_map(|iface| match iface.ip() {
                std::net::IpAddr::V4(ip) if usable_ipv4(ip) => Some(ip),
                _ => None,
            })
            .collect();
        let up = operstate_up(&sys_class_net.join(&name).join("operstate"));
        ifaces.push(IfaceView { name, ipv4, up });
    }
    ifaces
}

#[cfg(target_os = "linux")]
fn operstate_up(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            let state = raw.trim();
            state == "up" || state == "unknown"
        }
        Err(_) => true,
    }
}

pub fn apply_net(
    config: &Config,
    snapshot: &mut Snapshot,
    ifaces: &[IfaceView],
    listening: &BTreeSet<u16>,
    default_ifaces: &[String],
    tailscaled: bool,
) -> Option<String> {
    let mut lan_iface = None;
    if config.sensors.tailscale {
        let tailscale: Vec<_> = ifaces
            .iter()
            .filter(|iface| is_tailscale(&iface.name))
            .collect();
        let running = tailscale.iter().any(|iface| iface.up) || tailscaled;
        snapshot.set("tailscale_running", Value::Bool(running));
        let ip = tailscale
            .iter()
            .find(|iface| iface.up && !iface.ipv4.is_empty())
            .or_else(|| tailscale.iter().find(|iface| !iface.ipv4.is_empty()))
            .and_then(|iface| iface.ipv4.first())
            .map(ToString::to_string);
        snapshot.set("tailscale_ip", text_or_unavailable(ip));
    }
    if config.sensors.wireguard {
        let wg: Vec<_> = ifaces
            .iter()
            .filter(|iface| is_wireguard(&iface.name))
            .collect();
        snapshot.set(
            "wireguard_running",
            Value::Bool(wg.iter().any(|iface| iface.up)),
        );
        let ips: Vec<String> = wg
            .iter()
            .flat_map(|iface| iface.ipv4.iter().map(ToString::to_string))
            .collect();
        if ips.is_empty() {
            snapshot.set("wireguard_ip", Value::Unavailable);
        } else {
            snapshot.set("wireguard_ip", Value::Text(format_ip_list(&ips)));
        }
        snapshot.set_attr("wireguard_ips", serde_json::json!(ips));
    }
    if config.sensors.lan_ip {
        lan_iface = lan_iface_name(ifaces, default_ifaces);
        snapshot.set(
            "lan_ip",
            text_or_unavailable(lan_ipv4(ifaces, default_ifaces)),
        );
        if let Some(name) = &lan_iface {
            snapshot.set_attr("lan_iface", serde_json::json!(name));
        }
    }
    for listener in &config.listeners {
        let id = format!("{}_listening", listener.id);
        if config.is_disabled(&id) {
            continue;
        }
        snapshot.set(id, Value::Bool(listening.contains(&listener.port)));
    }
    lan_iface
}

fn text_or_unavailable(value: Option<String>) -> Value {
    match value {
        Some(text) if !text.is_empty() => Value::Text(text),
        _ => Value::Unavailable,
    }
}

fn usable_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_multicast()
}

pub fn lan_iface_name(ifaces: &[IfaceView], default_ifaces: &[String]) -> Option<String> {
    for name in default_ifaces {
        if skip_lan_iface(name) {
            continue;
        }
        if ipv4_for(ifaces, name).is_some() {
            return Some(name.clone());
        }
    }
    None
}

fn lan_ipv4(ifaces: &[IfaceView], default_ifaces: &[String]) -> Option<String> {
    lan_iface_name(ifaces, default_ifaces)
        .and_then(|name| ipv4_for(ifaces, &name).map(|ip| ip.to_string()))
}

fn ipv4_for(ifaces: &[IfaceView], name: &str) -> Option<Ipv4Addr> {
    ifaces
        .iter()
        .find(|iface| iface.name == name)
        .and_then(|iface| iface.ipv4.first())
        .copied()
}

pub fn format_ip_list(ips: &[String]) -> String {
    truncate_ha_state(&ips.join(", "))
}

pub fn parse_net_dev_counters(contents: &str, iface: &str) -> Option<(u64, u64)> {
    for line in contents.lines() {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != iface {
            continue;
        }
        let mut fields = rest.split_whitespace();
        let rx_bytes = fields.next()?.parse().ok()?;
        for _ in 0..7 {
            fields.next()?;
        }
        let tx_bytes = fields.next()?.parse().ok()?;
        return Some((rx_bytes, tx_bytes));
    }
    None
}

pub fn rates_kbps(prev: (u64, u64), now: (u64, u64), elapsed_s: f64) -> Option<(f64, f64)> {
    if elapsed_s <= 0.0 {
        return None;
    }
    let rx = now.0.saturating_sub(prev.0) as f64 / elapsed_s / 1000.0;
    let tx = now.1.saturating_sub(prev.1) as f64 / elapsed_s / 1000.0;
    Some((rx, tx))
}

pub fn parse_listening_ports(contents: &str) -> BTreeSet<u16> {
    let mut ports = BTreeSet::new();
    for line in contents.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(local) = parts.nth(1) else {
            continue;
        };
        let Some(_remote) = parts.next() else {
            continue;
        };
        let Some(state) = parts.next() else {
            continue;
        };
        if !state.eq_ignore_ascii_case("0A") {
            continue;
        }
        if let Some(port) = parse_hex_port(local) {
            ports.insert(port);
        }
    }
    ports
}

fn parse_hex_port(local_address: &str) -> Option<u16> {
    let port = local_address.rsplit_once(':')?.1;
    u16::from_str_radix(port, 16).ok()
}

pub fn parse_default_routes(contents: &str) -> Vec<String> {
    let mut rows: Vec<(u64, String)> = Vec::new();
    for line in contents.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(iface) = parts.next() else {
            continue;
        };
        let Some(destination) = parts.next() else {
            continue;
        };
        if !destination.eq_ignore_ascii_case("00000000") {
            continue;
        }
        let Some(metric) = parts.nth(4).and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        rows.push((metric, iface.to_string()));
    }
    rows.sort_by_key(|(metric, _)| *metric);
    rows.into_iter().map(|(_, iface)| iface).collect()
}

#[cfg(target_os = "linux")]
fn listening_ports_from_proc(proc_root: &Path) -> BTreeSet<u16> {
    let mut ports = BTreeSet::new();
    for name in ["net/tcp", "net/tcp6"] {
        if let Ok(contents) = std::fs::read_to_string(proc_root.join(name)) {
            ports.extend(parse_listening_ports(&contents));
        }
    }
    ports
}

#[cfg(target_os = "linux")]
fn read_to_string_lossy(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, PortListener};
    use crate::snapshot::Snapshot;

    fn iface(name: &str, ip: &str, up: bool) -> IfaceView {
        IfaceView {
            name: name.into(),
            ipv4: vec![ip.parse().unwrap()],
            up,
        }
    }

    #[test]
    fn classifies_ifaces() {
        assert!(is_tailscale("tailscale0"));
        assert!(is_wireguard("wg0"));
        assert!(!is_wireguard("tailscale0"));
        assert!(!is_wireguard("wg"));
        assert!(skip_lan_iface("docker0"));
        assert!(skip_lan_iface("tun0"));
        assert!(!skip_lan_iface("wlan0"));
    }

    #[test]
    fn tcp_listen_counts_port_22() {
        let ports = parse_listening_ports(include_str!("../../fixtures/proc_net_tcp.txt"));
        assert!(ports.contains(&22));
        assert!(!ports.contains(&6379));
    }

    #[test]
    fn tcp6_listen_any_22() {
        assert!(
            parse_listening_ports(include_str!("../../fixtures/proc_net_tcp6.txt")).contains(&22)
        );
    }

    #[test]
    fn default_route_picks_lowest_metric() {
        assert_eq!(
            parse_default_routes(include_str!("../../fixtures/proc_net_route.txt")),
            vec!["wlp2s0".to_string(), "tailscale0".to_string()]
        );
    }

    #[test]
    fn lan_skips_vpn_default_and_uses_up_iface() {
        let ifaces = vec![
            iface("tun0", "10.8.0.90", true),
            iface("tailscale0", "100.64.0.5", true),
            iface("wlp2s0", "192.168.1.72", true),
        ];
        assert_eq!(
            lan_ipv4(&ifaces, &["tun0".into(), "wlp2s0".into()]).as_deref(),
            Some("192.168.1.72")
        );
    }

    #[test]
    fn apply_sets_tailscale_and_ssh() {
        let mut config = Config::default();
        config.listeners = vec![PortListener {
            id: "ssh".into(),
            port: 22,
        }];
        let ifaces = vec![
            iface("tailscale0", "100.64.1.2", true),
            iface("wg0", "10.8.0.2", true),
            iface("wlp2s0", "192.168.1.72", true),
        ];
        let mut listening = BTreeSet::new();
        listening.insert(22);
        let mut snapshot = Snapshot::default();
        apply_net(
            &config,
            &mut snapshot,
            &ifaces,
            &listening,
            &["wlp2s0".into()],
            false,
        );
        assert_eq!(snapshot.get("tailscale_running"), Some(&Value::Bool(true)));
        assert_eq!(
            snapshot.get("tailscale_ip"),
            Some(&Value::Text("100.64.1.2".into()))
        );
        assert_eq!(
            snapshot.get("lan_ip"),
            Some(&Value::Text("192.168.1.72".into()))
        );
        assert_eq!(snapshot.get("wireguard_running"), Some(&Value::Bool(true)));
        assert_eq!(
            snapshot.get("wireguard_ip"),
            Some(&Value::Text("10.8.0.2".into()))
        );
        assert_eq!(snapshot.get("ssh_listening"), Some(&Value::Bool(true)));
    }

    #[test]
    fn missing_lan_is_unavailable() {
        let config = Config::default();
        let mut snapshot = Snapshot::default();
        apply_net(&config, &mut snapshot, &[], &BTreeSet::new(), &[], false);
        assert_eq!(snapshot.get("lan_ip"), Some(&Value::Unavailable));
        assert_eq!(snapshot.get("tailscale_ip"), Some(&Value::Unavailable));
        assert_eq!(snapshot.get("wireguard_ip"), Some(&Value::Unavailable));
    }

    #[test]
    fn parses_net_dev_counters() {
        let contents = "\
Inter-|   Receive                                                |  Transmit
 wlp2s0: 1000 1 0 0 0 0 0 0 2000 1 0 0 0 0 0 0
";
        assert_eq!(
            parse_net_dev_counters(contents, "wlp2s0"),
            Some((1000, 2000))
        );
    }

    #[test]
    fn truncates_ip_list() {
        let ips: Vec<String> = (0..40).map(|i| format!("10.0.0.{i}")).collect();
        let text = format_ip_list(&ips);
        assert!(text.chars().count() <= crate::entity::HA_STATE_MAX_CHARS);
        assert!(text.ends_with("..."));
    }
}
