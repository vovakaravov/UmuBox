use super::UmuBoxApp;
use crate::proton::{DownloadPhase, ProtonSource, is_release_installed};
use crate::runner::format_bytes;
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    Styled, div, prelude::FluentBuilder as _, px, relative,
};
use std::fs;

pub fn render_proton_view(app: &UmuBoxApp, cx: &mut Context<UmuBoxApp>) -> impl IntoElement {
    let border = hex_color(kd::BORDER_SUBTLE);
    let panel_bg = hex_color(kd::BG_SIDEBAR);
    let card_bg = hex_color(kd::BG_PANEL);

    let latest_missing = app
        .releases
        .first()
        .filter(|r| !is_release_installed(&r.tag, &app.installed_protons))
        .cloned();

    let installed_cards = app.installed_protons.iter().enumerate().map(|(ix, build)| {
        let is_default = app.config.default_proton.as_ref().is_some_and(|dp| {
            dp == &build.path
                || fs::canonicalize(dp).ok().as_ref() == Some(&build.canonical_path)
        });
        let can_remove = build.source == ProtonSource::User
            && !fs::symlink_metadata(&build.path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(true);
        let badge_color = match build.source {
            ProtonSource::User => hex_color(kd::GREEN),
            ProtonSource::SystemAur => hex_color(kd::BLUE),
            ProtonSource::Steam => hex_color(kd::VIOLET),
        };
        let path_for_default = build.path.clone();
        let path_for_remove = build.path.clone();

        h_flex()
            .w_full()
            .p_3()
            .rounded(px(8.))
            .bg(panel_bg)
            .border_1()
            .border_color(if is_default {
                hex_color(kd::BLUE)
            } else {
                border
            })
            .items_center()
            .justify_between()
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
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::FG))
                                    .child(build.name.clone()),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(px(4.))
                                    .bg(card_bg)
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(badge_color)
                                    .child(build.source.label()),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(px(4.))
                                    .bg(card_bg)
                                    .text_size(px(10.5))
                                    .text_color(hex_color(kd::YELLOW))
                                    .child(build.version_tag.clone()),
                            )
                            .when(is_default, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(accent_alpha(0x30))
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::BLUE))
                                        .child("DEFAULT"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::FG_MUTED))
                            .truncate()
                            .child(build.path.display().to_string()),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .flex_shrink_0()
                    .when(!is_default, |this| {
                        this.child(
                            Button::new(SharedString::from(format!("proton-set-def-{ix}")))
                                .with_variant(ButtonVariant::Secondary)
                                .xsmall()
                                .icon(IconName::Check)
                                .label("Set Default")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_default_proton(path_for_default.clone(), window, cx);
                                })),
                        )
                    })
                    .when(can_remove, |this| {
                        this.child(
                            Button::new(SharedString::from(format!("proton-rm-{ix}")))
                                .danger()
                                .xsmall()
                                .icon(IconName::Trash)
                                .label("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove_proton_build(path_for_remove.clone(), window, cx);
                                })),
                        )
                    }),
            )
    });

    let releases_header = h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::FG))
                        .child("GE-Proton Releases (GitHub)"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child("Downloads directly to ~/.local/share/Steam/compatibilitytools.d with SHA-512 verification"),
                ),
        )
        .child(
            Button::new("ge-check-releases")
                .primary()
                .small()
                .icon(IconName::Download)
                .label(if app.releases_loading {
                    "Checking…"
                } else {
                    "Check Releases"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.check_ge_proton_releases(cx);
                })),
        );

    let download_banner = app.download_state.as_ref().map(|(tag, prog)| {
        let frac = if prog.total_bytes > 0 {
            (prog.downloaded_bytes as f32 / prog.total_bytes as f32).clamp(0.02, 1.0)
        } else {
            0.35
        };
        let phase_label = match prog.phase {
            DownloadPhase::Downloading => format!(
                "{} / {}",
                format_bytes(prog.downloaded_bytes),
                format_bytes(prog.total_bytes)
            ),
            DownloadPhase::Verifying => "Verifying SHA-512…".to_string(),
            DownloadPhase::Extracting => "Extracting tarball…".to_string(),
        };

        v_flex()
            .w_full()
            .p_3()
            .gap_2()
            .rounded(px(8.))
            .bg(accent_alpha(0x1f))
            .border_1()
            .border_color(hex_color(kd::BLUE))
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex_color(kd::FG))
                            .child(format!("{tag}: {}", prog.message)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::BLUE))
                            .child(phase_label),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(8.))
                    .rounded_full()
                    .bg(hex_color(kd::BG_DEEP))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(frac))
                            .rounded_full()
                            .bg(hex_color(kd::BLUE)),
                    ),
            )
    });

    let update_banner = latest_missing.as_ref().map(|rel| {
        let rel_clone = rel.clone();
        let busy = app.download_state.is_some();
        h_flex()
            .w_full()
            .p_3()
            .rounded(px(8.))
            .bg(hex_alpha(0x87a98722))
            .border_1()
            .border_color(hex_color(kd::GREEN))
            .items_center()
            .justify_between()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::GREEN))
                            .child(format!("Newer GE-Proton Available: {}", rel.tag)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .child(format!(
                                "Released {} · {}",
                                rel.published_at,
                                format_bytes(rel.tarball_size)
                            )),
                    ),
            )
            .child(
                Button::new("ge-install-latest")
                    .primary()
                    .small()
                    .icon(IconName::Download)
                    .label(if busy {
                        "Installing…"
                    } else {
                        "Install Latest"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_ge_release(rel_clone.clone(), window, cx);
                    })),
            )
    });

    let release_cards = app.releases.iter().enumerate().map(|(ix, rel)| {
        let installed = is_release_installed(&rel.tag, &app.installed_protons);
        let busy = app.download_state.is_some();
        let rel_clone = rel.clone();

        h_flex()
            .w_full()
            .p_3()
            .rounded(px(8.))
            .bg(panel_bg)
            .border_1()
            .border_color(border)
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::FG))
                                    .child(rel.tag.clone()),
                            )
                            .when(installed, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(hex_alpha(0x87a9872b))
                                        .text_size(px(10.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::GREEN))
                                        .child("INSTALLED"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(format!(
                                "Published {} · {} · {}",
                                rel.published_at,
                                format_bytes(rel.tarball_size),
                                rel.tarball_name
                            )),
                    ),
            )
            .child(if installed {
                div()
                    .text_xs()
                    .text_color(hex_color(kd::GREEN))
                    .child("Ready")
                    .into_any_element()
            } else {
                Button::new(SharedString::from(format!("ge-install-{ix}")))
                    .outline()
                    .xsmall()
                    .icon(IconName::Download)
                    .label(if busy { "Busy…" } else { "Install" })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_ge_release(rel_clone.clone(), window, cx);
                    }))
                    .into_any_element()
            })
    });

    div()
        .id("proton-view-scroll")
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .overflow_y_scrollbar()
        .p_5()
        .child(
            v_flex()
                .max_w(px(1040.))
                .mx_auto()
                .gap_5()
                .child(
                    v_flex()
                        .gap_2p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hex_color(kd::FG))
                                .child(format!(
                                    "Installed Proton Builds ({})",
                                    app.installed_protons.len()
                                )),
                        )
                        .child(v_flex().gap_2().children(installed_cards)),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .pt_3()
                        .border_t_1()
                        .border_color(border)
                        .child(releases_header)
                        .when_some(download_banner, |this, b| this.child(b))
                        .when_some(update_banner, |this, b| this.child(b))
                        .when_some(app.releases_error.clone(), |this, err| {
                            this.child(
                                div()
                                    .p_3()
                                    .rounded(px(8.))
                                    .bg(hex_alpha(0xc4746e26))
                                    .border_1()
                                    .border_color(hex_color(kd::RED))
                                    .text_xs()
                                    .text_color(hex_color(kd::RED))
                                    .child(err),
                            )
                        })
                        .when(app.releases.is_empty() && !app.releases_loading, |this| {
                            this.child(
                                div()
                                    .p_4()
                                    .rounded(px(8.))
                                    .bg(panel_bg)
                                    .border_1()
                                    .border_color(border)
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("Click 'Check Releases' above to query GloriousEggroll/proton-ge-custom on GitHub."),
                            )
                        })
                        .child(v_flex().gap_2().children(release_cards)),
                ),
        )
}
