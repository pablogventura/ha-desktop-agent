mod agent;
#[cfg(target_os = "linux")]
mod battery;
mod disk;
mod hwmon;
mod nvidia;
mod proc;
mod processes;
mod rapl;

#[cfg(target_os = "linux")]
pub mod audio;
#[cfg(target_os = "linux")]
pub mod dnd;
#[cfg(target_os = "linux")]
mod net;

#[cfg(target_os = "linux")]
pub mod linux_session;

#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod windows;

use crate::config::Config;
use crate::entity::Value;
use crate::snapshot::Snapshot;
use agent::AgentCollector;
use proc::ProcCollector;
use rapl::RaplSampler;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct LanSampler {
    prev: Option<(String, u64, u64, Instant)>,
}

pub struct Collectors {
    proc: ProcCollector,
    rapl: RaplSampler,
    agent: AgentCollector,
    lan: LanSampler,
    #[cfg(target_os = "linux")]
    session: Option<Arc<linux_session::LinuxSession>>,
}

impl Collectors {
    pub async fn new(_config: &Config) -> Self {
        Self {
            proc: ProcCollector::default(),
            rapl: RaplSampler::default(),
            agent: AgentCollector::new(),
            lan: LanSampler::default(),
            #[cfg(target_os = "linux")]
            session: None,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn set_session(&mut self, session: Arc<linux_session::LinuxSession>) {
        self.session = Some(session);
    }

    pub async fn collect(&mut self, config: &Config, snapshot: &mut Snapshot) {
        self.proc.collect(snapshot);
        #[cfg(target_os = "linux")]
        battery::collect_chassis(Path::new("/sys/class/dmi/id/chassis_type"), snapshot);
        if let Some(temp) = hwmon::read_cpu_temperature_c(Path::new("/sys/class/hwmon")) {
            snapshot.set("cpu_temperature", Value::Number(temp));
        }
        let rapl = self
            .rapl
            .sample(Path::new("/sys/class/powercap"), Instant::now());
        if let Some(w) = rapl.package {
            snapshot.set("cpu_power", Value::Number(w));
        }
        if let Some(w) = rapl.dram {
            snapshot.set("dram_power", Value::Number(w));
        }
        if config.sensors.gpu {
            nvidia::collect_nvidia(snapshot);
        }
        self.agent.collect(snapshot);
        processes::collect_processes(Path::new("/proc"), &config.processes, snapshot);
        if config.sensors.disk {
            disk::collect_disk(snapshot);
        }
        #[cfg(target_os = "linux")]
        if config.sensors.battery {
            battery::collect_power_supply(Path::new("/sys/class/power_supply"), snapshot);
        }
        #[cfg(target_os = "linux")]
        let lan_iface = net::collect_net(
            config,
            snapshot,
            Path::new("/proc"),
            Path::new("/sys/class/net"),
        );
        #[cfg(target_os = "linux")]
        if config.sensors.lan_ip {
            self.lan.sample(
                lan_iface.as_deref(),
                Path::new("/proc"),
                Instant::now(),
                snapshot,
            );
        }
        #[cfg(target_os = "linux")]
        if config.sensors.audio {
            audio::collect_audio(snapshot).await;
            if let Some(Value::Text(sink)) = snapshot.get("audio_sink").cloned() {
                snapshot.set_attr("audio_sink", serde_json::json!(sink));
            }
        }
        #[cfg(target_os = "linux")]
        if config.sensors.dnd {
            dnd::collect_dnd(snapshot).await;
        }
        #[cfg(target_os = "linux")]
        if config.sensors.online {
            collect_online(config, snapshot).await;
        }
        #[cfg(target_os = "linux")]
        if let Some(session) = &self.session {
            session.collect(config, snapshot).await;
        }
    }
}

#[cfg(target_os = "linux")]
impl LanSampler {
    fn sample(
        &mut self,
        iface: Option<&str>,
        proc_root: &Path,
        now: Instant,
        snapshot: &mut Snapshot,
    ) {
        let Some(iface) = iface else {
            snapshot.set("lan_rx", Value::Unavailable);
            snapshot.set("lan_tx", Value::Unavailable);
            self.prev = None;
            return;
        };
        let Ok(contents) = std::fs::read_to_string(proc_root.join("net/dev")) else {
            return;
        };
        let Some(counters) = net::parse_net_dev_counters(&contents, iface) else {
            snapshot.set("lan_rx", Value::Unavailable);
            snapshot.set("lan_tx", Value::Unavailable);
            return;
        };
        if let Some((prev_iface, prev_rx, prev_tx, prev_at)) = &self.prev {
            if prev_iface == iface {
                let elapsed = now.duration_since(*prev_at).as_secs_f64();
                if let Some((rx, tx)) = net::rates_kbps((*prev_rx, *prev_tx), counters, elapsed) {
                    snapshot.set("lan_rx", Value::Number(rx));
                    snapshot.set("lan_tx", Value::Number(tx));
                }
            }
        }
        self.prev = Some((iface.to_string(), counters.0, counters.1, now));
    }
}

#[cfg(target_os = "linux")]
async fn collect_online(config: &Config, snapshot: &mut Snapshot) {
    let host = config.mqtt.host.clone();
    let port = config.mqtt.port;
    let reachable = tokio::time::timeout(
        Duration::from_millis(400),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some();
    snapshot.set("online", Value::Bool(reachable));
}

pub use proc::{
    bytes_as_gb, cpu_usage_percent, parse_meminfo, parse_os_release, parse_proc_stat, parse_uptime,
    MemInfo,
};
pub use rapl::power_from_energy;
