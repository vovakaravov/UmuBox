use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtonSource {
    User,
    SystemAur,
    Steam,
}

impl ProtonSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::SystemAur => "System / AUR",
            Self::Steam => "Steam",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonBuild {
    pub name: String,
    pub version_tag: String,
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub source: ProtonSource,
    pub wineserver: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeProtonRelease {
    pub tag: String,
    pub published_at: String,
    pub tarball_name: String,
    pub tarball_url: String,
    pub tarball_size: u64,
    pub sha512_url: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
    Downloading,
    Verifying,
    Extracting,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub message: String,
}

pub fn user_compat_tools_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Steam/compatibilitytools.d")
}

pub fn system_compat_tools_dir() -> PathBuf {
    PathBuf::from("/usr/share/steam/compatibilitytools.d")
}

pub fn steam_common_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Steam/steamapps/common")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("umubox")
}

pub fn host_proton_arch() -> &'static str {
    std::env::consts::ARCH
}

fn is_arch_token_in_name(lower: &str, token: &str) -> bool {
    lower.contains(&format!("-{token}"))
        || lower.contains(&format!("_{token}"))
        || lower.contains(&format!(".{token}"))
}

pub fn is_compatible_with_arch(name: &str, host_arch: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if is_arch_token_in_name(&lower, "armhf")
        || is_arch_token_in_name(&lower, "armv7")
        || is_arch_token_in_name(&lower, "riscv64")
    {
        return false;
    }
    let is_arm64 =
        is_arch_token_in_name(&lower, "aarch64") || is_arch_token_in_name(&lower, "arm64");
    let is_x86_64 =
        is_arch_token_in_name(&lower, "x86_64") || is_arch_token_in_name(&lower, "amd64");

    match host_arch {
        "aarch64" => is_arm64 && !is_x86_64,
        "x86_64" => !is_arm64,
        _ => true,
    }
}

fn is_wineserver_compatible_with_arch(wineserver: Option<&Path>, host_arch: &str) -> bool {
    let Some(path) = wineserver else {
        return true;
    };
    let Ok(mut file) = File::open(path) else {
        return true;
    };
    let mut hdr = [0u8; 20];
    if file.read_exact(&mut hdr).is_err() || &hdr[..4] != b"\x7fELF" || hdr[5] != 1 {
        return true;
    }
    let e_machine = u16::from_le_bytes([hdr[18], hdr[19]]);
    match host_arch {
        "x86_64" => !matches!(e_machine, 0x00B7 | 0x0028 | 0x00F3),
        "aarch64" => !matches!(e_machine, 0x003E | 0x0003),
        _ => true,
    }
}

pub fn parse_version_file(contents: &str) -> Option<String> {
    let line = contents.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let first = parts.next()?;
    if let Some(second) = parts.next() {
        Some(second.to_string())
    } else {
        Some(first.to_string())
    }
}

