use crate::entity::Value;
use crate::snapshot::Snapshot;
use tokio::process::Command;

const SCHEMA: &str = "org.gnome.desktop.notifications";
const KEY: &str = "show-banners";

/// GNOME Do Not Disturb is `show-banners = false`.
pub fn parse_gsettings_bool(output: &str) -> Option<bool> {
    match output.trim().trim_matches('\'') {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn dnd_from_show_banners(show_banners: bool) -> bool {
    !show_banners
}

pub async fn collect_dnd(snapshot: &mut Snapshot) {
    match read_show_banners().await {
        Some(show_banners) => {
            snapshot.set(
                "do_not_disturb",
                Value::Bool(dnd_from_show_banners(show_banners)),
            );
        }
        None => snapshot.set("do_not_disturb", Value::Unavailable),
    }
}

async fn read_show_banners() -> Option<bool> {
    let output = Command::new("gsettings")
        .args(["get", SCHEMA, KEY])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_gsettings_bool(&String::from_utf8(output.stdout).ok()?)
}

pub async fn set_dnd(on: bool) -> anyhow::Result<()> {
    let show_banners = if on { "false" } else { "true" };
    let status = Command::new("gsettings")
        .args(["set", SCHEMA, KEY, show_banners])
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("gsettings set {SCHEMA} {KEY} failed with {status}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gsettings_bool() {
        assert_eq!(parse_gsettings_bool("true\n"), Some(true));
        assert_eq!(parse_gsettings_bool("false"), Some(false));
        assert_eq!(parse_gsettings_bool("'false'\n"), Some(false));
        assert!(parse_gsettings_bool("uint32 1").is_none());
    }

    #[test]
    fn dnd_is_inverse_of_banners() {
        assert!(dnd_from_show_banners(false));
        assert!(!dnd_from_show_banners(true));
    }
}
