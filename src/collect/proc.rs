//! CPU, RAM, swap and uptime parsers. Paths are injected so tests can use fixtures.

use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuSample {
    pub idle: u64,
    pub total: u64,
}

pub fn parse_proc_stat(contents: &str) -> Option<CpuSample> {
    let line = contents.lines().find(|line| line.starts_with("cpu "))?;
    let mut parts = line.split_whitespace();
    parts.next()?;
    let mut fields = [0u64; 10];
    for (idx, field) in parts.take(10).enumerate() {
        fields[idx] = field.parse().ok()?;
    }
    // user nice system idle iowait irq softirq steal guest guest_nice
    let idle = fields[3].saturating_add(fields[4]);
    let total: u64 = fields.iter().take(8).sum();
    Some(CpuSample { idle, total })
}

pub fn cpu_usage_percent(previous: CpuSample, current: CpuSample) -> Option<f64> {
    let total_delta = current.total.saturating_sub(previous.total);
    if total_delta == 0 {
        return None;
    }
    let idle_delta = current.idle.saturating_sub(previous.idle);
    let busy = total_delta.saturating_sub(idle_delta) as f64;
    Some(((busy / total_delta as f64) * 100.0).clamp(0.0, 100.0))
}

pub fn parse_meminfo(contents: &str) -> Option<MemInfo> {
    let mut total = None;
    let mut available = None;
    let mut swap_total = None;
    let mut swap_free = None;
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let value: u64 = parts.next()?.parse().ok()?;
        let bytes = value.saturating_mul(1024);
        match key {
            "MemTotal:" => total = Some(bytes),
            "MemAvailable:" => available = Some(bytes),
            "SwapTotal:" => swap_total = Some(bytes),
            "SwapFree:" => swap_free = Some(bytes),
            _ => {}
        }
    }
    Some(MemInfo {
        total: total?,
        available: available?,
        swap_total: swap_total?,
        swap_free: swap_free?,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemInfo {
    pub fn apply(&self, snapshot: &mut Snapshot) {
        let used = self.total.saturating_sub(self.available);
        let ram_pct = if self.total == 0 {
            0.0
        } else {
            (used as f64 / self.total as f64) * 100.0
        };
        snapshot.set("ram_total", Value::Number(bytes_as_gb(self.total)));
        snapshot.set("ram_used", Value::Number(bytes_as_gb(used)));
        snapshot.set("ram_available", Value::Number(bytes_as_gb(self.available)));
        snapshot.set("ram_usage", Value::Number(ram_pct));
        snapshot.set("swap_total", Value::Number(bytes_as_gb(self.swap_total)));
        let swap_used = self.swap_total.saturating_sub(self.swap_free);
        snapshot.set("swap_used", Value::Number(bytes_as_gb(swap_used)));
        let swap_pct = if self.swap_total == 0 {
            0.0
        } else {
            (swap_used as f64 / self.swap_total as f64) * 100.0
        };
        snapshot.set("swap_usage", Value::Number(swap_pct));
    }
}

/// Home Assistant `data_size` unit `GB` is decimal (10^9 bytes).
pub fn bytes_as_gb(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000_000.0
}

pub fn parse_uptime(contents: &str) -> Option<f64> {
    contents
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
}

pub fn seconds_as_hours(seconds: f64) -> f64 {
    seconds / 3600.0
}

pub fn parse_os_release(contents: &str) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut version = None;
    for line in contents.lines() {
        if let Some((key, raw)) = line.split_once('=') {
            let value = unquote(raw);
            match key {
                "NAME" => name = Some(value),
                "VERSION_ID" => version = Some(value),
                "VERSION" if version.is_none() => version = Some(value),
                _ => {}
            }
        }
    }
    (name, version)
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(trimmed)
        .to_string()
}

pub fn average_cpu_frequency_mhz(cpu_root: &Path) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0.0;
    let entries = std::fs::read_dir(cpu_root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("cpu") {
            continue;
        }
        if !name[3..].chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let freq_path = entry.path().join("cpufreq/scaling_cur_freq");
        if let Ok(raw) = std::fs::read_to_string(freq_path) {
            if let Ok(khz) = raw.trim().parse::<f64>() {
                sum += khz / 1000.0;
                count += 1.0;
            }
        }
    }
    if count == 0.0 {
        None
    } else {
        Some(sum / count)
    }
}