pub fn find_wineserver_in_proton(proton_dir: &Path) -> Option<PathBuf> {
    for rel in ["files/bin/wineserver", "dist/bin/wineserver"] {
        let candidate = proton_dir.join(rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_proton_dir(dir: &Path) -> bool {
    let proton_script = dir.join("proton");
    let Ok(meta) = fs::metadata(&proton_script) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0 || find_wineserver_in_proton(dir).is_some())
}

fn read_build(dir: PathBuf, source: ProtonSource) -> Option<ProtonBuild> {
    if !is_proton_dir(&dir) {
        return None;
    }
    let canonical_path = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Proton")
        .to_string();
    let version_tag = fs::read_to_string(dir.join("version"))
        .ok()
        .and_then(|s| parse_version_file(&s))
        .unwrap_or_else(|| name.clone());
    let wineserver = find_wineserver_in_proton(&canonical_path);
    let host_arch = host_proton_arch();
    if !is_compatible_with_arch(&name, host_arch)
        || !is_compatible_with_arch(&version_tag, host_arch)
        || !is_wineserver_compatible_with_arch(wineserver.as_deref(), host_arch)
    {
        return None;
    }
    Some(ProtonBuild {
        name,
        version_tag,
        path: dir,
        canonical_path,
        source,
        wineserver,
    })
}

pub fn discover_proton_builds() -> Vec<ProtonBuild> {
    discover_proton_builds_in(&[
        (user_compat_tools_dir(), ProtonSource::User),
        (system_compat_tools_dir(), ProtonSource::SystemAur),
    ], &steam_common_dir())
}

pub fn discover_proton_builds_in(
    compat_roots: &[(PathBuf, ProtonSource)],
    steam_common: &Path,
) -> Vec<ProtonBuild> {
    let mut seen_canon: HashSet<PathBuf> = HashSet::new();
    let mut builds = Vec::new();
    let mut deferred_symlinks = Vec::new();

    for (root, source) in compat_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let is_symlink = fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if is_symlink {
                deferred_symlinks.push((path, *source));
                continue;
            }
            if let Some(build) = read_build(path, *source)
                && seen_canon.insert(build.canonical_path.clone())
            {
                builds.push(build);
            }
        }
    }

    for (path, source) in deferred_symlinks {
        let resolved_source = fs::canonicalize(&path)
            .ok()
            .filter(|c| c.starts_with("/usr"))
            .map(|_| ProtonSource::SystemAur)
            .unwrap_or(source);
        if let Some(build) = read_build(path, resolved_source)
            && seen_canon.insert(build.canonical_path.clone())
        {
            builds.push(build);
        }
    }

    if let Ok(entries) = fs::read_dir(steam_common) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir()
                && name.starts_with("Proton")
                && let Some(build) = read_build(path, ProtonSource::Steam)
                && seen_canon.insert(build.canonical_path.clone())
            {
                builds.push(build);
            }
        }
    }

    builds.sort_by(|a, b| {
        let rank = |s: ProtonSource| match s {
            ProtonSource::User => 0u8,
            ProtonSource::SystemAur => 1,
            ProtonSource::Steam => 2,
        };
        rank(a.source)
            .cmp(&rank(b.source))
            .then_with(|| b.version_tag.cmp(&a.version_tag))
            .then_with(|| a.name.cmp(&b.name))
    });
    builds
}

pub fn normalize_ge_tag(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_lowercase();
    if let Some(stripped) = s.strip_suffix(".tar.gz") {
        s = stripped.to_string();
    }
    for suffix in ["-x86_64", "-amd64", "-aarch64", "-arm64"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
            break;
        }
    }
    s
}

pub fn is_release_installed(tag: &str, installed: &[ProtonBuild]) -> bool {
    let target = normalize_ge_tag(tag);
    let host_arch = host_proton_arch();
    installed.iter().any(|b| {
        is_compatible_with_arch(&b.name, host_arch)
            && is_compatible_with_arch(&b.version_tag, host_arch)
            && (normalize_ge_tag(&b.version_tag) == target || normalize_ge_tag(&b.name) == target)
    })
}

