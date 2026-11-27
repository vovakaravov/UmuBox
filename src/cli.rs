use crate::config::{AppConfig, LaunchOptions, PrefixConfig, make_unique_id};
use crate::deps::{
    PREFIX_PRESETS, analyze_exe_dependencies, find_preset, read_installed_winetricks_verbs,
};
use crate::gamedb::{
    extract_steam_appid, fetch_protondb_summary, find_protonfix_for_game_id, load_umu_database,
    search_umu_database,
};
use crate::icons::{
    cached_icon_path, decode_best_ico_to_png, extract_best_icon_png_from_pe, sync_game_icon,
};
use crate::proton::{
    ProtonBuild, discover_proton_builds, fetch_ge_proton_releases, install_ge_proton_release,
    is_release_installed, remove_user_proton_build,
};
use crate::runner::{
    PrefixTool, build_launch_env, detect_running_prefixes, game_umu_args, game_working_dir,
    is_desktop_entry_installed, kill_prefix, remove_managed_desktop_entry, run_umu_cli,
    sync_managed_desktop_entry,
};
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Gui,
    Help,
    Version,
    Run {
        prefix: Option<String>,
        proton: Option<String>,
        game_id: Option<String>,
        no_mangohud: bool,
        dgpu: bool,
        wayland: bool,
        nvapi: Option<bool>,
        dll_overrides: Option<String>,
        target: String,
        args: Vec<String>,
    },
    Launch {
        game: String,
        no_mangohud: bool,
    },
    Kill {
        prefix: String,
    },
    Tool {
        prefix: String,
        tool: String,
        args: Vec<String>,
    },
    List {
        filter: String,
    },
    PrefixCreate {
        name: String,
        path: Option<PathBuf>,
        proton: Option<String>,
    },
    ProtonList,
    ProtonCheck,
    ProtonInstall {
        tag: Option<String>,
    },
    ProtonRemove {
        name: String,
    },
    DesktopList,
    DesktopAdd {
        game: String,
        icon: Option<PathBuf>,
    },
    DesktopRemove {
        game: String,
    },
    DesktopSync,
    IconSet {
        game: String,
        path: PathBuf,
    },
    IconClear {
        game: String,
    },
    IconExtract {
        exe: PathBuf,
        out_png: PathBuf,
    },
    Lookup {
        query: String,
    },
    Scan {
        exe: PathBuf,
        prefix: Option<String>,
        apply: bool,
    },
    PresetList,
    PresetApply {
        prefix: String,
        preset: String,
    },
}

pub fn print_help() {
    println!(
        "\
UmuBox {} - Native GPU-accelerated Proton/UMU Game & Prefix Manager

USAGE:
  umubox                                        Launch GUI
  umubox run [OPTIONS] <EXE|MSI> [--] [ARGS...] Run executable/installer inside a prefix
  umubox launch [--no-mangohud] <GAME>          Launch a saved game (e.g. battlenet or pfx/game)
  umubox kill <PREFIX>                          Force-kill wineserver for a prefix
  umubox tool <PREFIX> <TOOL> [ARGS...]         Run winecfg | regedit | taskmgr | cmd | wineboot | winetricks
  umubox list [all|prefixes|games]              List discovered prefixes, games, and live running PIDs
  umubox prefix create <NAME> [--path DIR] [--proton BUILD]
                                                Create and register a new Wine/Proton prefix
  umubox desktop list                           List saved games & KDE app menu / icon status
  umubox desktop add <GAME> [--icon PATH]       Create & manage KDE App Menu .desktop entry (auto .exe icon)
  umubox desktop remove <GAME>                  Remove managed KDE App Menu .desktop entry
  umubox desktop sync                           Re-sync icons & .desktop entries for all managed games
  umubox icon set <GAME> <PATH>                 Set custom icon override for a game
  umubox icon clear <GAME>                      Clear custom icon override and inherit from .exe
  umubox icon extract <EXE|ICO> <OUT.png>       Extract highest-res icon from .exe/.dll/.ico to PNG
  umubox proton list                            List installed Proton & GE-Proton builds
  umubox proton check                           Check GitHub for latest GE-Proton releases
  umubox proton install [TAG]                   Download, SHA-512 verify, and install GE-Proton
  umubox proton remove <BUILD>                  Remove a user-installed Proton build
  umubox lookup <QUERY>                         Search UMU DB, protonfixes, and ProtonDB rating
  umubox scan <EXE> [--prefix PFX] [--apply]    Scan PE imports/mod DLLs & install missing winetricks
  umubox preset list                            List curated winetricks prefix presets
  umubox preset apply <PREFIX> <PRESET_ID>      Apply a prefix preset quietly via winetricks -q

RUN OPTIONS:
  -p, --prefix <ID|NAME|PATH>   Prefix to use (defaults to default prefix)
      --proton <NAME|PATH>      Proton build override
      --game-id <ID>            UMU GAMEID (default: prefix default)
      --no-mangohud             Disable MangoHud overlay for this run
      --dgpu                    Enable explicit NVIDIA PRIME dGPU offload env vars
      --wayland                 Enable experimental PROTON_ENABLE_WAYLAND=1 driver
      --nvapi / --no-nvapi      Enable or disable PROTON_ENABLE_NVAPI
      --dll-overrides <VAL>     Set WINEDLLOVERRIDES
",
        env!("CARGO_PKG_VERSION")
    );
}

