//! 高保真渲染原语 — 直接写 Buffer.
//!
//! 所有 paint_* 函数采用绝对坐标 (x, y), 与设计稿 hifi-05.jsx 的 col/row 一致.
//! 牌张支持 wide (4 cells, 全角) / tight (3 cells, 紧凑) 两种模式.

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::domain::tile::Tile;
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

/// 牌的两段文本 + 颜色.
/// - 数牌: (数字段, 花色段)
/// - 字牌: (单字段, 空 padding)
struct TileSegments<'a> {
    seg1: &'a str,
    seg2: &'a str,
    seg1_color: ratatui::style::Color,
    seg2_color: ratatui::style::Color,
}

/// 拆解 wide 牌成两段 (各 2 cells = 1 全角中文).
fn tile_segments_wide(t: &Tile, theme: &Theme) -> TileSegments<'static> {
    let n = t.kind.0;
    if n <= 26 {
        let suit_idx = (n / 9) as usize;
        let num_idx = (n % 9 + 1) as usize;
        let num_color = if t.red {
            theme.tile_red
        } else {
            [theme.tile_man_num, theme.tile_pin_num, theme.tile_sou_num][suit_idx]
        };
        let suit_color = [
            theme.tile_man_suit,
            theme.tile_pin_suit,
            theme.tile_sou_suit,
        ][suit_idx];
        TileSegments {
            seg1: NUMERALS[num_idx],
            seg2: SUIT_CN[suit_idx],
            seg1_color: num_color,
            seg2_color: suit_color,
        }
    } else if n <= 33 {
        let honor_idx = (n - 27) as usize;
        let fg = match n {
            33 => theme.tile_chun,
            32 => theme.tile_hatsu,
            31 => theme.tile_haku,
            _ => theme.tile_wind,
        };
        TileSegments {
            seg1: HONORS[honor_idx],
            seg2: "  ", // 字牌只占 2 cells, 后 2 cells 留空
            seg1_color: fg,
            seg2_color: theme.tile_fg,
        }
    } else {
        TileSegments {
            seg1: "??",
            seg2: "  ",
            seg1_color: theme.tile_fg,
            seg2_color: theme.tile_fg,
        }
    }
}

/// 拆解 tight 牌成两段 (1 cell ASCII 数字 + 2 cells 全角花色 / 字牌单字 + 1 cell 空格).
fn tile_segments_tight(t: &Tile, theme: &Theme) -> TileSegments<'static> {
    let n = t.kind.0;
    if n <= 26 {
        let suit_idx = (n / 9) as usize;
        let num_idx = (n % 9 + 1) as usize;
        let num_color = if t.red {
            theme.tile_red
        } else {
            [theme.tile_man_num, theme.tile_pin_num, theme.tile_sou_num][suit_idx]
        };
        let suit_color = [
            theme.tile_man_suit,
            theme.tile_pin_suit,
            theme.tile_sou_suit,
        ][suit_idx];
        TileSegments {
            seg1: NUMERALS_ASCII[num_idx],
            seg2: SUIT_CN[suit_idx],
            seg1_color: num_color,
            seg2_color: suit_color,
        }
    } else if n <= 33 {
        let honor_idx = (n - 27) as usize;
        let fg = match n {
            33 => theme.tile_chun,
            32 => theme.tile_hatsu,
            31 => theme.tile_haku,
            _ => theme.tile_wind,
        };
        TileSegments {
            seg1: HONORS[honor_idx],
            seg2: " ", // tight 字牌: 全角占 2 cells + 1 空 = 3 cells
            seg1_color: fg,
            seg2_color: theme.tile_fg,
        }
    } else {
        TileSegments {
            seg1: "??",
            seg2: " ",
            seg1_color: theme.tile_fg,
            seg2_color: theme.tile_fg,
        }
    }
}

/// 单张 wide 牌渲染到 (x, y). 占 4 cells, 1 行.
/// 数牌分两段绘制: 数字 (前 2 cells) + 花色 (后 2 cells), 各自独立 fg.
/// 字牌单字 + 后 2 cells 空白 padding.
pub fn paint_tile_wide(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tile: Option<&Tile>,
    theme: &Theme,
    state: TileState,
) {
    if state == TileState::Back {
        let style = Style::default()
            .fg(theme.tile_back_pattern)
            .bg(theme.tile_back);
        paint_str(buf, x, y, "▒▒▒▒", style);
        return;
    }
    let Some(t) = tile else {
        let style = Style::default().fg(theme.tile_fg).bg(theme.tile_bg);
        paint_str(buf, x, y, "    ", style);
        return;
    };
    let segs = tile_segments_wide(t, theme);
    // Riichi: 整张换 danger 色 (盖 num+suit 双色).
    let (seg1_color, seg2_color) = if state == TileState::Riichi {
        (theme.danger, theme.danger)
    } else {
        (segs.seg1_color, segs.seg2_color)
    };
    let mut s1 = Style::default().fg(seg1_color).bg(theme.tile_bg);
    let mut s2 = Style::default().fg(seg2_color).bg(theme.tile_bg);
    if state == TileState::Dimmed {
        s1 = s1.add_modifier(Modifier::DIM);
        s2 = s2.add_modifier(Modifier::DIM);
    }
    if t.red {
        // 赤 5: 数字段加粗
        s1 = s1.add_modifier(Modifier::BOLD);
    }
    paint_str(buf, x, y, segs.seg1, s1);
    paint_str(buf, x + 2, y, segs.seg2, s2);
}

