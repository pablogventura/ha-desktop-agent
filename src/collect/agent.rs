use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::time::Instant;

pub struct AgentCollector {
    started: Instant,
}

impl AgentCollector {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub fn collect(&self, snapshot: &mut Snapshot) {
        snapshot.set(
            "agent_version",
            Value::Text(env!("CARGO_PKG_VERSION").to_string()),
        );
        snapshot.set(
            "agent_uptime",
            Value::Number(self.started.elapsed().as_secs_f64()),
        );
    }
}

impl Default for AgentCollector {
    fn default() -> Self {
        Self::new()
    }
}
