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
    if let Some(usage) = filesystem_usage(Path::new("/")) {
        apply_disk(snapshot, "root", usage);
    }
    let home = std::env::var("HOME").ok().filter(|value| !value.is_empty());
    if let Some(home) = home {
        let home_path = Path::new(&home);
        if same_filesystem(Path::new("/"), home_path) {
            snapshot.set("disk_home_used", Value::Unavailable);
            snapshot.set("disk_home_free", Value::Unavailable);
            snapshot.set("disk_home_usage", Value::Unavailable);
        } else if let Some(usage) = filesystem_usage(home_path) {
            apply_disk(snapshot, "home", usage);
        }
    }
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
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
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

#[cfg(not(unix))]
fn same_filesystem(_left: &Path, _right: &Path) -> bool {
    true
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
