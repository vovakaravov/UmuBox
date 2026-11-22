use super::{UmuBoxApp, modals};
use crate::icons::icon_key;
use crate::runner::{PrefixTool, format_bytes, is_desktop_entry_installed};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, FontWeight, ImageSource, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, div,
    img, prelude::FluentBuilder as _, px,
};
use std::fs;

pub fn render_prefix_view(app: &UmuBoxApp, cx: &mut Context<UmuBoxApp>) -> impl IntoElement {
    let Some(pfx) = app.config.prefixes.get(app.selected_prefix) else {
        return div()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .p_6()
            .text_sm()
            .text_color(hex_color(kd::FG_MUTED))
            .child("No Wine/Proton prefixes found. Click '+ New' in the sidebar to create one.")
            .into_any_element();
    };

    let idx = app.selected_prefix;
    let border = hex_color(kd::BORDER_SUBTLE);
    let panel_bg = hex_color(kd::BG_SIDEBAR);
    let card_bg = hex_color(kd::BG_PANEL);
    let app_entity = cx.entity();

    let running_procs = app
        .running_prefixes
        .get(&pfx.id)
        .cloned()
        .unwrap_or_default();
    let is_running = !running_procs.is_empty();
    let size_text = app
        .prefix_sizes
        .get(&pfx.id)
        .map(|b| format_bytes(*b))
        .unwrap_or_else(|| "Calculating…".to_string());
    let is_default_pfx = app.config.default_prefix_id.as_deref() == Some(pfx.id.as_str());

    // Header Card
    let header_card = v_flex()
        .w_full()
        .p_4()
        .gap_3()
        .rounded(px(10.))
        .bg(panel_bg)
        .border_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_start()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .min_w_0()
                        .flex_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::FG))
                                        .child(pfx.name.clone()),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(if is_running {
                                            hex_alpha(0x87a9872b)
                                        } else {
                                            card_bg
                                        })
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(if is_running {
                                            hex_color(kd::GREEN)
                                        } else {
                                            hex_color(kd::FG_MUTED)
                                        })
                                        .child(if is_running {
                                            format!("RUNNING ({} proc)", running_procs.len())
                                        } else {
                                            "IDLE".to_string()
                                        }),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(card_bg)
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_SUBTLE))
                                        .child(size_text),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .truncate()
                                .child(pfx.path.display().to_string()),
                        )
                        .when(is_running, |this| {
                            let proc_list: Vec<String> = running_procs
                                .iter()
                                .take(5)
                                .map(|p| format!("{} ({})", p.comm, p.pid))
                                .collect();
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::GREEN))
                                    .child(format!("Active processes: {}", proc_list.join(" · "))),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .flex_shrink_0()
                        .when(!is_default_pfx, |this| {
                            this.child(
                                Button::new("pfx-set-default")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Check)
                                    .label("Set Default")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_default_prefix(idx, window, cx);
                                    })),
                            )
                        })
                        .child(
                            Button::new("pfx-open-drive-c")
                                .outline()
                                .small()
                                .icon(IconName::FolderOpen)
                                .label("drive_c")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_prefix_drive_c(idx, window, cx);
                                })),
                        )
                        .child(
                            Button::new("pfx-kill-btn")
                                .danger()
                                .small()
                                .icon(IconName::Power)
                                .label("Force Kill")
                                .tooltip("Run wineserver -k for this prefix")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.kill_prefix_now(idx, window, cx);
                                })),
                        ),
                ),
        )
        // Proton selection strip for this prefix
        .child(
            v_flex()
                .gap_1p5()
                .pt_2()
                .border_t_1()
                .border_color(border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(kd::FG_SUBTLE))
                        .child("Proton Build"),
                )
                .child({
                    let inherit_selected = pfx.proton_path.is_none();
                    let builds = app.installed_protons.iter().enumerate().map(|(b_ix, b)| {
                        let sel = pfx.proton_path.as_ref().is_some_and(|p| {
                            p == &b.path
                                || fs::canonicalize(p).ok().as_ref() == Some(&b.canonical_path)
                        });
                        let bp = b.path.clone();
                        Button::new(SharedString::from(format!("pfx-proton-{b_ix}")))
                            .with_variant(if sel {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label(format!("{} ({})", b.name, b.version_tag))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_prefix_proton(idx, Some(bp.clone()), window, cx);
                            }))
                    });

                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            Button::new("pfx-proton-inherit")
                                .with_variant(if inherit_selected {
                                    ButtonVariant::Primary
                                } else {
                                    ButtonVariant::Ghost
                                })
                                .xsmall()
                                .label("Inherit App Default")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_prefix_proton(idx, None, window, cx);
                                })),
                        )
                        .children(builds)
                }),
        );

    // Games & Shortcuts section
    let game_cards = pfx.games.iter().enumerate().map(|(g_ix, game)| {
        let g_edit = game.clone();
        let g_name = game.name.clone();
        let pfx_path_for_dlg = pfx.path.clone();
        let builds_for_dlg = app.installed_protons.clone();
        let app_for_edit = app_entity.clone();
        let app_icon = app.game_icons.get(&icon_key(&pfx.id, &game.id)).cloned();
        let app_icon_for_dlg = app_icon.clone();
        let has_custom_icon = game.icon_override.is_some();
        let in_menu = game.desktop_managed || is_desktop_entry_installed(&pfx.id, &game.id);

        h_flex()
            .w_full()
            .p_3()
            .rounded(px(8.))
            .bg(card_bg)
            .border_1()
            .border_color(border)
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("game-icon-tile-{g_ix}")))
                            .w(px(40.))
                            .h(px(40.))
                            .flex_shrink_0()
                            .rounded(px(8.))
                            .bg(panel_bg)
                            .border_1()
                            .border_color(if has_custom_icon {
                                hex_color(kd::BLUE)
                            } else {
                                border
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.prompt_pick_game_icon(idx, g_ix, window, cx);
                            }))
                            .child(if let Some(arc_img) = app_icon {
                                img(ImageSource::Image(arc_img))
                                    .size(px(34.))
                                    .object_fit(ObjectFit::Contain)
                                    .into_any_element()
                            } else {
                                Icon::new(IconName::Gamepad2)
                                    .size(px(20.))
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .into_any_element()
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .min_w_0()
                            .flex_1()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_wrap()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(hex_color(kd::FG))
                                            .child(g_name),
                                    )
                                    .when(in_menu, |this| {
                                        this.child(tag_pill("App Menu", hex_color(kd::GREEN)))
                                    })
                                    .when(has_custom_icon, |this| {
                                        this.child(tag_pill("Custom Icon", hex_color(kd::BLUE)))
                                    })
                                    .when(!game.game_id.is_empty() && game.game_id != "0", |this| {
                                        this.child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(4.))
                                                .bg(panel_bg)
                                                .text_size(px(10.))
                                                .text_color(hex_color(kd::AQUA))
                                                .child(game.game_id.clone()),
                                        )
                                    })
                                    .when(game.options.mangohud, |this| {
                                        this.child(tag_pill("MangoHud", hex_color(kd::GREEN)))
                                    })
                                    .when(game.options.nvidia_prime_offload, |this| {
                                        this.child(tag_pill("RTX dGPU", hex_color(kd::YELLOW)))
                                    })
                                    .when(game.options.wayland_driver, |this| {
                                        this.child(tag_pill("Wayland", hex_color(kd::BLUE)))
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .truncate()
                                    .child(if game.args.is_empty() {
                                        game.exe_path.display().to_string()
                                    } else {
                                        format!("{} {}", game.exe_path.display(), game.args)
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .flex_shrink_0()
                    .child(
                        Button::new(SharedString::from(format!("game-launch-{g_ix}")))
                            .primary()
                            .small()
                            .icon(IconName::Play)
                            .label("Launch")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.launch_game(idx, g_ix, false, window, cx);
                            })),
                    )
                    .child(if in_menu {
                        Button::new(SharedString::from(format!("game-desktop-{g_ix}")))
                            .outline()
                            .xsmall()
                            .icon(IconName::AppWindow)
                            .label("In Menu")
                            .tooltip("Managed in KDE App Menu / KRunner — click to remove")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_game_desktop_entry(idx, g_ix, window, cx);
                            }))
                    } else {
                        Button::new(SharedString::from(format!("game-desktop-{g_ix}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::AppWindow)
                            .label("Add to Menu")
                            .tooltip("Create & sync KDE App Menu / KRunner .desktop shortcut with app icon")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_game_desktop_entry(idx, g_ix, window, cx);
                            }))
                    })
                    .when(has_custom_icon, |this| {
                        this.child(
                            Button::new(SharedString::from(format!("game-icon-reset-{g_ix}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCcw)
                                .tooltip("Clear Custom Icon & Inherit from .exe")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.clear_game_icon_override(idx, g_ix, window, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("game-edit-{g_ix}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Pencil)
                            .tooltip("Edit Game Options & Icon")
                            .on_click(move |_, window, cx| {
                                modals::open_game_dialog(
                                    app_for_edit.clone(),
                                    idx,
                                    pfx_path_for_dlg.clone(),
                                    builds_for_dlg.clone(),
                                    Some((g_ix, g_edit.clone())),
                                    app_icon_for_dlg.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("game-del-{g_ix}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Trash)
                            .tooltip("Remove Shortcut")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.delete_game(idx, g_ix, window, cx);
                            })),
                    ),
            )
    });

    let games_section = v_flex()
        .w_full()
        .p_4()
        .gap_3()
        .rounded(px(10.))
        .bg(panel_bg)
        .border_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::FG))
                        .child(format!("Games & Shortcuts ({})", pfx.games.len())),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            Button::new("sync-all-menu-btn")
                                .ghost()
                                .xsmall()
                                .icon(IconName::RefreshCw)
                                .label("Sync App Menu")
                                .tooltip("Re-sync icons & .desktop menu entries for all managed games")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.sync_all_managed_desktop_entries_ui(window, cx);
                                })),
                        )
                        .child(
                            Button::new("add-game-btn")
                                .outline()
                                .xsmall()
                                .icon(IconName::Plus)
                                .label("Add Game / .exe")
                                .on_click({
                                    let app_e = app_entity.clone();
                                    let pfx_path = pfx.path.clone();
                                    let builds = app.installed_protons.clone();
                                    move |_, window, cx| {
                                        modals::open_game_dialog(
                                            app_e.clone(),
                                            idx,
                                            pfx_path.clone(),
                                            builds.clone(),
                                            None,
                                            None,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        ),
                ),
        )
        .child(if pfx.games.is_empty() {
            div()
                .p_3()
                .rounded(px(6.))
                .bg(card_bg)
                .text_xs()
                .text_color(hex_color(kd::FG_MUTED))
                .child("No shortcuts pinned to this prefix yet. Click 'Add Game / .exe' above or use 'Run .exe / .msi…' below.")
                .into_any_element()
        } else {
            v_flex().gap_2().children(game_cards).into_any_element()
        });

    // Quick Prefix Tools section
    let tools_section = v_flex()
        .w_full()
        .p_4()
        .gap_3()
        .rounded(px(10.))
        .bg(panel_bg)
        .border_1()
        .border_color(border)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(hex_color(kd::FG))
                .child("Prefix Tools"),
        )
        .child(
            h_flex()
                .gap_1p5()
                .flex_wrap()
                .child(
                    Button::new("tool-run-exe")
                        .primary()
                        .xsmall()
                        .icon(IconName::Play)
                        .label("Run .exe / .msi…")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.prompt_run_exe_in_prefix(idx, window, cx);
                        })),
                )
                .child(tool_btn("tool-winecfg", PrefixTool::Winecfg, IconName::Settings, idx, cx))
                .child(tool_btn("tool-regedit", PrefixTool::Regedit, IconName::FileText, idx, cx))
                .child(tool_btn("tool-taskmgr", PrefixTool::Taskmgr, IconName::Cpu, idx, cx))
                .child(tool_btn("tool-cmd", PrefixTool::Cmd, IconName::Terminal, idx, cx))
                .child(tool_btn(
                    "tool-wineboot",
                    PrefixTool::WinebootUpdate,
                    IconName::RefreshCw,
                    idx,
                    cx,
                ))
                .child(
                    Button::new("tool-winetricks")
                        .outline()
                        .xsmall()
                        .icon(IconName::Wrench)
                        .label("Winetricks…")
                        .on_click({
                            let app_e = app_entity.clone();
                            let pfx_name = pfx.name.clone();
                            move |_, window, cx| {
                                modals::open_winetricks_dialog(
                                    app_e.clone(),
                                    idx,
                                    pfx_name.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .child(
                    Button::new("tool-clear-shader")
                        .outline()
                        .xsmall()
                        .icon(IconName::Trash)
                        .label("Clear Shader Cache")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.clear_prefix_shader_cache(idx, window, cx);
                        })),
                ),
        );

    // Prefix Default Runtime Options
    let opts = pfx.default_options.clone();
    let options_section = v_flex()
        .w_full()
        .p_4()
        .gap_3()
        .rounded(px(10.))
        .bg(panel_bg)
        .border_1()
        .border_color(border)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::FG))
                        .child("Prefix Default Runtime Options"),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(4.))
                        .bg(card_bg)
                        .text_xs()
                        .text_color(if app.ntsync_available {
                            hex_color(kd::GREEN)
                        } else {
                            hex_color(kd::FG_MUTED)
                        })
                        .child(if app.ntsync_available {
                            "/dev/ntsync detected"
                        } else {
                            "/dev/ntsync unavailable"
                        }),
                ),
        )
        .child(
            h_flex()
                .gap_4()
                .flex_wrap()
                .child(
                    Switch::new("opt-mangohud")
                        .checked(opts.mangohud)
                        .label("MangoHud Overlay")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.mangohud = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-prime")
                        .checked(opts.nvidia_prime_offload)
                        .label("RTX dGPU Offload")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.nvidia_prime_offload = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-wayland")
                        .checked(opts.wayland_driver)
                        .label("Native Wayland (PROTON_ENABLE_WAYLAND)")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.wayland_driver = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-ntsync")
                        .checked(opts.ntsync)
                        .label("NTSync")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.ntsync = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-nvapi")
                        .checked(opts.nvapi)
                        .label("NVAPI / DLSS")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.nvapi = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-dxvk-hud")
                        .checked(opts.dxvk_hud)
                        .label("DXVK HUD")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.dxvk_hud = val, cx);
                        })),
                )
                .child(
                    Switch::new("opt-shadercache")
                        .checked(opts.shader_cache)
                        .label("10 GiB Shader Cache in Prefix")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let val = *checked;
                            this.update_prefix_option(idx, |o| o.shader_cache = val, cx);
                        })),
                ),
        )
        .child(
            v_flex()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            v_flex()
                                .w(px(220.))
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("Default GAMEID"),
                                )
                                .child(Input::new(&app.default_game_id_input)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("WINEDLLOVERRIDES"),
                                )
                                .child(Input::new(&app.dll_overrides_input)),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child("Custom Env Vars (comma or space separated KEY=VALUE)"),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().flex_1().child(Input::new(&app.extra_env_input)))
                                .child(
                                    Button::new("save-prefix-env-btn")
                                        .primary()
                                        .small()
                                        .icon(IconName::Check)
                                        .label("Save Env")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.apply_prefix_text_fields(idx, window, cx);
                                        })),
                                ),
                        ),
                ),
        );

    div()
        .id("prefix-view-scroll")
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .overflow_y_scrollbar()
        .p_5()
        .child(
            v_flex()
                .max_w(px(1080.))
                .mx_auto()
                .gap_4()
                .child(header_card)
                .child(games_section)
                .child(tools_section)
                .child(options_section),
        )
        .into_any_element()
}

fn tag_pill(label: &'static str, color: gpui_kit::Hsla) -> impl IntoElement {
    div()
        .px_1p5()
        .py_0p5()
        .rounded(px(4.))
        .bg(hex_color(kd::BG_SIDEBAR))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(label)
}

fn tool_btn(
    id: &'static str,
    tool: PrefixTool,
    icon: IconName,
    pfx_idx: usize,
    cx: &mut Context<UmuBoxApp>,
) -> Button {
    let label = tool.label();
    Button::new(id)
        .outline()
        .xsmall()
        .icon(icon)
        .label(label)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.run_tool_on_prefix(pfx_idx, tool.clone(), Vec::new(), window, cx);
        }))
}
