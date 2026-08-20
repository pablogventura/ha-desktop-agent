#![cfg(target_os = "windows")]

mod machine;
mod pipe;
mod service;
mod session;

use crate::action::IncomingCommand;
use crate::config::Config;
use crate::entity::Value;
use crate::ipc::{IpcMessage, IpcValue};
use crate::snapshot::Snapshot;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use machine::{power_action, WindowsMachine};
pub use pipe::{spawn_pipe_server, SessionHub};
pub use service::dispatch_windows_service;
pub use session::run_session_loop;

pub struct WindowsCollectors {
    pub machine: WindowsMachine,
    pub session: Arc<Mutex<HashMap<String, Value>>>,
}

impl WindowsCollectors {
    pub fn new(session: Arc<Mutex<HashMap<String, Value>>>) -> Self {
        Self {
            machine: WindowsMachine::default(),
            session,
        }
    }

    pub fn collect(&mut self, config: &Config, snapshot: &mut Snapshot) {
        self.machine.collect(config, snapshot);
        snapshot.set("cpu_power", Value::Unavailable);
        snapshot.set("dram_power", Value::Unavailable);
        if config.sensors.dnd {
            snapshot.set("do_not_disturb", Value::Unavailable);
        }
        fill_session_defaults(config, snapshot);
        let session = self.session.lock().unwrap();
        for (key, value) in session.iter() {
            snapshot.set(key.clone(), value.clone());
        }
    }
}

fn fill_session_defaults(config: &Config, snapshot: &mut Snapshot) {
    snapshot.set("session_type", Value::Text("windows".into()));
    snapshot.set("desktop_environment", Value::Text("Windows".into()));
    snapshot.set("idle_time", Value::Unavailable);
    snapshot.set("user_active", Value::Unavailable);
    snapshot.set("locked", Value::Unavailable);
    snapshot.set("caffeine", Value::Bool(false));
    snapshot.set("suspend_inhibited", Value::Unavailable);
    if config.sensors.audio {
        snapshot.set("volume", Value::Unavailable);
        snapshot.set("muted", Value::Unavailable);
        snapshot.set("audio_sink", Value::Unavailable);
    }
    if config.sensors.mpris {
        snapshot.set("media_title", Value::Unavailable);
        snapshot.set("media_artist", Value::Unavailable);
        snapshot.set("media_playing", Value::Bool(false));
    }
}

pub fn apply_ipc_snapshot(target: &mut HashMap<String, Value>, values: HashMap<String, IpcValue>) {
    target.clear();
    for (key, value) in values {
        target.insert(key, Value::from(value));
    }
}

pub fn merge_tick_defaults(snapshot: &mut Snapshot) {
    if snapshot.get("session_type").is_none() {
        snapshot.set("session_type", Value::Text("windows".into()));
    }
}

pub fn rpc_from_command(command: &IncomingCommand) -> Option<IpcMessage> {
    match command {
        IncomingCommand::Switch { id, on } if id == "mute" => Some(IpcMessage::Rpc {
            id: 0,
            method: "set_mute".into(),
            title: None,
            body: None,
            urgent: false,
            on: Some(*on),
            delta: None,
            action: None,
        }),
        IncomingCommand::Switch { id, on } if id == "caffeine" => Some(IpcMessage::Rpc {
            id: 0,
            method: "set_caffeine".into(),
            title: None,
            body: None,
            urgent: false,
            on: Some(*on),
            delta: None,
            action: None,
        }),
        IncomingCommand::Switch { id, on: true } if id == "lock" => Some(IpcMessage::Rpc {
            id: 0,
            method: "lock".into(),
            title: None,
            body: None,
            urgent: false,
            on: None,
            delta: None,
            action: None,
        }),
        IncomingCommand::Press { id } if id == "lock" => Some(IpcMessage::Rpc {
            id: 0,
            method: "lock".into(),
            title: None,
            body: None,
            urgent: false,
            on: None,
            delta: None,
            action: None,
        }),
        IncomingCommand::Press { id } if id == "volume_up" => Some(IpcMessage::Rpc {
            id: 0,
            method: "bump_volume".into(),
            title: None,
            body: None,
            urgent: false,
            on: None,
            delta: Some(5),
            action: None,
        }),
        IncomingCommand::Press { id } if id == "volume_down" => Some(IpcMessage::Rpc {
            id: 0,
            method: "bump_volume".into(),
            title: None,
            body: None,
            urgent: false,
            on: None,
            delta: Some(-5),
            action: None,
        }),
        IncomingCommand::Press { id }
            if matches!(
                id.as_str(),
                "media_play_pause" | "media_next" | "media_previous"
            ) =>
        {
            Some(IpcMessage::Rpc {
                id: 0,
                method: "mpris".into(),
                title: None,
                body: None,
                urgent: false,
                on: None,
                delta: None,
                action: Some(id.clone()),
            })
        }
        IncomingCommand::Notify {
            title,
            body,
            urgency,
        } => Some(IpcMessage::Rpc {
            id: 0,
            method: "notify".into(),
            title: title.clone(),
            body: body.clone(),
            urgent: matches!(urgency, crate::action::NotificationUrgency::Critical),
            on: None,
            delta: None,
            action: None,
        }),
        _ => None,
    }
}
