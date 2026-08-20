use super::proc::bytes_as_gb;
use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

impl DiskUsage {
    pub fn usage_percent(self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }
}

pub fn collect_disk(snapshot: &mut Snapshot) {
    let root = system_root();
    if let Some(usage) = filesystem_usage(&root) {
        apply_disk(snapshot, "root", usage);
    }
    if let Some(home) = home_dir() {
        if same_filesystem(&root, &home) {
            snapshot.set("disk_home_used", Value::Unavailable);
            snapshot.set("disk_home_free", Value::Unavailable);
            snapshot.set("disk_home_usage", Value::Unavailable);
        } else if let Some(usage) = filesystem_usage(&home) {
            apply_disk(snapshot, "home", usage);
        }
    }
}

fn system_root() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        std::path::PathBuf::from(format!("{drive}\\"))
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from("/")
    }
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

fn apply_disk(snapshot: &mut Snapshot, kind: &str, usage: DiskUsage) {
    snapshot.set(
        format!("disk_{kind}_used"),
        Value::Number(bytes_as_gb(usage.used_bytes)),
    );
    snapshot.set(
        format!("disk_{kind}_free"),
        Value::Number(bytes_as_gb(usage.free_bytes)),
    );
    snapshot.set(
        format!("disk_{kind}_usage"),
        Value::Number(usage.usage_percent()),
    );
}

pub fn filesystem_usage(path: &Path) -> Option<DiskUsage> {
    #[cfg(unix)]
    {
        unix_statvfs(path)
    }
    #[cfg(windows)]
    {
        windows_usage(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn windows_usage(path: &Path) -> Option<DiskUsage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let mut free = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            Some(&mut total_free),
        )
        .ok()?;
    }
    if total == 0 {
        return None;
    }
    Some(DiskUsage {
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
        free_bytes: free,
    })
}

#[cfg(unix)]
fn unix_statvfs(path: &Path) -> Option<DiskUsage> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buf = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), buf.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let vfs = unsafe { buf.assume_init() };
    let frsize = vfs.f_frsize as u64;
    if frsize == 0 {
        return None;
    }
    let total_bytes = vfs.f_blocks as u64 * frsize;
    let free_bytes = vfs.f_bavail as u64 * frsize;
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    Some(DiskUsage {
        total_bytes,
        used_bytes,
        free_bytes,
    })
}

#[cfg(unix)]
fn same_filesystem(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(left_meta) = std::fs::metadata(left) else {
        return true;
    };
    let Ok(right_meta) = std::fs::metadata(right) else {
        return true;
    };
    left_meta.dev() == right_meta.dev()
}

#[cfg(windows)]
fn same_filesystem(left: &Path, right: &Path) -> bool {
    drive_letter(left) == drive_letter(right)
}

#[cfg(windows)]
fn drive_letter(path: &Path) -> Option<char> {
    path.to_string_lossy()
        .chars()
        .next()
        .map(|ch| ch.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_percent_from_totals() {
        let usage = DiskUsage {
            total_bytes: 1000,
            used_bytes: 250,
            free_bytes: 750,
        };
        assert!((usage.usage_percent() - 25.0).abs() < 0.01);
    }
}
