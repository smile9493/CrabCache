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
}

impl ChartPalette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                bg: rgb(0xff, 0xff, 0xff),
                text: rgb(0x1a, 0x18, 0x14),
                grid: rgb(0xde, 0xda, 0xd4),
                accent: rgb(0xc4, 0x4d, 0x2f),
                info: rgb(0x09, 0x69, 0xda),
                success: rgb(0x1a, 0x7f, 0x37),
                warning: rgb(0x9a, 0x67, 0x00),
                error: rgb(0xcf, 0x22, 0x2e),
                purple: rgb(0x82, 0x50, 0xdf),
                muted: rgb(0x5c, 0x57, 0x4f),
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
            },
            Theme::Midnight => Self {
                bg: rgb(0x14, 0x10, 0x1f),
                text: rgb(0xec, 0xe8, 0xf5),
                grid: rgb(0x23, 0x1e, 0x36),
                accent: rgb(0xf7, 0x81, 0x66),
                info: rgb(0x79, 0xb8, 0xff),
                success: rgb(0x3f, 0xb9, 0x50),
                warning: rgb(0xd2, 0x99, 0x1d),
                error: rgb(0xf8, 0x51, 0x49),
                purple: rgb(0xbc, 0x8c, 0xff),
                muted: rgb(0x9b, 0x92, 0xb0),
            },
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> RGBColor {
    RGBColor(r, g, b)
}

/// Map dashboard CSS variable strings used in `ChartSeries::color` to themed RGB.
pub fn resolve_series_color(css: &'static str, palette: &ChartPalette) -> RGBColor {
    match css {
        "var(--accent-primary)" | "var(--cc-accent)" | "var(--accent)" => palette.accent,
        "var(--info)" | "var(--cc-info)" | "var(--blue)" => palette.info,
        "var(--success)" | "var(--cc-success)" | "var(--green)" => palette.success,
        "var(--warning)" | "var(--cc-warning)" | "var(--yellow)" => palette.warning,
        "var(--error)" | "var(--cc-error)" | "var(--red)" => palette.error,
        "var(--cc-purple)" | "var(--purple)" => palette.purple,
        "var(--cc-text-muted)" | "var(--text-secondary)" => palette.muted,
        "var(--cc-border-light)" => palette.grid,
        _ => palette.accent,
    }
}
