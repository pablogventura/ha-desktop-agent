pub fn skip_lan_iface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "lo"
        || name == "loopback"
        || name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("br-")
        || name.starts_with("virbr")
        || name.starts_with("lxcbr")
        || name.starts_with("tun")
        || name.starts_with("tailscale")
        || name.starts_with("wg")
        || name.contains("wireguard")
        || name.starts_with("vethernet")
        || name.contains("wsl")
        || name.starts_with("isatap")
        || name.starts_with("teredo")
}

pub fn is_tailscale(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("tailscale") || name == "tailscale"
}

pub fn is_wireguard(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name.contains("wireguard") {
        return true;
    }
    let Some(rest) = name.strip_prefix("wg") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_vpn_and_windows_virtuals() {
        assert!(skip_lan_iface("lo"));
        assert!(skip_lan_iface("docker0"));
        assert!(skip_lan_iface("tailscale0"));
        assert!(skip_lan_iface("vEthernet (WSL)"));
        assert!(skip_lan_iface("WireGuard Tunnel"));
        assert!(!skip_lan_iface("eth0"));
        assert!(!skip_lan_iface("Ethernet"));
        assert!(!skip_lan_iface("Wi-Fi"));
    }
}
