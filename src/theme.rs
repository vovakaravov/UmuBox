use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb, rgba};

/// Midnight Cobalt dark palette tailored to UmuBox's cobalt cube & Proton orb icon.
#[allow(dead_code)]
pub mod kd {
    pub const BG_DEEP: u32 = 0x090e16;
    pub const BG_MAIN: u32 = 0x0f1724;
    pub const BG_SIDEBAR: u32 = 0x141f30;
    pub const BG_PANEL: u32 = 0x1b293f;
    pub const BG_BTN: u32 = 0x243652;
    pub const BG_BTN_HOVER: u32 = 0x2f4568;
    pub const BG_BTN_ACTIVE: u32 = 0x1d2c44;

    pub const BORDER: u32 = 0x2c4161;
    pub const BORDER_SUBTLE: u32 = 0x1e2d45;

    pub const FG: u32 = 0xdde7f4;
    pub const FG_MUTED: u32 = 0x667c9b;
    pub const FG_SUBTLE: u32 = 0x92a7c4;

    pub const BLUE: u32 = 0x499ef5;
    pub const LINK: u32 = 0x6cb5fa;
    pub const GREEN: u32 = 0x59c98c;
    pub const YELLOW: u32 = 0xf2b054;
    pub const RED: u32 = 0xe06c75;
    pub const VIOLET: u32 = 0x998ae6;
    pub const ORANGE: u32 = 0xe89948;
    pub const AQUA: u32 = 0x5cc4cf;
    pub const PINK: u32 = 0xd189b5;
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

/// Applies the Midnight Cobalt theme globally to GPUI Component.
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
        c.overlay = hex_alpha(0x090e16cc);

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
        c.primary_hover = hex_color(0x66b0f8);
        c.primary_active = hex_color(0x3386dc);
        c.primary_foreground = hex_color(kd::BG_DEEP);

        c.button_primary = hex_color(kd::BLUE);
        c.button_primary_hover = hex_color(0x66b0f8);
        c.button_primary_active = hex_color(0x3386dc);
        c.button_primary_foreground = hex_color(kd::BG_DEEP);

        c.secondary = hex_color(kd::BG_BTN);
        c.secondary_hover = hex_color(kd::BG_BTN_HOVER);
        c.secondary_active = hex_color(kd::BG_BTN_ACTIVE);
        c.secondary_foreground = hex_color(kd::FG);

        c.danger = hex_color(kd::RED);
        c.danger_hover = hex_color(0xe8838b);
        c.danger_active = hex_color(0xc7565f);
        c.danger_foreground = hex_color(kd::BG_DEEP);

        c.button_danger = hex_color(kd::RED);
        c.button_danger_hover = hex_color(0xe8838b);
        c.button_danger_active = hex_color(0xc7565f);
        c.button_danger_foreground = hex_color(kd::BG_DEEP);

        c.success = hex_color(kd::GREEN);
        c.success_hover = hex_color(0x73d6a0);
        c.success_active = hex_color(0x45b076);
        c.success_foreground = hex_color(kd::BG_DEEP);

        c.button_success = hex_color(kd::GREEN);
        c.button_success_hover = hex_color(0x73d6a0);
        c.button_success_active = hex_color(0x45b076);
        c.button_success_foreground = hex_color(kd::BG_DEEP);

        c.info = hex_color(kd::BLUE);
        c.info_hover = hex_color(0x66b0f8);
        c.info_active = hex_color(0x3386dc);
        c.info_foreground = hex_color(kd::BG_DEEP);

        c.warning = hex_color(kd::YELLOW);
        c.warning_hover = hex_color(0xf5c278);
        c.warning_active = hex_color(0xd9993c);
        c.warning_foreground = hex_color(kd::BG_DEEP);

        c.sidebar = hex_color(kd::BG_SIDEBAR);
        c.sidebar_border = hex_color(kd::BORDER_SUBTLE);
        c.sidebar_foreground = hex_color(kd::FG);
        c.sidebar_accent = hex_color(kd::BG_PANEL);
        c.sidebar_accent_foreground = hex_color(kd::FG);
        c.sidebar_primary = hex_color(kd::BLUE);
        c.sidebar_primary_foreground = hex_color(kd::BG_DEEP);

        c.title_bar = hex_color(kd::BG_DEEP);
        c.title_bar_border = hex_color(kd::BORDER_SUBTLE);
        c.status_bar = hex_color(kd::BG_DEEP);
        c.status_bar_border = hex_color(kd::BORDER_SUBTLE);

        c.input = hex_color(kd::BORDER);
        c.ring = hex_color(kd::BLUE);
        c.caret = hex_color(kd::BLUE);
        c.selection = accent_alpha(0x42);

        c.link = hex_color(kd::LINK);
        c.link_hover = hex_color(0x88c5fc);
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
        c.scrollbar_thumb = hex_alpha(0x667c9b66);
        c.scrollbar_thumb_hover = hex_alpha(0x92a7c4aa);

        c.red = hex_color(kd::RED);
        c.green = hex_color(kd::GREEN);
        c.blue = hex_color(kd::BLUE);
        c.yellow = hex_color(kd::YELLOW);
        c.magenta = hex_color(kd::PINK);
        c.cyan = hex_color(kd::AQUA);
    });
}
