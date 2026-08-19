mod agent;
mod hwmon;
mod nvidia;
mod proc;
mod processes;
mod rapl;

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
use std::time::Instant;

pub struct Collectors {
    proc: ProcCollector,
    rapl: RaplSampler,
    agent: AgentCollector,
    #[cfg(target_os = "linux")]
    session: Option<Arc<linux_session::LinuxSession>>,
}

impl Collectors {
    pub async fn new(_config: &Config) -> Self {
        Self {
            proc: ProcCollector::default(),
            rapl: RaplSampler::default(),
            agent: AgentCollector::new(),
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
        #[cfg(target_os = "linux")]
        if let Some(session) = &self.session {
            session.collect(config, snapshot).await;
        }
    }
}

pub use proc::{
    bytes_as_gb, cpu_usage_percent, parse_meminfo, parse_os_release, parse_proc_stat, parse_uptime,
    MemInfo,
};
pub use rapl::power_from_energy;
