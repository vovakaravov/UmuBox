use crate::proton::{ProtonBuild, ProtonSource};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchOptions {
    pub mangohud: bool,
    pub nvidia_prime_offload: bool,
    pub wayland_driver: bool,
    pub ntsync: bool,
    pub nvapi: bool,
    pub dxvk_hud: bool,
    pub shader_cache: bool,
    pub shader_cache_gb: u32,
    pub dll_overrides: String,
    pub extra_env: Vec<EnvVar>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            mangohud: true,
            nvidia_prime_offload: false,
            wayland_driver: false,
            ntsync: true,
            nvapi: true,
            dxvk_hud: false,
            shader_cache: true,
            shader_cache_gb: 10,
            dll_overrides: String::new(),
            extra_env: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameEntry {
    pub id: String,
    pub name: String,
    pub exe_path: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub args: String,
    pub game_id: String,
    pub proton_override: Option<PathBuf>,
    pub icon_override: Option<PathBuf>,
    pub desktop_managed: bool,
    pub options: LaunchOptions,
}

impl Default for GameEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            exe_path: PathBuf::new(),
            working_dir: None,
            args: String::new(),
            game_id: String::new(),
            proton_override: None,
            icon_override: None,
            desktop_managed: false,
            options: LaunchOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrefixConfig {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub default_game_id: String,
    pub proton_path: Option<PathBuf>,
    pub default_options: LaunchOptions,
    pub games: Vec<GameEntry>,
}

impl Default for PrefixConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default".to_string(),
            path: default_prefixes_root().join("umu-default"),
            default_game_id: "0".to_string(),
            proton_path: None,
            default_options: LaunchOptions::default(),
            games: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub default_proton: Option<PathBuf>,
    pub default_prefix_id: Option<String>,
    pub prefixes_root: PathBuf,
    pub prefixes: Vec<PrefixConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_proton: None,
            default_prefix_id: None,
            prefixes_root: default_prefixes_root(),
            prefixes: Vec::new(),
        }
    }
}

pub fn default_prefixes_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Games/umu")
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("umubox")
        .join("config.json")
}

pub fn is_wine_prefix_dir(dir: &Path) -> bool {
    dir.join("drive_c").is_dir() || dir.join("pfx/drive_c").is_dir()
}

pub fn slugify_id(raw: &str) -> String {
    let mut slug = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug
    }
}

pub fn make_unique_id<'a>(raw: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let used: HashSet<&str> = existing.into_iter().collect();
    let base = slugify_id(raw);
    if !used.contains(base.as_str()) {
        return base;
    }
    for n in 2usize.. {
        let cand = format!("{base}-{n}");
        if !used.contains(cand.as_str()) {
            return cand;
        }
    }
    base
}

pub fn parse_extra_env(raw: &str) -> Vec<EnvVar> {
    let mut out = Vec::new();
    for tok in raw.split(['\n', ',']).flat_map(str::split_whitespace) {
        let trimmed = tok.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            if !key.is_empty() {
                out.push(EnvVar {
                    key: key.to_string(),
                    value: v.trim().to_string(),
                });
            }
        }
    }
    out
}

pub fn format_extra_env(vars: &[EnvVar]) -> String {
    vars.iter()
        .map(|ev| format!("{}={}", ev.key, ev.value))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDesktopGame {
    pub name: String,
    pub wineprefix: PathBuf,
    pub exe_path: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub args: String,
    pub game_id: String,
    pub proton_path: Option<PathBuf>,
    pub icon_override: Option<PathBuf>,
    pub options: LaunchOptions,
}

pub fn split_shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut has_token = false;

    for ch in input.chars() {
        if escape {
            cur.push(ch);
            escape = false;
            has_token = true;
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                cur.push(ch);
            }
            has_token = true;
            continue;
        }
        if in_double {
            match ch {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => cur.push(ch),
            }
            has_token = true;
            continue;
        }
        match ch {
            '\'' => {
                in_single = true;
                has_token = true;
            }
            '"' => {
                in_double = true;
                has_token = true;
            }
            '\\' => {
                escape = true;
                has_token = true;
            }
            c if c.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(cur);
    }
    tokens
}

