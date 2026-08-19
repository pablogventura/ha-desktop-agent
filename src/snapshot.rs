use crate::entity::{EntityMeta, Value};
use serde_json::{Map, Value as JsonValue};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    values: HashMap<String, Value>,
}

impl Snapshot {
    pub fn set(&mut self, id: impl Into<String>, value: Value) {
        self.values.insert(id.into(), value);
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.values.get(id)
    }

    pub fn number(&self, id: &str) -> Option<f64> {
        match self.values.get(id) {
            Some(Value::Number(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn to_json_map(&self, entities: &[EntityMeta]) -> Map<String, JsonValue> {
        let mut map = Map::new();
        for entity in entities {
            if entity.kind == crate::entity::EntityKind::Button {
                continue;
            }
            let value = self
                .values
                .get(&entity.id)
                .cloned()
                .unwrap_or(Value::Unavailable);
            map.insert(entity.id.clone(), value.to_json());
        }
        map
    }
}

pub struct PublishDecision {
    pub should_publish: bool,
    last: HashMap<String, Value>,
    last_force: Option<Instant>,
}

impl PublishDecision {
    pub fn new() -> Self {
        Self {
            should_publish: true,
            last: HashMap::new(),
            last_force: None,
        }
    }

    pub fn evaluate(
        &mut self,
        snapshot: &Snapshot,
        entities: &[EntityMeta],
        force_every: std::time::Duration,
        now: Instant,
    ) -> bool {
        let force = self
            .last_force
            .map(|t| now.duration_since(t) >= force_every)
            .unwrap_or(true);
        let mut changed = self.last.is_empty();
        for entity in entities {
            if entity.kind == crate::entity::EntityKind::Button {
                continue;
            }
            let current = snapshot
                .get(&entity.id)
                .cloned()
                .unwrap_or(Value::Unavailable);
            match self.last.get(&entity.id) {
                Some(previous) => {
                    if current.changed_enough(previous, entity.hysteresis) {
                        changed = true;
                    }
                }
                None => changed = true,
            }
            self.last.insert(entity.id.clone(), current);
        }
        let publish = changed || force;
        if publish {
            self.last_force = Some(now);
        }
        self.should_publish = publish;
        publish
    }
}

impl Default for PublishDecision {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::entity::enabled_entities;
    use std::time::Duration;

    #[test]
    fn hysteresis_skips_tiny_cpu_changes() {
        let config = Config::default();
        let entities = enabled_entities(&config);
        let mut decision = PublishDecision::new();
        let mut snap = Snapshot::default();
        snap.set("cpu_usage", Value::Number(10.0));
        let t0 = Instant::now();
        assert!(decision.evaluate(&snap, &entities, Duration::from_secs(60), t0));
        snap.set("cpu_usage", Value::Number(10.2));
        assert!(!decision.evaluate(
            &snap,
            &entities,
            Duration::from_secs(60),
            t0 + Duration::from_secs(2)
        ));
        snap.set("cpu_usage", Value::Number(11.0));
        assert!(decision.evaluate(
            &snap,
            &entities,
            Duration::from_secs(60),
            t0 + Duration::from_secs(4)
        ));
    }
}
