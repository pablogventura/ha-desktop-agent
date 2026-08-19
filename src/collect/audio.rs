use crate::entity::{truncate_ha_state, Value};
use crate::snapshot::Snapshot;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct VolumeInfo {
    pub percent: f64,
    pub muted: bool,
    pub sink: Option<String>,
}

pub fn parse_wpctl_volume(output: &str) -> Option<(f64, bool)> {
    let line = output.lines().find(|row| row.contains("Volume:"))?;
    let muted = line.contains("[MUTED]");
    let number = line
        .split_whitespace()
        .find_map(|part| part.parse::<f64>().ok())?;
    Some(((number * 100.0).clamp(0.0, 150.0), muted))
}

pub fn parse_wpctl_sink_name(inspect: &str) -> Option<String> {
    for key in ["node.description", "node.nick", "media.name", "node.name"] {
        for line in inspect.lines() {
            let line = line.trim();
            let Some((left, right)) = line.split_once('=') else {
                continue;
            };
            if left.trim().trim_start_matches('*').trim() != key {
                continue;
            }
            let value = right.trim().trim_matches('"').trim();
            if !value.is_empty() {
                return Some(truncate_ha_state(value));
            }
        }
    }
    None
}

pub async fn collect_audio(snapshot: &mut Snapshot) {
    let Some(info) = read_volume().await else {
        snapshot.set("volume", Value::Unavailable);
        snapshot.set("muted", Value::Unavailable);
        snapshot.set("audio_sink", Value::Unavailable);
        return;
    };
    snapshot.set("volume", Value::Number(info.percent));
    snapshot.set("muted", Value::Bool(info.muted));
    match info.sink {
        Some(name) => snapshot.set("audio_sink", Value::Text(name)),
        None => snapshot.set("audio_sink", Value::Unavailable),
    }
}

async fn read_volume() -> Option<VolumeInfo> {
    let volume = wpctl(&["get-volume", "@DEFAULT_AUDIO_SINK@"]).await?;
    let (percent, muted) = parse_wpctl_volume(&volume)?;
    let inspect = wpctl(&["inspect", "@DEFAULT_AUDIO_SINK@"])
        .await
        .unwrap_or_default();
    Some(VolumeInfo {
        percent,
        muted,
        sink: parse_wpctl_sink_name(&inspect),
    })
}

async fn wpctl(args: &[&str]) -> Option<String> {
    let output = Command::new("wpctl").args(args).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

pub async fn set_muted(muted: bool) -> anyhow::Result<()> {
    let flag = if muted { "1" } else { "0" };
    run_wpctl(&["set-mute", "@DEFAULT_AUDIO_SINK@", flag]).await
}

pub async fn bump_volume(percent_delta: i32) -> anyhow::Result<()> {
    let spec = if percent_delta >= 0 {
        format!("{percent_delta}%+")
    } else {
        format!("{}%-", percent_delta.abs())
    };
    run_wpctl(&["set-volume", "@DEFAULT_AUDIO_SINK@", &spec]).await
}

async fn run_wpctl(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("wpctl").args(args).status().await?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("wpctl {args:?} failed with {status}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_volume_and_mute() {
        assert_eq!(parse_wpctl_volume("Volume: 0.40\n"), Some((40.0, false)));
        assert_eq!(
            parse_wpctl_volume("Volume: 0.50 [MUTED]\n"),
            Some((50.0, true))
        );
    }

    #[test]
    fn parses_inspect_description() {
        let inspect = "\
id 42, type PipeWire:Interface:Node
  * node.description = \"Built-in Audio Analog Stereo\"
  * node.name = \"alsa_output.pci\"
";
        assert_eq!(
            parse_wpctl_sink_name(inspect).as_deref(),
            Some("Built-in Audio Analog Stereo")
        );
    }
}
