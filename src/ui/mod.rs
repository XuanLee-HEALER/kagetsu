//! TUI 顶层: App + Screen 枚举 + 主循环 + 全局快捷键.
//!
//! 屏间通过 [`Transition`] 切换. App 持有 [`last_config`] / [`last_seed_choice`]
//! 用于"新游戏"复用上次配置.

pub mod screens;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

use crate::config::GameConfig;
use crate::score::Ranking;

pub use screens::config::{ConfigState, SeedChoice};
pub use screens::game::GameScreenState;
pub use screens::gameover::GameOverState;
pub use screens::main_menu::MainMenuState;

/// 屏间转换请求.
pub enum Transition {
    Quit,
    EnterMainMenu,
    EnterConfig,
    /// Config 屏 Enter 触发: 用 ConfigState 内的 config + seed_choice 起一局新游戏.
    StartGame,
    /// InGame 检测到 Phase::GameEnd 触发.
    EnterGameOver {
        rankings: [Ranking; 4],
    },
}

pub enum Screen {
    MainMenu(MainMenuState),
    Config(ConfigState),
    InGame(Box<GameScreenState>),
    GameOver(GameOverState),
}

pub struct App {
    pub screen: Screen,
    pub running: bool,
    pub last_config: GameConfig,
    pub last_seed_choice: SeedChoice,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::MainMenu(MainMenuState::new()),
            running: true,
            last_config: GameConfig::default(),
            last_seed_choice: SeedChoice::Random,
        }
    }

    pub fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        while self.running {
            terminal.draw(|f| self.render(f))?;
            self.tick()?;
        }
        Ok(())
    }

    /// 单步: poll 事件 → 全局/屏处理 → InGame advance → 应用 transition.
    fn tick(&mut self) -> Result<()> {
        let timeout = self.poll_timeout();
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(t) = self.handle_key(key)
        {
            self.apply_transition(t);
            return Ok(());
        }
        if let Screen::InGame(s) = &mut self.screen
            && let Some(t) = s.advance()
        {
            self.apply_transition(t);
        }
        Ok(())
    }

    fn poll_timeout(&self) -> Duration {
        const FRAME_MS: u64 = 80;
        if let Screen::InGame(s) = &self.screen
            && let Some(d) = s.decision_deadline
        {
            let now = Instant::now();
            let remaining = d.saturating_duration_since(now);
            return remaining.min(Duration::from_millis(FRAME_MS));
        }
        Duration::from_millis(FRAME_MS)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Transition> {
        // 全局快捷键: Q 总是退出.
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            return Some(Transition::Quit);
        }
        // Esc: 主菜单上不响应; 其它屏回主菜单.
        if key.code == KeyCode::Esc {
            return match self.screen {
                Screen::MainMenu(_) => None,
                _ => Some(Transition::EnterMainMenu),
            };
        }
        // 派发到屏.
        match &mut self.screen {
            Screen::MainMenu(s) => s.handle_event(key),
            Screen::Config(s) => s.handle_event(key),
            Screen::InGame(s) => s.handle_event(key),
            Screen::GameOver(s) => s.handle_event(key),
        }
    }

    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::Quit => {
                self.running = false;
            }
            Transition::EnterMainMenu => {
                self.screen = Screen::MainMenu(MainMenuState::new());
            }
            Transition::EnterConfig => {
                self.screen =
                    Screen::Config(ConfigState::from(&self.last_config, &self.last_seed_choice));
            }
            Transition::StartGame => {
                if let Screen::Config(c) = &self.screen {
                    self.last_config = c.config.clone();
                    self.last_seed_choice = c.seed_choice;
                }
                let seed = screens::config::resolve_seed(self.last_seed_choice);
                self.screen = Screen::InGame(Box::new(GameScreenState::new(
                    self.last_config.clone(),
                    seed,
                )));
            }
            Transition::EnterGameOver { rankings } => {
                self.screen = Screen::GameOver(GameOverState::new(rankings));
            }
        }
    }

    fn render(&self, f: &mut ratatui::Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(1)])
            .split(area);
        self.render_main(f, chunks[0]);
        self.render_global_footer(f, chunks[1]);
    }

    fn render_main(&self, f: &mut ratatui::Frame, area: Rect) {
        match &self.screen {
            Screen::MainMenu(s) => s.render(f, area),
            Screen::Config(s) => s.render(f, area),
            Screen::InGame(s) => s.render(f, area),
            Screen::GameOver(s) => s.render(f, area),
        }
    }

    fn render_global_footer(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        match &self.screen {
            Screen::MainMenu(_) => {
                spans.push(Span::styled(
                    "  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Screen::InGame(s) => {
                spans.push(Span::styled(
                    "  Esc 回主菜单  |  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
                if let Some(secs) = s.remaining_seconds() {
                    let color = if secs <= 5 { Color::Red } else { Color::Yellow };
                    spans.push(Span::styled(
                        format!("|  ⏱ 剩 {}s  ", secs),
                        Style::default().fg(color),
                    ));
                }
            }
            _ => {
                spans.push(Span::styled(
                    "  Esc 回主菜单  |  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        let p = Paragraph::new(Line::from(spans));
        f.render_widget(p, area);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