pub struct ProcCollector {
    previous_cpu: Option<(CpuSample, Instant)>,
    proc_stat: std::path::PathBuf,
    meminfo: std::path::PathBuf,
    uptime: std::path::PathBuf,
    os_release: std::path::PathBuf,
    hostname: std::path::PathBuf,
    cpu_root: std::path::PathBuf,
}

impl Default for ProcCollector {
    fn default() -> Self {
        Self {
            previous_cpu: None,
            proc_stat: "/proc/stat".into(),
            meminfo: "/proc/meminfo".into(),
            uptime: "/proc/uptime".into(),
            os_release: "/etc/os-release".into(),
            hostname: "/etc/hostname".into(),
            cpu_root: "/sys/devices/system/cpu".into(),
        }
    }
}

impl ProcCollector {
    pub fn collect(&mut self, snapshot: &mut Snapshot) {
        if let Ok(stat) = std::fs::read_to_string(&self.proc_stat) {
            if let Some(sample) = parse_proc_stat(&stat) {
                let now = Instant::now();
                if let Some((prev, _)) = self.previous_cpu {
                    if let Some(pct) = cpu_usage_percent(prev, sample) {
                        snapshot.set("cpu_usage", Value::Number(pct));
                    }
                }
                self.previous_cpu = Some((sample, now));
            }
        }
        if let Ok(mem) = std::fs::read_to_string(&self.meminfo) {
            if let Some(info) = parse_meminfo(&mem) {
                info.apply(snapshot);
            }
        }
        if let Ok(uptime) = std::fs::read_to_string(&self.uptime) {
            if let Some(seconds) = parse_uptime(&uptime) {
                snapshot.set("uptime", Value::Number(seconds_as_hours(seconds)));
            }
        }
        if let Ok(os) = std::fs::read_to_string(&self.os_release) {
            let (name, version) = parse_os_release(&os);
            if let Some(name) = name {
                snapshot.set("operating_system", Value::Text(name));
            }
            if let Some(version) = version {
                snapshot.set("os_version", Value::Text(version));
            }
        }
        if let Ok(hostname) = std::fs::read_to_string(&self.hostname) {
            snapshot.set("hostname", Value::Text(hostname.trim().to_string()));
        }
        if let Some(mhz) = average_cpu_frequency_mhz(&self.cpu_root) {
            snapshot.set("cpu_frequency", Value::Number(mhz));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_from_two_samples() {
        let a = parse_proc_stat("cpu  100 0 50 850 0 0 0 0 0 0\n").unwrap();
        let b = parse_proc_stat("cpu  150 0 70 880 0 0 0 0 0 0\n").unwrap();
        let pct = cpu_usage_percent(a, b).unwrap();
        // total 1000 -> 1100 (delta 100), idle 850 -> 880 (delta 30) => 70%
        assert!((pct - 70.0).abs() < 0.01);
    }

    #[test]
    fn meminfo_uses_available() {
        let contents = "\
MemTotal:       8000000 kB
MemFree:        1000000 kB
MemAvailable:   3000000 kB
SwapTotal:      2000000 kB
SwapFree:       1500000 kB
";
        let info = parse_meminfo(contents).unwrap();
        let mut snapshot = Snapshot::default();
        info.apply(&mut snapshot);
        assert_eq!(
            snapshot.number("ram_total"),
            Some(bytes_as_gb(8000000 * 1024))
        );
        assert_eq!(
            snapshot.number("ram_available"),
            Some(bytes_as_gb(3000000 * 1024))
        );
        assert_eq!(
            snapshot.number("swap_used"),
            Some(bytes_as_gb(500000 * 1024))
        );
    }

    #[test]
    fn os_release_and_uptime() {
        let (name, version) = parse_os_release("NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\n");
        assert_eq!(name.as_deref(), Some("Ubuntu"));
        assert_eq!(version.as_deref(), Some("24.04"));
        assert_eq!(parse_uptime("12345.67 890\n"), Some(12345.67));
        assert!((seconds_as_hours(7200.0) - 2.0).abs() < f64::EPSILON);
    }
}
