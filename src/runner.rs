use crate::config::{GameEntry, LaunchOptions, PrefixConfig, split_shell_tokens};
use crate::proton::{ProtonBuild, find_wineserver_in_proton};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub comm: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixTool {
    Winecfg,
    Regedit,
    Taskmgr,
    Cmd,
    WinebootUpdate,
    Winetricks,
    RunExe(PathBuf),
}

impl PrefixTool {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Winecfg => "winecfg",
            Self::Regedit => "regedit",
            Self::Taskmgr => "Task Manager",
            Self::Cmd => "cmd.exe",
            Self::WinebootUpdate => "Update Prefix (wineboot -u)",
            Self::Winetricks => "Winetricks",
            Self::RunExe(_) => "Run .exe / .msi",
        }
    }

    pub fn parse_cli(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "winecfg" => Some(Self::Winecfg),
            "regedit" => Some(Self::Regedit),
            "taskmgr" | "taskmanager" => Some(Self::Taskmgr),
            "cmd" | "cmd.exe" => Some(Self::Cmd),
            "wineboot" | "update" => Some(Self::WinebootUpdate),
            "winetricks" => Some(Self::Winetricks),
            _ => None,
        }
    }

    pub fn umu_args(&self, extra_args: &[String]) -> Vec<String> {
        match self {
            Self::Winecfg => vec!["winecfg".to_string()],
            Self::Regedit => vec!["regedit".to_string()],
            Self::Taskmgr => vec!["taskmgr".to_string()],
            Self::Cmd => vec!["cmd".to_string()],
            Self::WinebootUpdate => vec!["wineboot".to_string(), "-u".to_string()],
            Self::Winetricks => {
                let mut v = vec!["winetricks".to_string()];
                v.extend(extra_args.iter().cloned());
                v
            }
            Self::RunExe(path) => {
                let mut v = vec![path.display().to_string()];
                v.extend(extra_args.iter().cloned());
                v
            }
        }
    }
}

pub fn has_ntsync_device() -> bool {
    Path::new("/dev/ntsync").exists()
}

pub fn resolve_proton_path(
    prefix: &PrefixConfig,
    game: Option<&GameEntry>,
    default_proton: Option<&Path>,
) -> Option<PathBuf> {
    game.and_then(|g| g.proton_override.as_ref())
        .or(prefix.proton_path.as_ref())
        .map(PathBuf::from)
        .or_else(|| default_proton.map(Path::to_path_buf))
}

pub fn find_wineserver(proton: Option<&Path>, installed: &[ProtonBuild]) -> Option<PathBuf> {
    if let Some(p) = proton
        && let Some(ws) = find_wineserver_in_proton(p)
    {
        return Some(ws);
    }
    installed.iter().find_map(|b| b.wineserver.clone())
}

fn set_env_pair(env: &mut Vec<(String, String)>, key: impl Into<String>, value: impl Into<String>) {
    let k = key.into();
    let v = value.into();
    if let Some(pos) = env.iter().position(|(ek, _)| *ek == k) {
        env[pos].1 = v;
    } else {
        env.push((k, v));
    }
}

