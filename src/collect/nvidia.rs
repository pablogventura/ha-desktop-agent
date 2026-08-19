use crate::entity::Value;
use crate::snapshot::Snapshot;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;
use std::sync::OnceLock;

static NVML: OnceLock<Option<Nvml>> = OnceLock::new();

fn nvml() -> Option<&'static Nvml> {
    NVML.get_or_init(|| Nvml::init().ok()).as_ref()
}

pub fn collect_nvidia(snapshot: &mut Snapshot) {
    let Some(nvml) = nvml() else {
        return;
    };
    if let Ok(version) = nvml.sys_driver_version() {
        snapshot.set("gpu_driver", Value::Text(version));
    }
    let Ok(device) = nvml.device_by_index(0) else {
        return;
    };
    if let Ok(util) = device.utilization_rates() {
        snapshot.set("gpu_usage", Value::Number(f64::from(util.gpu)));
    }
    if let Ok(mem) = device.memory_info() {
        snapshot.set(
            "gpu_memory_used",
            Value::Number(super::proc::bytes_as_gb(mem.used)),
        );
    }
    if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
        snapshot.set("gpu_temperature", Value::Number(f64::from(temp)));
    }
    if let Ok(mw) = device.power_usage() {
        snapshot.set("gpu_power", Value::Number(f64::from(mw) / 1000.0));
    }
    if let Ok(fan) = device.fan_speed(0) {
        snapshot.set("gpu_fan", Value::Number(f64::from(fan)));
    }
    if let Ok(limit) = device.enforced_power_limit() {
        snapshot.set("gpu_power_limit", Value::Number(f64::from(limit) / 1000.0));
    } else if let Ok(limit) = device.power_management_limit() {
        snapshot.set("gpu_power_limit", Value::Number(f64::from(limit) / 1000.0));
    }
}