pub fn parse_cli_args(args: &[String]) -> Result<CliCommand> {
    if args.is_empty() {
        return Ok(CliCommand::Gui);
    }

    match args[0].as_str() {
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        "-v" | "--version" | "version" => Ok(CliCommand::Version),
        "run" => parse_run_args(&args[1..]),
        "launch" => parse_launch_args(&args[1..]),
        "kill" => {
            let prefix = args
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox kill <prefix>"))?
                .clone();
            Ok(CliCommand::Kill { prefix })
        }
        "tool" => parse_tool_args(&args[1..]),
        "list" | "ls" => {
            let filter = args
                .get(1)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "all".to_string());
            if !matches!(filter.as_str(), "all" | "prefixes" | "games") {
                bail!("Unknown list filter '{filter}' (expected: all, prefixes, games)");
            }
            Ok(CliCommand::List { filter })
        }
        "prefix" => parse_prefix_args(&args[1..]),
        "desktop" => parse_desktop_args(&args[1..]),
        "icon" => parse_icon_args(&args[1..]),
        "proton" => parse_proton_args(&args[1..]),
        "lookup" => {
            if args.len() < 2 {
                bail!("Usage: umubox lookup <query>");
            }
            Ok(CliCommand::Lookup {
                query: args[1..].join(" "),
            })
        }
        "scan" => parse_scan_args(&args[1..]),
        "preset" => parse_preset_args(&args[1..]),
        first => {
            let lower = first.to_ascii_lowercase();
            if Path::new(first).is_file()
                || lower.ends_with(".exe")
                || lower.ends_with(".msi")
                || lower.ends_with(".bat")
            {
                parse_run_args(args)
            } else {
                bail!("Unknown command '{first}'. Run `umubox --help` for usage.")
            }
        }
    }
}

fn parse_run_args(rest: &[String]) -> Result<CliCommand> {
    let mut prefix: Option<String> = None;
    let mut proton: Option<String> = None;
    let mut game_id: Option<String> = None;
    let mut no_mangohud = false;
    let mut dgpu = false;
    let mut wayland = false;
    let mut nvapi: Option<bool> = None;
    let mut dll_overrides: Option<String> = None;
    let mut target: Option<String> = None;
    let mut extra_args = Vec::new();

    let mut i = 0usize;
    while i < rest.len() {
        let tok = &rest[i];
        if target.is_some() {
            extra_args.push(tok.clone());
            i += 1;
            continue;
        }
        match tok.as_str() {
            "--" => {
                if let Some(next) = rest.get(i + 1) {
                    target = Some(next.clone());
                    extra_args.extend(rest[i + 2..].iter().cloned());
                }
                break;
            }
            "-p" | "--prefix" => {
                i += 1;
                prefix = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow!("--prefix requires a value"))?
                        .clone(),
                );
            }
            "--proton" => {
                i += 1;
                proton = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow!("--proton requires a value"))?
                        .clone(),
                );
            }
            "--game-id" => {
                i += 1;
                game_id = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow!("--game-id requires a value"))?
                        .clone(),
                );
            }
            "--no-mangohud" => no_mangohud = true,
            "--dgpu" => dgpu = true,
            "--wayland" => wayland = true,
            "--nvapi" => nvapi = Some(true),
            "--no-nvapi" => nvapi = Some(false),
            "--dll-overrides" => {
                i += 1;
                dll_overrides = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow!("--dll-overrides requires a value"))?
                        .clone(),
                );
            }
            flag if flag.starts_with('-') => {
                bail!("Unknown option '{flag}' for `umubox run`");
            }
            pos => {
                target = Some(pos.to_string());
            }
        }
        i += 1;
    }

    let target = target.ok_or_else(|| anyhow!("`umubox run` requires an executable path"))?;
    Ok(CliCommand::Run {
        prefix,
        proton,
        game_id,
        no_mangohud,
        dgpu,
        wayland,
        nvapi,
        dll_overrides,
        target,
        args: extra_args,
    })
}

fn parse_launch_args(rest: &[String]) -> Result<CliCommand> {
    let mut no_mangohud = false;
    let mut game: Option<String> = None;
    for tok in rest {
        match tok.as_str() {
            "--no-mangohud" => no_mangohud = true,
            flag if flag.starts_with('-') => bail!("Unknown option '{flag}' for `umubox launch`"),
            val => {
                if game.is_some() {
                    bail!("Unexpected argument '{val}' for `umubox launch`");
                }
                game = Some(val.to_string());
            }
        }
    }
    let game = game.ok_or_else(|| anyhow!("Usage: umubox launch [--no-mangohud] <game>"))?;
    Ok(CliCommand::Launch { game, no_mangohud })
}

fn parse_tool_args(rest: &[String]) -> Result<CliCommand> {
    if rest.len() < 2 {
        bail!("Usage: umubox tool <prefix> <winecfg|regedit|taskmgr|cmd|wineboot|winetricks> [args...]");
    }
    Ok(CliCommand::Tool {
        prefix: rest[0].clone(),
        tool: rest[1].clone(),
        args: rest[2..].to_vec(),
    })
}