pub fn build_launch_env(
    prefix: &PrefixConfig,
    game: Option<&GameEntry>,
    options: &LaunchOptions,
    default_proton: Option<&Path>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    set_env_pair(&mut env, "WINEPREFIX", prefix.path.display().to_string());

    let gid = game
        .map(|g| g.game_id.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let d = prefix.default_game_id.trim();
            (!d.is_empty()).then_some(d)
        })
        .unwrap_or("0");
    set_env_pair(&mut env, "GAMEID", gid);

    if let Some(proton) = resolve_proton_path(prefix, game, default_proton) {
        set_env_pair(&mut env, "PROTONPATH", proton.display().to_string());
    }

    if options.mangohud {
        set_env_pair(&mut env, "MANGOHUD", "1");
    } else {
        set_env_pair(&mut env, "MANGOHUD", "0");
    }

    if options.nvidia_prime_offload {
        set_env_pair(&mut env, "__NV_PRIME_RENDER_OFFLOAD", "1");
        set_env_pair(&mut env, "__GLX_VENDOR_LIBRARY_NAME", "nvidia");
        set_env_pair(&mut env, "__VK_LAYER_NV_optimus", "NVIDIA_only");
    }

    if options.wayland_driver {
        set_env_pair(&mut env, "PROTON_ENABLE_WAYLAND", "1");
    }

    if options.ntsync {
        set_env_pair(&mut env, "PROTON_USE_NTSYNC", "1");
    } else {
        set_env_pair(&mut env, "PROTON_USE_NTSYNC", "0");
        set_env_pair(&mut env, "PROTON_NO_NTSYNC", "1");
    }

    set_env_pair(
        &mut env,
        "PROTON_ENABLE_NVAPI",
        if options.nvapi { "1" } else { "0" },
    );

    if options.dxvk_hud {
        set_env_pair(&mut env, "DXVK_HUD", "fps,frametimes,gpuload");
    }

    if options.shader_cache {
        let bytes = (options.shader_cache_gb.max(1) as u64) * 1024 * 1024 * 1024;
        set_env_pair(&mut env, "__GL_SHADER_DISK_CACHE", "1");
        set_env_pair(
            &mut env,
            "__GL_SHADER_DISK_CACHE_PATH",
            prefix.path.display().to_string(),
        );
        set_env_pair(&mut env, "__GL_SHADER_DISK_CACHE_SIZE", bytes.to_string());
        set_env_pair(&mut env, "__GL_SHADER_DISK_CACHE_SKIP_CLEANUP", "1");
    }

    if !options.dll_overrides.trim().is_empty() {
        set_env_pair(&mut env, "WINEDLLOVERRIDES", options.dll_overrides.trim());
    }

    for extra in &options.extra_env {
        let k = extra.key.trim();
        if !k.is_empty() {
            set_env_pair(&mut env, k, extra.value.trim());
        }
    }

    env
}

pub fn extract_wineprefix_from_environ(environ_bytes: &[u8]) -> Option<String> {
    let mut compat_fallback: Option<String> = None;
    for chunk in environ_bytes.split(|&b| b == 0) {
        if let Some(val) = chunk.strip_prefix(b"WINEPREFIX=") {
            let s = String::from_utf8_lossy(val).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        } else if compat_fallback.is_none()
            && let Some(val) = chunk.strip_prefix(b"STEAM_COMPAT_DATA_PATH=")
        {
            let s = String::from_utf8_lossy(val).trim().to_string();
            if !s.is_empty() {
                compat_fallback = Some(s);
            }
        }
    }
    compat_fallback
}

fn normalize_prefix_key(p: &Path) -> PathBuf {
    let stripped = if p.file_name().is_some_and(|n| n == "pfx") {
        p.parent().unwrap_or(p)
    } else {
        p
    };
    fs::canonicalize(stripped).unwrap_or_else(|_| stripped.to_path_buf())
}

pub fn detect_running_prefixes(prefixes: &[PrefixConfig]) -> HashMap<String, Vec<RunningProcess>> {
    let mut by_path: HashMap<PathBuf, &str> = HashMap::new();
    let mut map: HashMap<String, Vec<RunningProcess>> = HashMap::new();

    for p in prefixes {
        by_path.insert(normalize_prefix_key(&p.path), p.id.as_str());
    }
    if by_path.is_empty() {
        return map;
    }

    let Ok(self_meta) = fs::metadata("/proc/self") else {
        return map;
    };
    let my_uid = self_meta.uid();

    let Ok(proc_entries) = fs::read_dir("/proc") else {
        return map;
    };
    for entry in proc_entries.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let proc_dir = entry.path();
        let Ok(meta) = fs::metadata(&proc_dir) else {
            continue;
        };
        if meta.uid() != my_uid {
            continue;
        }
        let Ok(env_bytes) = fs::read(proc_dir.join("environ")) else {
            continue;
        };
        let Some(raw_pfx) = extract_wineprefix_from_environ(&env_bytes) else {
            continue;
        };
        let norm = normalize_prefix_key(Path::new(&raw_pfx));
        let Some(&pfx_id) = by_path.get(&norm) else {
            continue;
        };
        let comm = fs::read_to_string(proc_dir.join("comm"))
            .unwrap_or_else(|_| "wine".to_string())
            .trim()
            .to_string();
        map.entry(pfx_id.to_string())
            .or_default()
            .push(RunningProcess { pid, comm });
    }

    for list in map.values_mut() {
        list.sort_by_key(|p| p.pid);
    }
    map
}

