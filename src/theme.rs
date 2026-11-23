use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb, rgba};

/// Kanagawa Dragon charcoal with a subtle steel-blue tint & cobalt wave accent.
#[allow(dead_code)]
pub mod kd {
    pub const BG_DEEP: u32 = 0x0c0d10;
    pub const BG_MAIN: u32 = 0x16171b;
    pub const BG_SIDEBAR: u32 = 0x1b1d22;
    pub const BG_PANEL: u32 = 0x25282f;
    pub const BG_BTN: u32 = 0x343842;
    pub const BG_BTN_HOVER: u32 = 0x414652;
    pub const BG_BTN_ACTIVE: u32 = 0x2c3038;

    pub const BORDER: u32 = 0x373c46;
    pub const BORDER_SUBTLE: u32 = 0x272a32;

    pub const FG: u32 = 0xdcd7ba;
    pub const FG_MUTED: u32 = 0x717580;
    pub const FG_SUBTLE: u32 = 0x9b9fa8;

    pub const BLUE: u32 = 0x6ea6d4;
    pub const LINK: u32 = 0x82b4dc;
    pub const GREEN: u32 = 0x87a987;
    pub const YELLOW: u32 = 0xcfb682;
    pub const RED: u32 = 0xc4746e;
    pub const VIOLET: u32 = 0x957fb8;
    pub const ORANGE: u32 = 0xcc966e;
    pub const AQUA: u32 = 0x8ea4a2;
    pub const PINK: u32 = 0xa292a3;
}

#[inline]
pub fn hex_color(hex: u32) -> Hsla {
    rgb(hex).into()
}

#[inline]
pub fn hex_alpha(hex_rgba: u32) -> Hsla {
    rgba(hex_rgba).into()
}

#[inline]
pub fn accent_alpha(alpha: u8) -> Hsla {
    rgba((kd::BLUE << 8) | (alpha as u32)).into()
}

