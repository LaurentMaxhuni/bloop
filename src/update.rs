//! Self-update from GitHub Releases: find a newer release, download its installer, check it
//! against the published SHA-256, and hand it to the UI to install.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Largest installer bloop will download.
pub const MAX_INSTALLER: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Io(String),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url().to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// A release version: exactly `major.minor.patch`, optionally written with a leading `v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.').map(|p| {
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse::<u32>().ok())
                .flatten()
        });
        let version = Version {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }

    /// The version of this build.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo.toml version is X.Y.Z")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A newer release with everything needed to download and check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub installer_url: String,
    pub checksum_url: String,
    pub size: u64,
}

impl Release {
    pub fn installer_name(&self) -> String {
        installer_name(&self.version)
    }
}

pub fn installer_name(version: &Version) -> String {
    format!("bloop-setup-{version}.exe")
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// Reads GitHub's "latest release" JSON. `Ok(None)` means there is nothing newer to install
/// (or the release is incomplete); `Err` means the reply wasn't a release at all.
pub fn parse_release(json: &[u8], current: &Version) -> Result<Option<Release>, Error> {
    let release: ApiRelease = serde_json::from_slice(json)
        .map_err(|_| Error::Protocol("GitHub returned an unexpected reply.".into()))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let Some(version) = Version::parse(&release.tag_name).filter(|v| v > current) else {
        return Ok(None);
    };
    let name = installer_name(&version);
    let find = |wanted: &str| release.assets.iter().find(|a| a.name == wanted);
    let (Some(installer), Some(checksum)) = (find(&name), find(&format!("{name}.sha256"))) else {
        return Ok(None);
    };
    if installer.size == 0 || installer.size > MAX_INSTALLER {
        return Ok(None);
    }
    Ok(Some(Release {
        version,
        installer_url: installer.browser_download_url.clone(),
        checksum_url: checksum.browser_download_url.clone(),
        size: installer.size,
    }))
}

/// Reads a `sha256sum`-style line (`<64 hex>  <file name>`) for `file_name`.
pub fn parse_checksum(text: &str, file_name: &str) -> Option<[u8; 32]> {
    text.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        if name.trim().trim_start_matches('*') != file_name || hex.len() != 64 {
            return None;
        }
        let mut digest = [0u8; 32];
        for (i, byte) in digest.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(digest)
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        for bad in ["1.2", "1.2.3.4", "a.b.c", "", "v", "1..3"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.1") > v("0.2.0"));
        assert_eq!(Version::current(), v(env!("CARGO_PKG_VERSION")));
    }

    fn release_json(tag: &str, draft: bool, prerelease: bool, assets: &[(&str, u64)]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|(name, size)| {
                format!(
                    r#"{{"name":"{name}","size":{size},"browser_download_url":"https://github.com/LaurentMaxhuni/bloop/releases/download/{tag}/{name}"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{prerelease},"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    #[test]
    fn release_json_maps_assets() {
        let current = v("0.2.0");
        let both = [
            ("bloop-setup-0.3.0.exe", 6_500_000),
            ("bloop-setup-0.3.0.exe.sha256", 90),
        ];
        let release = parse_release(
            release_json("v0.3.0", false, false, &both).as_bytes(),
            &current,
        )
        .unwrap()
        .unwrap();
        assert_eq!(release.version, v("0.3.0"));
        assert_eq!(release.size, 6_500_000);
        assert!(
            release
                .installer_url
                .ends_with("/v0.3.0/bloop-setup-0.3.0.exe")
        );
        assert!(release.checksum_url.ends_with(".exe.sha256"));

        let none = |json: String| parse_release(json.as_bytes(), &current).unwrap();
        assert!(
            none(release_json("v0.3.0", true, false, &both)).is_none(),
            "draft"
        );
        assert!(
            none(release_json("v0.3.0", false, true, &both)).is_none(),
            "prerelease"
        );
        assert!(
            none(release_json(
                "v0.2.0",
                false,
                false,
                &[
                    ("bloop-setup-0.2.0.exe", 1),
                    ("bloop-setup-0.2.0.exe.sha256", 1)
                ]
            ))
            .is_none(),
            "same version"
        );
        assert!(
            none(release_json("v0.3.0", false, false, &both[..1])).is_none(),
            "no checksum"
        );
        let huge = [
            ("bloop-setup-0.3.0.exe", 60 * 1024 * 1024),
            ("bloop-setup-0.3.0.exe.sha256", 90),
        ];
        assert!(
            none(release_json("v0.3.0", false, false, &huge)).is_none(),
            "too big"
        );
        assert!(parse_release(b"<html>rate limited</html>", &current).is_err());
    }

    #[test]
    fn checksum_file_parses() {
        let hex = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let name = "bloop-setup-0.3.0.exe";
        let digest = parse_checksum(&format!("{hex}  {name}\n"), name).unwrap();
        assert_eq!(digest[0], 0xba);
        assert_eq!(
            parse_checksum(&format!("{}  {name}", hex.to_uppercase()), name),
            Some(digest)
        );
        assert!(parse_checksum(&format!("{hex}  other.exe"), name).is_none());
        assert!(parse_checksum(&format!("{}  {name}", &hex[..60]), name).is_none());
        assert!(parse_checksum(&format!("zz{}  {name}", &hex[2..]), name).is_none());
    }

    #[test]
    fn sha256_of_known_bytes() {
        let dir = std::env::temp_dir().join(format!("bloop-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.bin");
        std::fs::write(&file, b"abc").unwrap();
        let digest = sha256_file(&file).unwrap();
        assert_eq!(
            hex(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