fn parse_prefix_args(rest: &[String]) -> Result<CliCommand> {
    match rest.first().map(String::as_str) {
        Some("create") => {
            let mut name: Option<String> = None;
            let mut path: Option<PathBuf> = None;
            let mut proton: Option<String> = None;
            let mut i = 1usize;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--path" => {
                        i += 1;
                        path = Some(PathBuf::from(
                            rest.get(i)
                                .ok_or_else(|| anyhow!("--path requires a directory"))?,
                        ));
                    }
                    "--proton" => {
                        i += 1;
                        proton = Some(
                            rest.get(i)
                                .ok_or_else(|| anyhow!("--proton requires a build name/path"))?
                                .clone(),
                        );
                    }
                    flag if flag.starts_with('-') => {
                        bail!("Unknown option '{flag}' for `umubox prefix create`");
                    }
                    val => {
                        if name.is_some() {
                            bail!("Unexpected argument '{val}'");
                        }
                        name = Some(val.to_string());
                    }
                }
                i += 1;
            }
            let name = name.ok_or_else(|| anyhow!("Usage: umubox prefix create <name>"))?;
            Ok(CliCommand::PrefixCreate { name, path, proton })
        }
        _ => bail!("Usage: umubox prefix create <name> [--path DIR] [--proton BUILD]"),
    }
}

fn parse_proton_args(rest: &[String]) -> Result<CliCommand> {
    match rest.first().map(String::as_str) {
        Some("list" | "ls") => Ok(CliCommand::ProtonList),
        Some("check") => Ok(CliCommand::ProtonCheck),
        Some("install") => Ok(CliCommand::ProtonInstall {
            tag: rest.get(1).cloned(),
        }),
        Some("remove" | "rm" | "delete") => {
            let name = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox proton remove <build-name>"))?
                .clone();
            Ok(CliCommand::ProtonRemove { name })
        }
        _ => bail!("Usage: umubox proton <list|check|install [TAG]|remove <BUILD>>"),
    }
}

fn parse_desktop_args(rest: &[String]) -> Result<CliCommand> {
    match rest.first().map(String::as_str) {
        Some("list" | "ls") => Ok(CliCommand::DesktopList),
        Some("sync") => Ok(CliCommand::DesktopSync),
        Some("remove" | "rm" | "delete") => {
            let game = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox desktop remove <game>"))?
                .clone();
            Ok(CliCommand::DesktopRemove { game })
        }
        Some("add" | "enable") => {
            let mut game: Option<String> = None;
            let mut icon: Option<PathBuf> = None;
            let mut i = 1usize;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--icon" => {
                        i += 1;
                        icon = Some(PathBuf::from(
                            rest.get(i)
                                .ok_or_else(|| anyhow!("--icon requires a file path"))?,
                        ));
                    }
                    flag if flag.starts_with('-') => {
                        bail!("Unknown option '{flag}' for `umubox desktop add`");
                    }
                    val => {
                        if game.is_some() {
                            bail!("Unexpected argument '{val}' for `umubox desktop add`");
                        }
                        game = Some(val.to_string());
                    }
                }
                i += 1;
            }
            let game = game.ok_or_else(|| anyhow!("Usage: umubox desktop add <game> [--icon PATH]"))?;
            Ok(CliCommand::DesktopAdd { game, icon })
        }
        _ => bail!("Usage: umubox desktop <list|add <GAME> [--icon PATH]|remove <GAME>|sync>"),
    }
}

fn parse_icon_args(rest: &[String]) -> Result<CliCommand> {
    match rest.first().map(String::as_str) {
        Some("set") => {
            let game = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox icon set <game> <icon-path>"))?
                .clone();
            let path = rest
                .get(2)
                .ok_or_else(|| anyhow!("Usage: umubox icon set <game> <icon-path>"))?;
            Ok(CliCommand::IconSet {
                game,
                path: PathBuf::from(path),
            })
        }
        Some("clear" | "reset") => {
            let game = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox icon clear <game>"))?
                .clone();
            Ok(CliCommand::IconClear { game })
        }
        Some("extract") => {
            let exe = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox icon extract <exe|ico> <out.png>"))?;
            let out_png = rest
                .get(2)
                .ok_or_else(|| anyhow!("Usage: umubox icon extract <exe|ico> <out.png>"))?;
            Ok(CliCommand::IconExtract {
                exe: PathBuf::from(exe),
                out_png: PathBuf::from(out_png),
            })
        }
        _ => bail!("Usage: umubox icon <set <GAME> <PATH>|clear <GAME>|extract <EXE> <OUT.png>>"),
    }
}

fn parse_scan_args(rest: &[String]) -> Result<CliCommand> {
    let mut exe: Option<PathBuf> = None;
    let mut prefix: Option<String> = None;
    let mut apply = false;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" | "--prefix" => {
                i += 1;
                prefix = Some(
                    rest.get(i)
                        .ok_or_else(|| anyhow!("--prefix requires a prefix name/id"))?
                        .clone(),
                );
            }
            "--apply" => apply = true,
            flag if flag.starts_with('-') => bail!("Unknown option '{flag}' for `umubox scan`"),
            val => {
                if exe.is_some() {
                    bail!("Unexpected argument '{val}' for `umubox scan`");
                }
                exe = Some(PathBuf::from(val));
            }
        }
        i += 1;
    }
    let exe = exe.ok_or_else(|| anyhow!("Usage: umubox scan <EXE> [--prefix PFX] [--apply]"))?;
    Ok(CliCommand::Scan { exe, prefix, apply })
}

fn parse_preset_args(rest: &[String]) -> Result<CliCommand> {
    match rest.first().map(String::as_str) {
        Some("list" | "ls") => Ok(CliCommand::PresetList),
        Some("apply" | "install") => {
            let prefix = rest
                .get(1)
                .ok_or_else(|| anyhow!("Usage: umubox preset apply <prefix> <preset-id>"))?
                .clone();
            let preset = rest
                .get(2)
                .ok_or_else(|| anyhow!("Usage: umubox preset apply <prefix> <preset-id>"))?
                .clone();
            Ok(CliCommand::PresetApply { prefix, preset })
        }
        _ => bail!("Usage: umubox preset <list|apply <PREFIX> <PRESET_ID>>"),
    }
}

