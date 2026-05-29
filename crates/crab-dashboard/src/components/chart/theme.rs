//! Theme-aware RGB palette for Plotters (mirrors `style/design-tokens.css`).

use crate::theme::Theme;
use plotters::style::RGBColor;

#[derive(Clone, Copy)]
pub struct ChartPalette {
    pub bg: RGBColor,
    pub text: RGBColor,
    pub grid: RGBColor,
    pub accent: RGBColor,
    pub info: RGBColor,
    pub success: RGBColor,
    pub warning: RGBColor,
    pub error: RGBColor,
    pub purple: RGBColor,
    pub muted: RGBColor,
    pub tier_l0: RGBColor,
    pub tier_l1: RGBColor,
    pub tier_l2: RGBColor,
    pub tier_l3: RGBColor,
    pub tier_miss: RGBColor,
}

impl ChartPalette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                bg: rgb(0xff, 0xff, 0xff),
                text: rgb(0x1a, 0x18, 0x14),
                grid: rgb(0xe5, 0xe0, 0xd8),
                accent: rgb(0xc4, 0x4d, 0x2f),
                info: rgb(0x09, 0x69, 0xda),
                success: rgb(0x1a, 0x7f, 0x37),
                warning: rgb(0x9a, 0x67, 0x00),
                error: rgb(0xcf, 0x22, 0x2e),
                purple: rgb(0x82, 0x50, 0xdf),
                muted: rgb(0x5c, 0x57, 0x4f),
                tier_l0: rgb(0xc4, 0x4d, 0x2f),
                tier_l1: rgb(0x09, 0x69, 0xda),
                tier_l2: rgb(0x9a, 0x67, 0x00),
                tier_l3: rgb(0x82, 0x50, 0xdf),
                tier_miss: rgb(0xa8, 0xa2, 0x9e),
            },
            Theme::Dark => Self {
                bg: rgb(0x16, 0x1b, 0x22),
                text: rgb(0xe6, 0xed, 0xf3),
                grid: rgb(0x21, 0x26, 0x2d),
                accent: rgb(0xf7, 0x81, 0x66),
                info: rgb(0x58, 0xa6, 0xff),
                success: rgb(0x3f, 0xb9, 0x50),
                warning: rgb(0xd2, 0x99, 0x1d),
                error: rgb(0xf8, 0x51, 0x49),
                purple: rgb(0xbc, 0x8c, 0xff),
                muted: rgb(0x8b, 0x94, 0x9e),
                tier_l0: rgb(0xf7, 0x81, 0x66),
                tier_l1: rgb(0x58, 0xa6, 0xff),
                tier_l2: rgb(0xd2, 0x99, 0x1d),
                tier_l3: rgb(0xbc, 0x8c, 0xff),
                tier_miss: rgb(0x48, 0x4f, 0x58),
            },
            Theme::Midnight => Self {
                bg: rgb(0x14, 0x10, 0x1f),
                text: rgb(0xec, 0xe8, 0xf5),
                grid: rgb(0x23, 0x1e, 0x36),
                accent: rgb(0xc4, 0xa1, 0xff),
                info: rgb(0x79, 0xb8, 0xff),
                success: rgb(0x3f, 0xb9, 0x50),
                warning: rgb(0xd2, 0x99, 0x1d),
                error: rgb(0xf8, 0x51, 0x49),
                purple: rgb(0xbc, 0x8c, 0xff),
                muted: rgb(0x9b, 0x92, 0xb0),
                tier_l0: rgb(0xc4, 0xa1, 0xff),
                tier_l1: rgb(0x79, 0xb8, 0xff),
                tier_l2: rgb(0xd2, 0x99, 0x1d),
                tier_l3: rgb(0xbc, 0x8c, 0xff),
                tier_miss: rgb(0x5a, 0x51, 0x6e),
            },
            Theme::Ocean => Self {
                bg: rgb(0x11, 0x1c, 0x26),
                text: rgb(0xe2, 0xed, 0xf4),
                grid: rgb(0x1a, 0x28, 0x34),
                accent: rgb(0x3d, 0xb8, 0xc9),
                info: rgb(0x58, 0xa6, 0xff),
                success: rgb(0x46, 0xc8, 0x80),
                warning: rgb(0xc9, 0xa2, 0x27),
                error: rgb(0xe8, 0x5d, 0x5d),
                purple: rgb(0x8b, 0x9c, 0xf6),
                muted: rgb(0x7d, 0x96, 0xa8),
                tier_l0: rgb(0x3d, 0xb8, 0xc9),
                tier_l1: rgb(0x58, 0xa6, 0xff),
                tier_l2: rgb(0xc9, 0xa2, 0x27),
                tier_l3: rgb(0x8b, 0x9c, 0xf6),
                tier_miss: rgb(0x5a, 0x6f, 0x80),
            },
            Theme::Sand => Self {
                bg: rgb(0xfa, 0xf7, 0xf2),
                text: rgb(0x2a, 0x26, 0x1f),
                grid: rgb(0xdd, 0xd6, 0xca),
                accent: rgb(0xb8, 0x5c, 0x28),
                info: rgb(0x1d, 0x6b, 0x9a),
                success: rgb(0x2d, 0x7a, 0x48),
                warning: rgb(0x8a, 0x6d, 0x1a),
                error: rgb(0xb8, 0x38, 0x32),
                purple: rgb(0x7a, 0x5c, 0xad),
                muted: rgb(0x6f, 0x67, 0x5c),
                tier_l0: rgb(0xb8, 0x5c, 0x28),
                tier_l1: rgb(0x1d, 0x6b, 0x9a),
                tier_l2: rgb(0x8a, 0x6d, 0x1a),
                tier_l3: rgb(0x7a, 0x5c, 0xad),
                tier_miss: rgb(0xa8, 0x9e, 0x90),
            },
            Theme::System => Self::for_theme(Theme::resolve_system()),
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> RGBColor {
    RGBColor(r, g, b)
}

/// Map dashboard CSS variable strings used in `ChartSeries::color` to themed RGB.
pub fn resolve_series_color(css: &str, palette: &ChartPalette) -> RGBColor {
    match css {
        "var(--accent-primary)" | "var(--cc-accent)" | "var(--accent)" => palette.accent,
        "var(--info)" | "var(--cc-info)" | "var(--blue)" => palette.info,
        "var(--success)" | "var(--cc-success)" | "var(--green)" => palette.success,
        "var(--warning)" | "var(--cc-warning)" | "var(--yellow)" => palette.warning,
        "var(--error)" | "var(--cc-error)" | "var(--red)" => palette.error,
        "var(--cc-purple)" | "var(--purple)" => palette.purple,
        "var(--cc-text-muted)" | "var(--text-secondary)" => palette.muted,
        "var(--cc-border-light)" => palette.grid,
        "var(--cc-tier-l0)" | "var(--tier-l0)" => palette.tier_l0,
        "var(--cc-tier-l1)" | "var(--tier-l1)" => palette.tier_l1,
        "var(--cc-tier-l2)" | "var(--tier-l2)" => palette.tier_l2,
        "var(--cc-tier-l3)" | "var(--tier-l3)" => palette.tier_l3,
        "var(--cc-tier-miss)" | "var(--tier-miss)" => palette.tier_miss,
        _ => palette.accent,
    }
}