pub fn verify_sha512_file(tarball: &Path, expected_sum_text: &str) -> Result<()> {
    let expected = expected_sum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("Empty SHA-512 checksum file"))?
        .trim()
        .to_ascii_lowercase();
    if expected.len() != 128 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("Invalid SHA-512 checksum format");
    }

    let file = File::open(tarball)
        .with_context(|| format!("open downloaded archive {}", tarball.display()))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        bail!("SHA-512 mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

fn asset_name(a: &serde_json::Value) -> Option<&str> {
    a.get("name").and_then(|n| n.as_str())
}

fn asset_download_url(a: &serde_json::Value) -> Option<&str> {
    a.get("browser_download_url").and_then(|u| u.as_str())
}

fn has_explicit_x86_64(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_arch_token_in_name(&lower, "x86_64") || is_arch_token_in_name(&lower, "amd64")
}

pub fn select_release_assets_for_arch<'a>(
    assets: &'a [serde_json::Value],
    host_arch: &str,
) -> Option<(&'a serde_json::Value, Option<&'a str>)> {
    let tarballs: Vec<&serde_json::Value> = assets
        .iter()
        .filter(|a| {
            asset_name(a)
                .is_some_and(|n| n.ends_with(".tar.gz") && !n.ends_with(".sha512sum"))
        })
        .collect();

    let tb = if host_arch == "x86_64" {
        tarballs
            .iter()
            .copied()
            .find(|a| asset_name(a).is_some_and(has_explicit_x86_64))
            .or_else(|| {
                tarballs
                    .iter()
                    .copied()
                    .find(|a| asset_name(a).is_some_and(|n| is_compatible_with_arch(n, host_arch)))
            })
    } else {
        tarballs
            .iter()
            .copied()
            .find(|a| asset_name(a).is_some_and(|n| is_compatible_with_arch(n, host_arch)))
    }?;

    let tb_name = asset_name(tb)?;
    let stem = tb_name.strip_suffix(".tar.gz").unwrap_or(tb_name);
    let exact_stem = format!("{stem}.sha512sum").to_ascii_lowercase();
    let exact_full = format!("{tb_name}.sha512sum").to_ascii_lowercase();

    let sums: Vec<&serde_json::Value> = assets
        .iter()
        .filter(|a| asset_name(a).is_some_and(|n| n.ends_with(".sha512sum")))
        .collect();

    let matched_sum = sums
        .iter()
        .copied()
        .find(|a| {
            asset_name(a).is_some_and(|n| {
                let lower = n.to_ascii_lowercase();
                lower == exact_stem || lower == exact_full
            })
        })
        .or_else(|| {
            let compat_sums: Vec<&serde_json::Value> = sums
                .iter()
                .copied()
                .filter(|a| asset_name(a).is_some_and(|n| is_compatible_with_arch(n, host_arch)))
                .collect();
            (compat_sums.len() == 1).then_some(compat_sums[0])
        })
        .and_then(asset_download_url);

    Some((tb, matched_sum))
}

pub fn fetch_ge_proton_releases(limit: usize) -> Result<Vec<GeProtonRelease>> {
    let url = format!(
        "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page={}",
        limit.clamp(1, 30)
    );
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "25",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: UmuBox/0.1",
            &url,
        ])
        .output()
        .context("failed to run curl to fetch GE-Proton releases")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("GitHub API request failed: {}", stderr.trim());
    }

    let val: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parse GitHub releases JSON")?;
    let arr = val
        .as_array()
        .ok_or_else(|| anyhow!("Unexpected GitHub releases payload"))?;

    let mut releases = Vec::new();
    for item in arr {
        let Some(tag) = item.get("tag_name").and_then(|v| v.as_str()) else {
            continue;
        };
        if item.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let published_at = item
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split('T')
            .next()
            .unwrap_or("")
            .to_string();
        let html_url = item
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let assets = item
            .get("assets")
            .and_then(|v| v.as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let (tb, sha512_url) =
            match select_release_assets_for_arch(assets, host_proton_arch()) {
                Some((tb, sum)) => (tb, sum.map(ToString::to_string)),
                None => continue,
            };
        let Some(tarball_name) = asset_name(tb) else {
            continue;
        };
        let Some(tarball_url) = asset_download_url(tb) else {
            continue;
        };
        let tarball_size = tb.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        releases.push(GeProtonRelease {
            tag: tag.to_string(),
            published_at,
            tarball_name: tarball_name.to_string(),
            tarball_url: tarball_url.to_string(),
            tarball_size,
            sha512_url,
            html_url,
        });
    }

    Ok(releases)
}

