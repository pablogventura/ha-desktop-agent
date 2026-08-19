use ha_desktop_agent::collect::{
    cpu_usage_percent, parse_meminfo, parse_os_release, parse_proc_stat, parse_uptime,
};
use ha_desktop_agent::snapshot::Snapshot;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(path).unwrap()
}

#[test]
fn proc_stat_fixture() {
    let a = parse_proc_stat(&fixture("proc_stat.txt")).unwrap();
    let b = parse_proc_stat("cpu  150 0 70 880 0 0 0 0 0 0\n").unwrap();
    let pct = cpu_usage_percent(a, b).unwrap();
    assert!((pct - 70.0).abs() < 0.01);
}

#[test]
fn meminfo_fixture() {
    let info = parse_meminfo(&fixture("meminfo.txt")).unwrap();
    let mut snapshot = Snapshot::default();
    info.apply(&mut snapshot);
    assert_eq!(
        snapshot.number("ram_available"),
        Some(ha_desktop_agent::collect::bytes_as_gb(3000000 * 1024))
    );
}

#[test]
fn os_release_and_uptime_fixtures() {
    let (name, version) = parse_os_release(&fixture("os-release"));
    assert_eq!(name.as_deref(), Some("Ubuntu"));
    assert_eq!(version.as_deref(), Some("24.04"));
    assert_eq!(parse_uptime(&fixture("uptime.txt")), Some(12345.67));
}
