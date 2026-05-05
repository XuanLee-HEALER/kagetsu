//! 吃 (chi) 多候选选择器 — 当 chi 选项 ≥ 2 时弹出, 让用户选哪种吃法.
//!
//! 输入: N 种 [Tile; 2] (本家 2 张) + target tile (别人切的 1 张).
//! 输出: 选中的 idx, 可直接传 do_chi / NetAction::Chi(idx).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::engine::domain::tile::Tile;
use crate::ui::paint::{paint_double_box, paint_fill, paint_str, paint_tile_wide};
use crate::ui::theme::Theme;

#[derive(Debug, Clone)]
pub enum ChiOutcome {
    Pick(usize),
    Cancel,
    Pending,
}

#[derive(Debug, Clone)]
pub struct ChiPicker {
    /// 每种吃法的本家 2 张 (与 CallOptions.chi 同).
    pub options: Vec<[Tile; 2]>,
    /// 别人切的牌 (第 3 张, 完成顺子用).
    pub target: Tile,
    pub selected: usize,
}

impl ChiPicker {
    pub fn new(options: Vec<[Tile; 2]>, target: Tile) -> Self {
        Self {
            options,
            target,
            selected: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ChiOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                ChiOutcome::Pending
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.options.len() {
                    self.selected += 1;
                }
                ChiOutcome::Pending
            }
            KeyCode::Enter | KeyCode::Char(' ') => ChiOutcome::Pick(self.selected),
            KeyCode::Esc => ChiOutcome::Cancel,
            // 数字键 1-9 直接选第 N 个
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = (c.to_digit(10).unwrap() - 1) as usize;
                if idx < self.options.len() {
                    ChiOutcome::Pick(idx)
                } else {
                    ChiOutcome::Pending
                }
            }
            _ => ChiOutcome::Pending,
        }
    }

    /// 渲染. modal 居中, 高度 = 4 边/hint + 2 行/option, 宽度固定 36.
    pub fn render(&self, buf: &mut Buffer, area: Rect, theme: &Theme) {
        let n = self.options.len() as u16;
        let w: u16 = 36;
        // 边框 (2) + 内边距上 (1) + 每选项 2 行 + 内边距下 (1) + hint (1) = 5 + 2n
        let h: u16 = 5 + 2 * n;
        if area.width < w || area.height < h {
            return;
        }
        let mx = area.x + (area.width - w) / 2;
        let my = area.y + (area.height - h) / 2;

        paint_fill(
            buf,
            mx,
            my,
            w,
            h,
            Style::default().bg(theme.panel).fg(theme.fg),
        );
        paint_double_box(buf, mx, my, w, h, theme, Some("选择吃法"));

        for (i, opts) in self.options.iter().enumerate() {
            let row = my + 1 + (i as u16) * 2;
            let highlight = i == self.selected;
            if highlight {
                paint_fill(
                    buf,
                    mx + 1,
                    row,
                    w - 2,
                    1,
                    Style::default().bg(theme.accent_soft).fg(theme.fg),
                );
            }
            // [N] 前缀
            let prefix = format!(" {}. ", i + 1);
            let prefix_style = if highlight {
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.accent_soft)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim).bg(theme.panel)
            };
            paint_str(buf, mx + 2, row, &prefix, prefix_style);
            // 3 张牌排序展示 (target + 2 张本家, 按 kind asc).
            let mut tiles = [self.target, opts[0], opts[1]];
            tiles.sort_by_key(|t| (t.kind.0, !t.red));
            let tiles_x = mx + 6;
            for (j, t) in tiles.iter().enumerate() {
                paint_tile_wide(
                    buf,
                    tiles_x + (j as u16) * 5,
                    row,
                    Some(t),
                    theme,
                    crate::ui::paint::TileState::Normal,
                );
            }
        }

        let hint = "↑↓ 选 · Enter 确认 · Esc 取消";
        paint_str(
            buf,
            mx + 2,
            my + h - 2,
            hint,
            Style::default().fg(theme.dim).bg(theme.panel),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::tile::TileIndex;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn t(kind: u8) -> Tile {
        Tile {
            id: 0,
            kind: TileIndex(kind),
            red: false,
        }
    }

    fn picker_2() -> ChiPicker {
        ChiPicker::new(vec![[t(2), t(3)], [t(3), t(5)]], t(4))
    }

    #[test]
    fn down_up_navigates() {
        let mut p = picker_2();
        assert_eq!(p.selected, 0);
        p.handle_key(key(KeyCode::Down));
        assert_eq!(p.selected, 1);
        p.handle_key(key(KeyCode::Up));
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn down_clamps_at_end() {
        let mut p = picker_2();
        for _ in 0..5 {
            p.handle_key(key(KeyCode::Down));
        }
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn enter_returns_pick() {
        let mut p = picker_2();
        p.handle_key(key(KeyCode::Down));
        match p.handle_key(key(KeyCode::Enter)) {
            ChiOutcome::Pick(i) => assert_eq!(i, 1),
            _ => panic!("expected Pick"),
        }
    }

    #[test]
    fn esc_returns_cancel() {
        let mut p = picker_2();
        assert!(matches!(
            p.handle_key(key(KeyCode::Esc)),
            ChiOutcome::Cancel
        ));
    }

    #[test]
    fn digit_picks_directly() {
        let mut p = picker_2();
        match p.handle_key(key(KeyCode::Char('2'))) {
            ChiOutcome::Pick(i) => assert_eq!(i, 1),
            _ => panic!("expected Pick(1)"),
        }
    }

    #[test]
    fn digit_out_of_range_is_pending() {
        let mut p = picker_2();
        assert!(matches!(
            p.handle_key(key(KeyCode::Char('5'))),
            ChiOutcome::Pending
        ));
    }

    #[test]
    fn render_does_not_panic() {
        let theme = crate::ui::theme::ThemeKind::Dark.theme();
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        let p = picker_2();
        p.render(&mut buf, area, &theme);
    }
}