pub fn install_ge_proton_release(
    release: &GeProtonRelease,
    progress: impl Fn(DownloadProgress),
) -> Result<PathBuf> {
    let cache = cache_dir();
    fs::create_dir_all(&cache).with_context(|| format!("create cache dir {}", cache.display()))?;
    let compat_dir = user_compat_tools_dir();
    fs::create_dir_all(&compat_dir)
        .with_context(|| format!("create compat dir {}", compat_dir.display()))?;

    let part_path = cache.join(format!("{}.part", release.tarball_name));
    let tarball_path = cache.join(&release.tarball_name);
    let _ = fs::remove_file(&part_path);

    progress(DownloadProgress {
        phase: DownloadPhase::Downloading,
        downloaded_bytes: 0,
        total_bytes: release.tarball_size,
        message: format!("Downloading {}…", release.tarball_name),
    });

    let mut child = Command::new("curl")
        .args([
            "-fL",
            "-sS",
            "--connect-timeout",
            "15",
            "-H",
            "User-Agent: UmuBox/0.1",
            "-o",
        ])
        .arg(&part_path)
        .arg(&release.tarball_url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn curl download")?;

    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                let mut err_buf = String::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_string(&mut err_buf);
                }
                let _ = fs::remove_file(&part_path);
                bail!("Download failed ({}): {}", status, err_buf.trim());
            }
            break;
        }
        let cur = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        progress(DownloadProgress {
            phase: DownloadPhase::Downloading,
            downloaded_bytes: cur,
            total_bytes: release.tarball_size,
            message: format!("Downloading {}…", release.tarball_name),
        });
        thread::sleep(Duration::from_millis(160));
    }

    fs::rename(&part_path, &tarball_path)
        .with_context(|| format!("finalize archive {}", tarball_path.display()))?;

    if let Some(sum_url) = &release.sha512_url {
        progress(DownloadProgress {
            phase: DownloadPhase::Verifying,
            downloaded_bytes: release.tarball_size,
            total_bytes: release.tarball_size,
            message: format!("Verifying SHA-512 checksum for {}…", release.tag),
        });
        let sum_out = Command::new("curl")
            .args([
                "-fsSL",
                "--connect-timeout",
                "10",
                "--max-time",
                "20",
                "-H",
                "User-Agent: UmuBox/0.1",
                sum_url,
            ])
            .output()
            .context("fetch sha512sum asset")?;
        if !sum_out.status.success() {
            let _ = fs::remove_file(&tarball_path);
            bail!("Failed to download SHA-512 checksum");
        }
        let sum_text = String::from_utf8_lossy(&sum_out.stdout);
        if let Err(err) = verify_sha512_file(&tarball_path, &sum_text) {
            let _ = fs::remove_file(&tarball_path);
            return Err(err);
        }
    }

    let list_out = Command::new("tar")
        .arg("-tzf")
        .arg(&tarball_path)
        .output()
        .context("inspect tarball root directory")?;
    let top_dir = String::from_utf8_lossy(&list_out.stdout)
        .lines()
        .next()
        .and_then(|l| l.trim_start_matches("./").split('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or(&release.tag)
        .to_string();

    progress(DownloadProgress {
        phase: DownloadPhase::Extracting,
        downloaded_bytes: release.tarball_size,
        total_bytes: release.tarball_size,
        message: format!("Extracting {} into compatibilitytools.d…", top_dir),
    });

    let extract = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(&compat_dir)
        .output()
        .context("failed to run tar -xzf")?;
    let _ = fs::remove_file(&tarball_path);

    if !extract.status.success() {
        let stderr = String::from_utf8_lossy(&extract.stderr);
        bail!("Extraction failed: {}", stderr.trim());
    }

    Ok(compat_dir.join(top_dir))
}

pub fn remove_user_proton_build(path: &Path) -> Result<()> {
    remove_user_proton_build_from(path, &user_compat_tools_dir())
}

pub fn remove_user_proton_build_from(path: &Path, user_compat_root: &Path) -> Result<()> {
    let sym_meta = fs::symlink_metadata(path)
        .with_context(|| format!("inspect Proton path {}", path.display()))?;
    if sym_meta.file_type().is_symlink() {
        bail!("Refusing to delete symlinked/system Proton entry {}", path.display());
    }
    let canon = fs::canonicalize(path)
        .with_context(|| format!("canonicalize {}", path.display()))?;
    let root_canon = fs::canonicalize(user_compat_root)
        .with_context(|| format!("canonicalize user compat dir {}", user_compat_root.display()))?;
    if canon.parent() != Some(root_canon.as_path()) {
        bail!(
            "Refusing to remove Proton outside user compatibilitytools.d: {}",
            canon.display()
        );
    }
    fs::remove_dir_all(&canon)
        .with_context(|| format!("remove Proton directory {}", canon.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn make_mock_proton(dir: &Path, version_line: &str) {
        fs::create_dir_all(dir.join("files/bin")).unwrap();
        let proton = dir.join("proton");
        fs::write(&proton, "#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&proton).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&proton, perms).unwrap();
        fs::write(dir.join("files/bin/wineserver"), "").unwrap();
        fs::write(dir.join("version"), version_line).unwrap();
    }

    #[test]
    fn parses_proton_version_file_cleanly() {
        assert_eq!(
            parse_version_file("1793687165 GE-Proton11-12\n").as_deref(),
            Some("GE-Proton11-12")
        );
        assert_eq!(
            parse_version_file("1794497090 experimental-11.0-20261112-x86_64").as_deref(),
            Some("experimental-11.0-20261112-x86_64")
        );
        assert_eq!(parse_version_file("   \n"), None);
    }

    #[test]
    fn matches_installed_releases_across_arch_suffixes() {
        let installed = vec![ProtonBuild {
            name: "GE-Proton11-12-x86_64".to_string(),
            version_tag: "GE-Proton11-12".to_string(),
            path: PathBuf::from("/tmp/GE-Proton11-12-x86_64"),
            canonical_path: PathBuf::from("/tmp/GE-Proton11-12-x86_64"),
            source: ProtonSource::User,
            wineserver: None,
        }];
        assert!(is_release_installed("GE-Proton11-12", &installed));
        assert!(is_release_installed("ge-proton11-12", &installed));
        assert!(!is_release_installed("GE-Proton11-13", &installed));
    }

    #[test]
    fn verifies_sha512_checksum_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("test.tar.gz");
        fs::write(&p, b"hello umubox").unwrap();

        let mut hasher = Sha512::new();
        hasher.update(b"hello umubox");
        let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let line = format!("{hex}  test.tar.gz\n");
        assert!(verify_sha512_file(&p, &line).is_ok());

        let bad_line = format!("{}  test.tar.gz\n", "0".repeat(128));
        assert!(verify_sha512_file(&p, &bad_line).is_err());
    }

    #[test]
    fn deduplicates_symlinks_and_guards_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        let user_dir = tmp.path().join("user_compat");
        let sys_dir = tmp.path().join("sys_compat");
        let steam_common = tmp.path().join("steam_common");
        fs::create_dir_all(&user_dir).unwrap();
        fs::create_dir_all(&sys_dir).unwrap();
        fs::create_dir_all(&steam_common).unwrap();

        let sys_ge = sys_dir.join("proton-ge-custom");
        make_mock_proton(&sys_ge, "1793687165 GE-Proton11-12\n");
        symlink(&sys_ge, user_dir.join("proton-ge-custom")).unwrap();

        let user_ge = user_dir.join("GE-Proton11-13");
        make_mock_proton(&user_ge, "1793689999 GE-Proton11-13\n");

        let found = discover_proton_builds_in(
            &[
                (user_dir.clone(), ProtonSource::User),
                (sys_dir.clone(), ProtonSource::SystemAur),
            ],
            &steam_common,
        );
        assert_eq!(found.len(), 2);

        // Symlink and external system build must be rejected by remove_user_proton_build_from
        assert!(remove_user_proton_build_from(&user_dir.join("proton-ge-custom"), &user_dir).is_err());
        assert!(remove_user_proton_build_from(&sys_ge, &user_dir).is_err());
        assert!(remove_user_proton_build_from(&user_ge, &user_dir).is_ok());
        assert!(!user_ge.exists());
    }

    #[test]
    fn selects_host_arch_tarball_and_matching_checksum_from_multiarch_release() {
        let assets = vec![
            serde_json::json!({
                "name": "GE-Proton11-14-aarch64.sha512sum",
                "browser_download_url": "https://example.invalid/GE-Proton11-14-aarch64.sha512sum"
            }),
            serde_json::json!({
                "name": "GE-Proton11-14-aarch64.tar.gz",
                "browser_download_url": "https://example.invalid/GE-Proton11-14-aarch64.tar.gz"
            }),
            serde_json::json!({
                "name": "GE-Proton11-14-x86_64.sha512sum",
                "browser_download_url": "https://example.invalid/GE-Proton11-14-x86_64.sha512sum"
            }),
            serde_json::json!({
                "name": "GE-Proton11-14-x86_64.tar.gz",
                "browser_download_url": "https://example.invalid/GE-Proton11-14-x86_64.tar.gz"
            }),
        ];

        let (tb_x86, sum_x86) = select_release_assets_for_arch(&assets, "x86_64").unwrap();
        assert_eq!(asset_name(tb_x86), Some("GE-Proton11-14-x86_64.tar.gz"));
        assert_eq!(
            sum_x86,
            Some("https://example.invalid/GE-Proton11-14-x86_64.sha512sum")
        );

        let (tb_arm, sum_arm) = select_release_assets_for_arch(&assets, "aarch64").unwrap();
        assert_eq!(asset_name(tb_arm), Some("GE-Proton11-14-aarch64.tar.gz"));
        assert_eq!(
            sum_arm,
            Some("https://example.invalid/GE-Proton11-14-aarch64.sha512sum")
        );

        let legacy_assets = vec![
            serde_json::json!({
                "name": "GE-Proton9-27.sha512sum",
                "browser_download_url": "https://example.invalid/GE-Proton9-27.sha512sum"
            }),
            serde_json::json!({
                "name": "GE-Proton9-27.tar.gz",
                "browser_download_url": "https://example.invalid/GE-Proton9-27.tar.gz"
            }),
        ];
        let (tb_leg, sum_leg) = select_release_assets_for_arch(&legacy_assets, "x86_64").unwrap();
        assert_eq!(asset_name(tb_leg), Some("GE-Proton9-27.tar.gz"));
        assert_eq!(
            sum_leg,
            Some("https://example.invalid/GE-Proton9-27.sha512sum")
        );
        assert!(select_release_assets_for_arch(&legacy_assets, "aarch64").is_none());
    }

    #[test]
    fn validates_arch_compatibility_and_tag_normalization() {
        assert!(is_compatible_with_arch("GE-Proton11-14-x86_64", "x86_64"));
        assert!(is_compatible_with_arch("GE-Proton9-27", "x86_64"));
        assert!(!is_compatible_with_arch("GE-Proton11-14-aarch64", "x86_64"));
        assert!(is_compatible_with_arch("GE-Proton11-14-aarch64", "aarch64"));
        assert!(!is_compatible_with_arch("GE-Proton11-14-x86_64", "aarch64"));

        assert_eq!(
            normalize_ge_tag("GE-Proton11-14-x86_64.tar.gz"),
            "ge-proton11-14"
        );
        assert_eq!(
            normalize_ge_tag("GE-Proton11-14-aarch64.tar.gz"),
            "ge-proton11-14"
        );
    }
}
