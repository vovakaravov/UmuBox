pub mod console_view;
pub mod modals;
pub mod prefix_view;
pub mod proton_view;
pub mod sidebar;

use crate::config::{
    AppConfig, GameEntry, LaunchOptions, PrefixConfig, format_extra_env, make_unique_id,
    parse_extra_env,
};
use crate::icons::{
    cached_icon_path, icon_key, load_gpui_image_from_cached, remove_cached_icon, sync_game_icon,
};
use crate::proton::{
    DownloadPhase, DownloadProgress, GeProtonRelease, ProtonBuild, discover_proton_builds,
    fetch_ge_proton_releases, install_ge_proton_release, is_release_installed,
    remove_user_proton_build,
};
use crate::runner::{
    PrefixTool, RunningProcess, build_launch_env, clear_shader_cache, detect_running_prefixes,
    dir_size_bytes, format_bytes, game_umu_args, game_working_dir, has_ntsync_device,
    is_desktop_entry_installed, kill_prefix, open_in_file_manager, remove_managed_desktop_entry,
    spawn_umu_captured, sync_managed_desktop_entry,
};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Image, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement, PathPromptOptions, Render, SharedString, Styled,
    Window, actions, div, prelude::FluentBuilder as _, px,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

actions!(umubox, [RefreshState, TabPrefixes, TabProton, TabConsole]);

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-r", RefreshState, None),
        KeyBinding::new("ctrl-1", TabPrefixes, None),
        KeyBinding::new("ctrl-2", TabProton, None),
        KeyBinding::new("ctrl-3", TabConsole, None),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Prefixes,
    Proton,
    Console,
}

const MAX_CONSOLE_LINES: usize = 1500;

pub struct UmuBoxApp {
    pub focus_handle: FocusHandle,
    pub config: AppConfig,
    pub selected_prefix: usize,
    pub active_tab: MainTab,
    pub installed_protons: Vec<ProtonBuild>,
    pub releases: Vec<GeProtonRelease>,
    pub releases_loading: bool,
    pub releases_error: Option<String>,
    pub download_state: Option<(String, DownloadProgress)>,
    pub running_prefixes: HashMap<String, Vec<RunningProcess>>,
    pub prefix_sizes: HashMap<String, u64>,
    pub game_icons: HashMap<String, Arc<Image>>,
    pub ntsync_available: bool,
    pub umu_version: String,
    pub console_lines: Vec<String>,
    pub log_tx: Sender<String>,

    pub default_game_id_input: Entity<InputState>,
    pub dll_overrides_input: Entity<InputState>,
    pub extra_env_input: Entity<InputState>,
}

impl Focusable for UmuBoxApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn detect_umu_version_label() -> String {
    let Ok(out) = Command::new("umu-run").arg("--version").output() else {
        return "umu-run missing".to_string();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    if let Some(rest) = line.strip_prefix("umu-launcher version ") {
        let ver = rest.split_whitespace().next().unwrap_or(rest);
        return format!("umu {ver}");
    }
    if line.is_empty() {
        "umu-run".to_string()
    } else {
        line.to_string()
    }
}

impl UmuBoxApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let installed_protons = discover_proton_builds();
        let mut config = AppConfig::load_or_discover(&installed_protons);
        for pfx in &mut config.prefixes {
            for game in &mut pfx.games {
                if !game.desktop_managed && is_desktop_entry_installed(&pfx.id, &game.id) {
                    game.desktop_managed = true;
                }
            }
        }
        let selected_prefix = config
            .default_prefix_id
            .as_deref()
            .and_then(|id| config.prefixes.iter().position(|p| p.id == id))
            .unwrap_or(0);

        let initial_pfx = config.prefixes.get(selected_prefix);
        let init_gid = initial_pfx
            .map(|p| p.default_game_id.clone())
            .unwrap_or_else(|| "0".to_string());
        let init_dll = initial_pfx
            .map(|p| p.default_options.dll_overrides.clone())
            .unwrap_or_default();
        let init_env = initial_pfx
            .map(|p| format_extra_env(&p.default_options.extra_env))
            .unwrap_or_default();

