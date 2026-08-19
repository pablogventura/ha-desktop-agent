use crate::config::PowerConfig;
use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct PowerFeatures {
    pub cpu_package_w: Option<f64>,
    pub dram_w: Option<f64>,
    pub gpu_w: Option<f64>,
    pub cpu_usage: Option<f64>,
    pub gpu_usage: Option<f64>,
}

pub trait PowerEstimator {
    fn estimate(&self, features: &PowerFeatures) -> f64;
}

pub struct LinearModel {
    idle_w: f64,
    coefficients: std::collections::HashMap<String, f64>,
}

impl LinearModel {
    pub fn from_config(config: &PowerConfig) -> Self {
        Self {
            idle_w: config.idle_w,
            coefficients: config.coefficients.clone(),
        }
    }

    fn coef(&self, name: &str) -> f64 {
        self.coefficients.get(name).copied().unwrap_or(0.0)
    }
}

impl PowerEstimator for LinearModel {
    fn estimate(&self, features: &PowerFeatures) -> f64 {
        self.idle_w
            + self.coef("cpu_package_w") * features.cpu_package_w.unwrap_or(0.0)
            + self.coef("dram_w") * features.dram_w.unwrap_or(0.0)
            + self.coef("gpu_w") * features.gpu_w.unwrap_or(0.0)
            + self.coef("cpu_usage") * features.cpu_usage.unwrap_or(0.0)
            + self.coef("gpu_usage") * features.gpu_usage.unwrap_or(0.0)
    }
}

pub fn features_from_snapshot(snapshot: &Snapshot) -> PowerFeatures {
    PowerFeatures {
        cpu_package_w: snapshot.number("cpu_power"),
        dram_w: snapshot.number("dram_power"),
        gpu_w: snapshot.number("gpu_power"),
        cpu_usage: snapshot.number("cpu_usage"),
        gpu_usage: snapshot.number("gpu_usage"),
    }
}

pub fn apply_estimate(snapshot: &mut Snapshot, watts: f64) {
    snapshot.set("estimated_power", Value::Number(watts.max(0.0)));
}

pub fn append_calibration_csv(
    path: &std::path::Path,
    features: &PowerFeatures,
    estimated_w: f64,
) -> std::io::Result<()> {
    let exists = path.exists();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if !exists {
        writeln!(
            file,
            "unix_ts,cpu_package_w,dram_w,gpu_w,cpu_usage,gpu_usage,estimated_w"
        )?;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    writeln!(
        file,
        "{ts:.3},{},{},{},{},{},{estimated_w:.3}",
        fmt(features.cpu_package_w),
        fmt(features.dram_w),
        fmt(features.gpu_w),
        fmt(features.cpu_usage),
        fmt(features.gpu_usage),
    )?;
    Ok(())
}

fn fmt(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.4}"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PowerConfig;

    #[test]
    fn linear_model_sums_available_terms() {
        let model = LinearModel::from_config(&PowerConfig::default());
        let watts = model.estimate(&PowerFeatures {
            cpu_package_w: Some(20.0),
            dram_w: Some(5.0),
            gpu_w: Some(40.0),
            cpu_usage: Some(50.0),
            gpu_usage: None,
        });
        assert!((watts - 95.0).abs() < 1e-9);
    }
}
