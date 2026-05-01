//! 局域网游戏 · 房间内: 显示 RoomView + ready / 改 config / 房主 start.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

use crate::net::protocol::{ClientMsg, RoomLifecycle, RoomView, ServerMsg};
use crate::net::room::{RoomCmd, RoomHandle};
use crate::ui::Transition;

/// 在线房间. 直接持 RoomHandle (发 cmd) + 自己的 inbox (收 ServerMsg).
pub struct OnlineRoomState {
    pub handle: RoomHandle,
    pub inbox: UnboundedReceiver<ServerMsg>,
    pub room_view: RoomView,
    pub my_player_id: u32,
    pub my_token: Uuid,
    pub message: String,
}

impl OnlineRoomState {
    /// 拉取所有可读消息, 更新 room_view. 检测 InGame 状态切换.
    pub fn advance(&mut self) -> Option<Transition> {
        loop {
            match self.inbox.try_recv() {
                Ok(msg) => {
                    if let Some(t) = self.handle_msg(msg) {
                        return Some(t);
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return None,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.message = "连接断开".into();
                    return None;
                }
            }
        }
    }

    fn handle_msg(&mut self, msg: ServerMsg) -> Option<Transition> {
        match msg {
            ServerMsg::Welcome {
                player_id,
                reconnect_token,
                room,
            } => {
                self.my_player_id = player_id;
                self.my_token = reconnect_token;
                self.room_view = *room;
            }
            ServerMsg::RoomUpdate(view) => {
                self.room_view = *view;
                if self.room_view.state == RoomLifecycle::InGame {
                    return Some(Transition::EnterOnlineGame);
                }
            }
            ServerMsg::GameStateView(_) => {
                return Some(Transition::EnterOnlineGame);
            }
            ServerMsg::Error(e) => {
                self.message = e;
            }
            _ => {}
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let me = self
                    .room_view
                    .players
                    .iter()
                    .find(|p| p.id == self.my_player_id);
                let new_ready = me.map(|p| !p.ready).unwrap_or(true);
                self.send(ClientMsg::Ready(new_ready));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.is_host() {
                    self.send(ClientMsg::StartGame);
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.send(ClientMsg::Leave);
                return Some(Transition::EnterMainMenu);
            }
            _ => {}
        }
        None
    }

    fn send(&self, msg: ClientMsg) {
        let _ = self.handle.tx.send(RoomCmd::PlayerMsg {
            player_id: self.my_player_id,
            msg,
        });
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" 房间 {} ", self.room_view.room_id))
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("房间 ID: {}", self.room_view.room_id),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "玩家",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for p in &self.room_view.players {
            let host_tag = if p.is_host { "★ " } else { "  " };
            let ready_tag = if p.ready {
                "[已准备]"
            } else {
                "[未准备]"
            };
            let me_tag = if p.id == self.my_player_id {
                " (你)"
            } else {
                ""
            };
            let style = if p.ready {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            lines.push(Line::from(Span::styled(
                format!("  {}{} {}{}", host_tag, p.nickname, ready_tag, me_tag),
                style,
            )));
        }

        let empty = 4usize.saturating_sub(self.room_view.players.len());
        for i in 0..empty {
            lines.push(Line::from(Span::styled(
                format!("  - 空座位 {} (开局补 AI)", i + 1),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "配置",
            Style::default().fg(Color::Cyan),
        )));
        let len_label = match self.room_view.config.length {
            crate::config::LengthRule::Hanchan => "半庄战",
            crate::config::LengthRule::Tonpuusen => "东风战",
        };
        lines.push(Line::from(format!(
            "  赛制: {} · 思考时长: {}",
            len_label,
            self.room_view
                .config
                .thinking_time_secs
                .map(|s| format!("{} 秒", s))
                .unwrap_or_else(|| "不限时".into()),
        )));

        if !self.message.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                self.message.clone(),
                Style::default().fg(Color::Red),
            )));
        }

        lines.push(Line::from(""));
        let mut hints = vec!["R 切换准备".to_string()];
        if self.is_host() {
            hints.push("Enter 开始游戏 (空座位补 AI)".into());
        }
        hints.push("L 离开房间".into());
        hints.push("Esc 回主菜单".into());
        lines.push(Line::from(Span::styled(
            hints.join("  ·  "),
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }

    fn is_host(&self) -> bool {
        self.room_view.host_id == self.my_player_id
    }
}
