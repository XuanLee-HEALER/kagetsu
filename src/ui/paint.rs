//! 高保真渲染原语 — 直接写 Buffer.
//!
//! 所有 paint_* 函数采用绝对坐标 (x, y), 与设计稿 hifi-05.jsx 的 col/row 一致.
//! 牌张支持 wide (4 cells, 全角) / tight (3 cells, 紧凑) 两种模式.

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::tile::{Tile, TileIndex};
use crate::ui::theme::Theme;

/// 牌张视觉状态.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    /// 正常显示.
    Normal,
    /// 选中 (BoxedTile 才有意义, 边框换 accent + 下方 ▲).
    Selected,
    /// 摸到 (BoxedTile 才有意义, 反色背景 + 下方"摸").
    Drawn,
    /// 立直 (横置, 简化为红色边框).
    Riichi,
    /// 牌背.
    Back,
    /// 危険 (淡化).
    Dimmed,
}

const NUMERALS: [&str; 10] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
const NUMERALS_ASCII: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const HONORS: [&str; 7] = ["東", "南", "西", "北", "白", "發", "中"];
const SUIT_CN: [&str; 3] = ["萬", "筒", "索"];

/// 写一段文本到 (x, y), 单行. 不做宽度截断.
pub fn paint_str(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style) {
    let area = buf.area;
    if y >= area.y + area.height {
        return;
    }
    if x >= area.x + area.width {
        return;
    }
    buf.set_string(x, y, s, style);
}

/// 整块矩形填充背景色 (用于 panel 区).
pub fn paint_fill(buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16, style: Style) {
    let area = buf.area;
    let x_end = (x + w).min(area.x + area.width);
    let y_end = (y + h).min(area.y + area.height);
    for cy in y..y_end {
        for cx in x..x_end {
            if let Some(cell) = buf.cell_mut((cx, cy)) {
                cell.set_char(' ');
                cell.set_style(style);
            }
        }
    }
}

/// 单张 wide 牌的文本 (4 cells = 2 全角字符).
fn tile_text_wide(kind: TileIndex) -> String {
    let n = kind.0;
    match n {
        0..=8 => format!("{}{}", NUMERALS[(n + 1) as usize], SUIT_CN[0]),
        9..=17 => format!("{}{}", NUMERALS[(n - 9 + 1) as usize], SUIT_CN[1]),
        18..=26 => format!("{}{}", NUMERALS[(n - 18 + 1) as usize], SUIT_CN[2]),
        27..=33 => format!("{}  ", HONORS[(n - 27) as usize]),
        _ => "??  ".into(),
    }
}

/// 单张 tight 牌的文本 (3 cells: 1 半角数字 + 1 全角花色 / 1 全角字牌 + 1 空格).
fn tile_text_tight(kind: TileIndex) -> String {
    let n = kind.0;
    match n {
        0..=8 => format!("{}{}", NUMERALS_ASCII[(n + 1) as usize], SUIT_CN[0]),
        9..=17 => format!("{}{}", NUMERALS_ASCII[(n - 9 + 1) as usize], SUIT_CN[1]),
        18..=26 => format!("{}{}", NUMERALS_ASCII[(n - 18 + 1) as usize], SUIT_CN[2]),
        27..=33 => format!("{} ", HONORS[(n - 27) as usize]),
        _ => "?? ".into(),
    }
}

/// 单张 wide 牌渲染到 (x, y). 占 4 cells, 1 行. 含边框 (左右 │, 上下 ─).
/// 实际占用 4 cells 宽 × 1 行高.  牌内容居中.
pub fn paint_tile_wide(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tile: Option<&Tile>,
    theme: &Theme,
    state: TileState,
) {
    let (text, fg, bg) = match state {
        TileState::Back => ("▒▒▒▒".to_string(), theme.tile_back_pattern, theme.tile_back),
        _ => match tile {
            Some(t) => {
                let mut fg = theme.tile_fg;
                if t.red {
                    fg = theme.tile_red;
                }
                (tile_text_wide(t.kind), fg, theme.tile_bg)
            }
            None => ("    ".into(), theme.tile_fg, theme.tile_bg),
        },
    };

    let mut style = Style::default().fg(fg).bg(bg);
    if state == TileState::Riichi {
        style = style.fg(theme.danger);
    }
    if state == TileState::Dimmed {
        style = style.add_modifier(Modifier::DIM);
    }
    paint_str(buf, x, y, &text, style);
}

