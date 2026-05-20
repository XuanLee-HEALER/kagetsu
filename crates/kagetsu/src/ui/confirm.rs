//! 通用 ConfirmModal — 二次确认 modal 组件.
//!
//! 设计目标: 集中状态/渲染/按键处理, 屏只 trigger, App 持有.
//! 详见 plan ancient-sniffing-cake.md.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::ui::paint::{paint_double_box, paint_fill, paint_str};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    pub yes_label: String,
    pub no_label: String,
    pub selected: ConfirmChoice,
}

impl ConfirmModal {
    /// 默认 selected = No (更安全).
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            yes_label: "确认".into(),
            no_label: "取消".into(),
            selected: ConfirmChoice::No,
        }
    }

    pub fn with_labels(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        self.yes_label = yes.into();
        self.no_label = no.into();
        self
    }

    /// 处理按键. 返回 Some(choice) = 用户做出决定 (modal 应关闭),
    /// None = 仍在交互.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ConfirmChoice> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected = ConfirmChoice::Yes;
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected = ConfirmChoice::No;
                None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.selected = match self.selected {
                    ConfirmChoice::Yes => ConfirmChoice::No,
                    ConfirmChoice::No => ConfirmChoice::Yes,
                };
                None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(ConfirmChoice::Yes),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(ConfirmChoice::No),
            KeyCode::Enter | KeyCode::Char(' ') => Some(self.selected),
            _ => None,
        }
    }

    /// 居中渲染到 area. 宽 56, 高 9 (足够 1 行标题 + 2 行消息 + 1 行按钮).
    pub fn render(&self, buf: &mut Buffer, area: Rect, theme: &Theme) {
        let w: u16 = 56;
        let h: u16 = 9;
        if area.width < w || area.height < h {
            return;
        }
        let mx = area.x + (area.width - w) / 2;
        let my = area.y + (area.height - h) / 2;

        // 背景填充 panel 色.
        paint_fill(
            buf,
            mx,
            my,
            w,
            h,
            Style::default().bg(theme.panel).fg(theme.fg),
        );
        paint_double_box(buf, mx, my, w, h, theme, Some(&self.title));

        // 消息行 (第 2-3 行内). 简化: 单行截断到宽度 - 4.
        let msg_max = (w - 4) as usize;
        let msg_display = truncate_display(&self.message, msg_max);
        let msg_w = msg_display.width() as u16;
        let msg_x = mx + 2 + (w - 4 - msg_w) / 2;
        paint_str(
            buf,
            msg_x,
            my + 3,
            &msg_display,
            Style::default().fg(theme.fg).bg(theme.panel),
        );

        // 按钮行 (第 6 行).
        let button_y = my + 6;
        let yes_text = format!("[ Y · {} ]", self.yes_label);
        let no_text = format!("[ N · {} ]", self.no_label);
        let yes_w = yes_text.width() as u16;
        let no_w = no_text.width() as u16;
        let total_w = yes_w + no_w + 4; // 4 = 中间 spacing
        let start_x = mx + 1 + (w - 2 - total_w) / 2;
        let yes_x = start_x;
        let no_x = start_x + yes_w + 4;

        let yes_style = button_style(self.selected == ConfirmChoice::Yes, theme);
        let no_style = button_style(self.selected == ConfirmChoice::No, theme);
        paint_str(buf, yes_x, button_y, &yes_text, yes_style);
        paint_str(buf, no_x, button_y, &no_text, no_style);
    }
}

fn button_style(highlighted: bool, theme: &Theme) -> Style {
    if highlighted {
        Style::default()
            .fg(theme.bg)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.dim)
            .bg(theme.panel)
            .add_modifier(Modifier::BOLD)
    }
}

/// 按显示宽度截断字符串 (CJK 占 2 列), 超宽末尾加 `…`.
fn truncate_display(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.into();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > max_cols {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn default_selection_is_no() {
        let m = ConfirmModal::new("退出", "确定吗?");
        assert_eq!(m.selected, ConfirmChoice::No);
    }

    #[test]
    fn left_right_switches_selection() {
        let mut m = ConfirmModal::new("t", "m");
        // 默认 No, ← 切到 Yes
        assert_eq!(m.handle_key(key(KeyCode::Left)), None);
        assert_eq!(m.selected, ConfirmChoice::Yes);
        // → 切回 No
        assert_eq!(m.handle_key(key(KeyCode::Right)), None);
        assert_eq!(m.selected, ConfirmChoice::No);
    }

    #[test]
    fn tab_toggles() {
        let mut m = ConfirmModal::new("t", "m");
        m.handle_key(key(KeyCode::Tab));
        assert_eq!(m.selected, ConfirmChoice::Yes);
        m.handle_key(key(KeyCode::Tab));
        assert_eq!(m.selected, ConfirmChoice::No);
    }

    #[test]
    fn y_returns_yes_immediately() {
        let mut m = ConfirmModal::new("t", "m");
        assert_eq!(
            m.handle_key(key(KeyCode::Char('y'))),
            Some(ConfirmChoice::Yes)
        );
        let mut m2 = ConfirmModal::new("t", "m");
        assert_eq!(
            m2.handle_key(key(KeyCode::Char('Y'))),
            Some(ConfirmChoice::Yes)
        );
    }

    #[test]
    fn n_and_esc_return_no() {
        let mut m = ConfirmModal::new("t", "m");
        assert_eq!(
            m.handle_key(key(KeyCode::Char('n'))),
            Some(ConfirmChoice::No)
        );
        let mut m2 = ConfirmModal::new("t", "m");
        assert_eq!(m2.handle_key(key(KeyCode::Esc)), Some(ConfirmChoice::No));
    }

    #[test]
    fn enter_picks_current_selection() {
        let mut m = ConfirmModal::new("t", "m");
        // 默认 No
        assert_eq!(m.handle_key(key(KeyCode::Enter)), Some(ConfirmChoice::No));
        // 切到 Yes 后 Enter
        let mut m2 = ConfirmModal::new("t", "m");
        m2.selected = ConfirmChoice::Yes;
        assert_eq!(m2.handle_key(key(KeyCode::Enter)), Some(ConfirmChoice::Yes));
    }

    #[test]
    fn unknown_key_returns_none() {
        let mut m = ConfirmModal::new("t", "m");
        assert_eq!(m.handle_key(key(KeyCode::Char('x'))), None);
        assert_eq!(m.handle_key(key(KeyCode::Up)), None);
    }

    #[test]
    fn render_does_not_panic_in_small_area() {
        let theme = crate::ui::theme::Theme::from_kind(crate::ui::theme::ThemeKind::Dark);
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        let m = ConfirmModal::new("退出", "确定退出?");
        m.render(&mut buf, area, &theme);
        // 太小不渲染但不 panic
        let small = Rect::new(0, 0, 10, 5);
        let mut small_buf = Buffer::empty(small);
        m.render(&mut small_buf, small, &theme);
    }

    #[test]
    fn truncate_display_handles_cjk() {
        assert_eq!(super::truncate_display("hello", 10), "hello");
        // 5 CJK = 10 cols, 截到 8 cols → 3 chars + …
        let r = super::truncate_display("一二三四五", 8);
        assert!(r.ends_with('…'));
        assert!(r.width() <= 8);
    }
}
