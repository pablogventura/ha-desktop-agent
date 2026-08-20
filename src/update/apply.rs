use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Deb,
    LinuxBinary,
    #[allow(dead_code)]
    WindowsSetup,
}

impl InstallKind {
    pub fn detect() -> Self {
        #[cfg(windows)]
        {
            return Self::WindowsSetup;
        }
        #[cfg(not(windows))]
        {
            if Path::new("/usr/bin/ha-desktop-agent").exists() {
                Self::Deb
            } else {
                Self::LinuxBinary
            }
        }
    }
}

pub async fn apply_update(asset_name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let _ = asset_name;
        return apply_windows_setup(bytes).await;
    }
    #[cfg(not(windows))]
    {
        match InstallKind::detect() {
            InstallKind::Deb => apply_linux_deb(asset_name, bytes).await,
            InstallKind::LinuxBinary => apply_linux_binary(bytes).await,
            InstallKind::WindowsSetup => anyhow::bail!("windows setup on non-windows host"),
        }
    }
}

#[cfg(not(windows))]
async fn apply_linux_deb(asset_name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(asset_name);
    tokio::fs::write(&path, bytes).await?;
    info!(path = %path.display(), "installing deb via pkexec dpkg");
    let status = Command::new("pkexec")
        .args(["dpkg", "-i", path.to_str().unwrap_or_default()])
        .status()
        .await;
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => anyhow::bail!("pkexec dpkg exited with {status}"),
        Err(err) => {
            warn!("pkexec failed ({err}); trying sudo -n dpkg");
            let status = Command::new("sudo")
                .args(["-n", "dpkg", "-i", path.to_str().unwrap_or_default()])
                .status()
                .await?;
            if status.success() {
                Ok(())
            } else {
                anyhow::bail!("sudo dpkg exited with {status}");
            }
        }
    }
}

#[cfg(not(windows))]
async fn apply_linux_binary(bytes: &[u8]) -> anyhow::Result<()> {
    let dest = linux_binary_dest()?;
    let tmp = dest.with_extension("new");
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&tmp, bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&tmp).await?.permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&tmp, perms).await?;
    }
    tokio::fs::rename(&tmp, &dest).await?;
    info!(path = %dest.display(), "replaced local agent binary");
    Ok(())
}

#[cfg(not(windows))]
fn linux_binary_dest() -> anyhow::Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if exe.ends_with("ha-desktop-agent") {
            return Ok(exe);
        }
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME unset"))?;
    Ok(PathBuf::from(home).join(".local/bin/ha-desktop-agent"))
}

#[cfg(windows)]
async fn apply_windows_setup(bytes: &[u8]) -> anyhow::Result<()> {
    let path = std::env::temp_dir().join("ha-desktop-agent-setup.exe");
    tokio::fs::write(&path, bytes).await?;
    info!(path = %path.display(), "running silent NSIS setup");
    let status = Command::new(&path).arg("/S").status().await?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("NSIS setup exited with {status}");
    }
}

pub fn restart_agent() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("sc.exe")
            .args(["stop", "ha-desktop-agent"])
            .status();
        let _ = std::process::Command::new("sc.exe")
            .args(["start", "ha-desktop-agent"])
            .status();
        return;
    }
    #[cfg(not(windows))]
    {
        // Spawn and do not wait: waiting on `systemctl restart` races with our own SIGTERM.
        let spawn = std::process::Command::new("systemctl")
            .args(["--user", "restart", "ha-desktop-agent.service"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Err(err) = spawn {
            warn!("systemctl restart spawn failed: {err}");
        }
        // If not under systemd, exit so a supervisor/user restarts us.
        if !Path::new("/usr/lib/systemd/user/ha-desktop-agent.service").exists()
            && std::env::var_os("XDG_RUNTIME_DIR").is_none()
        {
            std::process::exit(0);
        }
    }
}