/// Applies the Kanagawa Dragon + Cobalt Wave theme globally to GPUI Component.
pub fn apply_kanagawa_dragon_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);

    Theme::update(cx, |theme| {
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.font_size = px(13.5);
        theme.mono_font_size = px(13.);

        let c = &mut theme.colors;
        c.background = hex_color(kd::BG_MAIN);
        c.foreground = hex_color(kd::FG);
        c.border = hex_color(kd::BORDER);
        c.window_border = hex_color(kd::BORDER);
        c.muted = hex_color(kd::BG_PANEL);
        c.muted_foreground = hex_color(kd::FG_MUTED);
        c.accent = hex_color(kd::BG_PANEL);
        c.accent_foreground = hex_color(kd::FG);
        c.overlay = hex_alpha(0x0c0d10cc);

        c.popover = hex_color(kd::BG_PANEL);
        c.popover_foreground = hex_color(kd::FG);
        c.group_box = hex_color(kd::BG_SIDEBAR);
        c.group_box_foreground = hex_color(kd::FG);

        c.button = hex_color(kd::BG_BTN);
        c.button_hover = hex_color(kd::BG_BTN_HOVER);
        c.button_active = hex_color(kd::BG_BTN_ACTIVE);
        c.button_foreground = hex_color(kd::FG);

        c.button_secondary = hex_color(kd::BG_PANEL);
        c.button_secondary_hover = hex_color(kd::BG_BTN);
        c.button_secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.button_secondary_foreground = hex_color(kd::FG);

        c.primary = hex_color(kd::BLUE);
        c.primary_hover = hex_color(0x85b7e0);
        c.primary_active = hex_color(0x5990bd);
        c.primary_foreground = hex_color(kd::BG_MAIN);

        c.button_primary = hex_color(kd::BLUE);
        c.button_primary_hover = hex_color(0x85b7e0);
        c.button_primary_active = hex_color(0x5990bd);
        c.button_primary_foreground = hex_color(kd::BG_MAIN);

        c.secondary = hex_color(kd::BG_BTN);
        c.secondary_hover = hex_color(kd::BG_BTN_HOVER);
        c.secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.secondary_foreground = hex_color(kd::FG);

        c.danger = hex_color(kd::RED);
        c.danger_hover = hex_color(0xd4847e);
        c.danger_active = hex_color(0xb0635e);
        c.danger_foreground = hex_color(kd::BG_MAIN);

        c.button_danger = hex_color(kd::RED);
        c.button_danger_hover = hex_color(0xd4847e);
        c.button_danger_active = hex_color(0xb0635e);
        c.button_danger_foreground = hex_color(kd::BG_MAIN);

        c.success = hex_color(kd::GREEN);
        c.success_hover = hex_color(0x97ba97);
        c.success_active = hex_color(0x759675);
        c.success_foreground = hex_color(kd::BG_MAIN);

        c.button_success = hex_color(kd::GREEN);
        c.button_success_hover = hex_color(0x97ba97);
        c.button_success_active = hex_color(0x759675);
        c.button_success_foreground = hex_color(kd::BG_MAIN);

        c.info = hex_color(kd::BLUE);
        c.info_hover = hex_color(0x85b7e0);
        c.info_active = hex_color(0x5990bd);
        c.info_foreground = hex_color(kd::BG_MAIN);

        c.warning = hex_color(kd::YELLOW);
        c.warning_hover = hex_color(0xdcc597);
        c.warning_active = hex_color(0xb89f6b);
        c.warning_foreground = hex_color(kd::BG_MAIN);

        c.sidebar = hex_color(kd::BG_SIDEBAR);
        c.sidebar_border = hex_color(kd::BORDER_SUBTLE);
        c.sidebar_foreground = hex_color(kd::FG);
        c.sidebar_accent = hex_color(kd::BG_PANEL);
        c.sidebar_accent_foreground = hex_color(kd::FG);
        c.sidebar_primary = hex_color(kd::BLUE);
        c.sidebar_primary_foreground = hex_color(kd::BG_MAIN);

        c.title_bar = hex_color(kd::BG_DEEP);
        c.title_bar_border = hex_color(kd::BORDER_SUBTLE);
        c.status_bar = hex_color(kd::BG_DEEP);
        c.status_bar_border = hex_color(kd::BORDER_SUBTLE);

        c.input = hex_color(kd::BORDER);
        c.ring = hex_color(kd::BLUE);
        c.caret = hex_color(kd::BLUE);
        c.selection = accent_alpha(0x42);

        c.link = hex_color(kd::LINK);
        c.link_hover = hex_color(0x9ac5e8);
        c.link_active = hex_color(kd::BLUE);

        c.list = hex_color(kd::BG_SIDEBAR);
        c.list_even = hex_color(kd::BG_SIDEBAR);
        c.list_head = hex_color(kd::BG_PANEL);
        c.list_hover = hex_color(kd::BG_PANEL);
        c.list_active = accent_alpha(0x2e);
        c.list_active_border = hex_color(kd::BLUE);
        c.list_highlight_foreground = hex_color(kd::YELLOW);

        c.tab_bar = hex_color(kd::BG_DEEP);
        c.tab_bar_segmented = hex_color(kd::BG_PANEL);
        c.tab = gpui_kit::transparent_black();
        c.tab_foreground = hex_color(kd::FG_MUTED);
        c.tab_active = hex_color(kd::BG_MAIN);
        c.tab_active_foreground = hex_color(kd::FG);

        c.scrollbar = hex_alpha(0x00000000);
        c.scrollbar_thumb = hex_alpha(0x71758066);
        c.scrollbar_thumb_hover = hex_alpha(0x9b9fa8aa);

        c.red = hex_color(kd::RED);
        c.green = hex_color(kd::GREEN);
        c.blue = hex_color(kd::BLUE);
        c.yellow = hex_color(kd::YELLOW);
        c.magenta = hex_color(kd::PINK);
        c.cyan = hex_color(kd::AQUA);
    });
}
