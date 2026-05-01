//! 局域网游戏 · 局内 (Phase 4a 占位).
//!
//! 显示 phase + 4 家分数 + 当前 turn, 不渲染牌面/河/手牌. 完整 UI (复用
//! 现有 paint_*) 留 Phase 4b.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

use crate::net::protocol::{ClientMsg, GameStateView, NetAction, RoomLifecycle, ServerMsg};
use crate::net::room::{RoomCmd, RoomHandle};
use crate::ui::Transition;

pub struct OnlineGameState {
    pub handle: RoomHandle,
    pub inbox: UnboundedReceiver<ServerMsg>,
    pub my_player_id: u32,
    pub my_token: Uuid,
    pub state_view: Option<GameStateView>,
    pub message: String,
}

impl OnlineGameState {
    pub fn new(
        handle: RoomHandle,
        inbox: UnboundedReceiver<ServerMsg>,
        my_player_id: u32,
        my_token: Uuid,
    ) -> Self {
        Self {
            handle,
            inbox,
            my_player_id,
            my_token,
            state_view: None,
            message: "等待 server 推送状态...".into(),
        }
    }

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
            ServerMsg::GameStateView(view) => {
                self.state_view = Some(*view);
                self.message.clear();
            }
            ServerMsg::RoundResult(r) => {
                self.message = format!("局结算: {} | 分数 {:?}", r.message, r.scores);
            }
            ServerMsg::GameEnd(_) => {
                self.message = "整庄结束, 按 Esc 回主菜单".into();
            }
            ServerMsg::RoomUpdate(view) => {
                if view.state == RoomLifecycle::Lobby {
                    return Some(Transition::EnterMainMenu);
                }
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
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.send(ClientMsg::Leave);
                return Some(Transition::EnterMainMenu);
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.send(ClientMsg::Action(NetAction::NextRound));
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
            .title(" 局域网游戏 · 局内 (Phase 4a 占位) ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));

        if let Some(view) = &self.state_view {
            lines.push(Line::from(Span::styled(
                format!(
                    "{:?} {} 局 · 本场 {} · 立直棒 {} · 山 {}",
                    view.round_wind,
                    view.kyoku,
                    view.honba,
                    view.riichi_sticks,
                    view.wall_remaining
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!(
                "Phase: {:?} · 当前 turn: {:?}",
                view.phase, view.turn
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "玩家分数",
                Style::default().fg(Color::Cyan),
            )));
            for p in &view.players {
                let me_tag = if p.seat == view.my_seat { " (你)" } else { "" };
                lines.push(Line::from(format!(
                    "  {:?} {} · {} 点 · 手牌 {} 张{}",
                    p.seat, p.nickname, p.score, p.hand_count, me_tag
                )));
            }
            lines.push(Line::from(""));
            let hand_short: Vec<String> = view.my_hand.iter().map(|t| t.kind.short()).collect();
            lines.push(Line::from(format!(
                "你的手牌 ({}): {}",
                view.my_hand.len(),
                hand_short.join(" ")
            )));
            if let Some(d) = view.my_last_drawn {
                lines.push(Line::from(format!("摸到: {}", d.kind.short())));
            }
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
            "N 下一局 · L 离开 · Esc 回主菜单",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "(完整 UI 渲染待 Phase 4b 实施)",
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }
}
