use crate::config::ProcessMonitor;
use crate::entity::Value;
use crate::snapshot::Snapshot;
use std::fs;
use std::path::Path;

pub fn collect_processes(proc_root: &Path, monitors: &[ProcessMonitor], snapshot: &mut Snapshot) {
    let names = running_idents(proc_root).unwrap_or_default();
    for monitor in monitors {
        let needle = monitor.match_name.to_ascii_lowercase();
        let running = names.iter().any(|name| name.contains(&needle));
        snapshot.set(format!("{}_running", monitor.id), Value::Bool(running));
    }
}

pub fn ident_contains(proc_root: &Path, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    running_idents(proc_root)
        .unwrap_or_default()
        .iter()
        .any(|name| name.contains(&needle))
}

fn running_idents(proc_root: &Path) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(proc_root)? {
        let entry = entry?;
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if !fname.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        if let Ok(comm) = fs::read_to_string(dir.join("comm")) {
            names.push(comm.trim().to_ascii_lowercase());
        }
        if let Ok(cmdline) = fs::read(dir.join("cmdline")) {
            if let Some(argv0) = cmdline.split(|byte| *byte == 0).next() {
                if let Ok(argv0) = std::str::from_utf8(argv0) {
                    let argv0 = argv0.to_ascii_lowercase();
                    if let Some(base) = Path::new(&argv0).file_name() {
                        names.push(base.to_string_lossy().into_owned());
                    }
                    names.push(argv0);
                }
            }
        }
        if let Ok(exe) = fs::read_link(dir.join("exe")) {
            if let Some(base) = exe.file_name() {
                names.push(base.to_string_lossy().to_ascii_lowercase());
            }
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProcessMonitor;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn monitor() -> ProcessMonitor {
        ProcessMonitor {
            id: "discord".into(),
            match_name: "discord".into(),
        }
    }

    #[test]
    fn detects_matching_comm() {
        let root =
            std::env::temp_dir().join(format!("ha-desktop-proc-comm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1")).unwrap();
        fs::create_dir_all(root.join("42")).unwrap();
        fs::write(root.join("1/comm"), "systemd\n").unwrap();
        fs::write(root.join("42/comm"), "Discord\n").unwrap();
        let mut snapshot = Snapshot::default();
        collect_processes(&root, &[monitor()], &mut snapshot);
        assert_eq!(snapshot.get("discord_running"), Some(&Value::Bool(true)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_flatpak_argv0_when_comm_is_truncated() {
        let root = std::env::temp_dir().join(format!("ha-desktop-proc-cmd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("7")).unwrap();
        fs::write(root.join("7/comm"), "com.discordapp.\n").unwrap();
        fs::write(root.join("7/cmdline"), b"/app/bin/com.discordapp.Discord\0").unwrap();
        let mut snapshot = Snapshot::default();
        collect_processes(&root, &[monitor()], &mut snapshot);
        assert_eq!(snapshot.get("discord_running"), Some(&Value::Bool(true)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_exe_basename() {
        let root = std::env::temp_dir().join(format!("ha-desktop-proc-exe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("9")).unwrap();
        fs::write(root.join("9/comm"), "bwrap\n").unwrap();
        let target = root.join("Discord");
        fs::write(&target, "").unwrap();
        symlink(&target, root.join("9/exe")).unwrap();
        let mut snapshot = Snapshot::default();
        collect_processes(&root, &[monitor()], &mut snapshot);
        assert_eq!(snapshot.get("discord_running"), Some(&Value::Bool(true)));
        let _ = fs::remove_dir_all(&root);
    }
}
