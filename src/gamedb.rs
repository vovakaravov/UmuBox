use crate::proton::{ProtonBuild, cache_dir};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const UMU_DATABASE_URL: &str =
    "https://raw.githubusercontent.com/Open-Wine-Components/umu-database/main/umu-database.csv";
const PROTONDB_CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UmuDbEntry {
    pub title: String,
    pub store: String,
    pub codename: String,
    pub umu_id: String,
    pub acronym: String,
    pub note: String,
    pub exe_strings: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonFixInfo {
    pub script_path: PathBuf,
    pub store_bucket: String,
    pub summary: String,
    pub detected_verbs: Vec<String>,
    pub detected_dll_overrides: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtonDbSummary {
    #[serde(default)]
    pub app_id: u64,
    #[serde(default)]
    pub tier: String,
    #[serde(default, rename = "bestReportedTier")]
    pub best_reported_tier: String,
    #[serde(default, rename = "trendingTier")]
    pub trending_tier: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub total: u64,
}

pub fn cached_umu_db_path() -> PathBuf {
    cache_dir().join("umu-database.csv")
}

/// Parses RFC-4180 CSV lines handling quoted fields and escaped `""` quotes.
fn parse_csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(ch);
            }
        } else if ch == '"' && cur.is_empty() {
            in_quotes = true;
        } else if ch == ',' {
            fields.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

pub fn parse_umu_csv(contents: &str) -> Vec<UmuDbEntry> {
    let mut out = Vec::new();
    for (line_no, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let cols = parse_csv_record(line);
        if line_no == 0
            && cols
                .first()
                .is_some_and(|c| c.eq_ignore_ascii_case("TITLE"))
        {
            continue;
        }
        if cols.len() < 4 {
            continue;
        }
        let title = cols[0].clone();
        let umu_id = cols[3].clone();
        if title.is_empty() || umu_id.is_empty() {
            continue;
        }
        out.push(UmuDbEntry {
            title,
            store: cols[1].clone(),
            codename: cols[2].clone(),
            umu_id,
            acronym: cols.get(4).cloned().unwrap_or_default(),
            note: cols.get(5).cloned().unwrap_or_default(),
            exe_strings: cols.get(6).cloned().unwrap_or_default(),
        });
    }
    out
}

pub fn load_umu_database(installed_protons: &[ProtonBuild]) -> (Vec<UmuDbEntry>, Option<PathBuf>) {
    let cached = cached_umu_db_path();
    if cached.is_file()
        && let Ok(text) = fs::read_to_string(&cached)
    {
        let parsed = parse_umu_csv(&text);
        if !parsed.is_empty() {
            return (parsed, Some(cached));
        }
    }

    for build in installed_protons {
        for base in [&build.canonical_path, &build.path] {
            let bundled = base.join("protonfixes/umu-database.csv");
            if bundled.is_file()
                && let Ok(text) = fs::read_to_string(&bundled)
            {
                let parsed = parse_umu_csv(&text);
                if !parsed.is_empty() {
                    return (parsed, Some(bundled));
                }
            }
        }
    }

    (Vec::new(), None)
}

pub fn refresh_umu_database_online() -> Result<(Vec<UmuDbEntry>, PathBuf)> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "8",
            "--max-time",
            "20",
            "-H",
            "User-Agent: UmuBox/0.1",
            UMU_DATABASE_URL,
        ])
        .output()
        .context("failed to spawn curl to refresh umu-database.csv")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("Failed downloading umu-database.csv: {}", stderr.trim());
    }

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !text.starts_with("TITLE,") {
        bail!("Unexpected payload from umu-database (missing TITLE header)");
    }
    let parsed = parse_umu_csv(&text);
    if parsed.len() < 50 {
        bail!("Downloaded umu-database.csv looks incomplete ({} rows)", parsed.len());
    }

    let dest = cached_umu_db_path();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("csv.tmp");
    fs::write(&tmp, &text)?;
    if fs::rename(&tmp, &dest).is_err() {
        let _ = fs::remove_file(&tmp);
        fs::write(&dest, &text)?;
    }

    Ok((parsed, dest))
}

pub fn search_umu_database<'a>(
    entries: &'a [UmuDbEntry],
    query: &str,
    limit: usize,
) -> Vec<&'a UmuDbEntry> {
    let trimmed = query.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return entries.iter().take(limit).collect();
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mut scored: Vec<(u8, &'a UmuDbEntry)> = entries
        .iter()
        .filter_map(|e| {
            let t_low = e.title.to_ascii_lowercase();
            let id_low = e.umu_id.to_ascii_lowercase();
            let acro_low = e.acronym.to_ascii_lowercase();
            let code_low = e.codename.to_ascii_lowercase();
            let exe_low = e.exe_strings.to_ascii_lowercase();
            let note_low = e.note.to_ascii_lowercase();
            let store_low = e.store.to_ascii_lowercase();

            let all_match = tokens.iter().all(|tok| {
                t_low.contains(tok)
                    || id_low.contains(tok)
                    || acro_low.contains(tok)
                    || code_low.contains(tok)
                    || exe_low.contains(tok)
                    || note_low.contains(tok)
                    || store_low.contains(tok)
            });
            if !all_match {
                return None;
            }

            let rank = if id_low == trimmed || acro_low == trimmed || t_low == trimmed {
                0
            } else if t_low.starts_with(&trimmed) || id_low.starts_with(&trimmed) {
                1
            } else if t_low.contains(&trimmed) {
                2
            } else if exe_low.contains(&trimmed) {
                3
            } else {
                4
            };
            Some((rank, e))
        })
        .collect();

    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.title.cmp(&b.1.title)));
    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