        let default_game_id_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("0 or umu-...")
                .default_value(init_gid)
        });
        let dll_overrides_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. locationapi=d;winmm=n,b")
                .default_value(init_dll)
        });
        let extra_env_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("KEY=VAL, KEY2=VAL2")
                .default_value(init_env)
        });

        let (log_tx, log_rx): (Sender<String>, Receiver<String>) = std::sync::mpsc::channel();
        let running_prefixes = detect_running_prefixes(&config.prefixes);

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            config,
            selected_prefix,
            active_tab: MainTab::Prefixes,
            installed_protons,
            releases: Vec::new(),
            releases_loading: false,
            releases_error: None,
            download_state: None,
            running_prefixes,
            prefix_sizes: HashMap::new(),
            game_icons: HashMap::new(),
            ntsync_available: has_ntsync_device(),
            umu_version: detect_umu_version_label(),
            console_lines: vec![
                "[UmuBox] Ready — Kanban & Proton prefixes loaded.".to_string(),
            ],
            log_tx,
            default_game_id_input,
            dll_overrides_input,
            extra_env_input,
        };
        app.console_lines[0] = format!(
            "[UmuBox] Loaded {} prefix(es) & {} Proton build(s).",
            app.config.prefixes.len(),
            app.installed_protons.len()
        );

        app.refresh_prefix_sizes(cx);
        app.refresh_game_icons(false, cx);

        // Poll log receiver cleanly at 150ms & refresh live Wine processes every ~2.1s
        let rx = Arc::new(Mutex::new(log_rx));
        cx.spawn(async move |this, cx| {
            let mut tick = 0u64;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                tick = tick.wrapping_add(1);

                let mut batch = Vec::new();
                if let Ok(guard) = rx.lock() {
                    while let Ok(line) = guard.try_recv() {
                        batch.push(line);
                        if batch.len() >= 200 {
                            break;
                        }
                    }
                }

                let check_procs = tick.is_multiple_of(14);
                let prefixes_snap = if check_procs {
                    this.read_with(cx, |app, _| app.config.prefixes.clone()).ok()
                } else {
                    None
                };

                let new_running = if let Some(pfxs) = prefixes_snap {
                    Some(
                        cx.background_executor()
                            .spawn(async move { detect_running_prefixes(&pfxs) })
                            .await,
                    )
                } else {
                    None
                };

                let updated = this
                    .update(cx, |app, cx| {
                        let mut dirty = false;
                        if !batch.is_empty() {
                            app.console_lines.extend(batch);
                            if app.console_lines.len() > MAX_CONSOLE_LINES {
                                let excess = app.console_lines.len() - MAX_CONSOLE_LINES;
                                app.console_lines.drain(0..excess);
                            }
                            dirty = true;
                        }
                        if let Some(running) = new_running
                            && running != app.running_prefixes
                        {
                            app.running_prefixes = running;
                            dirty = true;
                        }
                        if dirty {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !updated {
                    break;
                }
            }
        })
        .detach();

        app
    }

    pub fn toast_ok(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(msg.into()), cx);
    }

    pub fn toast_err(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(msg.into()), cx);
    }

    pub fn append_log(&mut self, line: impl Into<String>, cx: &mut Context<Self>) {
        self.console_lines.push(line.into());
        if self.console_lines.len() > MAX_CONSOLE_LINES {
            let excess = self.console_lines.len() - MAX_CONSOLE_LINES;
            self.console_lines.drain(0..excess);
        }
        cx.notify();
    }

    pub fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.console_lines.clear();
        cx.notify();
    }

    pub fn refresh_prefix_sizes(&self, cx: &mut Context<Self>) {
        let targets: Vec<(String, PathBuf)> = self
            .config
            .prefixes
            .iter()
            .map(|p| (p.id.clone(), p.path.clone()))
            .collect();

        cx.spawn(async move |this, cx| {
            let sizes: HashMap<String, u64> = cx
                .background_executor()
                .spawn(async move {
                    targets
                        .into_iter()
                        .map(|(id, path)| (id, dir_size_bytes(&path)))
                        .collect()
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.prefix_sizes = sizes;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn refresh_game_icons(&self, force: bool, cx: &mut Context<Self>) {
        let targets: Vec<(String, GameEntry)> = self
            .config
            .prefixes
            .iter()
            .flat_map(|p| {
                let pid = p.id.clone();
                p.games.iter().cloned().map(move |g| (pid.clone(), g))
            })
            .collect();

        cx.spawn(async move |this, cx| {
            let loaded: HashMap<String, Arc<Image>> = cx
                .background_executor()
                .spawn(async move {
                    let mut map = HashMap::new();
                    for (pfx_id, game) in targets {
                        if let Ok(Some(path)) = sync_game_icon(&pfx_id, &game, force)
                            && let Some(img) = load_gpui_image_from_cached(&path)
                        {
                            map.insert(icon_key(&pfx_id, &game.id), img);
                        }
                    }
                    map
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.game_icons = loaded;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.installed_protons = discover_proton_builds();
        self.ntsync_available = has_ntsync_device();
        let pfxs = self.config.prefixes.clone();
        self.refresh_prefix_sizes(cx);
        self.refresh_game_icons(false, cx);

        cx.spawn(async move |this, cx| {
            let running = cx
                .background_executor()
                .spawn(async move { detect_running_prefixes(&pfxs) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.running_prefixes = running;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn select_prefix(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.config.prefixes.len() {
            return;
        }
        self.selected_prefix = idx;
        self.sync_prefix_inputs(window, cx);
        cx.notify();
    }

    pub fn sync_prefix_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pfx) = self.config.prefixes.get(self.selected_prefix) else {
            return;
        };
        let gid = pfx.default_game_id.clone();
        let dll = pfx.default_options.dll_overrides.clone();
        let env = format_extra_env(&pfx.default_options.extra_env);
        self.default_game_id_input
            .update(cx, |s, cx| s.set_value(gid, window, cx));
        self.dll_overrides_input
            .update(cx, |s, cx| s.set_value(dll, window, cx));
        self.extra_env_input
            .update(cx, |s, cx| s.set_value(env, window, cx));
    }

    pub fn create_prefix(
        &mut self,
        name: String,
        path: PathBuf,
        proton_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = fs::create_dir_all(&path) {
            self.toast_err(format!("Failed to create folder: {err}"), window, cx);
            return;
        }
        let canon = fs::canonicalize(&path).unwrap_or(path);
        let id = make_unique_id(&name, self.config.prefixes.iter().map(|p| p.id.as_str()));
        self.config.prefixes.push(PrefixConfig {
            id: id.clone(),
            name: name.clone(),
            path: canon,
            default_game_id: "0".to_string(),
            proton_path,
            default_options: LaunchOptions::default(),
            games: Vec::new(),
        });
        if self.config.default_prefix_id.is_none() {
            self.config.default_prefix_id = Some(id);
        }
        let _ = self.config.save();
        self.selected_prefix = self.config.prefixes.len() - 1;
        self.sync_prefix_inputs(window, cx);
        self.refresh_prefix_sizes(cx);
        self.toast_ok(format!("Created prefix '{name}'"), window, cx);
        cx.notify();
    }

    pub fn rename_prefix(
        &mut self,
        idx: usize,
        new_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pfx) = self.config.prefixes.get_mut(idx) {
            pfx.name = new_name;
            let pfx_snap = pfx.clone();
            let _ = self.config.save();
            for game in &pfx_snap.games {
                if game.desktop_managed {
                    let icon = cached_icon_path(&pfx_snap.id, &game.id);
                    let _ = sync_managed_desktop_entry(&pfx_snap, game, icon.as_deref());
                }
            }
            self.toast_ok("Renamed prefix", window, cx);
            cx.notify();
        }
    }

    pub fn delete_prefix(
        &mut self,
        idx: usize,
        delete_files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if idx >= self.config.prefixes.len() {
            return;
        }
        let removed = self.config.prefixes.remove(idx);
        for game in &removed.games {
            self.game_icons.remove(&icon_key(&removed.id, &game.id));
            remove_cached_icon(&removed.id, &game.id);
            let _ = remove_managed_desktop_entry(&removed.id, &game.id);
        }
        if self.config.default_prefix_id.as_deref() == Some(removed.id.as_str()) {
            self.config.default_prefix_id = self.config.prefixes.first().map(|p| p.id.clone());
        }
        if delete_files && removed.path.is_dir() {
            if let Err(err) = fs::remove_dir_all(&removed.path) {
                self.toast_err(format!("Removed from list, disk delete failed: {err}"), window, cx);
            } else {
                self.toast_ok(format!("Deleted prefix '{}' and files", removed.name), window, cx);
            }
        } else {
            self.toast_ok(format!("Removed prefix '{}'", removed.name), window, cx);
        }
        let _ = self.config.save();
        if self.selected_prefix >= self.config.prefixes.len() {
            self.selected_prefix = self.config.prefixes.len().saturating_sub(1);
        }
        self.sync_prefix_inputs(window, cx);
        cx.notify();
    }

    pub fn set_default_prefix(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pfx) = self.config.prefixes.get(idx) {
            self.config.default_prefix_id = Some(pfx.id.clone());
            let name = pfx.name.clone();
            let _ = self.config.save();
            self.toast_ok(format!("Default prefix set to '{name}'"), window, cx);
            cx.notify();
        }
    }

    pub fn set_prefix_proton(
        &mut self,
        idx: usize,
        proton_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pfx) = self.config.prefixes.get_mut(idx) {
            pfx.proton_path = proton_path;
            let _ = self.config.save();
            self.toast_ok("Updated prefix Proton build", window, cx);
            cx.notify();
        }
    }

    pub fn set_default_proton(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.default_proton = Some(path);
        let _ = self.config.save();
        self.toast_ok("Updated app default Proton build", window, cx);
        cx.notify();
    }

    pub fn save_game(
        &mut self,
        pfx_idx: usize,
        edit_idx: Option<usize>,
        mut entry: GameEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) else {
            return;
        };
        let (saved_idx, toast_msg) = if let Some(gi) = edit_idx
            && let Some(slot) = pfx.games.get_mut(gi)
        {
            if entry.id.is_empty() {
                entry.id = slot.id.clone();
            }
            *slot = entry;
            (gi, "Updated shortcut".to_string())
        } else {
            entry.id = make_unique_id(
                &entry.name,
                pfx.games.iter().map(|g| g.id.as_str()),
            );
            let gname = entry.name.clone();
            pfx.games.push(entry);
            (pfx.games.len() - 1, format!("Added '{gname}'"))
        };
        self.toast_ok(toast_msg, window, cx);
        let _ = self.config.save();
        self.sync_single_game_icon_and_desktop(pfx_idx, saved_idx, true, window, cx);
        cx.notify();
    }

    pub fn delete_game(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) else {
            return;
        };
        if game_idx < pfx.games.len() {
            let pfx_id = pfx.id.clone();
            let removed = pfx.games.remove(game_idx);
            self.game_icons.remove(&icon_key(&pfx_id, &removed.id));
            remove_cached_icon(&pfx_id, &removed.id);
            let _ = remove_managed_desktop_entry(&pfx_id, &removed.id);
            let _ = self.config.save();
            self.toast_ok(format!("Removed '{}'", removed.name), window, cx);
            cx.notify();
        }
    }

    pub fn launch_game(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        no_mangohud: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx) else {
            return;
        };
        let Some(game) = pfx.games.get(game_idx) else {
            return;
        };
        let mut opts = game.options.clone();
        if no_mangohud {
            opts.mangohud = false;
        }
        let env_pairs =
            build_launch_env(pfx, Some(game), &opts, self.config.default_proton.as_deref());
        let args = game_umu_args(game);
        let wd = game_working_dir(game);
        let label = game.name.clone();

        match spawn_umu_captured(label.clone(), args, wd, env_pairs, self.log_tx.clone()) {
            Ok(()) => {
                self.toast_ok(format!("Launched {label} (streaming to Live Console)"), window, cx);
                cx.notify();
            }
            Err(err) => {
                self.append_log(format!("[UmuBox] ERROR launching {label}: {err}"), cx);
                self.toast_err(format!("Launch failed: {err}"), window, cx);
            }
        }
    }

    pub fn sync_single_game_icon_and_desktop(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        force_icon: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx).cloned() else {
            return;
        };
        let Some(game) = pfx.games.get(game_idx).cloned() else {
            return;
        };
        let key = icon_key(&pfx.id, &game.id);
        let icon_path = match sync_game_icon(&pfx.id, &game, force_icon) {
            Ok(Some(path)) => {
                if let Some(img) = load_gpui_image_from_cached(&path) {
                    self.game_icons.insert(key, img);
                }
                Some(path)
            }
            Ok(None) => {
                self.game_icons.remove(&key);
                None
            }
            Err(err) => {
                self.append_log(
                    format!("[UmuBox] ERROR extracting/setting icon for {}: {err}", game.name),
                    cx,
                );
                self.toast_err(format!("Icon error for {}: {err}", game.name), window, cx);
                cached_icon_path(&pfx.id, &game.id)
            }
        };

        if game.desktop_managed {
            match sync_managed_desktop_entry(&pfx, &game, icon_path.as_deref()) {
                Ok(dest) => {
                    self.append_log(
                        format!("[UmuBox] Synced app menu entry: {}", dest.display()),
                        cx,
                    );
                }
                Err(err) => {
                    self.append_log(
                        format!("[UmuBox] ERROR syncing desktop entry: {err}"),
                        cx,
                    );
                    self.toast_err(format!("Desktop sync failed: {err}"), window, cx);
                }
            }
        } else {
            let _ = remove_managed_desktop_entry(&pfx.id, &game.id);
        }
        cx.notify();
    }

    pub fn toggle_game_desktop_entry(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) else {
            return;
        };
        let Some(game) = pfx.games.get_mut(game_idx) else {
            return;
        };
        game.desktop_managed = !game.desktop_managed;
        let now_managed = game.desktop_managed;
        let game_name = game.name.clone();
        let _ = self.config.save();

        self.sync_single_game_icon_and_desktop(pfx_idx, game_idx, false, window, cx);
        if now_managed {
            self.toast_ok(format!("Added '{game_name}' to KDE App Menu"), window, cx);
        } else {
            self.append_log(
                format!("[UmuBox] Removed app menu entry for {game_name}"),
                cx,
            );
            self.toast_ok(format!("Removed '{game_name}' from KDE App Menu"), window, cx);
        }
    }

    pub fn prompt_pick_game_icon(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select Custom Icon (.png, .svg, .ico, .jpg, .webp, .exe)".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(icon_path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, window, cx| {
                    if let Some(pfx) = app.config.prefixes.get_mut(pfx_idx)
                        && let Some(game) = pfx.games.get_mut(game_idx)
                    {
                        game.icon_override = Some(icon_path);
                        let gname = game.name.clone();
                        let _ = app.config.save();
                        app.sync_single_game_icon_and_desktop(pfx_idx, game_idx, true, window, cx);
                        app.toast_ok(format!("Updated custom icon for '{gname}'"), window, cx);
                    }
                });
            }
        })
        .detach();
    }

    pub fn clear_game_icon_override(
        &mut self,
        pfx_idx: usize,
        game_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) else {
            return;
        };
        let Some(game) = pfx.games.get_mut(game_idx) else {
            return;
        };
        game.icon_override = None;
        let gname = game.name.clone();
        let _ = self.config.save();
        self.sync_single_game_icon_and_desktop(pfx_idx, game_idx, true, window, cx);
        self.toast_ok(format!("Reverted '{gname}' to .exe icon"), window, cx);
    }

    pub fn sync_all_managed_desktop_entries_ui(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pfxs = self.config.prefixes.clone();
        let mut count = 0usize;
        for (pi, pfx) in pfxs.iter().enumerate() {
            for (gi, game) in pfx.games.iter().enumerate() {
                if game.desktop_managed || is_desktop_entry_installed(&pfx.id, &game.id) {
                    if !game.desktop_managed
                        && let Some(p_mut) = self.config.prefixes.get_mut(pi)
                        && let Some(g_mut) = p_mut.games.get_mut(gi)
                    {
                        g_mut.desktop_managed = true;
                        let _ = self.config.save();
                    }
                    self.sync_single_game_icon_and_desktop(pi, gi, false, window, cx);
                    count += 1;
                }
            }
        }
        self.toast_ok(format!("Synced {count} managed app menu entry(ies)"), window, cx);
    }

    pub fn run_tool_on_prefix(
        &mut self,
        pfx_idx: usize,
        tool: PrefixTool,
        extra_args: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx) else {
            return;
        };
        let env_pairs = build_launch_env(
            pfx,
            None,
            &pfx.default_options,
            self.config.default_proton.as_deref(),
        );
        let wd = match &tool {
            PrefixTool::RunExe(p) => p
                .parent()
                .filter(|d| d.is_dir())
                .map(PathBuf::from)
                .or_else(|| Some(pfx.path.clone())),
            _ => Some(pfx.path.clone()),
        };
        let umu_args = tool.umu_args(&extra_args);
        let label = format!("{} ({})", tool.label(), pfx.name);

        match spawn_umu_captured(label.clone(), umu_args, wd, env_pairs, self.log_tx.clone()) {
            Ok(()) => {
                self.toast_ok(format!("Started {label} — check Live Console"), window, cx);
                cx.notify();
            }
            Err(err) => {
                self.append_log(format!("[UmuBox] ERROR running {label}: {err}"), cx);
                self.toast_err(format!("Failed to start tool: {err}"), window, cx);
            }
        }
    }

    pub fn prompt_run_exe_in_prefix(
        &mut self,
        pfx_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select Windows .exe / .msi to Run in Prefix".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(exe) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |app, window, cx| {
                    app.run_tool_on_prefix(pfx_idx, PrefixTool::RunExe(exe), Vec::new(), window, cx);
                });
            }
        })
        .detach();
    }

    pub fn kill_prefix_now(
        &mut self,
        pfx_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx) else {
            return;
        };
        match kill_prefix(pfx, &self.installed_protons) {
            Ok(msg) => {
                self.append_log(format!("[UmuBox] {msg}"), cx);
                self.toast_ok(msg, window, cx);
                self.refresh_all(cx);
            }
            Err(err) => {
                self.append_log(format!("[UmuBox] ERROR killing prefix: {err}"), cx);
                self.toast_err(format!("{err}"), window, cx);
            }
        }
    }

    pub fn clear_prefix_shader_cache(
        &mut self,
        pfx_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx) else {
            return;
        };
        match clear_shader_cache(&pfx.path) {
            Ok(freed) => {
                let msg = format!(
                    "Cleared {} shader cache in '{}'",
                    format_bytes(freed),
                    pfx.name
                );
                self.append_log(format!("[UmuBox] {msg}"), cx);
                self.toast_ok(msg, window, cx);
                self.refresh_prefix_sizes(cx);
            }
            Err(err) => self.toast_err(format!("{err}"), window, cx),
        }
    }

    pub fn open_prefix_drive_c(
        &mut self,
        pfx_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pfx) = self.config.prefixes.get(pfx_idx) else {
            return;
        };
        let target = if pfx.path.join("drive_c").is_dir() {
            pfx.path.join("drive_c")
        } else {
            pfx.path.clone()
        };
        if let Err(err) = open_in_file_manager(&target) {
            self.toast_err(format!("{err}"), window, cx);
        }
    }

    pub fn update_prefix_option(
        &mut self,
        pfx_idx: usize,
        f: impl FnOnce(&mut LaunchOptions),
        cx: &mut Context<Self>,
    ) {
        if let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) {
            f(&mut pfx.default_options);
            let _ = self.config.save();
            cx.notify();
        }
    }

    pub fn apply_prefix_text_fields(
        &mut self,
        pfx_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let gid = self.default_game_id_input.read(cx).value().trim().to_string();
        let dll = self.dll_overrides_input.read(cx).value().trim().to_string();
        let env_raw = self.extra_env_input.read(cx).value().to_string();
        let extra = parse_extra_env(&env_raw);

        if let Some(pfx) = self.config.prefixes.get_mut(pfx_idx) {
            pfx.default_game_id = if gid.is_empty() { "0".to_string() } else { gid };
            pfx.default_options.dll_overrides = dll;
            pfx.default_options.extra_env = extra;
            let _ = self.config.save();
            self.toast_ok("Saved prefix environment settings", window, cx);
            cx.notify();
        }
    }

    pub fn check_ge_proton_releases(&mut self, cx: &mut Context<Self>) {
        if self.releases_loading {
            return;
        }
        self.releases_loading = true;
        self.releases_error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { fetch_ge_proton_releases(15) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.releases_loading = false;
                match res {
                    Ok(list) => {
                        app.append_log(
                            format!(
                                "[UmuBox] Fetched {} GE-Proton release(s) from GitHub.",
                                list.len()
                            ),
                            cx,
                        );
                        app.releases = list;
                        app.releases_error = None;
                    }
                    Err(err) => {
                        let msg = format!("{err}");
                        app.append_log(format!("[UmuBox] ERROR checking GE-Proton: {msg}"), cx);
                        app.releases_error = Some(msg);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn install_ge_release(
        &mut self,
        release: GeProtonRelease,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.download_state.is_some() {
            self.toast_err("Another GE-Proton download is already in progress.", window, cx);
            return;
        }
        let tag = release.tag.clone();
        self.download_state = Some((
            tag.clone(),
            DownloadProgress {
                phase: DownloadPhase::Downloading,
                downloaded_bytes: 0,
                total_bytes: release.tarball_size,
                message: format!("Starting {}…", release.tarball_name),
            },
        ));
        self.append_log(format!("[UmuBox] Installing GE-Proton {tag}…"), cx);
        cx.notify();

        let progress_shared = Arc::new(Mutex::new(None::<DownloadProgress>));
        let result_shared = Arc::new(Mutex::new(None::<anyhow::Result<PathBuf>>));
        let p_bg = Arc::clone(&progress_shared);
        let r_bg = Arc::clone(&result_shared);

        thread::spawn(move || {
            let res = install_ge_proton_release(&release, |p| {
                if let Ok(mut g) = p_bg.lock() {
                    *g = Some(p);
                }
            });
            if let Ok(mut g) = r_bg.lock() {
                *g = Some(res);
            }
        });

        cx.spawn_in(window, async move |this, cx| {
            let mut last_logged_phase: Option<DownloadPhase> = None;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(160))
                    .await;

                let prog_opt = progress_shared.lock().ok().and_then(|g| g.clone());
                let res_opt = result_shared.lock().ok().and_then(|mut g| g.take());

                let still_alive = this
                    .update_in(cx, |app, window, cx| {
                        if let Some(p) = prog_opt {
                            if last_logged_phase != Some(p.phase) {
                                last_logged_phase = Some(p.phase);
                                app.append_log(format!("[UmuBox] {}", p.message), cx);
                            }
                            app.download_state = Some((tag.clone(), p));
                        }
                        if let Some(res) = res_opt {
                            app.download_state = None;
                            app.installed_protons = discover_proton_builds();
                            match res {
                                Ok(installed_path) => {
                                    let msg = format!(
                                        "Installed {} to {}",
                                        tag,
                                        installed_path.display()
                                    );
                                    app.append_log(format!("[UmuBox] {msg}"), cx);
                                    app.toast_ok(msg, window, cx);
                                }
                                Err(err) => {
                                    let msg = format!("Failed to install {tag}: {err}");
                                    app.append_log(format!("[UmuBox] ERROR: {msg}"), cx);
                                    app.toast_err(msg, window, cx);
                                }
                            }
                            cx.notify();
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);

                if !still_alive {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn remove_proton_build(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match remove_user_proton_build(&path) {
            Ok(()) => {
                if self.config.default_proton.as_ref() == Some(&path) {
                    self.config.default_proton = None;
                }
                for pfx in &mut self.config.prefixes {
                    if pfx.proton_path.as_ref() == Some(&path) {
                        pfx.proton_path = None;
                    }
                    for g in &mut pfx.games {
                        if g.proton_override.as_ref() == Some(&path) {
                            g.proton_override = None;
                        }
                    }
                }
                self.installed_protons = discover_proton_builds();
                if self.config.default_proton.is_none() {
                    self.config.default_proton =
                        crate::config::pick_best_default_proton(&self.installed_protons);
                }
                let _ = self.config.save();
                self.toast_ok("Removed Proton build", window, cx);
                cx.notify();
            }
            Err(err) => self.toast_err(format!("{err}"), window, cx),
        }
    }
}

impl Render for UmuBoxApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app_bg = hex_color(kd::BG_MAIN);
        let deep_bg = hex_color(kd::BG_DEEP);
        let border = hex_color(kd::BORDER_SUBTLE);
        let running_count = self.running_prefixes.values().filter(|v| !v.is_empty()).count();
        let has_ge_update = self
            .releases
            .first()
            .is_some_and(|r| !is_release_installed(&r.tag, &self.installed_protons));

        let top_bar = h_flex()
            .h(px(44.))
            .w_full()
            .px_3()
            .gap_3()
            .items_center()
            .justify_between()
            .bg(deep_bg)
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .bg(hex_color(kd::BG_PANEL))
                            .items_center()
                            .child(div().w(px(8.)).h(px(8.)).rounded(px(2.)).bg(hex_color(kd::BLUE)))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_color(hex_color(kd::BLUE))
                                    .child("UmuBox"),
                            ),
                    )
                    .child(
                        Button::new("nav-prefixes")
                            .with_variant(if self.active_tab == MainTab::Prefixes {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .small()
                            .icon(IconName::Box)
                            .label("Prefixes & Games")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_tab = MainTab::Prefixes;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("nav-proton")
                            .with_variant(if self.active_tab == MainTab::Proton {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .small()
                            .icon(IconName::Rocket)
                            .label(if has_ge_update {
                                "Proton Builds ●"
                            } else {
                                "Proton Builds"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_tab = MainTab::Proton;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("nav-console")
                            .with_variant(if self.active_tab == MainTab::Console {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .small()
                            .icon(IconName::Terminal)
                            .label(format!("Live Console ({})", self.console_lines.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_tab = MainTab::Console;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .when(running_count > 0, |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(hex_alpha(0x87a9872b))
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(hex_color(kd::GREEN))
                                .child(format!("{running_count} active prefix")),
                        )
                    })
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(hex_color(kd::BG_PANEL))
                            .text_xs()
                            .text_color(if self.ntsync_available {
                                hex_color(kd::GREEN)
                            } else {
                                hex_color(kd::FG_MUTED)
                            })
                            .child(if self.ntsync_available {
                                "NTSync"
                            } else {
                                "FSync/ESync"
                            }),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(hex_color(kd::BG_PANEL))
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .child(self.umu_version.clone()),
                    )
                    .child(
                        Button::new("top-refresh")
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .tooltip("Refresh Prefixes, Protons & Processes (Ctrl+R)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_all(cx);
                            })),
                    ),
            );

        let body = match self.active_tab {
            MainTab::Prefixes => h_resizable("umubox-main-split")
                .child(
                    resizable_panel()
                        .size(px(265.))
                        .size_range(px(200.)..px(420.))
                        .child(sidebar::render_sidebar(self, cx)),
                )
                .child(resizable_panel().child(prefix_view::render_prefix_view(self, cx)))
                .into_any_element(),
            MainTab::Proton => proton_view::render_proton_view(self, cx).into_any_element(),
            MainTab::Console => console_view::render_console_view(self, cx).into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(app_bg)
            .text_color(hex_color(kd::FG))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &RefreshState, _, cx| {
                this.refresh_all(cx);
            }))
            .on_action(cx.listener(|this, _: &TabPrefixes, _, cx| {
                this.active_tab = MainTab::Prefixes;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TabProton, _, cx| {
                this.active_tab = MainTab::Proton;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TabConsole, _, cx| {
                this.active_tab = MainTab::Console;
                cx.notify();
            }))
            .child(top_bar)
            .child(div().flex_1().min_h_0().w_full().child(body))
    }
}