fn resolve_proton_spec(spec: &str, builds: &[ProtonBuild]) -> Result<PathBuf> {
    let as_path = Path::new(spec);
    if as_path.is_dir() {
        return Ok(fs::canonicalize(as_path).unwrap_or_else(|_| as_path.to_path_buf()));
    }
    if let Some(found) = builds.iter().find(|b| {
        b.name.eq_ignore_ascii_case(spec) || b.version_tag.eq_ignore_ascii_case(spec)
    }) {
        return Ok(found.path.clone());
    }
    bail!("Proton build '{spec}' not found. Run `umubox proton list` to see available builds.");
}

fn resolve_prefix<'a>(cfg: &'a AppConfig, spec: Option<&str>) -> Result<&'a PrefixConfig> {
    let wanted = spec
        .or(cfg.default_prefix_id.as_deref())
        .ok_or_else(|| anyhow!("No prefix specified and no default prefix configured"))?;

    cfg.prefixes
        .iter()
        .find(|p| p.id == wanted || p.name.eq_ignore_ascii_case(wanted) || p.path == Path::new(wanted))
        .ok_or_else(|| anyhow!("Prefix '{wanted}' not found. Run `umubox list` to see prefixes."))
}

fn resolve_game<'a>(cfg: &'a AppConfig, spec: &str) -> Result<(&'a PrefixConfig, &'a crate::config::GameEntry)> {
    let (pi, gi) = resolve_game_indices(cfg, spec)?;
    Ok((&cfg.prefixes[pi], &cfg.prefixes[pi].games[gi]))
}

fn resolve_game_indices(cfg: &AppConfig, spec: &str) -> Result<(usize, usize)> {
    if let Some((pfx_spec, game_spec)) = spec.split_once('/') {
        let pi = cfg
            .prefixes
            .iter()
            .position(|p| {
                p.id == pfx_spec || p.name.eq_ignore_ascii_case(pfx_spec) || p.path == Path::new(pfx_spec)
            })
            .ok_or_else(|| anyhow!("Prefix '{pfx_spec}' not found. Run `umubox list` to see prefixes."))?;
        let pfx = &cfg.prefixes[pi];
        let gi = pfx
            .games
            .iter()
            .position(|g| g.id == game_spec || g.name.eq_ignore_ascii_case(game_spec))
            .ok_or_else(|| anyhow!("Game '{game_spec}' not found in prefix '{}'", pfx.name))?;
        return Ok((pi, gi));
    }

    for (pi, pfx) in cfg.prefixes.iter().enumerate() {
        if let Some(gi) = pfx
            .games
            .iter()
            .position(|g| g.id == spec || g.name.eq_ignore_ascii_case(spec))
        {
            return Ok((pi, gi));
        }
    }

    bail!("Saved game '{spec}' not found. Run `umubox list games` to see saved shortcuts.")
}