/// 单张 tight 牌 (3 cells, 1 行). 数字 (1 cell ASCII) + 花色 (2 cells 全角).
pub fn paint_tile_tight(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    tile: Option<&Tile>,
    theme: &Theme,
    state: TileState,
) {
    if state == TileState::Back {
        let style = Style::default()
            .fg(theme.tile_back_pattern)
            .bg(theme.tile_back);
        paint_str(buf, x, y, "▒▒▒", style);
        return;
    }
    let Some(t) = tile else {
        let style = Style::default().fg(theme.tile_fg).bg(theme.tile_bg);
        paint_str(buf, x, y, "   ", style);
        return;
    };
    let segs = tile_segments_tight(t, theme);
    let (seg1_color, seg2_color) = if state == TileState::Riichi {
        (theme.danger, theme.danger)
    } else {
        (segs.seg1_color, segs.seg2_color)
    };
    let mut s1 = Style::default().fg(seg1_color).bg(theme.tile_bg);
    let mut s2 = Style::default().fg(seg2_color).bg(theme.tile_bg);
    if state == TileState::Dimmed {
        s1 = s1.add_modifier(Modifier::DIM);
        s2 = s2.add_modifier(Modifier::DIM);
    }
    if t.red {
        s1 = s1.add_modifier(Modifier::BOLD);
    }
    // 数牌: seg1 是 1 cell ASCII, seg2 在 x+1; 字牌: seg1 是 2 cells 全角, seg2 在 x+2.
    let seg2_x = if t.kind.0 <= 26 { x + 1 } else { x + 2 };
    paint_str(buf, x, y, segs.seg1, s1);
    paint_str(buf, seg2_x, y, segs.seg2, s2);
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
    // 计算每张牌的左 x 坐标 (用于摸到的牌前留隙, 视觉上跟手牌分离).
    let drawn_gap = 3u16;
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

        // 内容: 两段绘制 (数字 + 花色 / 字牌单字).
        let segs = tile_segments_wide(t, theme);
        let bg = if is_drawn {
            theme.accent
        } else {
            theme.tile_bg
        };
        // is_drawn 时整张换 accent 反色, 失去花色区分 — 用 theme.bg 一致写两段.
        let (fg1, fg2) = if is_drawn {
            (theme.bg, theme.bg)
        } else {
            (segs.seg1_color, segs.seg2_color)
        };
        let mut s1 = Style::default().fg(fg1).bg(bg);
        let s2 = Style::default().fg(fg2).bg(bg);
        if t.red {
            s1 = s1.add_modifier(Modifier::BOLD);
        }
        paint_str(buf, cx + 1, y + 1, segs.seg1, s1);
        paint_str(buf, cx + 3, y + 1, segs.seg2, s2);
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

/// 6 列弃牌网格 (wide 模式 4 cells), 每张牌共边竖线分隔.
/// 占 31 cells × 4 行 (= 1 + 6×5 cells, 最多 24 张).
/// 布局: `│一萬│二筒│三索│ ... │`
pub fn paint_discard_grid_wide(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    river: &[Tile],
    theme: &Theme,
    riichi_at: Option<usize>,
) {
    const COLS: usize = 6;
    const ROWS: usize = 4;
    let border = Style::default().fg(theme.tile_border).bg(theme.bg);
    let empty = Style::default().bg(theme.bg).fg(theme.fg);

    for r in 0..ROWS {
        let cy = y + r as u16;
        // 左首 │
        paint_str(buf, x, cy, "│", border);
        for c in 0..COLS {
            let i = r * COLS + c;
            let cx = x + 1 + (c as u16) * 5;
            if i < river.len() {
                let state = if Some(i) == riichi_at {
                    TileState::Riichi
                } else {
                    TileState::Normal
                };
                paint_tile_wide(buf, cx, cy, Some(&river[i]), theme, state);
            } else {
                paint_str(buf, cx, cy, "    ", empty);
            }
            paint_str(buf, cx + 4, cy, "│", border);
        }
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
