use semver::Version;

pub fn parse_tag_version(tag: &str) -> anyhow::Result<Version> {
    let trimmed = tag.trim().trim_start_matches('v');
    Version::parse(trimmed).map_err(|err| anyhow::anyhow!("invalid version '{tag}': {err}"))
}

pub fn is_newer(remote: &str, current: &str) -> anyhow::Result<bool> {
    let remote = parse_tag_version(remote)?;
    let current = parse_tag_version(current)?;
    Ok(remote > current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_tags() {
        assert!(is_newer("v0.2.0", "0.1.0").unwrap());
        assert!(!is_newer("0.1.0", "0.1.0").unwrap());
        assert!(!is_newer("0.1.0", "0.2.0").unwrap());
        assert_eq!(parse_tag_version("v1.2.3").unwrap().to_string(), "1.2.3");
    }
}
