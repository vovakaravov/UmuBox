use super::{UmuBoxApp, modals};
use crate::runner::{format_bytes, open_in_file_manager};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

pub fn render_sidebar(app: &UmuBoxApp, cx: &mut Context<UmuBoxApp>) -> impl IntoElement {
    let sidebar_bg = hex_color(kd::BG_SIDEBAR);
    let border = hex_color(kd::BORDER_SUBTLE);
    let app_entity = cx.entity();

    let header = h_flex()
        .h(px(38.))
        .w_full()
        .px_3()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(hex_color(kd::FG_SUBTLE))
                .child(format!("PREFIXES ({})", app.config.prefixes.len())),
        )
        .child(
            Button::new("sidebar-new-prefix")
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .label("New")
                .tooltip("Create a new Wine/Proton Prefix")
                .on_click({
                    let app_e = app_entity.clone();
                    move |_, window, cx| {
                        let root = app_e.read(cx).config.prefixes_root.clone();
                        let builds = app_e.read(cx).installed_protons.clone();
                        modals::open_new_prefix_dialog(app_e.clone(), root, builds, window, cx);
                    }
                }),
        );

    let items = app.config.prefixes.iter().enumerate().map(|(idx, pfx)| {
        let is_selected = idx == app.selected_prefix;
        let is_default = app.config.default_prefix_id.as_deref() == Some(pfx.id.as_str());
        let running = app
            .running_prefixes
            .get(&pfx.id)
            .map(|v| v.len())
            .unwrap_or(0);
        let size_label = app
            .prefix_sizes
            .get(&pfx.id)
            .map(|b| format_bytes(*b))
            .unwrap_or_else(|| "…".to_string());
        let games_count = pfx.games.len();
        let name = pfx.name.clone();
        let name_ren = pfx.name.clone();
        let pfx_path = pfx.path.clone();
        let app_menu = app_entity.clone();

        div()
            .id(SharedString::from(format!("sb-pfx-{idx}-{}", pfx.id)))
            .w_full()
            .px_2p5()
            .py_2()
            .rounded(px(6.))
            .border_l_2()
            .border_color(if is_selected {
                hex_color(kd::BLUE)
            } else {
                gpui_kit::transparent_black()
            })
            .bg(if is_selected {
                hex_alpha(0x8ba4b024)
            } else {
                gpui_kit::transparent_black()
            })
            .hover(move |s| {
                if is_selected {
                    s
                } else {
                    s.bg(hex_color(kd::BG_PANEL))
                }
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_prefix(idx, window, cx);
            }))
            .context_menu(move |menu, _, _| {
                let app_open = app_menu.clone();
                let app_kill = app_menu.clone();
                let app_ren = app_menu.clone();
                let app_def = app_menu.clone();
                let app_del = app_menu.clone();
                let open_dir = if pfx_path.join("drive_c").is_dir() {
                    pfx_path.join("drive_c")
                } else {
                    pfx_path.clone()
                };
                let ren_name = name_ren.clone();
                let del_name = name_ren.clone();
                let del_path = pfx_path.clone();

                menu.label(name_ren.clone())
                    .item(
                        PopupMenuItem::new("Open drive_c in File Manager")
                            .icon(IconName::FolderOpen)
                            .on_click(move |_, window, cx| {
                                if let Err(err) = open_in_file_manager(&open_dir) {
                                    app_open.update(cx, |app, cx| {
                                        app.toast_err(format!("{err}"), window, cx);
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Set as Default Prefix")
                            .icon(IconName::Check)
                            .on_click(move |_, window, cx| {
                                app_def.update(cx, |app, cx| {
                                    app.set_default_prefix(idx, window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Rename Prefix…")
                            .icon(IconName::Pencil)
                            .on_click(move |_, window, cx| {
                                modals::open_rename_prefix_dialog(
                                    app_ren.clone(),
                                    idx,
                                    ren_name.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Force Kill Prefix (wineserver -k)")
                            .icon(IconName::Power)
                            .on_click(move |_, window, cx| {
                                app_kill.update(cx, |app, cx| {
                                    app.kill_prefix_now(idx, window, cx);
                                });
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete Prefix…")
                            .icon(IconName::Trash)
                            .on_click(move |_, window, cx| {
                                modals::open_delete_prefix_dialog(
                                    app_del.clone(),
                                    idx,
                                    del_name.clone(),
                                    del_path.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .w(px(7.))
                                            .h(px(7.))
                                            .rounded_full()
                                            .flex_shrink_0()
                                            .bg(if running > 0 {
                                                hex_color(kd::GREEN)
                                            } else {
                                                hex_color(kd::FG_MUTED)
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(if is_selected {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .text_color(if is_selected {
                                                hex_color(kd::FG)
                                            } else {
                                                hex_color(kd::FG_SUBTLE)
                                            })
                                            .truncate()
                                            .child(name),
                                    ),
                            )
                            .when(is_default, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(hex_color(kd::BG_PANEL))
                                        .text_size(px(9.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::BLUE))
                                        .child("DEFAULT"),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(px(11.))
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(if running > 0 {
                                format!("Running ({running} proc) · {games_count} app(s)")
                            } else {
                                format!("{games_count} app(s)")
                            })
                            .child(size_label),
                    ),
            )
    });

    v_flex()
        .size_full()
        .bg(sidebar_bg)
        .border_r_1()
        .border_color(border)
        .child(header)
        .child(
            div()
                .id("sidebar-prefix-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_2()
                .child(v_flex().gap_1().children(items)),
        )
}