pub fn prepare_launch_command(
    umu_args: &[String],
    working_dir: Option<&Path>,
    env_pairs: &[(String, String)],
) -> Command {
    let mut cmd = Command::new("umu-run");
    cmd.args(umu_args);
    if let Some(dir) = working_dir
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }
    for (k, v) in env_pairs {
        cmd.env(k, v);
    }
    cmd
}

pub fn run_umu_cli(
    umu_args: &[String],
    working_dir: Option<&Path>,
    env_pairs: &[(String, String)],
) -> Result<i32> {
    let mut cmd = prepare_launch_command(umu_args, working_dir, env_pairs);
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to execute umu-run")?;
    Ok(status.code().unwrap_or(1))
}

pub fn spawn_umu_captured(
    label: String,
    umu_args: Vec<String>,
    working_dir: Option<PathBuf>,
    env_pairs: Vec<(String, String)>,
    tx: Sender<String>,
) -> Result<()> {
    let _ = fs::create_dir_all(
        env_pairs
            .iter()
            .find(|(k, _)| k == "WINEPREFIX")
            .map(|(_, v)| Path::new(v))
            .unwrap_or_else(|| Path::new(".")),
    );
    let mut cmd = prepare_launch_command(&umu_args, working_dir.as_deref(), &env_pairs);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let _ = tx.send(format!("[UmuBox] Launching {label}: umu-run {}", umu_args.join(" ")));
    let mut child = cmd.spawn().context("failed to spawn umu-run")?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tx_out = tx.clone();
    let h_out = stdout.map(|out| {
        thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        })
    });

    let tx_err = tx.clone();
    let h_err = stderr.map(|err| {
        thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx_err.send(line);
            }
        })
    });

    thread::spawn(move || {
        let status = child.wait();
        if let Some(h) = h_out {
            let _ = h.join();
        }
        if let Some(h) = h_err {
            let _ = h.join();
        }
        match status {
            Ok(s) => {
                let _ = tx.send(format!("[UmuBox] {label} finished ({s})."));
            }
            Err(err) => {
                let _ = tx.send(format!("[UmuBox] ERROR waiting on {label}: {err}"));
            }
        }
    });

    Ok(())
}

pub fn game_umu_args(game: &GameEntry) -> Vec<String> {
    let mut args = vec![game.exe_path.display().to_string()];
    if !game.args.trim().is_empty() {
        args.extend(split_shell_tokens(&game.args));
    }
    args
}

pub fn game_working_dir(game: &GameEntry) -> Option<PathBuf> {
    game.working_dir
        .clone()
        .or_else(|| game.exe_path.parent().map(Path::to_path_buf))
}

pub fn kill_prefix(prefix: &PrefixConfig, installed: &[ProtonBuild]) -> Result<String> {
    let wineserver = find_wineserver(prefix.proton_path.as_deref(), installed)
        .ok_or_else(|| anyhow::anyhow!("No Proton wineserver binary found on system"))?;
    let out = Command::new(&wineserver)
        .arg("-k")
        .env("WINEPREFIX", &prefix.path)
        .output()
        .with_context(|| format!("run {} -k", wineserver.display()))?;
    if out.status.success() {
        Ok(format!("Sent wineserver -k to {}", prefix.name))
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("wineserver -k exited {}: {}", out.status, err.trim());
    }
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter_map(|e| {
            if e.file_type().is_file() {
                e.metadata().ok().map(|m| m.len())
            } else {
                None
            }
        })
        .sum()
}

pub fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.0} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn clear_shader_cache(prefix_path: &Path) -> Result<u64> {
    let mut freed = 0u64;
    for sub in ["GLCache", "shadercache"] {
        let p = prefix_path.join(sub);
        if p.is_dir() {
            freed += dir_size_bytes(&p);
            fs::remove_dir_all(&p)
                .with_context(|| format!("remove shader cache {}", p.display()))?;
            let _ = fs::create_dir_all(&p);
        }
    }
    Ok(freed)
}

pub fn desktop_applications_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("applications")
}

