use super::UmuBoxApp;
use crate::config::{GameEntry, LaunchOptions, format_extra_env, parse_extra_env, slugify_id};
use crate::icons::load_gpui_preview_for_source;
use crate::proton::ProtonBuild;
use crate::runner::PrefixTool;
use crate::theme::{accent_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, Focusable as _, Image, ImageSource, IntoElement, ObjectFit,
    ParentElement, PathPromptOptions, SharedString, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub fn open_new_prefix_dialog(
    app_entity: Entity<UmuBoxApp>,
    prefixes_root: PathBuf,
    installed_protons: Vec<ProtonBuild>,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Cyberpunk, Mods, Tools"));
    let custom_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let selected_proton: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }

        let cur_name = name_input.read(cx).value().trim().to_string();
        let slug = slugify_id(if cur_name.is_empty() {
            "new-prefix"
        } else {
            &cur_name
        });
        let display_path = custom_path
            .borrow()
            .clone()
            .unwrap_or_else(|| prefixes_root.join(&slug));
        let cur_proton = selected_proton.borrow().clone();

        let pick_path = custom_path.clone();
        let app_pick = app_entity.clone();
        let input_ok = name_input.clone();
        let path_ok = custom_path.clone();
        let proton_ok = selected_proton.clone();
        let root_ok = prefixes_root.clone();
        let app_ok = app_entity.clone();

        let proton_pills = installed_protons.iter().enumerate().map(|(ix, b)| {
            let is_sel = cur_proton.as_ref() == Some(&b.path);
            let p_state = selected_proton.clone();
            let bp = b.path.clone();
            let app_r = app_entity.clone();
            Button::new(SharedString::from(format!("new-pfx-prot-{ix}")))
                .with_variant(if is_sel {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label(b.name.clone())
                .on_click(move |_, window, cx| {
                    *p_state.borrow_mut() = Some(bp.clone());
                    window.refresh();
                    cx.notify(app_r.entity_id());
                })
        });

        let inherit_sel = cur_proton.is_none();
        let p_inherit = selected_proton.clone();
        let app_inherit = app_entity.clone();

        dialog
            .title("Create Wine / Proton Prefix")
            .w(px(540.))
            .child(
                v_flex()
                    .gap_3()
                    .child(field_block("Prefix Name", Input::new(&name_input)))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("Prefix Directory"))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .rounded(px(6.))
                                            .bg(hex_color(kd::BG_PANEL))
                                            .border_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .text_xs()
                                            .text_color(hex_color(kd::FG))
                                            .truncate()
                                            .child(display_path.display().to_string()),
                                    )
                                    .child(
                                        Button::new("new-pfx-browse")
                                            .small()
                                            .outline()
                                            .icon(IconName::FolderOpen)
                                            .label("Choose…")
                                            .on_click(move |_, window, cx| {
                                                let rx = cx.prompt_for_paths(PathPromptOptions {
                                                    files: false,
                                                    directories: true,
                                                    multiple: false,
                                                    prompt: Some("Select Prefix Folder".into()),
                                                });
                                                let st = pick_path.clone();
                                                let vid = app_pick.entity_id();
                                                window
                                                    .spawn(cx, async move |cx| {
                                                        if let Ok(Ok(Some(paths))) = rx.await
                                                            && let Some(dir) =
                                                                paths.into_iter().next()
                                                        {
                                                            *st.borrow_mut() = Some(dir);
                                                            let _ = cx.update(|window, cx| {
                                                                window.refresh();
                                                                cx.notify(vid);
                                                            });
                                                        }
                                                    })
                                                    .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("Proton Build"))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .flex_wrap()
                                    .child(
                                        Button::new("new-pfx-prot-inherit")
                                            .with_variant(if inherit_sel {
                                                ButtonVariant::Primary
                                            } else {
                                                ButtonVariant::Ghost
                                            })
                                            .xsmall()
                                            .label("Inherit Default")
                                            .on_click(move |_, window, cx| {
                                                *p_inherit.borrow_mut() = None;
                                                window.refresh();
                                                cx.notify(app_inherit.entity_id());
                                            }),
                                    )
                                    .children(proton_pills),
                            ),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-pfx-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-pfx-ok")
                            .primary()
                            .label("Create Prefix")
                            .on_click(move |_, window, cx| {
                                let name = input_ok.read(cx).value().trim().to_string();
                                if name.is_empty() {
                                    return;
                                }
                                let chosen = path_ok
                                    .borrow()
                                    .clone()
                                    .unwrap_or_else(|| root_ok.join(slugify_id(&name)));
                                let prot = proton_ok.borrow().clone();
                                window.close_dialog(cx);
                                app_ok.update(cx, |app, cx| {
                                    app.create_prefix(name, chosen, prot, window, cx);
                                });
                            }),
                    ),
            )
    });
}