pub fn execute_cli(cmd: CliCommand) -> Result<i32> {
    match cmd {
        CliCommand::Gui => Ok(0),
        CliCommand::Help => {
            print_help();
            Ok(0)
        }
        CliCommand::Version => {
            println!("umubox {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        CliCommand::Run {
            prefix,
            proton,
            game_id,
            no_mangohud,
            dgpu,
            wayland,
            nvapi,
            dll_overrides,
            target,
            args,
        } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let pfx = resolve_prefix(&cfg, prefix.as_deref())?;

            let target_path = Path::new(&target);
            let resolved_target = if target_path.exists() {
                fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf())
            } else {
                target_path.to_path_buf()
            };
            let work_dir = resolved_target
                .parent()
                .filter(|p| p.is_dir())
                .map(Path::to_path_buf);

            let mut opts: LaunchOptions = pfx.default_options.clone();
            if no_mangohud {
                opts.mangohud = false;
            }
            if dgpu {
                opts.nvidia_prime_offload = true;
            }
            if wayland {
                opts.wayland_driver = true;
            }
            if let Some(n) = nvapi {
                opts.nvapi = n;
            }
            if let Some(over) = dll_overrides {
                opts.dll_overrides = over;
            }

            let proton_override = proton
                .as_deref()
                .map(|s| resolve_proton_spec(s, &builds))
                .transpose()?;
            let mut pfx_eff = pfx.clone();
            if let Some(gid) = game_id {
                pfx_eff.default_game_id = gid;
            }
            if proton_override.is_some() {
                pfx_eff.proton_path = proton_override;
            }

            let env_pairs = build_launch_env(&pfx_eff, None, &opts, cfg.default_proton.as_deref());
            let mut umu_args = vec![resolved_target.display().to_string()];
            umu_args.extend(args);
            run_umu_cli(&umu_args, work_dir.as_deref(), &env_pairs)
        }
        CliCommand::Launch { game, no_mangohud } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let (pfx, entry) = resolve_game(&cfg, &game)?;
            let mut opts = entry.options.clone();
            if no_mangohud {
                opts.mangohud = false;
            }
            let env_pairs =
                build_launch_env(pfx, Some(entry), &opts, cfg.default_proton.as_deref());
            let umu_args = game_umu_args(entry);
            let wd = game_working_dir(entry);
            run_umu_cli(&umu_args, wd.as_deref(), &env_pairs)
        }
        CliCommand::Kill { prefix } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let pfx = resolve_prefix(&cfg, Some(&prefix))?;
            let msg = kill_prefix(pfx, &builds)?;
            println!("{msg}");
            Ok(0)
        }
        CliCommand::Tool { prefix, tool, args } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let pfx = resolve_prefix(&cfg, Some(&prefix))?;
            let parsed_tool = PrefixTool::parse_cli(&tool).ok_or_else(|| {
                anyhow!("Unknown tool '{tool}'. Expected: winecfg, regedit, taskmgr, cmd, wineboot, winetricks")
            })?;
            let env_pairs =
                build_launch_env(pfx, None, &pfx.default_options, cfg.default_proton.as_deref());
            let umu_args = parsed_tool.umu_args(&args);
            run_umu_cli(&umu_args, Some(&pfx.path), &env_pairs)
        }
        CliCommand::List { filter } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let running = detect_running_prefixes(&cfg.prefixes);

            if matches!(filter.as_str(), "all" | "prefixes") {
                println!("Prefixes ({}):", cfg.prefixes.len());
                for p in &cfg.prefixes {
                    let def_mark = if cfg.default_prefix_id.as_deref() == Some(&p.id) {
                        " [default]"
                    } else {
                        ""
                    };
                    let run_info = match running.get(&p.id) {
                        Some(procs) if !procs.is_empty() => {
                            let names: Vec<&str> = procs.iter().take(4).map(|r| r.comm.as_str()).collect();
                            format!(" [RUNNING ({}): {}]", procs.len(), names.join(", "))
                        }
                        _ => String::new(),
                    };
                    println!(
                        "  - {} (id={}, games={}, path={}){}{}",
                        p.name,
                        p.id,
                        p.games.len(),
                        p.path.display(),
                        def_mark,
                        run_info
                    );
                }
            }

            if matches!(filter.as_str(), "all" | "games") {
                println!("\nSaved Games:");
                let mut total = 0usize;
                for p in &cfg.prefixes {
                    for g in &p.games {
                        total += 1;
                        let menu_mark = if g.desktop_managed || is_desktop_entry_installed(&p.id, &g.id) {
                            " [menu]"
                        } else {
                            ""
                        };
                        println!(
                            "  - {}/{}  \"{}\"  exe={}  GAMEID={}{}",
                            p.id,
                            g.id,
                            g.name,
                            g.exe_path.display(),
                            if g.game_id.is_empty() { "0" } else { &g.game_id },
                            menu_mark
                        );
                    }
                }
                if total == 0 {
                    println!("  (none saved yet)");
                }
            }
            Ok(0)
        }
        CliCommand::PrefixCreate { name, path, proton } => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let id = make_unique_id(&name, cfg.prefixes.iter().map(|p| p.id.as_str()));
            let dir = path.unwrap_or_else(|| cfg.prefixes_root.join(&id));
            fs::create_dir_all(&dir)
                .with_context(|| format!("create prefix dir {}", dir.display()))?;
            let canon = fs::canonicalize(&dir).unwrap_or(dir);
            let proton_path = proton
                .as_deref()
                .map(|s| resolve_proton_spec(s, &builds))
                .transpose()?;
            cfg.prefixes.push(PrefixConfig {
                id: id.clone(),
                name: name.clone(),
                path: canon.clone(),
                default_game_id: "0".to_string(),
                proton_path,
                default_options: LaunchOptions::default(),
                games: Vec::new(),
            });
            if cfg.default_prefix_id.is_none() {
                cfg.default_prefix_id = Some(id.clone());
            }
            cfg.save()?;
            println!("Created prefix '{}' (id={}) at {}", name, id, canon.display());
            Ok(0)
        }
        CliCommand::ProtonList => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            println!("Installed Proton builds ({}):", builds.len());
            for b in &builds {
                let is_def = cfg.default_proton.as_ref().is_some_and(|dp| {
                    dp == &b.path || fs::canonicalize(dp).ok().as_ref() == Some(&b.canonical_path)
                });
                let mark = if is_def { " [default]" } else { "" };
                println!(
                    "  - {} ({}) [{}] -> {}{}",
                    b.name,
                    b.version_tag,
                    b.source.label(),
                    b.path.display(),
                    mark
                );
            }
            Ok(0)
        }
        CliCommand::ProtonCheck => {
            let builds = discover_proton_builds();
            let releases = fetch_ge_proton_releases(8)?;
            println!("Latest GE-Proton releases on GitHub:");
            for r in &releases {
                let status = if is_release_installed(&r.tag, &builds) {
                    " [installed]"
                } else {
                    ""
                };
                let mb = r.tarball_size / (1024 * 1024);
                println!("  - {} ({}, ~{} MiB){}", r.tag, r.published_at, mb, status);
            }
            Ok(0)
        }
        CliCommand::ProtonInstall { tag } => {
            let releases = fetch_ge_proton_releases(15)?;
            let target = if let Some(t) = tag {
                releases
                    .into_iter()
                    .find(|r| r.tag.eq_ignore_ascii_case(&t))
                    .ok_or_else(|| anyhow!("GE-Proton release '{t}' not found in recent GitHub releases"))?
            } else {
                releases
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("No GE-Proton releases returned by GitHub"))?
            };

            let installed = discover_proton_builds();
            if is_release_installed(&target.tag, &installed) {
                println!("{} is already installed.", target.tag);
                return Ok(0);
            }

            let last_phase = std::cell::Cell::new(None);
            let path = install_ge_proton_release(&target, |p| {
                if last_phase.get() != Some(p.phase) {
                    last_phase.set(Some(p.phase));
                    println!("{}", p.message);
                }
            })?;
            println!("Installed {} at {}", target.tag, path.display());
            Ok(0)
        }
        CliCommand::ProtonRemove { name } => {
            let builds = discover_proton_builds();
            let target = builds
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(&name) || b.version_tag.eq_ignore_ascii_case(&name))
                .ok_or_else(|| anyhow!("Proton build '{name}' not found"))?;
            remove_user_proton_build(&target.path)?;
            println!("Removed user Proton build '{}'", target.name);
            Ok(0)
        }
        CliCommand::DesktopList => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            println!("Managed Desktop Entries & Game Icons:");
            let mut total = 0usize;
            for p in &cfg.prefixes {
                for g in &p.games {
                    total += 1;
                    let in_menu = g.desktop_managed || is_desktop_entry_installed(&p.id, &g.id);
                    let menu_str = if in_menu { "[menu]" } else { "[not-in-menu]" };
                    let icon_str = if let Some(ref ov) = g.icon_override {
                        format!("[override: {}]", ov.display())
                    } else if cached_icon_path(&p.id, &g.id).is_some() {
                        "[exe-icon]".to_string()
                    } else {
                        "[exe-icon (pending/fallback)]".to_string()
                    };
                    println!("  - {}/{}  \"{}\"  {}  {}", p.id, g.id, g.name, menu_str, icon_str);
                }
            }
            if total == 0 {
                println!("  (no saved games)");
            }
            Ok(0)
        }
        CliCommand::DesktopAdd { game, icon } => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let (pi, gi) = resolve_game_indices(&cfg, &game)?;
            if let Some(icon_p) = icon {
                let canon = if icon_p.exists() {
                    fs::canonicalize(&icon_p).unwrap_or(icon_p)
                } else {
                    icon_p
                };
                cfg.prefixes[pi].games[gi].icon_override = Some(canon);
            }
            cfg.prefixes[pi].games[gi].desktop_managed = true;
            let force_icon = cfg.prefixes[pi].games[gi].icon_override.is_some();
            let icon_path = sync_game_icon(
                &cfg.prefixes[pi].id,
                &cfg.prefixes[pi].games[gi],
                force_icon,
            )?;
            let dest = sync_managed_desktop_entry(
                &cfg.prefixes[pi],
                &cfg.prefixes[pi].games[gi],
                icon_path.as_deref(),
            )?;
            cfg.save()?;
            println!(
                "Managed desktop entry written to {} (icon: {})",
                dest.display(),
                icon_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "umubox fallback".to_string())
            );
            Ok(0)
        }
        CliCommand::DesktopRemove { game } => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let (pi, gi) = resolve_game_indices(&cfg, &game)?;
            cfg.prefixes[pi].games[gi].desktop_managed = false;
            let pfx_id = cfg.prefixes[pi].id.clone();
            let game_id = cfg.prefixes[pi].games[gi].id.clone();
            let removed = remove_managed_desktop_entry(&pfx_id, &game_id)?;
            cfg.save()?;
            if removed {
                println!("Removed managed desktop entry for {pfx_id}/{game_id}");
            } else {
                println!("No managed desktop file existed on disk for {pfx_id}/{game_id} (flag cleared)");
            }
            Ok(0)
        }
        CliCommand::DesktopSync => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let mut synced = 0usize;
            let mut changed = false;
            for pfx in &mut cfg.prefixes {
                let pfx_id = pfx.id.clone();
                let pfx_snap = pfx.clone();
                for g in &mut pfx.games {
                    if g.desktop_managed || is_desktop_entry_installed(&pfx_id, &g.id) {
                        if !g.desktop_managed {
                            g.desktop_managed = true;
                            changed = true;
                        }
                        let icon = sync_game_icon(&pfx_id, g, false).ok().flatten();
                        let dest = sync_managed_desktop_entry(&pfx_snap, g, icon.as_deref())?;
                        println!("Synced {} -> {}", g.name, dest.display());
                        synced += 1;
                    }
                }
            }
            if changed {
                cfg.save()?;
            }
            println!("Synced {synced} managed desktop entry(ies).");
            Ok(0)
        }
        CliCommand::IconSet { game, path } => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let (pi, gi) = resolve_game_indices(&cfg, &game)?;
            if !path.is_file() {
                bail!("Icon file not found: {}", path.display());
            }
            let canon = fs::canonicalize(&path).unwrap_or(path);
            cfg.prefixes[pi].games[gi].icon_override = Some(canon);
            let pfx_id = cfg.prefixes[pi].id.clone();
            let cached = sync_game_icon(&pfx_id, &cfg.prefixes[pi].games[gi], true)?;
            if cfg.prefixes[pi].games[gi].desktop_managed {
                sync_managed_desktop_entry(
                    &cfg.prefixes[pi],
                    &cfg.prefixes[pi].games[gi],
                    cached.as_deref(),
                )?;
            }
            cfg.save()?;
            println!(
                "Set custom icon for {}/{} -> {}",
                pfx_id,
                cfg.prefixes[pi].games[gi].id,
                cached
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            Ok(0)
        }
        CliCommand::IconClear { game } => {
            let builds = discover_proton_builds();
            let mut cfg = AppConfig::load_or_discover(&builds);
            let (pi, gi) = resolve_game_indices(&cfg, &game)?;
            cfg.prefixes[pi].games[gi].icon_override = None;
            let pfx_id = cfg.prefixes[pi].id.clone();
            let cached = sync_game_icon(&pfx_id, &cfg.prefixes[pi].games[gi], true)?;
            if cfg.prefixes[pi].games[gi].desktop_managed {
                sync_managed_desktop_entry(
                    &cfg.prefixes[pi],
                    &cfg.prefixes[pi].games[gi],
                    cached.as_deref(),
                )?;
            }
            cfg.save()?;
            println!(
                "Cleared custom icon for {}/{} (re-extracted: {})",
                pfx_id,
                cfg.prefixes[pi].games[gi].id,
                cached
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "none available".to_string())
            );
            Ok(0)
        }
        CliCommand::IconExtract { exe, out_png } => {
            if !exe.is_file() {
                bail!("Input file not found: {}", exe.display());
            }
            let ext = exe
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let png = if ext == "ico" {
                let raw = fs::read(&exe).with_context(|| format!("read {}", exe.display()))?;
                decode_best_ico_to_png(&raw)?
            } else {
                extract_best_icon_png_from_pe(&exe)?
            };
            if let Some(parent) = out_png.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out_png, &png)
                .with_context(|| format!("write {}", out_png.display()))?;
            println!("Extracted PNG icon ({} bytes) -> {}", png.len(), out_png.display());
            Ok(0)
        }
        CliCommand::Lookup { query } => {
            let builds = discover_proton_builds();
            let (db, src) = load_umu_database(&builds);
            if db.is_empty() {
                bail!("No local umu-database.csv found in installed GE-Proton builds or cache");
            }
            println!(
                "UMU Database ({} entries from {}):",
                db.len(),
                src.map(|p| p.display().to_string())
                    .unwrap_or_else(|| "unknown".into())
            );
            let hits = search_umu_database(&db, &query, 10);
            if hits.is_empty() {
                println!("  No matches for '{query}'.");
                return Ok(0);
            }
            for e in &hits {
                let fix = find_protonfix_for_game_id(&e.umu_id, &builds);
                let fix_tag = match &fix {
                    Some(f) if !f.detected_verbs.is_empty() => {
                        format!(" [Auto-Fix: {}]", f.detected_verbs.join(","))
                    }
                    Some(_) => " [Auto-Fix]".to_string(),
                    None => String::new(),
                };
                println!(
                    "  - {:<14} | {:<10} | {}{}",
                    e.umu_id, e.store, e.title, fix_tag
                );
            }
            if let Some(app_id) = hits.iter().find_map(|h| extract_steam_appid(&h.umu_id)) {
                match fetch_protondb_summary(app_id) {
                    Ok(pdb) => println!(
                        "\nProtonDB AppID {app_id}: tier={} (score {:.2}, {}, {} reports)",
                        pdb.tier, pdb.score, pdb.confidence, pdb.total
                    ),
                    Err(err) => println!("\nProtonDB AppID {app_id}: {err}"),
                }
            }
            Ok(0)
        }
        CliCommand::Scan { exe, prefix, apply } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let pfx_opt = if prefix.is_some() || apply {
                Some(resolve_prefix(&cfg, prefix.as_deref())?)
            } else {
                cfg.default_prefix_id
                    .as_deref()
                    .and_then(|id| cfg.prefixes.iter().find(|p| p.id == id))
            };
            let report = analyze_exe_dependencies(&exe, pfx_opt.map(|p| p.path.as_path()))?;
            println!(
                "Scanned {} ({} PE binary/ies, {} unique DLL imports)",
                report.target_exe.display(),
                report.scanned_binaries,
                report.imported_dlls.len()
            );
            if report.recommendations.is_empty() {
                println!("  No missing MSVC/DirectX/.NET runtime verbs flagged.");
            } else {
                println!("\nRecommended Winetricks Verbs:");
                for r in &report.recommendations {
                    let st = if r.already_installed { "installed" } else { "MISSING" };
                    println!(
                        "  - {:<14} [{}] {} ({})",
                        r.verb,
                        st,
                        r.label,
                        r.matched_dlls.join(", ")
                    );
                }
            }
            if !report.side_by_side_overrides.is_empty() {
                println!("\nSide-by-Side Mod / Proxy DLL Overrides:");
                for s in &report.side_by_side_overrides {
                    println!("  - {} -> WINEDLLOVERRIDES={}", s.filename, s.override_spec);
                }
            }
            if apply {
                let pfx = pfx_opt.ok_or_else(|| anyhow!("No prefix selected to apply verbs"))?;
                let missing: Vec<String> = report
                    .recommendations
                    .iter()
                    .filter(|r| !r.already_installed)
                    .map(|r| r.verb.clone())
                    .collect();
                if missing.is_empty() {
                    println!("\nAll recommended verbs are already installed in '{}'.", pfx.name);
                    return Ok(0);
                }
                println!("\nInstalling in '{}': umu-run winetricks -q {}", pfx.name, missing.join(" "));
                let mut args = vec!["winetricks".to_string(), "-q".to_string()];
                args.extend(missing);
                let env_pairs = build_launch_env(
                    pfx,
                    None,
                    &pfx.default_options,
                    cfg.default_proton.as_deref(),
                );
                return run_umu_cli(&args, Some(&pfx.path), &env_pairs);
            }
            Ok(0)
        }
        CliCommand::PresetList => {
            println!("Curated Prefix Presets ({}):", PREFIX_PRESETS.len());
            for p in PREFIX_PRESETS {
                println!("  - {:<16} | {} [{}]", p.id, p.name, p.verbs.join(" "));
            }
            Ok(0)
        }
        CliCommand::PresetApply { prefix, preset } => {
            let builds = discover_proton_builds();
            let cfg = AppConfig::load_or_discover(&builds);
            let pfx = resolve_prefix(&cfg, Some(&prefix))?;
            let found = find_preset(&preset).ok_or_else(|| {
                anyhow!("Unknown preset '{preset}'. Run `umubox preset list` to see available presets.")
            })?;
            let installed = read_installed_winetricks_verbs(&pfx.path);
            let missing: Vec<String> = found
                .verbs
                .iter()
                .filter(|v| !installed.contains(&v.to_ascii_lowercase()))
                .map(|v| (*v).to_string())
                .collect();
            let to_install: Vec<String> = if missing.is_empty() {
                found.verbs.iter().map(|s| (*s).to_string()).collect()
            } else {
                missing
            };
            println!(
                "Applying preset '{}' to '{}': umu-run winetricks -q {}",
                found.name,
                pfx.name,
                to_install.join(" ")
            );
            let mut args = vec!["winetricks".to_string(), "-q".to_string()];
            args.extend(to_install);
            let env_pairs = build_launch_env(
                pfx,
                None,
                &pfx.default_options,
                cfg.default_proton.as_deref(),
            );
            run_umu_cli(&args, Some(&pfx.path), &env_pairs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_cli_subcommands_cleanly() {
        assert_eq!(parse_cli_args(&[]).unwrap(), CliCommand::Gui);
        assert_eq!(parse_cli_args(&vec_s(&["--help"])).unwrap(), CliCommand::Help);

        assert_eq!(
            parse_cli_args(&vec_s(&[
                "run",
                "--prefix",
                "battlenet",
                "--no-mangohud",
                "--dgpu",
                "--wayland",
                "C:\\setup.exe",
                "/S"
            ]))
            .unwrap(),
            CliCommand::Run {
                prefix: Some("battlenet".into()),
                proton: None,
                game_id: None,
                no_mangohud: true,
                dgpu: true,
                wayland: true,
                nvapi: None,
                dll_overrides: None,
                target: "C:\\setup.exe".into(),
                args: vec_s(&["/S"]),
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["launch", "--no-mangohud", "battlenet/battle-net"])).unwrap(),
            CliCommand::Launch {
                game: "battlenet/battle-net".into(),
                no_mangohud: true,
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["kill", "battlenet"])).unwrap(),
            CliCommand::Kill {
                prefix: "battlenet".into()
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["tool", "umu-default", "winetricks", "vcrun2022"])).unwrap(),
            CliCommand::Tool {
                prefix: "umu-default".into(),
                tool: "winetricks".into(),
                args: vec_s(&["vcrun2022"]),
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["list", "games"])).unwrap(),
            CliCommand::List {
                filter: "games".into()
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&[
                "prefix",
                "create",
                "Mods",
                "--path",
                "/tmp/mods",
                "--proton",
                "proton-ge-custom"
            ]))
            .unwrap(),
            CliCommand::PrefixCreate {
                name: "Mods".into(),
                path: Some(PathBuf::from("/tmp/mods")),
                proton: Some("proton-ge-custom".into()),
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["proton", "install", "GE-Proton11-12"])).unwrap(),
            CliCommand::ProtonInstall {
                tag: Some("GE-Proton11-12".into())
            }
        );

        assert_eq!(
            parse_cli_args(&vec_s(&["desktop", "list"])).unwrap(),
            CliCommand::DesktopList
        );
        assert_eq!(
            parse_cli_args(&vec_s(&[
                "desktop",
                "add",
                "umu-default/azerothsurvivors",
                "--icon",
                "/tmp/icon.png"
            ]))
            .unwrap(),
            CliCommand::DesktopAdd {
                game: "umu-default/azerothsurvivors".into(),
                icon: Some(PathBuf::from("/tmp/icon.png")),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["desktop", "remove", "azerothsurvivors"])).unwrap(),
            CliCommand::DesktopRemove {
                game: "azerothsurvivors".into(),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["desktop", "sync"])).unwrap(),
            CliCommand::DesktopSync
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["icon", "set", "azerothsurvivors", "/tmp/my.svg"])).unwrap(),
            CliCommand::IconSet {
                game: "azerothsurvivors".into(),
                path: PathBuf::from("/tmp/my.svg"),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["icon", "clear", "azerothsurvivors"])).unwrap(),
            CliCommand::IconClear {
                game: "azerothsurvivors".into(),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["icon", "extract", "/tmp/a.exe", "/tmp/a.png"])).unwrap(),
            CliCommand::IconExtract {
                exe: PathBuf::from("/tmp/a.exe"),
                out_png: PathBuf::from("/tmp/a.png"),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["lookup", "Witcher", "3"])).unwrap(),
            CliCommand::Lookup {
                query: "Witcher 3".into(),
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["scan", "/tmp/game.exe", "--prefix", "default", "--apply"])).unwrap(),
            CliCommand::Scan {
                exe: PathBuf::from("/tmp/game.exe"),
                prefix: Some("default".into()),
                apply: true,
            }
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["preset", "list"])).unwrap(),
            CliCommand::PresetList
        );
        assert_eq!(
            parse_cli_args(&vec_s(&["preset", "apply", "default", "modern-dx"])).unwrap(),
            CliCommand::PresetApply {
                prefix: "default".into(),
                preset: "modern-dx".into(),
            }
        );
    }
}
