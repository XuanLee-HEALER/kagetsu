//! 主菜单屏幕.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ui::Transition;

const ITEMS: &[&str] = &["单人游戏", "在线游戏", "退出"];

#[derive(Debug, Default)]
pub struct MainMenuState {
    pub selected: usize,
    pub message: String,
}

impl MainMenuState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                None
            }
            KeyCode::Down => {
                if self.selected + 1 < ITEMS.len() {
                    self.selected += 1;
                }
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => match self.selected {
                0 => Some(Transition::EnterConfig),
                1 => Some(Transition::EnterOnlineLobby),
                2 => Some(Transition::Quit),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, startup_banner: Option<&str>) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" tui-majo · 主菜单 ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "终端日本麻将",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        // 启动一次性 banner (prefs schema 升级 / 损坏修复 / 不可访问提示).
        // 任意按键后 App 会清掉这个 banner.
        if let Some(msg) = startup_banner {
            lines.push(Line::from(Span::styled(
                format!("⚠ {}", msg),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "(按任意键关闭)",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
        }
        for (i, item) in ITEMS.iter().enumerate() {
            let prefix = if i == self.selected { "▶ " } else { "  " };
            let mut style = Style::default();
            if i == self.selected {
                style = style
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, item),
                style,
            )));
        }
        if !self.message.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                self.message.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑↓ 选 · Enter 确认",
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }
}
