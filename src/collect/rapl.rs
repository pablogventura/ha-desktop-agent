use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
struct RaplZone {
    name: String,
    energy_uj: u64,
    max_range_uj: Option<u64>,
}

#[derive(Debug, Default)]
pub struct RaplSampler {
    previous: Option<(Instant, Vec<RaplZone>)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RaplWatts {
    pub package: Option<f64>,
    pub dram: Option<f64>,
}

pub fn parse_energy_uj(contents: &str) -> Option<u64> {
    contents.trim().parse().ok()
}

pub fn power_from_energy(
    prev_uj: u64,
    prev_at: Instant,
    now_uj: u64,
    now_at: Instant,
    max_range_uj: Option<u64>,
) -> Option<f64> {
    let elapsed = now_at.duration_since(prev_at).as_secs_f64();
    if elapsed <= 0.0 {
        return None;
    }
    let delta = if now_uj >= prev_uj {
        now_uj - prev_uj
    } else {
        let range = max_range_uj?;
        now_uj + range.saturating_sub(prev_uj)
    };
    Some((delta as f64 / 1_000_000.0) / elapsed)
}

fn read_rapl_tree(root: &Path) -> Vec<RaplZone> {
    let mut zones = Vec::new();
    walk_rapl(root, &mut zones, 0);
    zones
}

fn walk_rapl(dir: &Path, zones: &mut Vec<RaplZone>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let name = std::fs::read_to_string(dir.join("name"))
        .ok()
        .map(|s| s.trim().to_string());
    let energy = std::fs::read_to_string(dir.join("energy_uj"))
        .ok()
        .and_then(|s| parse_energy_uj(&s));
    if let (Some(name), Some(energy_uj)) = (name, energy) {
        let max_range_uj = std::fs::read_to_string(dir.join("max_energy_range_uj"))
            .ok()
            .and_then(|s| s.trim().parse().ok());
        zones.push(RaplZone {
            name,
            energy_uj,
            max_range_uj,
        });
    }
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        // sysfs `device`/`subsystem` symlinks point at parents and would recurse forever.
        if !fname.starts_with("intel-rapl") {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk_rapl(&path, zones, depth + 1);
        }
    }
}

fn is_package(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("package") || lower == "psys" || lower.starts_with("package-")
}

fn is_dram(name: &str) -> bool {
    name.eq_ignore_ascii_case("dram")
}

impl RaplSampler {
    pub fn sample(&mut self, root: &Path, now: Instant) -> RaplWatts {
        let zones = read_rapl_tree(root);
        if zones.is_empty() {
            return RaplWatts::default();
        }
        let watts = if let Some((prev_at, prev_zones)) = &self.previous {
            let mut package = None;
            let mut dram = None;
            for zone in &zones {
                let Some(prev) = prev_zones.iter().find(|prev| prev.name == zone.name) else {
                    continue;
                };
                let Some(w) = power_from_energy(
                    prev.energy_uj,
                    *prev_at,
                    zone.energy_uj,
                    now,
                    zone.max_range_uj.or(prev.max_range_uj),
                ) else {
                    continue;
                };
                if is_package(&zone.name) {
                    package = Some(package.unwrap_or(0.0) + w);
                } else if is_dram(&zone.name) {
                    dram = Some(dram.unwrap_or(0.0) + w);
                }
            }
            RaplWatts { package, dram }
        } else {
            RaplWatts::default()
        };
        self.previous = Some((now, zones));
        watts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn converts_microjoules_to_watts() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(2);
        let watts = power_from_energy(1_000_000, t0, 3_000_000, t1, None).unwrap();
        assert!((watts - 1.0).abs() < 1e-9);
    }

    #[test]
    fn does_not_follow_sysfs_parent_symlinks() {
        let root = std::env::temp_dir().join(format!("ha-rapl-cycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let zone = root.join("intel-rapl:0");
        std::fs::create_dir_all(&zone).unwrap();
        std::fs::write(zone.join("name"), "package-0\n").unwrap();
        std::fs::write(zone.join("energy_uj"), "1000\n").unwrap();
        std::os::unix::fs::symlink(&root, zone.join("device")).unwrap();
        let zones = read_rapl_tree(&root);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].name, "package-0");
        let _ = std::fs::remove_dir_all(&root);
    }
}