pub fn parse_umu_desktop_entry(contents: &str) -> Option<ImportedDesktopGame> {
    let mut in_main = false;
    let mut name: Option<String> = None;
    let mut exec_line: Option<String> = None;
    let mut path_line: Option<String> = None;
    let mut icon_line: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some(val) = line.strip_prefix("Name=") {
            if name.is_none() {
                name = Some(val.trim().to_string());
            }
        } else if let Some(val) = line.strip_prefix("Exec=") {
            exec_line = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Path=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                path_line = Some(trimmed.to_string());
            }
        } else if let Some(val) = line.strip_prefix("Icon=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                icon_line = Some(trimmed.to_string());
            }
        }
    }

    let exec_str = exec_line?;
    let tokens = split_shell_tokens(&exec_str);
    let umu_pos = tokens.iter().position(|t| {
        Path::new(t)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|f| f == "umu-run")
    })?;

    let mut idx = 0usize;
    if tokens.first().is_some_and(|t| t == "env") {
        idx = 1;
    }

    let mut wineprefix: Option<PathBuf> = None;
    let mut proton_path: Option<PathBuf> = None;
    let mut game_id = String::new();
    let mut options = LaunchOptions {
        mangohud: false,
        ..LaunchOptions::default()
    };

    for tok in &tokens[idx..umu_pos] {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "WINEPREFIX" => wineprefix = Some(PathBuf::from(v)),
            "GAMEID" => game_id = v.to_string(),
            "PROTONPATH" => proton_path = Some(PathBuf::from(v)),
            "MANGOHUD" => options.mangohud = v == "1" || v.eq_ignore_ascii_case("true"),
            "WINEDLLOVERRIDES" => options.dll_overrides = v.to_string(),
            "PROTON_ENABLE_WAYLAND" => options.wayland_driver = v == "1",
            "PROTON_ENABLE_NVAPI" => options.nvapi = v == "1",
            "__NV_PRIME_RENDER_OFFLOAD" => options.nvidia_prime_offload = v == "1",
            "DXVK_HUD" => options.dxvk_hud = !v.is_empty() && v != "0",
            "__GL_SHADER_DISK_CACHE" => options.shader_cache = v == "1",
            "__GL_SHADER_DISK_CACHE_SIZE" => {
                if let Ok(bytes) = v.parse::<u64>() {
                    let gb = (bytes / (1024 * 1024 * 1024)).max(1) as u32;
                    options.shader_cache_gb = gb;
                }
            }
            "__GL_SHADER_DISK_CACHE_PATH" | "__GL_SHADER_DISK_CACHE_SKIP_CLEANUP" => {}
            _ => {
                options.extra_env.push(EnvVar {
                    key: k.to_string(),
                    value: v.to_string(),
                });
            }
        }
    }

    let exe_tok = tokens.get(umu_pos + 1)?.clone();
    if exe_tok.starts_with('%') {
        return None;
    }
    let exe_path = PathBuf::from(&exe_tok);
    let args = tokens
        .iter()
        .skip(umu_pos + 2)
        .filter(|a| !a.starts_with('%'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let game_name = name.unwrap_or_else(|| {
        exe_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Windows App")
            .to_string()
    });
    let wineprefix = wineprefix.unwrap_or_else(|| default_prefixes_root().join("umu-default"));

    let icon_override = icon_line
        .map(PathBuf::from)
        .filter(|p| p.is_file());

    Some(ImportedDesktopGame {
        name: game_name,
        wineprefix,
        exe_path,
        working_dir: path_line.map(PathBuf::from),
        args,
        game_id,
        proton_path,
        icon_override,
        options,
    })
}

impl AppConfig {
    pub fn load_or_discover(installed_protons: &[ProtonBuild]) -> Self {
        let path = config_path();
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&raw)
        {
            if cfg.default_proton.is_none() {
                cfg.default_proton = pick_best_default_proton(installed_protons);
            }
            return cfg;
        }

        let cfg = Self::discover_initial(installed_protons);
        let _ = cfg.save();
        cfg
    }

    pub fn discover_initial(installed_protons: &[ProtonBuild]) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let games_dir = home.join("Games");
        let umu_dir = games_dir.join("umu");

        let apps_dir = dirs::data_local_dir()
            .unwrap_or_else(|| home.join(".local/share"))
            .join("applications");

        Self::discover_initial_in(&games_dir, &umu_dir, &apps_dir, installed_protons)
    }

    pub fn discover_initial_in(
        games_dir: &Path,
        umu_dir: &Path,
        apps_dir: &Path,
        installed_protons: &[ProtonBuild],
    ) -> Self {
        let mut seen_paths: HashSet<PathBuf> = HashSet::new();
        let mut prefixes: Vec<PrefixConfig> = Vec::new();
        let best_proton = pick_best_default_proton(installed_protons);

        let mut scan_roots = Vec::new();
        for root in [umu_dir, games_dir] {
            if let Ok(entries) = fs::read_dir(root) {
                let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
                paths.sort();
                for p in paths {
                    if p.is_dir() && is_wine_prefix_dir(&p) {
                        scan_roots.push(p);
                    }
                }
            }
        }

        for p in scan_roots {
            let canon = fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if !seen_paths.insert(canon.clone()) {
                continue;
            }
            let dir_name = canon
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Prefix")
                .to_string();
            let id = make_unique_id(
                &dir_name,
                prefixes.iter().map(|px| px.id.as_str()),
            );
            prefixes.push(PrefixConfig {
                id,
                name: dir_name,
                path: canon,
                default_game_id: "0".to_string(),
                proton_path: None,
                default_options: LaunchOptions::default(),
                games: Vec::new(),
            });
        }

        // Import read-only existing user `.desktop` entries running `umu-run`
        if let Ok(entries) = fs::read_dir(apps_dir) {
            let mut desktop_files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            desktop_files.sort();
            for dp in desktop_files {
                let fname = dp.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !fname.ends_with(".desktop") || fname.starts_with("umubox-") {
                    continue;
                }
                let Ok(content) = fs::read_to_string(&dp) else {
                    continue;
                };
                let Some(imported) = parse_umu_desktop_entry(&content) else {
                    continue;
                };
                let pfx_canon = fs::canonicalize(&imported.wineprefix)
                    .unwrap_or_else(|_| imported.wineprefix.clone());
                let pfx_idx = if let Some(idx) = prefixes.iter().position(|px| {
                    fs::canonicalize(&px.path).unwrap_or_else(|_| px.path.clone()) == pfx_canon
                }) {
                    idx
                } else {
                    let dir_name = pfx_canon
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Prefix")
                        .to_string();
                    let id = make_unique_id(&dir_name, prefixes.iter().map(|px| px.id.as_str()));
                    prefixes.push(PrefixConfig {
                        id,
                        name: dir_name,
                        path: pfx_canon,
                        default_game_id: "0".to_string(),
                        proton_path: None,
                        default_options: LaunchOptions::default(),
                        games: Vec::new(),
                    });
                    prefixes.len() - 1
                };

                let pfx = &mut prefixes[pfx_idx];
                if !imported.game_id.is_empty() && pfx.default_game_id == "0" {
                    pfx.default_game_id = imported.game_id.clone();
                }
                if pfx.proton_path.is_none() && imported.proton_path.is_some() {
                    pfx.proton_path = imported.proton_path.clone();
                }
                if !pfx.games.iter().any(|g| g.exe_path == imported.exe_path) {
                    let gid = make_unique_id(
                        &imported.name,
                        pfx.games.iter().map(|g| g.id.as_str()),
                    );
                    pfx.games.push(GameEntry {
                        id: gid,
                        name: imported.name,
                        exe_path: imported.exe_path,
                        working_dir: imported.working_dir,
                        args: imported.args,
                        game_id: imported.game_id,
                        proton_override: imported.proton_path,
                        icon_override: imported.icon_override,
                        desktop_managed: false,
                        options: imported.options,
                    });
                }
            }
        }

        let default_prefix_id = prefixes
            .iter()
            .find(|p| p.id == "umu-default")
            .or_else(|| prefixes.first())
            .map(|p| p.id.clone());

        Self {
            default_proton: best_proton,
            default_prefix_id,
            prefixes_root: umu_dir.to_path_buf(),
            prefixes,
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {}", parent.display()))?;
        }
        let body = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.umubox-tmp");
        fs::write(&tmp, &body)
            .with_context(|| format!("write temp config {}", tmp.display()))?;
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
            fs::write(&path, &body)
                .with_context(|| format!("write config {}", path.display()))?;
        }
        Ok(())
    }
}