pub fn managed_desktop_entry_path(prefix_id: &str, game_id: &str) -> PathBuf {
    desktop_applications_dir().join(format!("umubox-{prefix_id}-{game_id}.desktop"))
}

pub fn is_desktop_entry_installed(prefix_id: &str, game_id: &str) -> bool {
    managed_desktop_entry_path(prefix_id, game_id).is_file()
}

pub fn refresh_desktop_caches() {
    let apps = desktop_applications_dir();
    let _ = Command::new("update-desktop-database").arg(&apps).output();
    let _ = Command::new("kbuildsycoca6").output();
}

pub fn render_desktop_entry(
    prefix: &PrefixConfig,
    game: &GameEntry,
    umubox_bin: &str,
    icon_value: &str,
) -> String {
    let work_dir = game_working_dir(game)
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let comment = format!("{} (UmuBox: {})", game.name, prefix.name);
    let target_spec = format!("{}/{}", prefix.id, game.id);
    let icon = if icon_value.trim().is_empty() {
        "umubox"
    } else {
        icon_value.trim()
    };
    let wm_class_line = game
        .exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .map(|wm| format!("StartupWMClass={wm}\n"))
        .unwrap_or_default();

    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={comment}\n\
         Exec={bin} launch \"{target}\"\n\
         Path={path}\n\
         Icon={icon}\n\
         Categories=Game;\n\
         Terminal=false\n\
         StartupNotify=true\n\
         {wm_class}\
         Actions=NoMangoHud;KillPrefix;\n\
         \n\
         [Desktop Action NoMangoHud]\n\
         Name=Launch without MangoHud Overlay\n\
         Exec={bin} launch --no-mangohud \"{target}\"\n\
         Icon={icon}\n\
         \n\
         [Desktop Action KillPrefix]\n\
         Name=Force Stop Prefix ({pfx_name})\n\
         Exec={bin} kill \"{pfx_id}\"\n\
         Icon=process-stop\n",
        name = game.name,
        comment = comment,
        bin = umubox_bin,
        target = target_spec,
        path = work_dir,
        icon = icon,
        wm_class = wm_class_line,
        pfx_name = prefix.name,
        pfx_id = prefix.id,
    )
}

pub fn sync_managed_desktop_entry(
    prefix: &PrefixConfig,
    game: &GameEntry,
    icon_path: Option<&Path>,
) -> Result<PathBuf> {
    let apps = desktop_applications_dir();
    fs::create_dir_all(&apps)
        .with_context(|| format!("create desktop entries dir {}", apps.display()))?;
    let local_bin = dirs::home_dir()
        .map(|h| h.join(".local/bin/umubox"))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "umubox".to_string());

    let icon_val = icon_path
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "umubox".to_string());

    let dest = managed_desktop_entry_path(&prefix.id, &game.id);
    let body = render_desktop_entry(prefix, game, &local_bin, &icon_val);
    fs::write(&dest, body).with_context(|| format!("write desktop entry {}", dest.display()))?;
    refresh_desktop_caches();
    Ok(dest)
}