pub fn find_protonfix_for_game_id(
    game_id: &str,
    installed_protons: &[ProtonBuild],
) -> Option<ProtonFixInfo> {
    let norm = game_id.trim();
    if norm.is_empty() || norm == "0" {
        return None;
    }

    let mut filenames = vec![format!("{norm}.py")];
    if let Some(rest) = norm.strip_prefix("umu-")
        && !rest.is_empty()
    {
        filenames.push(format!("{rest}.py"));
    }

    let priority_buckets = [
        "gamefixes-umu",
        "gamefixes-steam",
        "gamefixes-battlenet",
        "gamefixes-gog",
        "gamefixes-egs",
        "gamefixes-ea",
        "gamefixes-ubisoft",
        "gamefixes-amazon",
        "gamefixes-humble",
        "gamefixes-itchio",
        "gamefixes-zoomplatform",
    ];

    for build in installed_protons {
        for base in [&build.canonical_path, &build.path] {
            let pf_root = base.join("protonfixes");
            if !pf_root.is_dir() {
                continue;
            }

            for bucket in priority_buckets {
                let bucket_dir = pf_root.join(bucket);
                if !bucket_dir.is_dir() {
                    continue;
                }
                for fname in &filenames {
                    let cand = bucket_dir.join(fname);
                    if cand.is_file()
                        && let Ok(content) = fs::read_to_string(&cand)
                    {
                        return Some(parse_protonfix_script(cand, bucket.to_string(), &content));
                    }
                }
            }

            if let Ok(entries) = fs::read_dir(&pf_root) {
                for entry in entries.flatten() {
                    let b_name = entry.file_name().to_string_lossy().to_string();
                    if !b_name.starts_with("gamefixes-")
                        || priority_buckets.contains(&b_name.as_str())
                        || !entry.path().is_dir()
                    {
                        continue;
                    }
                    for fname in &filenames {
                        let cand = entry.path().join(fname);
                        if cand.is_file()
                            && let Ok(content) = fs::read_to_string(&cand)
                        {
                            return Some(parse_protonfix_script(cand, b_name, &content));
                        }
                    }
                }
            }
        }
    }

    None
}

pub fn parse_protonfix_script(
    script_path: PathBuf,
    store_bucket: String,
    content: &str,
) -> ProtonFixInfo {
    let summary = extract_python_docstring_summary(content);
    let mut verbs = Vec::new();
    let mut dlls = Vec::new();
    let mut seen_verbs = HashSet::new();
    let mut seen_dlls = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("util.protontricks(") || trimmed.contains("util.protontricks_proton_5(") {
            for arg in extract_quoted_args(trimmed) {
                if arg.contains('=') {
                    if seen_dlls.insert(arg.clone()) {
                        dlls.push(arg);
                    }
                } else {
                    for tok in arg.split_whitespace() {
                        if seen_verbs.insert(tok.to_string()) {
                            verbs.push(tok.to_string());
                        }
                    }
                }
            }
        } else if trimmed.contains("util.winedll_override(") {
            let args = extract_quoted_args(trimmed);
            if args.len() >= 2 {
                let spec = format!("{}={}", args[0], args[1]);
                if seen_dlls.insert(spec.clone()) {
                    dlls.push(spec);
                }
            } else if let Some(first) = args.into_iter().next()
                && seen_dlls.insert(first.clone())
            {
                dlls.push(first);
            }
        }
    }

    ProtonFixInfo {
        script_path,
        store_bucket,
        summary,
        detected_verbs: verbs,
        detected_dll_overrides: dlls,
    }
}

fn extract_python_docstring_summary(content: &str) -> String {
    let delim = if content.contains("\"\"\"") {
        "\"\"\""
    } else if content.contains("'''") {
        "'''"
    } else {
        return String::new();
    };
    let Some(start) = content.find(delim) else {
        return String::new();
    };
    let after = &content[start + 3..];
    let Some(end) = after.find(delim) else {
        return String::new();
    };
    let doc = &after[..end];
    doc.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" — ")
}

fn extract_quoted_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\'' || chars[i] == '"' {
            let q = chars[i];
            i += 1;
            let mut val = String::new();
            while i < chars.len() && chars[i] != q {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                val.push(chars[i]);
                i += 1;
            }
            if !val.trim().is_empty() {
                out.push(val.trim().to_string());
            }
        }
        i += 1;
    }
    out
}

