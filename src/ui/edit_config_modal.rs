//! 房主在 OnlineRoom 内修改 GameRules 的 modal.
//!
//! 7 字段: 赛制 / 思考时长 / 鸣牌窗口 / 食断 / 赤宝牌 / 一发 / 里宝牌.
//! Enter 保存 → ClientMsg::UpdateRules, Esc 取消.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::engine::rules::{GameRules, LengthRule};
use crate::ui::paint::{paint_double_box, paint_fill, paint_str};
use crate::ui::theme::Theme;

const FIELDS: usize = 7;
const THINKING_OPTIONS: &[Option<u32>] = &[Some(10), Some(20), Some(30), Some(60), None];
const CALL_WINDOW_OPTIONS: &[u8] = &[3, 5, 8];

#[derive(Debug, Clone)]
pub enum EditOutcome {
    Save(GameRules),
    Cancel,
    Pending,
}

#[derive(Debug, Clone)]
pub struct EditConfigModal {
    pub config: GameRules,
    pub selected: usize,
}

impl EditConfigModal {
    pub fn new(current: GameRules) -> Self {
        Self {
            config: current,
            selected: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EditOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                EditOutcome::Pending
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < FIELDS {
                    self.selected += 1;
                }
                EditOutcome::Pending
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_field(-1);
                EditOutcome::Pending
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.cycle_field(1);
                EditOutcome::Pending
            }
            KeyCode::Char(' ') => {
                self.cycle_field(1);
                EditOutcome::Pending
            }
            KeyCode::Enter => EditOutcome::Save(self.config.clone()),
            KeyCode::Esc => EditOutcome::Cancel,
            _ => EditOutcome::Pending,
        }
    }

    /// 调整当前字段的值. dir = +1 / -1, 布尔字段忽略方向直接 toggle.
    fn cycle_field(&mut self, dir: i32) {
        match self.selected {
            0 => {
                // 赛制
                self.config.length = match self.config.length {
                    LengthRule::Hanchan => LengthRule::Tonpuusen,
                    LengthRule::Tonpuusen => LengthRule::Hanchan,
                };
            }
            1 => {
                // 思考时长
                let cur_idx = THINKING_OPTIONS
                    .iter()
                    .position(|t| *t == self.config.thinking_time_secs)
                    .unwrap_or(0);
                let len = THINKING_OPTIONS.len() as i32;
                let new_idx = (cur_idx as i32 + dir).rem_euclid(len) as usize;
                self.config.thinking_time_secs = THINKING_OPTIONS[new_idx];
            }
            2 => {
                // 鸣牌窗口
                let cur_idx = CALL_WINDOW_OPTIONS
                    .iter()
                    .position(|&t| t == self.config.call_window_secs)
                    .unwrap_or(1);
                let len = CALL_WINDOW_OPTIONS.len() as i32;
                let new_idx = (cur_idx as i32 + dir).rem_euclid(len) as usize;
                self.config.call_window_secs = CALL_WINDOW_OPTIONS[new_idx];
            }
            3 => self.config.kuitan = !self.config.kuitan,
            4 => self.config.aka_dora = !self.config.aka_dora,
            5 => self.config.ippatsu = !self.config.ippatsu,
            6 => self.config.ura_dora = !self.config.ura_dora,
            _ => {}
        }
    }

    pub fn render(&self, buf: &mut Buffer, area: Rect, theme: &Theme) {
        let w: u16 = 56;
        // 7 字段每字段 2 行 = 14 行内容 + 边框 + 内边距 + hint 行 = 18 行.
        let h: u16 = 18;
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
        paint_double_box(buf, mx, my, w, h, theme, Some("修改房间配置"));

        for i in 0..FIELDS {
            let row = my + 2 + (i as u16) * 2;
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
            let (label, value) = self.field_display(i);
            let label_style = if highlight {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.accent_soft)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg).bg(theme.panel)
            };
            paint_str(buf, mx + 3, row, label, label_style);
            // 值右对齐
            let val_w = value.width() as u16;
            let val_x = mx + w - 3 - val_w;
            let val_style = if highlight {
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.accent_soft)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim).bg(theme.panel)
            };
            paint_str(buf, val_x, row, &value, val_style);
        }

        // 底部 hint
        let hint = "↑↓ 选字段 · ←→/Space 改值 · Enter 保存 · Esc 取消";
        let hint_x = mx + 2;
        paint_str(
            buf,
            hint_x,
            my + h - 2,
            hint,
            Style::default().fg(theme.dim).bg(theme.panel),
        );
    }

    fn field_display(&self, idx: usize) -> (&'static str, String) {
        match idx {
            0 => (
                "赛制",
                match self.config.length {
                    LengthRule::Hanchan => "半庄战".into(),
                    LengthRule::Tonpuusen => "东风战".into(),
                },
            ),
            1 => (
                "思考时长",
                self.config
                    .thinking_time_secs
                    .map(|s| format!("{} 秒", s))
                    .unwrap_or_else(|| "不限时".into()),
            ),
            2 => ("鸣牌窗口", format!("{} 秒", self.config.call_window_secs)),
            3 => ("食断", bool_label(self.config.kuitan)),
            4 => ("赤宝牌", bool_label(self.config.aka_dora)),
            5 => ("一发", bool_label(self.config.ippatsu)),
            6 => ("里宝牌", bool_label(self.config.ura_dora)),
            _ => ("?", "?".into()),
        }
    }
}