pub fn remove_managed_desktop_entry(prefix_id: &str, game_id: &str) -> Result<bool> {
    let dest = managed_desktop_entry_path(prefix_id, game_id);
    if dest.exists() {
        fs::remove_file(&dest)
            .with_context(|| format!("remove desktop entry {}", dest.display()))?;
        refresh_desktop_caches();
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn open_in_file_manager(dir: &Path) -> Result<()> {
    Command::new("xdg-open")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("open {} in file manager", dir.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_umu_env_with_all_runtime_toggles() {
        let prefix = PrefixConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            path: PathBuf::from("/tmp/pfx"),
            default_game_id: "umu-0".to_string(),
            proton_path: Some(PathBuf::from("/opt/proton-prefix")),
            default_options: LaunchOptions::default(),
            games: Vec::new(),
        };
        let mut opts = LaunchOptions {
            mangohud: true,
            nvidia_prime_offload: true,
            wayland_driver: true,
            ntsync: true,
            nvapi: true,
            dxvk_hud: true,
            shader_cache: true,
            shader_cache_gb: 8,
            dll_overrides: "locationapi=d".to_string(),
            extra_env: vec![crate::config::EnvVar {
                key: "CUSTOM_FLAG".to_string(),
                value: "yes".to_string(),
            }],
        };
        let game = GameEntry {
            id: "app".to_string(),
            name: "App".to_string(),
            exe_path: PathBuf::from("/tmp/pfx/drive_c/app.exe"),
            working_dir: None,
            args: "--fast".to_string(),
            game_id: "umu-custom".to_string(),
            proton_override: Some(PathBuf::from("/opt/proton-game")),
            icon_override: None,
            desktop_managed: false,
            options: opts.clone(),
        };

        let env = build_launch_env(&prefix, Some(&game), &opts, None);
        let find = |k: &str| env.iter().find(|(ek, _)| ek == k).map(|(_, v)| v.as_str());

        assert_eq!(find("WINEPREFIX"), Some("/tmp/pfx"));
        assert_eq!(find("GAMEID"), Some("umu-custom"));
        assert_eq!(find("PROTONPATH"), Some("/opt/proton-game"));
        assert_eq!(find("MANGOHUD"), Some("1"));
        assert_eq!(find("__NV_PRIME_RENDER_OFFLOAD"), Some("1"));
        assert_eq!(find("PROTON_ENABLE_WAYLAND"), Some("1"));
        assert_eq!(find("PROTON_USE_NTSYNC"), Some("1"));
        assert_eq!(find("WINEDLLOVERRIDES"), Some("locationapi=d"));
        let sc_size = (8u64 * 1024 * 1024 * 1024).to_string();
        assert_eq!(find("__GL_SHADER_DISK_CACHE_SIZE"), Some(sc_size.as_str()));
        assert_eq!(find("CUSTOM_FLAG"), Some("yes"));

        opts.ntsync = false;
        opts.mangohud = false;
        let env2 = build_launch_env(&prefix, None, &opts, None);
        let find2 = |k: &str| env2.iter().find(|(ek, _)| ek == k).map(|(_, v)| v.as_str());
        assert_eq!(find2("PROTON_NO_NTSYNC"), Some("1"));
        assert_eq!(find2("MANGOHUD"), Some("0"));
        assert_eq!(find2("GAMEID"), Some("umu-0"));
    }

    #[test]
    fn extracts_wineprefix_and_compat_path_from_proc_environ() {
        let raw = b"HOME=/home/user\0WINEPREFIX=/home/user/Games/battlenet\0TERM=xterm\0";
        assert_eq!(
            extract_wineprefix_from_environ(raw).as_deref(),
            Some("/home/user/Games/battlenet")
        );
        let steam = b"STEAM_COMPAT_DATA_PATH=/home/user/Games/pfx\0";
        assert_eq!(
            extract_wineprefix_from_environ(steam).as_deref(),
            Some("/home/user/Games/pfx")
        );
    }

    #[test]
    fn renders_clean_kde_desktop_entry_with_actions() {
        let prefix = PrefixConfig {
            id: "battlenet".to_string(),
            name: "battlenet".to_string(),
            path: PathBuf::from("/home/user/Games/battlenet"),
            default_game_id: "umu-battlenet".to_string(),
            proton_path: None,
            default_options: LaunchOptions::default(),
            games: Vec::new(),
        };
        let game = GameEntry {
            id: "battle-net".to_string(),
            name: "Battle.net".to_string(),
            exe_path: PathBuf::from("/home/user/Games/battlenet/drive_c/Bnet/Launcher.exe"),
            working_dir: None,
            args: String::new(),
            game_id: "umu-battlenet".to_string(),
            proton_override: None,
            icon_override: None,
            desktop_managed: true,
            options: LaunchOptions::default(),
        };
        let rendered = render_desktop_entry(
            &prefix,
            &game,
            "/home/user/.local/bin/umubox",
            "/home/user/.local/share/umubox/icons/battlenet-battle-net.png",
        );
        assert!(rendered.contains("Exec=/home/user/.local/bin/umubox launch \"battlenet/battle-net\""));
        assert!(rendered.contains("Icon=/home/user/.local/share/umubox/icons/battlenet-battle-net.png"));
        assert!(rendered.contains("StartupWMClass=launcher.exe"));
        assert!(rendered.contains("[Desktop Action KillPrefix]"));
        assert!(rendered.contains("Exec=/home/user/.local/bin/umubox kill \"battlenet\""));
    }
}