pub fn pick_best_default_proton(installed: &[ProtonBuild]) -> Option<PathBuf> {
    installed
        .iter()
        .find(|b| matches!(b.source, ProtonSource::User | ProtonSource::SystemAur))
        .or_else(|| installed.first())
        .map(|b| b.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_umu_desktop_shortcut_without_action_bleed() {
        let desktop = r#"[Desktop Entry]
Actions=NoMangoHud;KillWine;
Categories=Game;
Comment=Blizzard Battle.net Launcher (Proton-GE + UMU)
Exec=env MANGOHUD=1 WINEPREFIX=/home/user/Games/battlenet GAMEID=umu-battlenet PROTONPATH=/usr/share/steam/compatibilitytools.d/proton-ge-custom WINEDLLOVERRIDES=locationapi=d __GL_SHADER_DISK_CACHE=1 __GL_SHADER_DISK_CACHE_PATH=/home/user/Games/battlenet __GL_SHADER_DISK_CACHE_SIZE=10737418240 __GL_SHADER_DISK_CACHE_SKIP_CLEANUP=1 umu-run '/home/user/Games/battlenet/drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe'
Icon=battlenet
Name=Battle.net
Type=Application

[Desktop Action KillWine]
Exec=sh -c 'wineserver -k'
Name=Force Stop Battle.net
"#;
        let parsed = parse_umu_desktop_entry(desktop).unwrap();
        assert_eq!(parsed.name, "Battle.net");
        assert_eq!(parsed.wineprefix, PathBuf::from("/home/user/Games/battlenet"));
        assert_eq!(parsed.game_id, "umu-battlenet");
        assert_eq!(
            parsed.proton_path,
            Some(PathBuf::from("/usr/share/steam/compatibilitytools.d/proton-ge-custom"))
        );
        assert_eq!(
            parsed.exe_path,
            PathBuf::from("/home/user/Games/battlenet/drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe")
        );
        assert!(parsed.options.mangohud);
        assert_eq!(parsed.options.dll_overrides, "locationapi=d");
        assert!(parsed.options.shader_cache);
        assert_eq!(parsed.options.shader_cache_gb, 10);
    }

    #[test]
    fn discovers_prefixes_and_imports_desktop_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let games_dir = tmp.path().join("Games");
        let umu_dir = games_dir.join("umu");
        let apps_dir = tmp.path().join("applications");
        fs::create_dir_all(umu_dir.join("umu-default/drive_c")).unwrap();
        fs::create_dir_all(games_dir.join("battlenet/drive_c")).unwrap();
        fs::create_dir_all(&apps_dir).unwrap();

        let bnet = format!(
            "[Desktop Entry]\nName=Battle.net\nExec=env MANGOHUD=1 WINEPREFIX={} GAMEID=umu-battlenet WINEDLLOVERRIDES=locationapi=d umu-run '/tmp/Launcher.exe'\n",
            games_dir.join("battlenet").display()
        );
        fs::write(apps_dir.join("battlenet.desktop"), bnet).unwrap();

        let cfg = AppConfig::discover_initial_in(&games_dir, &umu_dir, &apps_dir, &[]);
        assert_eq!(cfg.prefixes.len(), 2);
        assert_eq!(cfg.default_prefix_id.as_deref(), Some("umu-default"));

        let bnet_pfx = cfg.prefixes.iter().find(|p| p.id == "battlenet").unwrap();
        assert_eq!(bnet_pfx.games.len(), 1);
        assert_eq!(bnet_pfx.games[0].name, "Battle.net");
        assert_eq!(bnet_pfx.default_game_id, "umu-battlenet");
    }
}
