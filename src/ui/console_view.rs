use super::UmuBoxApp;
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    ClipboardItem, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Styled, div, px,
};

pub fn render_console_view(app: &UmuBoxApp, cx: &mut Context<UmuBoxApp>) -> impl IntoElement {
    let border = hex_color(kd::BORDER_SUBTLE);
    let bg_main = hex_color(kd::BG_MAIN);
    let bg_bar = hex_color(kd::BG_SIDEBAR);

    let header = h_flex()
        .h(px(42.))
        .w_full()
        .px_4()
        .items_center()
        .justify_between()
        .bg(bg_bar)
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(kd::FG))
                        .child("Live Launch & Tool Output"),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(4.))
                        .bg(hex_color(kd::BG_PANEL))
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child(format!("{} lines", app.console_lines.len())),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .child(
                    Button::new("console-copy")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .label("Copy All")
                        .on_click(cx.listener(|this, _, window, cx| {
                            let joined = this.console_lines.join("\n");
                            cx.write_to_clipboard(ClipboardItem::new_string(joined));
                            this.toast_ok("Copied console output to clipboard", window, cx);
                        })),
                )
                .child(
                    Button::new("console-clear")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Trash)
                        .label("Clear")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.clear_logs(cx);
                        })),
                ),
        );

    let body = if app.console_lines.is_empty() {
        div()
            .p_6()
            .text_sm()
            .text_color(hex_color(kd::FG_MUTED))
            .child("No output captured yet. Launch a game, installer, winecfg, or winetricks verb to stream live stdout/stderr here.")
            .into_any_element()
    } else {
        v_flex()
            .p_4()
            .gap_0p5()
            .font_family("monospace")
            .text_xs()
            .children(app.console_lines.iter().map(|line| {
                let lower = line.to_ascii_lowercase();
                let color = if line.starts_with("[UmuBox]") {
                    hex_color(kd::BLUE)
                } else if lower.contains("err:")
                    || lower.contains("error")
                    || lower.contains("fatal")
                    || lower.contains("fail")
                {
                    hex_color(kd::RED)
                } else if lower.contains("warn:") || lower.contains("fixme:") {
                    hex_color(kd::YELLOW)
                } else {
                    hex_color(kd::FG)
                };
                div().text_color(color).child(line.clone())
            }))
            .into_any_element()
    };

    v_flex()
        .size_full()
        .bg(bg_main)
        .child(header)
        .child(
            div()
                .id("umubox-console-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .child(body),
        )
}