/// 单张 tight 牌 (3 cells, 1 行).
pub fn paint_tile_tight(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tile: Option<&Tile>,
    theme: &Theme,
    state: TileState,
) {
    let (text, fg, bg) = match state {
        TileState::Back => ("▒▒▒".to_string(), theme.tile_back_pattern, theme.tile_back),
        _ => match tile {
            Some(t) => {
                let fg = if t.red { theme.tile_red } else { theme.tile_fg };
                (tile_text_tight(t.kind), fg, theme.tile_bg)
            }
            None => ("   ".into(), theme.tile_fg, theme.tile_bg),
        },
    };
    let mut style = Style::default().fg(fg).bg(bg);
    if state == TileState::Riichi {
        style = style.fg(theme.danger);
    }
    if state == TileState::Dimmed {
        style = style.add_modifier(Modifier::DIM);
    }
    paint_str(buf, x, y, &text, style);
}

/// 自家手牌行: 13 张 (或更少) BoxedTile 共边盒子, 4 cells × 3 行.
///
/// 共边布局: `┌────┬────┬────┐` / `│一萬│二萬│三萬│` / `└────┴────┴────┘`
/// 每盒占 4 cells 宽 (4 内容 + 1 共享右 │, 末尾 +1 总计宽 = 4N+1).
/// 选中态: 那一格的左/右/上/下边换 accent 色, 第 4 行写 ▲.
/// 摸到态: 那一格内容反色 (bg=accent, fg=bg), 第 4 行写「摸」.
///
/// drawn_offset = Some(idx) 表示 idx 处是新摸的牌, 在它前面留 1 cell 间隙.
pub fn paint_boxed_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tiles: &[Tile],
    theme: &Theme,
    selected: Option<usize>,
    drawn_idx: Option<usize>,
) {
    if tiles.is_empty() {
        return;
    }
    let border_style = Style::default().fg(theme.tile_border).bg(theme.bg);
    let accent_style = Style::default()
        .fg(theme.accent)
        .bg(theme.bg)
        .add_modifier(Modifier::BOLD);

    let n = tiles.len();
    // 计算每张牌的左 x 坐标 (用于 selected/drawn 间留隙).
    let drawn_gap = 1u16;
    let positions: Vec<u16> = {
        let mut out = Vec::with_capacity(n);
        let mut cx = x;
        for i in 0..n {
            if Some(i) == drawn_idx && i > 0 {
                cx += drawn_gap;
            }
            out.push(cx);
            cx += 5; // 5 = 4 内容 + 1 右边框 (┬/┐)
        }
        out
    };

    // 顶行: 每盒在 positions[i] 起点画 "┌────" (5 chars), 末尾 +"┐"
    // 但选中/摸到的盒子换 accent 色.
    for (i, &cx) in positions.iter().enumerate() {
        let st = if Some(i) == selected || Some(i) == drawn_idx {
            accent_style
        } else {
            border_style
        };
        // 左边角: 第一个用 ┌, 后续用 ┬, 但若前一格不存在(drawn 留隙) 也用 ┌
        let prev_exists = i > 0 && positions[i - 1] + 5 == cx;
        let left = if i == 0 {
            "┌"
        } else if prev_exists {
            "┬"
        } else {
            "┌"
        };
        paint_str(buf, cx, y, left, st);
        paint_str(buf, cx + 1, y, "────", st);
    }
    // 末尾右边角
    if let Some(&last_x) = positions.last() {
        let st = if Some(n - 1) == selected || Some(n - 1) == drawn_idx {
            accent_style
        } else {
            border_style
        };
        paint_str(buf, last_x + 5, y, "┐", st);
    }

    // 中行: 内容 + 边框
    for (i, &cx) in positions.iter().enumerate() {
        let t = &tiles[i];
        let is_sel = Some(i) == selected;
        let is_drawn = Some(i) == drawn_idx;

        // 左边框
        let left_st = if is_sel || is_drawn {
            accent_style
        } else {
            border_style
        };
        // 但如果前一张也是 selected/drawn, 共享边框颜色
        let prev_exists = i > 0 && positions[i - 1] + 5 == cx;
        let prev_hot = prev_exists && (Some(i - 1) == selected || Some(i - 1) == drawn_idx);
        let left_color = if is_sel || is_drawn || prev_hot {
            accent_style
        } else {
            left_st
        };
        paint_str(buf, cx, y + 1, "│", left_color);

        // 内容
        let content = tile_text_wide(t.kind);
        let (fg, bg) = if is_drawn {
            (theme.bg, theme.accent)
        } else if t.red {
            (theme.tile_red, theme.tile_bg)
        } else {
            (theme.tile_fg, theme.tile_bg)
        };
        let style = Style::default().fg(fg).bg(bg);
        paint_str(buf, cx + 1, y + 1, &content, style);
    }
    // 末尾右边框
    if let Some(&last_x) = positions.last() {
        let st = if Some(n - 1) == selected || Some(n - 1) == drawn_idx {
            accent_style
        } else {
            border_style
        };
        paint_str(buf, last_x + 5, y + 1, "│", st);
    }

    // 底行
    for (i, &cx) in positions.iter().enumerate() {
        let st = if Some(i) == selected || Some(i) == drawn_idx {
            accent_style
        } else {
            border_style
        };
        let prev_exists = i > 0 && positions[i - 1] + 5 == cx;
        let left = if i == 0 || !prev_exists { "└" } else { "┴" };
        paint_str(buf, cx, y + 2, left, st);
        paint_str(buf, cx + 1, y + 2, "────", st);
    }
    if let Some(&last_x) = positions.last() {
        let st = if Some(n - 1) == selected || Some(n - 1) == drawn_idx {
            accent_style
        } else {
            border_style
        };
        paint_str(buf, last_x + 5, y + 2, "┘", st);
    }

    // 第 4 行: 选中 ▲ / 摸到 摸
    for (i, &cx) in positions.iter().enumerate() {
        let center = cx + 2; // 4 cells 内容居中点 (2)
        if Some(i) == selected {
            paint_str(buf, center, y + 3, "▲", accent_style);
        } else if Some(i) == drawn_idx {
            paint_str(buf, center, y + 3, "摸", accent_style);
        }
    }
}

