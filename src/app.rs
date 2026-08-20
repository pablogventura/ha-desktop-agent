use crate::action::{ActionRouter, IncomingCommand};
use crate::collect::Collectors;
use crate::config::Config;
use crate::entity::enabled_entities;
use crate::power::{
    append_calibration_csv, apply_estimate, features_from_snapshot, LinearModel, PowerEstimator,
};
use crate::snapshot::{PublishDecision, Snapshot};
use crate::transport::{resolve_device_id, warn_if_world_readable, MqttTransport};
use crate::update::UpdateController;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

pub async fn run(config: Config, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        return run_windows_service(config, config_path, None).await;
    }
    #[cfg(not(target_os = "windows"))]
    run_linux(config, config_path).await
}

#[cfg(not(target_os = "windows"))]
async fn run_linux(config: Config, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(path) = &config_path {
        warn_if_world_readable(path);
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

    let update = UpdateController::new(&config)?;
    mqtt_loop(
        config,
        device_id,
        entities,
        collectors,
        Some(session),
        None,
        Some(update),
    )
    .await
}

#[cfg(target_os = "windows")]
pub async fn run_windows_service(
    config: Config,
    config_path: Option<PathBuf>,
    scm_stop: Option<std::sync::mpsc::Receiver<()>>,
) -> anyhow::Result<()> {
    if let Some(path) = &config_path {
        warn_if_world_readable(path);
    }
    let device_id = resolve_device_id(&config);
    let entities = enabled_entities(&config);
    info!(
        device_id = %device_id,
        entity_count = entities.len(),
        "starting ha-desktop-agent windows service"
    );
    let hub = Arc::new(crate::collect::windows::SessionHub::new());
    crate::collect::windows::spawn_pipe_server(config.clone(), hub.clone());
    let collectors = Collectors::new_windows(hub.values()).await;
    let update = UpdateController::new(&config)?;
    mqtt_loop(
        config,
        device_id,
        entities,
        collectors,
        Some(hub),
        scm_stop,
        Some(update),
    )
    .await
}

async fn mqtt_loop(
    config: Config,
    device_id: String,
    entities: Vec<crate::entity::EntityMeta>,
    mut collectors: Collectors,
    #[cfg(target_os = "linux")] session: Option<Arc<crate::collect::linux_session::LinuxSession>>,
    #[cfg(target_os = "windows")] hub: Option<Arc<crate::collect::windows::SessionHub>>,
    scm_stop: Option<std::sync::mpsc::Receiver<()>>,
    update: Option<Arc<UpdateController>>,
) -> anyhow::Result<()> {
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
            _ = async {
                if let Some(rx) = &scm_stop {
                    while rx.try_recv().is_err() {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("service stop requested");
                break;
            }
            command = mqtt.recv_command() => {
                let Some(command) = command else { break };
                handle_command(
                    &config,
                    #[cfg(target_os = "linux")] session.as_deref(),
                    #[cfg(target_os = "windows")] hub.as_deref(),
                    update.as_deref(),
                    command,
                ).await;
            }
            _ = interval.tick() => {
                if let Some(update) = &update {
                    update.tick().await;
                }
                let mut snapshot = Snapshot::default();
                collectors.collect(&config, &mut snapshot).await;
                if let Some(update) = &update {
                    update.publish_state(&mut snapshot).await;
                }
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
                    handle_command(
                        &config,
                        #[cfg(target_os = "linux")] session.as_deref(),
                        #[cfg(target_os = "windows")] hub.as_deref(),
                        update.as_deref(),
                        command,
                    ).await;
                }
            }
        }
    }
    mqtt.graceful_disconnect().await;
    Ok(())
}

async fn handle_command(
    config: &Config,
    #[cfg(target_os = "linux")] session: Option<&crate::collect::linux_session::LinuxSession>,
    #[cfg(target_os = "windows")] hub: Option<&crate::collect::windows::SessionHub>,
    update: Option<&UpdateController>,
    command: IncomingCommand,
) {
    let router = ActionRouter::new(
        config,
        #[cfg(target_os = "linux")]
        session,
        #[cfg(target_os = "windows")]
        hub,
        update,
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
