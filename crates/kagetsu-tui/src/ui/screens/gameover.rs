//! 终局屏幕: 顺位 + uma + oka + 两个出口(新游戏 / 回主菜单).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use kagetsu_core::engine::score::Ranking;
use crate::ui::Transition;
use crate::ui::widgets::seat_label;

#[derive(Debug, Clone)]
pub struct GameOverState {
    pub rankings: [Ranking; 4],
    /// 0 = 新游戏, 1 = 回主菜单.
    pub selected: usize,
}

impl GameOverState {
    pub fn new(rankings: [Ranking; 4]) -> Self {
        Self {
            rankings,
            selected: 0,
        }
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Left | KeyCode::Tab => {
                self.selected = if self.selected == 0 { 1 } else { 0 };
                None
            }
            KeyCode::Right => {
                self.selected = if self.selected == 1 { 0 } else { 1 };
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => match self.selected {
                0 => Some(Transition::EnterConfig),
                1 => Some(Transition::EnterMainMenu),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 整场结束 ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "终局成绩 (单位 K = 千点)",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "  {:<4}  {:<6}  {:>8}  {:>8}  {:>6}  {:>6}  {:>8}",
            "位次", "座位", "终点", "返点差", "Uma", "Oka", "最终分",
        )));
        lines.push(Line::from(
            "  ────────────────────────────────────────────────────",
        ));
        for r in self.rankings.iter() {
            let total_color = if r.final_score > 0 {
                Color::Green
            } else if r.final_score < 0 {
                Color::Red
            } else {
                Color::White
            };
            lines.push(Line::from(vec![
                Span::raw(format!(
                    "  {:<4}  {:<6}  {:>8}  {:>+8}  {:>+6}  {:>+6}  ",
                    format!("{} 位", r.place),
                    seat_label(r.seat),
                    r.raw_score,
                    r.return_diff_k,
                    r.uma,
                    r.oka,
                )),
                Span::styled(
                    format!("{:>+8}", r.final_score),
                    Style::default()
                        .fg(total_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // 按钮
        let btn_new = if self.selected == 0 {
            Span::styled(
                " [ 新游戏 ] ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("   新游戏   ", Style::default().fg(Color::White))
        };
        let btn_menu = if self.selected == 1 {
            Span::styled(
                " [ 回主菜单 ] ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("   回主菜单   ", Style::default().fg(Color::White))
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            btn_new,
            Span::raw("    "),
            btn_menu,
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "←→ / Tab 选 · Enter 确认",
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }
}