/// 6 列弃牌网格 (wide 模式 4 cells).
/// 占 24 cells × 4 行 (最多 24 张).
pub fn paint_discard_grid_wide(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    river: &[Tile],
    theme: &Theme,
    riichi_at: Option<usize>,
) {
    const COLS: usize = 6;
    const MAX: usize = COLS * 4;
    for (i, t) in river.iter().take(MAX).enumerate() {
        let r = (i / COLS) as u16;
        let c = (i % COLS) as u16;
        let cx = x + c * 4;
        let cy = y + r;
        let state = if Some(i) == riichi_at {
            TileState::Riichi
        } else {
            TileState::Normal
        };
        paint_tile_wide(buf, cx, cy, Some(t), theme, state);
    }
}

/// 13 张牌背水平排成一行 (对家用), wide 模式, 4 cells × 1 行.
pub fn paint_back_row_wide(buf: &mut Buffer, x: u16, y: u16, count: usize, theme: &Theme) {
    for i in 0..count {
        paint_tile_wide(buf, x + (i as u16) * 4, y, None, theme, TileState::Back);
    }
}

/// 13 张牌背竖排一列 (上家/下家用), wide 模式, 每张 4 cells × 1 行.
pub fn paint_back_column_wide(buf: &mut Buffer, x: u16, y: u16, count: usize, theme: &Theme) {
    for i in 0..count {
        paint_tile_wide(buf, x, y + (i as u16), None, theme, TileState::Back);
    }
}

/// 副露 inline (一行多个), tight 模式紧凑展示.
pub fn paint_meld_row_tight(buf: &mut Buffer, x: u16, y: u16, tiles: &[Tile], theme: &Theme) {
    for (i, t) in tiles.iter().enumerate() {
        let cx = x + (i as u16) * 3;
        paint_tile_tight(buf, cx, y, Some(t), theme, TileState::Normal);
    }
}

/// 一条横线 (┌─ 风格), 颜色 = theme.line.
pub fn paint_hr(buf: &mut Buffer, x: u16, y: u16, w: u16, theme: &Theme) {
    let s = "─".repeat(w as usize);
    paint_str(buf, x, y, &s, Style::default().fg(theme.line).bg(theme.bg));
}

/// 强调横线 (━×N, accent 色).
pub fn paint_hr_accent(buf: &mut Buffer, x: u16, y: u16, w: u16, theme: &Theme) {
    let s = "━".repeat(w as usize);
    paint_str(
        buf,
        x,
        y,
        &s,
        Style::default()
            .fg(theme.accent)
            .bg(theme.bg)
            .add_modifier(Modifier::BOLD),
    );
}

/// 双线框 (用于 modal): ╔═╗║╚╝.
pub fn paint_double_box(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    theme: &Theme,
    title: Option<&str>,
) {
    if w < 2 || h < 2 {
        return;
    }
    let st = Style::default().fg(theme.accent).bg(theme.panel);
    let top = format!("╔{}╗", "═".repeat((w - 2) as usize));
    let bot = format!("╚{}╝", "═".repeat((w - 2) as usize));
    paint_str(buf, x, y, &top, st);
    paint_str(buf, x, y + h - 1, &bot, st);
    for i in 1..(h - 1) {
        paint_str(buf, x, y + i, "║", st);
        paint_str(buf, x + w - 1, y + i, "║", st);
    }
    if let Some(t) = title {
        let tw = t.chars().filter(|_| true).count() as u16;
        // 标题挂在第一行第 2 cell 起.
        if tw + 4 < w {
            paint_str(
                buf,
                x + 2,
                y,
                &format!(" {} ", t),
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}
