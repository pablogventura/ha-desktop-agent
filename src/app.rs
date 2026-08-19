use crate::action::{ActionRouter, IncomingCommand};
use crate::collect::Collectors;
use crate::config::Config;
use crate::entity::enabled_entities;
use crate::power::{
    append_calibration_csv, apply_estimate, features_from_snapshot, LinearModel, PowerEstimator,
};
use crate::snapshot::{PublishDecision, Snapshot};
use crate::transport::{resolve_device_id, warn_if_world_readable, MqttTransport};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

pub async fn run(config: Config, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(path) = config_path {
        warn_if_world_readable(&path);
    }
    if cfg!(unix) && is_root() {
        warn!("running as root is discouraged; use a systemd --user unit");
    }

    let device_id = resolve_device_id(&config);
    let entities = enabled_entities(&config);
    info!(
        device_id = %device_id,
        entity_count = entities.len(),
        "starting ha-desktop-agent"
    );

    #[cfg(target_os = "linux")]
    let caffeine = crate::collect::linux_session::new_caffeine_lock();
    #[cfg(target_os = "linux")]
    let session = Arc::new(crate::collect::linux_session::LinuxSession::connect(caffeine).await);

    let mut collectors = Collectors::new(&config).await;
    #[cfg(target_os = "linux")]
    collectors.set_session(session.clone());

    let mut mqtt = MqttTransport::start(&config, &device_id)?;
    mqtt.wait_connected().await;
    mqtt.subscribe_commands(&config, &device_id).await?;
    mqtt.publish_discovery(&config, &device_id).await?;
    mqtt.publish_online(&config, &device_id).await?;

    let model = LinearModel::from_config(&config.power);
    let mut decision = PublishDecision::new();
    let fast = Duration::from_millis(config.poll.fast_ms);
    let force_every = Duration::from_secs(config.poll.force_publish_s.max(1));
    let mut interval = tokio::time::interval(fast);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut shutdown = shutdown_signal();
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!("shutdown signal received");
                break;
            }
            command = mqtt.recv_command() => {
                let Some(command) = command else { break };
                handle_command(&config, #[cfg(target_os = "linux")] Some(session.as_ref()), command).await;
            }
            _ = interval.tick() => {
                let mut snapshot = Snapshot::default();
                collectors.collect(&config, &mut snapshot).await;
                if config.sensors.estimated_power {
                    let features = features_from_snapshot(&snapshot);
                    let watts = model.estimate(&features);
                    apply_estimate(&mut snapshot, watts);
                    if let Some(path) = &config.power.log_csv {
                        if let Err(err) = append_calibration_csv(path, &features, watts) {
                            warn!("failed to append calibration csv: {err}");
                        }
                    }
                }
                if decision.evaluate(&snapshot, &entities, force_every, Instant::now()) {
                    let payload =
                        serde_json::Value::Object(snapshot.to_json_map(&entities)).to_string();
                    if let Err(err) = mqtt.publish_state(&config, &device_id, payload).await {
                        warn!("failed to publish state: {err}");
                    } else {
                        tracing::debug!("published MQTT state");
                    }
                }
                while let Some(command) = mqtt.try_recv_command() {
                    handle_command(&config, #[cfg(target_os = "linux")] Some(session.as_ref()), command).await;
                }
            }
        }
    }
    Ok(())
}

async fn handle_command(
    config: &Config,
    #[cfg(target_os = "linux")] session: Option<&crate::collect::linux_session::LinuxSession>,
    command: IncomingCommand,
) {
    let router = ActionRouter::new(
        config,
        #[cfg(target_os = "linux")]
        session,
    );
    if let Err(err) = router.handle(command).await {
        error!("action failed: {err:#}");
    }
}

fn is_root() -> bool {
    #[cfg(unix)]
    {
        libc_geteuid() == 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn libc_geteuid() -> u32 {
    // Avoid a libc crate: read /proc/self/status.
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(uid) = rest.split_whitespace().next().and_then(|s| s.parse().ok()) {
                    return uid;
                }
            }
        }
    }
    1
}

fn shutdown_signal() -> tokio::sync::mpsc::Receiver<()> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {}
                _ = term.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        let _ = tx.send(()).await;
    });
    rx
}
