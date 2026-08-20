use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct AvailableRelease {
    pub version: String,
    pub assets: Vec<ReleaseAsset>,
    pub sha256sums_url: Option<String>,
    pub signature_url: Option<String>,
}

pub fn parse_release_json(body: &str) -> anyhow::Result<AvailableRelease> {
    let release: GithubRelease = serde_json::from_str(body)?;
    let version = super::parse_tag_version(&release.tag_name)?.to_string();
    let mut assets = Vec::new();
    let mut sha256sums_url = None;
    let mut signature_url = None;
    for asset in release.assets {
        if asset.name == "SHA256SUMS" {
            sha256sums_url = Some(asset.browser_download_url.clone());
        } else if asset.name == "SHA256SUMS.sig" {
            signature_url = Some(asset.browser_download_url.clone());
        } else {
            assets.push(ReleaseAsset {
                name: asset.name,
                url: asset.browser_download_url,
            });
        }
    }
    Ok(AvailableRelease {
        version,
        assets,
        sha256sums_url,
        signature_url,
    })
}

pub fn validate_github_repo(repo: &str) -> anyhow::Result<()> {
    let mut parts = repo.split('/');
    let owner = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| anyhow::anyhow!("update.github_repo must be owner/name"))?;
    let name = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| anyhow::anyhow!("update.github_repo must be owner/name"))?;
    if parts.next().is_some() {
        anyhow::bail!("update.github_repo must be owner/name");
    }
    for part in [owner, name] {
        if !part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
        {
            anyhow::bail!("update.github_repo contains invalid characters");
        }
    }
    Ok(())
}

pub async fn fetch_latest_release(
    http: &reqwest::Client,
    repo: &str,
) -> anyhow::Result<AvailableRelease> {
    validate_github_repo(repo)?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let body = http
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_release_json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_release() {
        let body = include_str!("../../fixtures/update/github_release.json");
        let release = parse_release_json(body).unwrap();
        assert_eq!(release.version, "0.1.1");
        assert!(release.sha256sums_url.is_some());
        assert!(release.signature_url.is_some());
        assert!(release
            .assets
            .iter()
            .any(|asset| asset.name.contains("amd64.deb")));
    }

    #[test]
    fn validates_repo_slug() {
        assert!(validate_github_repo("pablogventura/ha-desktop-agent").is_ok());
        assert!(validate_github_repo("../etc/passwd").is_err());
        assert!(validate_github_repo("onlyone").is_err());
    }
}
