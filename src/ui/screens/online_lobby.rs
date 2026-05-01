//! 局域网大厅: 输入 nickname → 创建房间 / 加入房间.
//! 同时跑 mDNS browser 自动发现局域网内房间.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::net::discovery::{DiscoveryBrowser, RoomEntry};
use crate::ui::Transition;

/// 大厅项目焦点.
const FOCUS_NICKNAME: usize = 0;
const FOCUS_CREATE: usize = 1;
const FOCUS_DISCOVERED: usize = 2;
const FOCUS_ADDR: usize = 3;
const FOCUS_JOIN: usize = 4;
const ITEM_COUNT: usize = 5;

pub struct OnlineLobbyState {
    pub nickname: String,
    /// 加入房间用的 host 地址, 形如 `192.168.1.5:34567`.
    pub addr: String,
    pub focus: usize,
    pub message: String,
    /// mDNS browser, 启动失败时 None (e.g. 容器/受限网络).
    pub browser: Option<DiscoveryBrowser>,
    /// 当前发现到的房间列表 (每帧 poll 更新).
    pub discovered: Vec<RoomEntry>,
    /// discovered 列表里选中行 (focus=FOCUS_DISCOVERED 时生效).
    pub discovered_selected: usize,
}

impl OnlineLobbyState {
    pub fn new() -> Self {
        let browser = DiscoveryBrowser::start().ok();
        Self {
            nickname: String::new(),
            addr: String::new(),
            focus: FOCUS_NICKNAME,
            message: String::new(),
            browser,
            discovered: Vec::new(),
            discovered_selected: 0,
        }
    }

    pub fn with_message(message: String) -> Self {
        Self {
            message,
            ..Self::new()
        }
    }

    /// App.tick 调用: 让 browser poll mDNS 事件.
    pub fn advance(&mut self) -> Option<Transition> {
        if let Some(b) = self.browser.as_mut() {
            b.poll();
            self.discovered = b.rooms();
            if self.discovered_selected >= self.discovered.len() && !self.discovered.is_empty() {
                self.discovered_selected = self.discovered.len() - 1;
            }
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        // 在 discovered 列表内, 上下/J/K 改 discovered_selected
        if self.focus == FOCUS_DISCOVERED && !self.discovered.is_empty() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Char('J') => {
                    if self.discovered_selected + 1 < self.discovered.len() {
                        self.discovered_selected += 1;
                    }
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Char('K') => {
                    self.discovered_selected = self.discovered_selected.saturating_sub(1);
                    return None;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                self.focus = (self.focus + 1) % ITEM_COUNT;
                None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = (self.focus + ITEM_COUNT - 1) % ITEM_COUNT;
                None
            }
            KeyCode::Char(c) if self.focus == FOCUS_NICKNAME => {
                if self.nickname.chars().count() < 16 {
                    self.nickname.push(c);
                }
                None
            }
            KeyCode::Backspace if self.focus == FOCUS_NICKNAME => {
                self.nickname.pop();
                None
            }
            KeyCode::Char(c) if self.focus == FOCUS_ADDR => {
                if self.addr.chars().count() < 64 {
                    self.addr.push(c);
                }
                None
            }
            KeyCode::Backspace if self.focus == FOCUS_ADDR => {
                self.addr.pop();
                None
            }
            KeyCode::Enter => match self.focus {
                FOCUS_NICKNAME => {
                    self.focus = FOCUS_CREATE;
                    None
                }
                FOCUS_CREATE => {
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    Some(Transition::CreateOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                    })
                }
                FOCUS_DISCOVERED => {
                    if self.discovered.is_empty() {
                        self.message = "暂未发现房间, 用下方手动输 IP".into();
                        self.focus = FOCUS_ADDR;
                        return None;
                    }
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    let entry = &self.discovered[self.discovered_selected];
                    Some(Transition::JoinOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                        addr: entry.addr.clone(),
                    })
                }
                FOCUS_ADDR => {
                    self.focus = FOCUS_JOIN;
                    None
                }
                FOCUS_JOIN => {
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    if self.addr.trim().is_empty() {
                        self.message = "请输入房间地址 (形如 192.168.1.5:34567)".into();
                        self.focus = FOCUS_ADDR;
                        return None;
                    }
                    Some(Transition::JoinOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                        addr: self.addr.trim().to_string(),
                    })
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 局域网游戏 · 大厅 ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));

        // 昵称输入
        let nickname_label = if self.focus == FOCUS_NICKNAME {
            "▶ 昵称: "
        } else {
            "  昵称: "
        };
        let mut nickname_text = self.nickname.clone();
        if self.focus == FOCUS_NICKNAME {
            nickname_text.push('_');
        }
        lines.push(Line::from(vec![
            Span::raw(nickname_label),
            Span::styled(
                nickname_text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));

        // 创建房间按钮
        {
            let prefix = if self.focus == FOCUS_CREATE {
                "▶ "
            } else {
                "  "
            };
            let mut style = Style::default();
            if self.focus == FOCUS_CREATE {
                style = style
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!("{}创建房间 (本机做房主, 监听 LAN)", prefix),
                style,
            )));
        }
        lines.push(Line::from(""));

        // mDNS 发现到的房间列表
        let discovered_focus = self.focus == FOCUS_DISCOVERED;
        let header_prefix = if discovered_focus { "▶ " } else { "  " };
        let header_style = if discovered_focus {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(Span::styled(
            format!("{}局域网发现 ({})", header_prefix, self.discovered.len()),
            header_style,
        )));
        if self.browser.is_none() {
            lines.push(Line::from(Span::styled(
                "    (mDNS 启动失败, 用下方手动 IP)",
                Style::default().fg(Color::DarkGray),
            )));
        } else if self.discovered.is_empty() {
            lines.push(Line::from(Span::styled(
                "    暂无发现, 等几秒或手动输 IP",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (i, room) in self.discovered.iter().enumerate() {
                let cursor = if discovered_focus && i == self.discovered_selected {
                    "  ▶"
                } else {
                    "   "
                };
                let style = if discovered_focus && i == self.discovered_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {} @ {} · {}/4 · {}",
                        cursor, room.host_nick, room.addr, room.players, room.state
                    ),
                    style,
                )));
            }
            if discovered_focus {
                lines.push(Line::from(Span::styled(
                    "    (j/k 选, Enter 加入)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines.push(Line::from(""));

        // 房间地址输入
        let addr_label = if self.focus == FOCUS_ADDR {
            "▶ 地址: "
        } else {
            "  地址: "
        };
        let mut addr_text = if self.addr.is_empty() {
            "(例如 192.168.1.5:34567)".to_string()
        } else {
            self.addr.clone()
        };
        if self.focus == FOCUS_ADDR {
            addr_text.push('_');
        }
        let addr_style = if self.addr.is_empty() && self.focus != FOCUS_ADDR {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        };
        lines.push(Line::from(vec![
            Span::raw(addr_label),
            Span::styled(addr_text, addr_style),
        ]));

        // 加入房间按钮
        {
            let prefix = if self.focus == FOCUS_JOIN {
                "▶ "
            } else {
                "  "
            };
            let mut style = Style::default();
            if self.focus == FOCUS_JOIN {
                style = style
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!("{}加入房间 (输入地址后回车)", prefix),
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
            "↑↓/Tab 切焦点 · 回车 确认 (输入框时回车前进焦点)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "Esc 回主菜单 · Q 退出",
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }
}

impl Default for OnlineLobbyState {
    fn default() -> Self {
        Self::new()
    }
}
