//! Release update checks against GitHub Releases.

mod apply;
mod github;
mod semver_util;
mod verify;

use crate::config::{Config, UpdateConfig};
use crate::entity::Value;
use crate::snapshot::Snapshot;
use apply::{apply_update, InstallKind};
use github::{fetch_latest_release, AvailableRelease};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub use github::validate_github_repo;
pub use semver_util::{is_newer, parse_tag_version};

#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sha256sums_url: Option<String>,
    pub signature_url: Option<String>,
}

struct Inner {
    last_check: Option<Instant>,
    available: Option<AvailableUpdate>,
    applying: bool,
    last_error: Option<String>,
    applied_version: Option<String>,
}

pub struct UpdateController {
    config: UpdateConfig,
    auto: AtomicBool,
    inner: Mutex<Inner>,
    http: reqwest::Client,
}

impl UpdateController {
    pub fn new(config: &Config) -> anyhow::Result<Arc<Self>> {
        let http = reqwest::Client::builder()
            .user_agent(format!("ha-desktop-agent/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Arc::new(Self {
            config: config.update.clone(),
            auto: AtomicBool::new(config.update.auto),
            inner: Mutex::new(Inner {
                last_check: None,
                available: None,
                applying: false,
                last_error: None,
                applied_version: None,
            }),
            http,
        }))
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn auto(&self) -> bool {
        self.auto.load(Ordering::Relaxed)
    }

    pub fn set_auto(&self, on: bool) {
        self.auto.store(on, Ordering::Relaxed);
    }

    pub async fn publish_state(&self, snapshot: &mut Snapshot) {
        if !self.config.enabled {
            snapshot.set("update_available", Value::Bool(false));
            snapshot.set("update_latest_version", Value::Unavailable);
            snapshot.set("update_auto", Value::Bool(false));
            return;
        }
        snapshot.set("update_auto", Value::Bool(self.auto()));
        let guard = self.inner.lock().await;
        match &guard.available {
            Some(update) => {
                snapshot.set("update_available", Value::Bool(true));
                snapshot.set("update_latest_version", Value::Text(update.version.clone()));
            }
            None => {
                snapshot.set("update_available", Value::Bool(false));
                snapshot.set("update_latest_version", Value::Unavailable);
            }
        }
        if let Some(err) = &guard.last_error {
            snapshot.set_attr("update_error", serde_json::json!(err));
        }
    }

    pub async fn tick(&self) {
        if !self.config.enabled {
            return;
        }
        let interval = Duration::from_secs(self.config.check_interval_hours.max(1) * 3600);
        let due = {
            let guard = self.inner.lock().await;
            match guard.last_check {
                None => true,
                Some(at) => at.elapsed() >= interval,
            }
        };
        if due {
            if let Err(err) = self.check_now().await {
                warn!("update check failed: {err:#}");
                let mut guard = self.inner.lock().await;
                guard.last_error = Some(err.to_string());
                guard.last_check = Some(Instant::now());
            }
        }
        let should_apply = {
            let guard = self.inner.lock().await;
            self.auto()
                && guard.available.is_some()
                && !guard.applying
                && guard.applied_version.as_deref()
                    != guard.available.as_ref().map(|u| u.version.as_str())
        };
        if should_apply {
            if let Err(err) = self.apply_pending().await {
                warn!("auto update apply failed: {err:#}");
            }
        }
    }

    pub async fn check_now(&self) -> anyhow::Result<()> {
        let release = fetch_latest_release(&self.http, &self.config.github_repo).await?;
        let current = env!("CARGO_PKG_VERSION");
        let mut guard = self.inner.lock().await;
        guard.last_check = Some(Instant::now());
        guard.last_error = None;
        if !is_newer(&release.version, current)? {
            guard.available = None;
            return Ok(());
        }
        let kind = InstallKind::detect();
        let Some(asset) = release.asset_for(kind) else {
            guard.available = None;
            anyhow::bail!(
                "release {} has no asset for install kind {:?}",
                release.version,
                kind
            );
        };
        guard.available = Some(AvailableUpdate {
            version: release.version.clone(),
            asset_name: asset.name,
            asset_url: asset.url,
            sha256sums_url: release.sha256sums_url.clone(),
            signature_url: release.signature_url.clone(),
        });
        info!(
            latest = %release.version,
            asset = %guard.available.as_ref().unwrap().asset_name,
            "update available"
        );
        Ok(())
    }

    pub async fn apply_pending(&self) -> anyhow::Result<()> {
        let update = {
            let mut guard = self.inner.lock().await;
            if guard.applying {
                anyhow::bail!("update already in progress");
            }
            let Some(update) = guard.available.clone() else {
                anyhow::bail!("no update available");
            };
            guard.applying = true;
            guard.last_error = None;
            update
        };
        let result = apply_downloaded(&self.http, &update).await;
        let mut guard = self.inner.lock().await;
        guard.applying = false;
        match result {
            Ok(()) => {
                guard.applied_version = Some(update.version.clone());
                info!(version = %update.version, "update applied; restarting");
                drop(guard);
                apply::restart_agent();
                Ok(())
            }
            Err(err) => {
                guard.last_error = Some(err.to_string());
                Err(err)
            }
        }
    }
}

async fn apply_downloaded(http: &reqwest::Client, update: &AvailableUpdate) -> anyhow::Result<()> {
    let bytes = http
        .get(&update.asset_url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let sums = match &update.sha256sums_url {
        Some(url) => Some(
            http.get(url)
                .header(reqwest::header::ACCEPT, "application/octet-stream")
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?,
        ),
        None => None,
    };
    let sig = match &update.signature_url {
        Some(url) => Some(
            http.get(url)
                .header(reqwest::header::ACCEPT, "application/octet-stream")
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?
                .to_vec(),
        ),
        None => None,
    };
    verify::verify_asset(&update.asset_name, &bytes, sums.as_deref(), sig.as_deref())?;
    apply_update(&update.asset_name, &bytes).await
}

impl AvailableRelease {
    fn asset_for(&self, kind: InstallKind) -> Option<github::ReleaseAsset> {
        let needle = match kind {
            InstallKind::Deb => "_amd64.deb",
            InstallKind::LinuxBinary => "linux-x86_64",
            InstallKind::WindowsSetup => "setup.exe",
        };
        self.assets
            .iter()
            .find(|asset| asset.name.contains(needle))
            .cloned()
    }
}
