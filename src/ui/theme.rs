//! 配色主题. 两套色板: dark / light. 数值取自设计稿 tui-core.jsx + 实物麻将牌配色.
//!
//! 牌张分色: 数字色 (tile_*_num) + 花色字色 (tile_*_suit) 两段独立绘制.
//! 字牌按汉字含义上色 (中红/发绿/白蓝/风牌黑).

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
}

impl ThemeKind {
    pub fn next(self) -> Self {
        match self {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::Dark,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::Dark => "暗",
            ThemeKind::Light => "亮",
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeKind::Dark => DARK,
            ThemeKind::Light => LIGHT,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub line: Color,
    pub panel: Color,
    pub panel_hi: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub danger: Color,
    pub danger_soft: Color,
    pub ok: Color,
    pub ok_soft: Color,
    pub info: Color,
    pub info_soft: Color,
    /// 牌字默认色 (字牌 padding / 空牌位 fallback).
    pub tile_fg: Color,
    pub tile_bg: Color,
    pub tile_border: Color,
    /// 赤 5 数字色 (覆盖 tile_*_num 当 t.red=true).
    pub tile_red: Color,
    pub tile_back: Color,
    pub tile_back_pattern: Color,

    // ── 数牌数字色 (一/二/.../九 或 1/2/.../9) ──
    pub tile_man_num: Color,
    pub tile_pin_num: Color,
    pub tile_sou_num: Color,
    // ── 数牌花色字 (萬/筒/索) ──
    pub tile_man_suit: Color,
    pub tile_pin_suit: Color,
    pub tile_sou_suit: Color,
    // ── 字牌单字 (按汉字含义上色) ──
    pub tile_chun: Color,  // 中
    pub tile_hatsu: Color, // 發
    pub tile_haku: Color,  // 白
    pub tile_wind: Color,  // 东南西北
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

pub const DARK: Theme = Theme {
    bg: rgb(0x16, 0x18, 0x1c),
    fg: rgb(0xe8, 0xe4, 0xdc),
    dim: rgb(0x7a, 0x74, 0x68),
    line: rgb(0x3a, 0x3a, 0x3a),
    panel: rgb(0x1c, 0x1f, 0x24),
    panel_hi: rgb(0x23, 0x27, 0x2d),
    accent: rgb(0xe3, 0xb3, 0x41),
    accent_soft: rgb(0x5a, 0x49, 0x20),
    danger: rgb(0xe0, 0x5a, 0x4a),
    danger_soft: rgb(0x5a, 0x2a, 0x26),
    ok: rgb(0x8b, 0xc3, 0x4a),
    ok_soft: rgb(0x2c, 0x4a, 0x1c),
    info: rgb(0x6a, 0xa9, 0xd9),
    info_soft: rgb(0x1f, 0x3a, 0x52),
    tile_fg: rgb(0x1a, 0x1a, 0x1a),
    tile_bg: rgb(0xec, 0xe4, 0xd3),
    tile_border: rgb(0x0a, 0x0a, 0x0a),
    tile_red: rgb(0xc8, 0x33, 0x2a),
    tile_back: rgb(0x3d, 0x6b, 0x8a),
    tile_back_pattern: rgb(0x56, 0x89, 0xab),
    // 米色底, 数字统一红, 花色按经典实物配色.
    tile_man_num: rgb(0xc8, 0x33, 0x2a),
    tile_pin_num: rgb(0xc8, 0x33, 0x2a),
    tile_sou_num: rgb(0xc8, 0x33, 0x2a),
    tile_man_suit: rgb(0x1a, 0x1a, 0x1a), // 萬黑
    tile_pin_suit: rgb(0x2a, 0x5a, 0x8a), // 筒蓝
    tile_sou_suit: rgb(0x7a, 0x3a, 0x8a), // 索紫
    tile_chun: rgb(0xc8, 0x33, 0x2a),     // 中红
    tile_hatsu: rgb(0x3d, 0x6b, 0x1f),    // 發绿
    tile_haku: rgb(0x2a, 0x5a, 0x8a),     // 白蓝
    tile_wind: rgb(0x1a, 0x1a, 0x1a),     // 风牌黑
};

pub const LIGHT: Theme = Theme {
    bg: rgb(0xf4, 0xf1, 0xea),
    fg: rgb(0x1f, 0x1d, 0x1a),
    dim: rgb(0x9a, 0x93, 0x88),
    line: rgb(0xbc, 0xb6, 0xaa),
    panel: rgb(0xeb, 0xe6, 0xdc),
    panel_hi: rgb(0xdf, 0xd9, 0xcc),
    accent: rgb(0xa8, 0x70, 0x00),
    accent_soft: rgb(0xf0, 0xd9, 0xa8),
    danger: rgb(0x9c, 0x2a, 0x2a),
    danger_soft: rgb(0xf0, 0xc8, 0xc2),
    ok: rgb(0x3d, 0x6b, 0x1f),
    ok_soft: rgb(0xce, 0xe0, 0xbb),
    info: rgb(0x2a, 0x5a, 0x8a),
    info_soft: rgb(0xc4, 0xd8, 0xec),
    tile_fg: rgb(0x1a, 0x1a, 0x1a),
    tile_bg: rgb(0xfa, 0xfa, 0xfa),
    tile_border: rgb(0x1f, 0x1d, 0x1a),
    tile_red: rgb(0xb0, 0x28, 0x18),
    tile_back: rgb(0x3d, 0x6b, 0x8a),
    tile_back_pattern: rgb(0xbc, 0xd4, 0xe8),
    // 极浅米底, 数字深红, 花色加深保持对比.
    tile_man_num: rgb(0xb0, 0x28, 0x18),
    tile_pin_num: rgb(0xb0, 0x28, 0x18),
    tile_sou_num: rgb(0xb0, 0x28, 0x18),
    tile_man_suit: rgb(0x0a, 0x0a, 0x0a),
    tile_pin_suit: rgb(0x1a, 0x4a, 0x78),
    tile_sou_suit: rgb(0x6a, 0x2a, 0x78),
    tile_chun: rgb(0xb0, 0x28, 0x18),
    tile_hatsu: rgb(0x2a, 0x58, 0x18),
    tile_haku: rgb(0x1a, 0x4a, 0x78),
    tile_wind: rgb(0x0a, 0x0a, 0x0a),
};