fn bool_label(v: bool) -> String {
    if v {
        "☑ 开".into()
    } else {
        "☐ 关".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn default_config() -> GameRules {
        GameRules::default()
    }

    #[test]
    fn down_up_navigates_fields() {
        let mut m = EditConfigModal::new(default_config());
        assert_eq!(m.selected, 0);
        m.handle_key(key(KeyCode::Down));
        assert_eq!(m.selected, 1);
        m.handle_key(key(KeyCode::Down));
        assert_eq!(m.selected, 2);
        m.handle_key(key(KeyCode::Up));
        assert_eq!(m.selected, 1);
    }

    #[test]
    fn down_clamps_at_last_field() {
        let mut m = EditConfigModal::new(default_config());
        for _ in 0..10 {
            m.handle_key(key(KeyCode::Down));
        }
        assert_eq!(m.selected, FIELDS - 1);
    }

    #[test]
    fn up_clamps_at_first_field() {
        let mut m = EditConfigModal::new(default_config());
        for _ in 0..5 {
            m.handle_key(key(KeyCode::Up));
        }
        assert_eq!(m.selected, 0);
    }

    #[test]
    fn left_right_toggles_length() {
        let mut m = EditConfigModal::new(default_config());
        let initial = m.config.length;
        m.handle_key(key(KeyCode::Right));
        assert_ne!(m.config.length, initial);
        m.handle_key(key(KeyCode::Left));
        assert_eq!(m.config.length, initial);
    }

    #[test]
    fn space_toggles_bool_fields() {
        let mut m = EditConfigModal::new(default_config());
        // 转到食断 (idx 3, 在赛制/思考时长/鸣牌窗口之后)
        for _ in 0..3 {
            m.handle_key(key(KeyCode::Down));
        }
        let initial = m.config.kuitan;
        m.handle_key(key(KeyCode::Char(' ')));
        assert_ne!(m.config.kuitan, initial);
    }

    #[test]
    fn right_cycles_call_window() {
        let mut m = EditConfigModal::new(default_config());
        // 跳到鸣牌窗口 (idx 2)
        m.handle_key(key(KeyCode::Down));
        m.handle_key(key(KeyCode::Down));
        let initial = m.config.call_window_secs;
        m.handle_key(key(KeyCode::Right));
        assert_ne!(m.config.call_window_secs, initial);
        // 3 次循环回到原值 (CALL_WINDOW_OPTIONS 长度 3)
        for _ in 0..2 {
            m.handle_key(key(KeyCode::Right));
        }
        assert_eq!(m.config.call_window_secs, initial);
    }

    #[test]
    fn right_cycles_thinking_time() {
        let mut m = EditConfigModal::new(default_config());
        m.handle_key(key(KeyCode::Down));
        let initial = m.config.thinking_time_secs;
        m.handle_key(key(KeyCode::Right));
        assert_ne!(m.config.thinking_time_secs, initial);
        // 5 次循环应回到原值
        for _ in 0..4 {
            m.handle_key(key(KeyCode::Right));
        }
        assert_eq!(m.config.thinking_time_secs, initial);
    }

    #[test]
    fn enter_returns_save_with_modified_config() {
        let mut m = EditConfigModal::new(default_config());
        let original_length = m.config.length;
        m.handle_key(key(KeyCode::Right)); // 切赛制
        let outcome = m.handle_key(key(KeyCode::Enter));
        match outcome {
            EditOutcome::Save(cfg) => {
                assert_ne!(cfg.length, original_length);
            }
            _ => panic!("expected Save"),
        }
    }

    #[test]
    fn esc_returns_cancel() {
        let mut m = EditConfigModal::new(default_config());
        assert!(matches!(
            m.handle_key(key(KeyCode::Esc)),
            EditOutcome::Cancel
        ));
    }

    #[test]
    fn render_does_not_panic() {
        let theme = crate::ui::theme::ThemeKind::Dark.theme();
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        let m = EditConfigModal::new(default_config());
        m.render(&mut buf, area, &theme);
    }
}