pub fn extract_steam_appid(game_id: &str) -> Option<u64> {
    let trimmed = game_id.trim();
    let num_part = trimmed.strip_prefix("umu-").unwrap_or(trimmed);
    if num_part.is_empty() || !num_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id = num_part.parse::<u64>().ok()?;
    (id > 0).then_some(id)
}

pub fn protondb_url(app_id: u64) -> String {
    format!("https://www.protondb.com/app/{app_id}")
}

pub fn protondb_cache_path(app_id: u64) -> PathBuf {
    cache_dir().join("protondb").join(format!("{app_id}.json"))
}

pub fn load_cached_protondb(app_id: u64) -> Option<ProtonDbSummary> {
    let path = protondb_cache_path(app_id);
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > PROTONDB_CACHE_MAX_AGE {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    let mut summary: ProtonDbSummary = serde_json::from_str(&raw).ok()?;
    summary.app_id = app_id;
    (!summary.tier.is_empty()).then_some(summary)
}

pub fn fetch_protondb_summary(app_id: u64) -> Result<ProtonDbSummary> {
    if let Some(cached) = load_cached_protondb(app_id) {
        return Ok(cached);
    }

    let url = format!("https://www.protondb.com/api/v1/reports/summaries/{app_id}.json");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "6",
            "--max-time",
            "12",
            "-H",
            "User-Agent: UmuBox/0.1",
            &url,
        ])
        .output()
        .context("failed to query ProtonDB summary API")?;

    if !out.status.success() {
        bail!(
            "ProtonDB returned HTTP error for AppID {app_id} (no reports or unavailable)"
        );
    }

    let mut summary: ProtonDbSummary =
        serde_json::from_slice(&out.stdout).context("parse ProtonDB summary JSON")?;
    summary.app_id = app_id;
    if summary.tier.is_empty() {
        return Err(anyhow!("ProtonDB summary missing tier for AppID {app_id}"));
    }

    let cache_file = protondb_cache_path(app_id);
    if let Some(parent) = cache_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&cache_file, &out.stdout);

    Ok(summary)
}

pub fn open_url_in_browser(url: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("open URL {url}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_umu_csv_with_quoted_commas() {
        let csv = "\
TITLE,STORE,CODENAME,UMU_ID,COMMON ACRONYM (Optional),NOTE (Optional),EXE_STRINGS (Optional)
\"The Witcher 3: Wild Hunt, Complete\",gog,1495134320,umu-292030,tw3,\"Classic, GOTY\",witcher3.exe
Borderlands 3,egs,Catnip,umu-397540,bl3,,
";
        let rows = parse_umu_csv(csv);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "The Witcher 3: Wild Hunt, Complete");
        assert_eq!(rows[0].umu_id, "umu-292030");
        assert_eq!(rows[0].note, "Classic, GOTY");
        assert_eq!(rows[0].exe_strings, "witcher3.exe");
        assert_eq!(rows[1].store, "egs");
    }

    #[test]
    fn searches_and_ranks_umu_entries() {
        let entries = vec![
            UmuDbEntry {
                title: "Borderlands 3".into(),
                store: "egs".into(),
                codename: "Catnip".into(),
                umu_id: "umu-397540".into(),
                acronym: "bl3".into(),
                note: "".into(),
                exe_strings: "".into(),
            },
            UmuDbEntry {
                title: "The Witcher 3: Wild Hunt".into(),
                store: "gog".into(),
                codename: "123".into(),
                umu_id: "umu-292030".into(),
                acronym: "tw3".into(),
                note: "".into(),
                exe_strings: "witcher3.exe".into(),
            },
        ];

        assert_eq!(search_umu_database(&entries, "tw3", 10)[0].umu_id, "umu-292030");
        assert_eq!(
            search_umu_database(&entries, "witcher 3", 10)[0].umu_id,
            "umu-292030"
        );
        assert_eq!(
            search_umu_database(&entries, "397540", 10)[0].title,
            "Borderlands 3"
        );
    }

    #[test]
    fn extracts_steam_appid_only_for_numeric_ids() {
        assert_eq!(extract_steam_appid("umu-292030"), Some(292030));
        assert_eq!(extract_steam_appid("220"), Some(220));
        assert_eq!(extract_steam_appid("umu-0"), None);
        assert_eq!(extract_steam_appid("0"), None);
        assert_eq!(extract_steam_appid("umu-battlenet"), None);
    }

    #[test]
    fn parses_protonfix_python_docstring_verbs_and_dlls() {
        let py = r#""""Game fix for Demo Game

Installs runtime and hides Wine exports.
"""
from protonfixes import util

def main() -> None:
    util.protontricks('vcrun2022')
    util.protontricks('hidewineexports=enable')
    util.winedll_override('dinput8', 'n,b')
"#;
        let info = parse_protonfix_script(
            PathBuf::from("/tmp/umu-12345.py"),
            "gamefixes-umu".to_string(),
            py,
        );
        assert!(info.summary.contains("Game fix for Demo Game"));
        assert_eq!(info.detected_verbs, vec!["vcrun2022"]);
        assert_eq!(
            info.detected_dll_overrides,
            vec!["hidewineexports=enable", "dinput8=n,b"]
        );
    }
}
