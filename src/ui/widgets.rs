//! 跨屏共享的渲染件: 牌张/河/副露/座位标签等.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::meld::{Meld, MeldKind, Seat};
use crate::tile::Tile;

/// 河的列宽(每行最多几张).
pub const RIVER_COLS: usize = 6;

/// 返回(显示文本, 默认颜色). 文本永远 2 列宽: 数牌 "5p", 字牌单字符宽中文(占 2 列).
pub fn tile_label(t: Tile) -> (String, Color) {
    let suit_color = match t.kind.0 {
        0..=8 => Color::Yellow,
        9..=17 => Color::Cyan,
        18..=26 => Color::Green,
        27..=30 => Color::White, // 风牌
        31 => Color::White,      // 白
        32 => Color::Green,      // 發
        33 => Color::Red,        // 中
        _ => Color::DarkGray,
    };
    let text = match t.kind.0 {
        0..=8 => {
            let n = if t.red && t.kind.0 == 4 {
                0
            } else {
                t.kind.0 + 1
            };
            format!("{}m", n)
        }
        9..=17 => {
            let n = if t.red && t.kind.0 == 13 {
                0
            } else {
                t.kind.0 - 9 + 1
            };
            format!("{}p", n)
        }
        18..=26 => {
            let n = if t.red && t.kind.0 == 22 {
                0
            } else {
                t.kind.0 - 18 + 1
            };
            format!("{}s", n)
        }
        27 => "東".into(),
        28 => "南".into(),
        29 => "西".into(),
        30 => "北".into(),
        31 => "白".into(),
        32 => "發".into(),
        33 => "中".into(),
        _ => "??".into(),
    };
    (text, suit_color)
}

pub fn separator_span() -> Span<'static> {
    Span::styled("│", Style::default().fg(Color::DarkGray))
}

pub fn tile_content_span(t: Tile, selected: bool, drawn: bool) -> Span<'static> {
    let (text, color) = tile_label(t);
    let mut style = Style::default().fg(color);
    if t.red {
        style = style.fg(Color::Red).add_modifier(Modifier::BOLD);
    }
    if drawn {
        style = style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
    }
    if selected {
        style = Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

/// 副露区(单行 inline). 每组用 `[标签 牌 牌 牌]` 的样式, 暗杠中间两张盖牌.
pub fn render_melds_inline(melds: &[Meld]) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for meld in melds {
        let (label, label_color) = match &meld.kind {
            MeldKind::Chi { .. } => ("吃", Color::Cyan),
            MeldKind::Pon { .. } => ("碰", Color::Cyan),
            MeldKind::Minkan { .. } => ("明杠", Color::Magenta),
            MeldKind::Shouminkan { .. } => ("加杠", Color::Magenta),
            MeldKind::Ankan { .. } => ("暗杠", Color::DarkGray),
        };
        out.push(Span::styled(
            format!("[{}", label),
            Style::default().fg(label_color),
        ));
        let mut sorted: Vec<Tile> = meld.tiles().to_vec();
        sorted.sort_by_key(|t| t.kind.0);
        for (i, t) in sorted.iter().enumerate() {
            let show_back = matches!(meld.kind, MeldKind::Ankan { .. }) && (i == 0 || i == 3);
            out.push(Span::raw(" "));
            if show_back {
                out.push(Span::styled("▒▒", Style::default().fg(Color::DarkGray)));
            } else {
                let (text, color) = tile_label(*t);
                let style = if t.red {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                out.push(Span::styled(text, style));
            }
        }
        out.push(Span::styled("] ", Style::default().fg(label_color)));
    }
    out
}

/// 河(多行 outline). 按弃牌顺序 6 列分行, 每张牌 `│xx` 紧贴.
pub fn render_river_lines(river: &[Tile]) -> Vec<Line<'static>> {
    if river.is_empty() {
        return vec![Line::from(Span::styled(
            "(空)",
            Style::default().fg(Color::DarkGray),
        ))];
    }
    river
        .chunks(RIVER_COLS)
        .map(|chunk| {
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(chunk.len() * 2 + 1);
            for t in chunk {
                spans.push(separator_span());
                let (text, color) = tile_label(*t);
                let style = if t.red {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                spans.push(Span::styled(text, style));
            }
            spans.push(separator_span());
            Line::from(spans)
        })
        .collect()
}

pub fn seat_label(s: Seat) -> &'static str {
    match s {
        Seat::East => "东",
        Seat::South => "南",
        Seat::West => "西",
        Seat::North => "北",
    }
}