pub fn open_rename_prefix_dialog(
    app_entity: Entity<UmuBoxApp>,
    pfx_idx: usize,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input = cx.new(|cx| InputState::new(window, cx).default_value(current_name.clone()));
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }
        let input_ok = name_input.clone();
        let app_ok = app_entity.clone();

        dialog
            .title(format!("Rename Prefix '{current_name}'"))
            .w(px(420.))
            .child(Input::new(&name_input))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("ren-pfx-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("ren-pfx-ok")
                            .primary()
                            .label("Rename")
                            .on_click(move |_, window, cx| {
                                let new_name = input_ok.read(cx).value().trim().to_string();
                                if new_name.is_empty() {
                                    return;
                                }
                                window.close_dialog(cx);
                                app_ok.update(cx, |app, cx| {
                                    app.rename_prefix(pfx_idx, new_name, window, cx);
                                });
                            }),
                    ),
            )
    });
}

pub fn open_delete_prefix_dialog(
    app_entity: Entity<UmuBoxApp>,
    pfx_idx: usize,
    name: String,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let delete_files = Rc::new(Cell::new(false));

    window.open_dialog(cx, move |dialog, _window, _cx| {
        let df_toggle = delete_files.clone();
        let app_ref = app_entity.clone();
        let df_ok = delete_files.clone();
        let app_ok = app_entity.clone();

        dialog
            .title(format!("Delete Prefix '{name}'"))
            .w(px(460.))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(hex_color(kd::FG))
                            .child(format!(
                                "Remove '{}' from UmuBox?",
                                path.display()
                            )),
                    )
                    .child(
                        Switch::new("del-pfx-disk")
                            .checked(delete_files.get())
                            .label("Also permanently delete prefix files from disk")
                            .on_click(move |checked: &bool, window, cx| {
                                df_toggle.set(*checked);
                                window.refresh();
                                cx.notify(app_ref.entity_id());
                            }),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("del-pfx-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("del-pfx-ok")
                            .danger()
                            .label("Delete")
                            .on_click(move |_, window, cx| {
                                let rm_disk = df_ok.get();
                                window.close_dialog(cx);
                                app_ok.update(cx, |app, cx| {
                                    app.delete_prefix(pfx_idx, rm_disk, window, cx);
                                });
                            }),
                    ),
            )
    });
}

