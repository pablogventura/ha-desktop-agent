use std::path::Path;

/// Prefer a package/coretemp/k10temp/zenpower sensor labelled with "tctl", "tdie" or "package".
pub fn read_cpu_temperature_c(hwmon_root: &Path) -> Option<f64> {
    let mut fallback = None;
    let entries = std::fs::read_dir(hwmon_root).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = std::fs::read_to_string(dir.join("name"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let interesting = matches!(
            name.as_str(),
            "coretemp" | "k10temp" | "zenpower" | "k8temp" | "cpu_thermal" | "acpitz"
        );
        if let Some(temp) = read_preferred_temp(&dir) {
            if interesting {
                return Some(temp);
            }
            fallback = Some(temp);
        }
    }
    fallback
}

fn read_preferred_temp(dir: &Path) -> Option<f64> {
    let mut first = None;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if !fname.starts_with("temp") || !fname.ends_with("_input") {
            continue;
        }
        let label_name = fname.replace("_input", "_label");
        let label = std::fs::read_to_string(dir.join(label_name))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let millideg: f64 = std::fs::read_to_string(entry.path())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let celsius = millideg / 1000.0;
        if !(0.0..=125.0).contains(&celsius) {
            continue;
        }
        if first.is_none() {
            first = Some(celsius);
        }
        if label.contains("tctl")
            || label.contains("tdie")
            || label.contains("package")
            || label.contains("cpu")
        {
            return Some(celsius);
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn reads_package_temp_from_fixture() {
        let root = env::temp_dir().join(format!("ha-desktop-hwmon-{}", std::process::id()));
        let chip = root.join("hwmon0");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&chip).unwrap();
        fs::write(chip.join("name"), "coretemp\n").unwrap();
        fs::write(chip.join("temp1_input"), "45000\n").unwrap();
        fs::write(chip.join("temp1_label"), "Package id 0\n").unwrap();
        assert!((read_cpu_temperature_c(&root).unwrap() - 45.0).abs() < f64::EPSILON);
        let _ = fs::remove_dir_all(&root);
    }
}
