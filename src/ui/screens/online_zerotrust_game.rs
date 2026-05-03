//! ZeroTrust 模式游戏屏 (M5.B.9 scaffold).
//!
//! 房间收到 [`crate::net::protocol::ServerMsg::MpStart`] 后切到此屏. 持有
//! MpStart 参数 (own_index, all_peer_ids, session_label, deck_size, cnc_k_rounds)
//! + [`crate::net::session::NetSession`] 让 UI 仍可发送非游戏消息 (e.g. Leave).
//!
//! 当前实现是 **scaffold**:
//! - 渲染 MpStart 参数 + 状态 banner "ZeroTrust 已就绪, 等待协议层接入"
//! - 不 spawn MpPlayerActor (需要 swarm-bound mp_bridge transport, 见 M5.C)
//! - 不处理游戏动作 (Discard / Pon / Tsumo) — 留 M5.C
//!
//! 真实 P2P swarm 集成 (`spawn_mp_player + mp_bridge::SwarmTransport`)
//! 是 M5.C 的工作 — host.rs / join.rs swarm task 加 mp_topic 订阅 + rr_mp
//! event 路由 + 把 MpInbound 喂给 bridge.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::net::session::NetSession;
use crate::ui::Transition;
use crate::ui::theme::ThemeKind;

/// MpStart 参数 — 跟 [`crate::net::protocol::ServerMsg::MpStart`] 字段一致, 用结构体单独
/// 持有方便 UI 跨屏传递.
#[derive(Debug, Clone)]
pub struct MpStartArgs {
    pub all_peer_ids: Vec<Vec<u8>>,
    pub own_index: u32,
    pub session_label: Vec<u8>,
    pub deck_size: u32,
    pub cnc_k_rounds: u32,
}

pub struct ZeroTrustGameState {
    pub session: NetSession,
    pub args: MpStartArgs,
    pub theme_kind: ThemeKind,
    /// 屏内提示信息 (e.g. 连接断开 / 协议错误).
    pub message: String,
}

impl ZeroTrustGameState {
    pub fn new(session: NetSession, args: MpStartArgs) -> Self {
        Self {
            session,
            args,
            theme_kind: ThemeKind::default(),
            message: String::new(),
        }
    }

    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme_kind = kind;
    }

    /// 处理 server 推送 (drain). 当前 scaffold 仅 catch Error / 断线.
    pub fn advance(&mut self) -> Option<Transition> {
        while let Some(msg) = self.session.try_recv() {
            if let crate::net::protocol::ServerMsg::Error { message } = msg {
                self.message = message;
            }
        }
        if self.session.is_disconnected() && self.message.is_empty() {
            self.message = "连接断开".into();
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('L') => {
                Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "离开 ZeroTrust 游戏",
                        "确定离开? 当前局会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::LeaveOnlineGame,
                })
            }
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = self.theme_kind.theme();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // 标题
                Constraint::Length(7), // MpStart 参数
                Constraint::Min(3),    // 状态 banner
                Constraint::Length(3), // 操作提示
            ])
            .split(area);

        // 标题
        let title = Paragraph::new(format!(
            "ZeroTrust 游戏 · own_index = {} / {}",
            self.args.own_index,
            self.args.all_peer_ids.len()
        ))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme.fg).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(theme.bg)),
        );
        f.render_widget(title, chunks[0]);

        // MpStart 参数
        let label_hex = hex_short(&self.args.session_label);
        let peer_lines: Vec<Line> = self
            .args
            .all_peer_ids
            .iter()
            .enumerate()
            .map(|(i, pid)| {
                let marker = if i as u32 == self.args.own_index {
                    " ← 你"
                } else {
                    ""
                };
                Line::from(vec![
                    Span::raw(format!("  player[{i}] = {}", hex_short(pid))),
                    Span::styled(marker, Style::default().fg(theme.accent)),
                ])
            })
            .collect();
        let mut info_lines = vec![
            Line::from(vec![
                Span::raw("session_label = "),
                Span::styled(label_hex, Style::default().fg(theme.accent)),
            ]),
            Line::from(format!(
                "deck_size = {} · cnc_k_rounds = {}",
                self.args.deck_size, self.args.cnc_k_rounds
            )),
        ];
        info_lines.extend(peer_lines);
        let info = Paragraph::new(info_lines)
            .style(Style::default().fg(theme.fg).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Mp 协议参数")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(info, chunks[1]);

        // 状态 banner
        let banner_text = if self.message.is_empty() {
            "ZeroTrust 模式 scaffold — 协议层已就绪 (mental_poker 协议 0-7 实现 + mp_bridge 抽象).\n等待 swarm 集成 (M5.C): host/join 加 rr_mp + mp_topic 订阅 + bridge wiring."
        } else {
            self.message.as_str()
        };
        let banner = Paragraph::new(banner_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.info).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("状态")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(banner, chunks[2]);

        // 操作提示
        let hint = Paragraph::new("Esc / L: 离开")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.dim).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(hint, chunks[3]);
    }
}

fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut out = String::with_capacity(take * 2);
    for b in &bytes[..take] {
        out.push_str(&format!("{b:02x}"));
    }
    if bytes.len() > take {
        out.push_str("..");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp_start_args_basic_construction() {
        let args = MpStartArgs {
            all_peer_ids: vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]],
            own_index: 2,
            session_label: vec![0xAA; 32],
            deck_size: 136,
            cnc_k_rounds: 80,
        };
        assert_eq!(args.all_peer_ids.len(), 4);
        assert_eq!(args.own_index, 2);
    }

    #[test]
    fn hex_short_truncates() {
        let s = hex_short(&[0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde]);
        assert_eq!(s, "abcdef123456789a..");
    }

    #[test]
    fn hex_short_no_truncate_for_short() {
        let s = hex_short(&[0xab, 0xcd]);
        assert_eq!(s, "abcd");
    }
}