pub fn open_game_dialog(
    app_entity: Entity<UmuBoxApp>,
    pfx_idx: usize,
    pfx_path: PathBuf,
    installed_protons: Vec<ProtonBuild>,
    existing: Option<(usize, GameEntry)>,
    initial_icon_preview: Option<Arc<Image>>,
    window: &mut Window,
    cx: &mut App,
) {
    let edit_idx = existing.as_ref().map(|(i, _)| *i);
    let init = existing.map(|(_, g)| g).unwrap_or_default();

    let name_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("e.g. World of Warcraft / Game Launcher")
            .default_value(init.name.clone())
    });
    let exe_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(format!("{}/drive_c/...", pfx_path.display()))
            .default_value(if init.exe_path.as_os_str().is_empty() {
                String::new()
            } else {
                init.exe_path.display().to_string()
            })
    });
    let args_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Optional launch arguments")
            .default_value(init.args.clone())
    });
    let game_id_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("UMU ID (e.g. umu-battlenet or 0)")
            .default_value(init.game_id.clone())
    });
    let dll_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("e.g. locationapi=d;winmm=n,b")
            .default_value(init.options.dll_overrides.clone())
    });
    let env_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("KEY=VAL, KEY2=VAL2")
            .default_value(format_extra_env(&init.options.extra_env))
    });

    let options_state = Rc::new(RefCell::new(init.options.clone()));
    let proton_state = Rc::new(RefCell::new(init.proton_override.clone()));
    let icon_override_state = Rc::new(RefCell::new(init.icon_override.clone()));
    let icon_preview_state = Rc::new(RefCell::new(initial_icon_preview));
    let desktop_managed_state = Rc::new(Cell::new(if edit_idx.is_some() {
        init.desktop_managed
    } else {
        true
    }));
    let existing_working_dir = init.working_dir.clone();
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }

        let opts = options_state.borrow().clone();
        let cur_prot = proton_state.borrow().clone();
        let cur_icon_override = icon_override_state.borrow().clone();
        let cur_icon_preview = icon_preview_state.borrow().clone();
        let cur_desktop_managed = desktop_managed_state.get();

        let exe_browse = exe_input.clone();
        let name_browse = name_input.clone();
        let app_browse = app_entity.clone();
        let ov_for_exe_browse = icon_override_state.clone();
        let prev_for_exe_browse = icon_preview_state.clone();

        let ov_for_icon_pick = icon_override_state.clone();
        let prev_for_icon_pick = icon_preview_state.clone();
        let app_icon_pick = app_entity.clone();

        let ov_for_icon_clear = icon_override_state.clone();
        let prev_for_icon_clear = icon_preview_state.clone();
        let exe_for_icon_clear = exe_input.clone();
        let app_icon_clear = app_entity.clone();

        let dm_toggle = desktop_managed_state.clone();
        let app_dm = app_entity.clone();

        let proton_pills = installed_protons.iter().enumerate().map(|(ix, b)| {
            let is_sel = cur_prot.as_ref() == Some(&b.path);
            let p_state = proton_state.clone();
            let bp = b.path.clone();
            let app_r = app_entity.clone();
            Button::new(SharedString::from(format!("game-prot-{ix}")))
                .with_variant(if is_sel {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label(b.name.clone())
                .on_click(move |_, window, cx| {
                    *p_state.borrow_mut() = Some(bp.clone());
                    window.refresh();
                    cx.notify(app_r.entity_id());
                })
        });

        let p_inherit = proton_state.clone();
        let app_inherit = app_entity.clone();

        let app_ok = app_entity.clone();
        let name_ok = name_input.clone();
        let exe_ok = exe_input.clone();
        let args_ok = args_input.clone();
        let gid_ok = game_id_input.clone();
        let dll_ok = dll_input.clone();
        let env_ok = env_input.clone();
        let opts_ok = options_state.clone();
        let prot_ok = proton_state.clone();
        let ov_ok = icon_override_state.clone();
        let dm_ok = desktop_managed_state.clone();
        let wd_ok = existing_working_dir.clone();
        let existing_id = init.id.clone();

        let make_toggle = |id: &'static str, label: &'static str, checked: bool, apply: fn(&mut LaunchOptions, bool)| {
            let st = options_state.clone();
            let app_r = app_entity.clone();
            Switch::new(id)
                .checked(checked)
                .label(label)
                .on_click(move |val: &bool, window, cx| {
                    apply(&mut st.borrow_mut(), *val);
                    window.refresh();
                    cx.notify(app_r.entity_id());
                })
        };

        let icon_status_text = if let Some(ref ov_path) = cur_icon_override {
            let fname = ov_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("custom icon");
            format!("Custom icon override: {fname}")
        } else {
            "Inherited from .exe (auto-extracted)".to_string()
        };

        dialog
            .title(if edit_idx.is_some() {
                "Edit Game / Shortcut"
            } else {
                "Add Game / Executable"
            })
            .w(px(640.))
            .child(
                v_flex()
                    .gap_2p5()
                    .child(field_block("Display Name", Input::new(&name_input)))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("Executable (.exe / .msi / .bat)"))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().flex_1().child(Input::new(&exe_input)))
                                    .child(
                                        Button::new("game-exe-browse")
                                            .small()
                                            .outline()
                                            .icon(IconName::FolderOpen)
                                            .label("Browse…")
                                            .on_click(move |_, window, cx| {
                                                let rx = cx.prompt_for_paths(PathPromptOptions {
                                                    files: true,
                                                    directories: false,
                                                    multiple: false,
                                                    prompt: Some(
                                                        "Select Windows Executable".into(),
                                                    ),
                                                });
                                                let exe_target = exe_browse.clone();
                                                let name_target = name_browse.clone();
                                                let vid = app_browse.entity_id();
                                                let ov_st = ov_for_exe_browse.clone();
                                                let prev_st = prev_for_exe_browse.clone();
                                                window
                                                    .spawn(cx, async move |cx| {
                                                        if let Ok(Ok(Some(paths))) = rx.await
                                                            && let Some(file) =
                                                                paths.into_iter().next()
                                                        {
                                                            let s = file.display().to_string();
                                                            let stem = file
                                                                .file_stem()
                                                                .and_then(|n| n.to_str())
                                                                .unwrap_or("App")
                                                                .to_string();
                                                            let exe_prev = if ov_st.borrow().is_none() {
                                                                load_gpui_preview_for_source(&file)
                                                            } else {
                                                                None
                                                            };
                                                            let _ = cx.update(|window, cx| {
                                                                exe_target.update(cx, |st, cx| {
                                                                    st.set_value(s, window, cx);
                                                                });
                                                                if ov_st.borrow().is_none() {
                                                                    *prev_st.borrow_mut() = exe_prev;
                                                                }
                                                                let cur = name_target
                                                                    .read(cx)
                                                                    .value()
                                                                    .trim()
                                                                    .to_string();
                                                                if cur.is_empty() {
                                                                    name_target.update(
                                                                        cx,
                                                                        |st, cx| {
                                                                            st.set_value(
                                                                                stem, window, cx,
                                                                            );
                                                                        },
                                                                    );
                                                                }
                                                                window.refresh();
                                                                cx.notify(vid);
                                                            });
                                                        }
                                                    })
                                                    .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("App Icon"))
                            .child(
                                h_flex()
                                    .gap_2p5()
                                    .items_center()
                                    .p_2()
                                    .rounded(px(6.))
                                    .bg(hex_color(kd::BG_PANEL))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .child(
                                        div()
                                            .w(px(36.))
                                            .h(px(36.))
                                            .flex_shrink_0()
                                            .rounded(px(6.))
                                            .bg(hex_color(kd::BG_MAIN))
                                            .border_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .overflow_hidden()
                                            .child(if let Some(arc_img) = cur_icon_preview {
                                                img(ImageSource::Image(arc_img))
                                                    .size(px(30.))
                                                    .object_fit(ObjectFit::Contain)
                                                    .into_any_element()
                                            } else {
                                                Icon::new(IconName::Gamepad2)
                                                    .size(px(18.))
                                                    .text_color(hex_color(kd::FG_MUTED))
                                                    .into_any_element()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(if cur_icon_override.is_some() {
                                                hex_color(kd::BLUE)
                                            } else {
                                                hex_color(kd::FG_SUBTLE)
                                            })
                                            .truncate()
                                            .child(icon_status_text),
                                    )
                                    .child(
                                        Button::new("game-icon-choose")
                                            .small()
                                            .outline()
                                            .icon(IconName::Image)
                                            .label("Choose Icon…")
                                            .on_click(move |_, window, cx| {
                                                let rx = cx.prompt_for_paths(PathPromptOptions {
                                                    files: true,
                                                    directories: false,
                                                    multiple: false,
                                                    prompt: Some(
                                                        "Select Icon (.png, .svg, .ico, .jpg, .webp, .exe)".into(),
                                                    ),
                                                });
                                                let ov_st = ov_for_icon_pick.clone();
                                                let prev_st = prev_for_icon_pick.clone();
                                                let vid = app_icon_pick.entity_id();
                                                window
                                                    .spawn(cx, async move |cx| {
                                                        if let Ok(Ok(Some(paths))) = rx.await
                                                            && let Some(icon_file) =
                                                                paths.into_iter().next()
                                                        {
                                                            let preview =
                                                                load_gpui_preview_for_source(&icon_file);
                                                            *ov_st.borrow_mut() = Some(icon_file);
                                                            *prev_st.borrow_mut() = preview;
                                                            let _ = cx.update(|window, cx| {
                                                                window.refresh();
                                                                cx.notify(vid);
                                                            });
                                                        }
                                                    })
                                                    .detach();
                                            }),
                                    )
                                    .when(cur_icon_override.is_some(), |this| {
                                        this.child(
                                            Button::new("game-icon-reset")
                                                .small()
                                                .ghost()
                                                .icon(IconName::RotateCcw)
                                                .label("Use .exe Icon")
                                                .on_click(move |_, window, cx| {
                                                    *ov_for_icon_clear.borrow_mut() = None;
                                                    let cur_exe = PathBuf::from(
                                                        exe_for_icon_clear
                                                            .read(cx)
                                                            .value()
                                                            .trim(),
                                                    );
                                                    *prev_for_icon_clear.borrow_mut() =
                                                        load_gpui_preview_for_source(&cur_exe);
                                                    window.refresh();
                                                    cx.notify(app_icon_clear.entity_id());
                                                }),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(field_block("Arguments", Input::new(&args_input))))
                            .child(div().w(px(200.)).child(field_block("UMU GAMEID", Input::new(&game_id_input)))),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("Proton Build Override"))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .flex_wrap()
                                    .child(
                                        Button::new("game-prot-inherit")
                                            .with_variant(if cur_prot.is_none() {
                                                ButtonVariant::Primary
                                            } else {
                                                ButtonVariant::Ghost
                                            })
                                            .xsmall()
                                            .label("Inherit Prefix Default")
                                            .on_click(move |_, window, cx| {
                                                *p_inherit.borrow_mut() = None;
                                                window.refresh();
                                                cx.notify(app_inherit.entity_id());
                                            }),
                                    )
                                    .children(proton_pills),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_wrap()
                            .pt_1()
                            .child(make_toggle("gopt-mh", "MangoHud", opts.mangohud, |o, v| o.mangohud = v))
                            .child(make_toggle("gopt-dgpu", "RTX dGPU Offload", opts.nvidia_prime_offload, |o, v| o.nvidia_prime_offload = v))
                            .child(make_toggle("gopt-wl", "Wayland", opts.wayland_driver, |o, v| o.wayland_driver = v))
                            .child(make_toggle("gopt-nt", "NTSync", opts.ntsync, |o, v| o.ntsync = v))
                            .child(make_toggle("gopt-nv", "NVAPI/DLSS", opts.nvapi, |o, v| o.nvapi = v))
                            .child(make_toggle("gopt-dx", "DXVK HUD", opts.dxvk_hud, |o, v| o.dxvk_hud = v))
                            .child(make_toggle("gopt-sc", "Shader Cache", opts.shader_cache, |o, v| o.shader_cache = v)),
                    )
                    .child(
                        Switch::new("game-dlg-desktop-managed")
                            .checked(cur_desktop_managed)
                            .label("Manage KDE App Menu / KRunner entry (.desktop with app icon)")
                            .on_click(move |val: &bool, window, cx| {
                                dm_toggle.set(*val);
                                window.refresh();
                                cx.notify(app_dm.entity_id());
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(field_block("WINEDLLOVERRIDES", Input::new(&dll_input))))
                            .child(div().flex_1().child(field_block("Extra Env (KEY=VAL, ...)", Input::new(&env_input)))),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("game-dlg-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("game-dlg-save")
                            .primary()
                            .label("Save Shortcut")
                            .on_click(move |_, window, cx| {
                                let name = name_ok.read(cx).value().trim().to_string();
                                let exe = exe_ok.read(cx).value().trim().to_string();
                                if name.is_empty() || exe.is_empty() {
                                    app_ok.update(cx, |app, cx| {
                                        app.toast_err("Name and executable path are required", window, cx);
                                    });
                                    return;
                                }
                                let mut options = opts_ok.borrow().clone();
                                options.dll_overrides = dll_ok.read(cx).value().trim().to_string();
                                options.extra_env = parse_extra_env(env_ok.read(cx).value().as_ref());

                                let entry = GameEntry {
                                    id: existing_id.clone(),
                                    name,
                                    exe_path: PathBuf::from(exe),
                                    working_dir: wd_ok.clone(),
                                    args: args_ok.read(cx).value().trim().to_string(),
                                    game_id: gid_ok.read(cx).value().trim().to_string(),
                                    proton_override: prot_ok.borrow().clone(),
                                    icon_override: ov_ok.borrow().clone(),
                                    desktop_managed: dm_ok.get(),
                                    options,
                                };
                                window.close_dialog(cx);
                                app_ok.update(cx, |app, cx| {
                                    app.save_game(pfx_idx, edit_idx, entry, window, cx);
                                });
                            }),
                    ),
            )
    });
}

pub fn open_winetricks_dialog(
    app_entity: Entity<UmuBoxApp>,
    pfx_idx: usize,
    pfx_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let verb_input = cx.new(|cx| {
        InputState::new(window, cx).placeholder("Verbs separated by space, e.g. vcrun2022 corefonts")
    });
    let initial_focus = Cell::new(true);

    let presets = [
        "vcrun2022",
        "corefonts",
        "d3dcompiler_47",
        "dotnet48",
        "faudio",
        "dxvk",
    ];

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            verb_input.focus_handle(cx).focus(window, cx);
        }

        let input_ok = verb_input.clone();
        let app_ok = app_entity.clone();

        let preset_btns = presets.iter().map(|&verb| {
            let v_inp = verb_input.clone();
            let app_r = app_entity.clone();
            Button::new(SharedString::from(format!("wt-preset-{verb}")))
                .outline()
                .xsmall()
                .label(verb)
                .on_click(move |_, window, cx| {
                    let cur = v_inp.read(cx).value().trim().to_string();
                    let next = if cur.is_empty() {
                        verb.to_string()
                    } else if cur.split_whitespace().any(|w| w == verb) {
                        cur
                    } else {
                        format!("{cur} {verb}")
                    };
                    v_inp.update(cx, |st, cx| st.set_value(next, window, cx));
                    window.refresh();
                    cx.notify(app_r.entity_id());
                })
        });

        dialog
            .title(format!("Run Winetricks in '{pfx_name}'"))
            .w(px(500.))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .p_2p5()
                            .rounded(px(6.))
                            .bg(accent_alpha(0x1a))
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .child("Runs `umu-run winetricks <verbs...>` directly inside the selected prefix and streams live output to the Live Console tab."),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(field_label("Common Verbs"))
                            .child(h_flex().gap_1().flex_wrap().children(preset_btns)),
                    )
                    .child(field_block("Verbs to Install", Input::new(&verb_input))),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("wt-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("wt-run")
                            .primary()
                            .icon(IconName::Play)
                            .label("Run Winetricks")
                            .on_click(move |_, window, cx| {
                                let raw = input_ok.read(cx).value().trim().to_string();
                                if raw.is_empty() {
                                    return;
                                }
                                let verbs: Vec<String> =
                                    raw.split_whitespace().map(ToString::to_string).collect();
                                window.close_dialog(cx);
                                app_ok.update(cx, |app, cx| {
                                    app.run_tool_on_prefix(
                                        pfx_idx,
                                        PrefixTool::Winetricks,
                                        verbs,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
    });
}

fn field_label(label: &'static str) -> impl gpui_kit::IntoElement {
    div()
        .text_xs()
        .text_color(hex_color(kd::FG_MUTED))
        .child(label)
}

fn field_block(label: &'static str, input: Input) -> impl gpui_kit::IntoElement {
    v_flex().gap_1().child(field_label(label)).child(input)
}
