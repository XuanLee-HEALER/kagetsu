//! 局域网游戏 · 房间内: 显示 RoomView + ready / 改 config / 房主 start.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::net::protocol::{ClientMsg, RoomLifecycle, RoomView, ServerMsg};
use crate::net::session::NetSession;
use crate::ui::Transition;
use crate::ui::edit_config_modal::{EditConfigModal, EditOutcome};
use crate::ui::theme::ThemeKind;

/// 在线房间. 通过 NetSession 与 server 收发 (本地房主或远程加入者皆可).
pub struct OnlineRoomState {
    pub session: NetSession,
    pub room_view: RoomView,
    pub message: String,
    /// 房主开启 config 编辑器. 非 None 时优先吃所有按键.
    pub editing_config: Option<EditConfigModal>,
    /// 当前 UI 主题 (App 切主题时同步).
    pub theme_kind: ThemeKind,
}

impl OnlineRoomState {
    pub fn new(session: NetSession, room_view: RoomView) -> Self {
        Self {
            session,
            room_view,
            message: String::new(),
            editing_config: None,
            theme_kind: ThemeKind::default(),
        }
    }

    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme_kind = kind;
    }

    pub fn my_player_id(&self) -> u32 {
        self.session.player_id
    }

    /// 拉取所有可读消息, 更新 room_view. 检测 InGame 状态切换.
    pub fn advance(&mut self) -> Option<Transition> {
        while let Some(msg) = self.session.try_recv() {
            if let Some(t) = self.handle_msg(msg) {
                return Some(t);
            }
        }
        if self.session.is_disconnected() && self.message.is_empty() {
            self.message = "连接断开".into();
        }
        None
    }

    fn handle_msg(&mut self, msg: ServerMsg) -> Option<Transition> {
        match msg {
            ServerMsg::Welcome { room, .. } => {
                self.room_view = *room;
            }
            ServerMsg::RoomUpdate(view) => {
                self.room_view = *view;
                // ZeroTrust 模式 InGame 状态等 MpStart 路由 (不走 Standard).
                if self.room_view.state == RoomLifecycle::InGame
                    && self.room_view.mode == crate::net::p2p::RoomMode::Standard
                {
                    return Some(Transition::EnterOnlineGame);
                }
            }
            ServerMsg::GameStateView(_) => {
                return Some(Transition::EnterOnlineGame);
            }
            ServerMsg::MpStart {
                all_peer_ids,
                own_index,
                session_label,
                deck_size,
                cnc_k_rounds,
            } => {
                let args = crate::ui::screens::online_zerotrust_game::MpStartArgs {
                    all_peer_ids,
                    own_index,
                    session_label,
                    deck_size,
                    cnc_k_rounds,
                };
                return Some(Transition::EnterZeroTrustGame {
                    args: Box::new(args),
                });
            }
            ServerMsg::Error { message } => {
                self.message = message;
            }
            _ => {}
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        // EditConfigModal 优先吃所有按键 (含 Esc).
        if let Some(modal) = self.editing_config.as_mut() {
            match modal.handle_key(key) {
                EditOutcome::Save(cfg) => {
                    self.session.send(ClientMsg::UpdateRules(cfg));
                    self.editing_config = None;
                    self.message = "已提交配置更新.".into();
                }
                EditOutcome::Cancel => {
                    self.editing_config = None;
                }
                EditOutcome::Pending => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let my_id = self.my_player_id();
                let me = self.room_view.players.iter().find(|p| p.id == my_id);
                let new_ready = me.map(|p| !p.ready).unwrap_or(true);
                self.session.send(ClientMsg::Ready { ready: new_ready });
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if self.is_host() {
                    self.editing_config = Some(EditConfigModal::new(self.room_view.config.clone()));
                } else {
                    self.message = "只有房主可改配置.".into();
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.is_host() {
                    self.session.send(ClientMsg::StartGame);
                }
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "离开房间",
                        "确定离开房间? 所有进度会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::LeaveOnlineRoom,
                });
            }
            KeyCode::Esc => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "回主菜单",
                        "确定离开房间回主菜单?",
                    )),
                    action: crate::ui::ConfirmAction::LeaveOnlineRoomViaEsc,
                });
            }
            _ => {}
        }
        None
    }

    pub fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        nat: Option<&crate::net::p2p::host::NatReachability>,
        dial_addr: Option<&libp2p::Multiaddr>,
    ) {
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
        // M5.E.2: 显示房间模式 (Standard / ZeroTrust).
        let (mode_label, mode_color) = match self.room_view.mode {
            crate::net::p2p::RoomMode::Standard => ("Standard (房主权威)", Color::Cyan),
            crate::net::p2p::RoomMode::ZeroTrust => {
                ("ZeroTrust (P2P mental poker, 需 4 真人)", Color::Magenta)
            }
        };
        lines.push(Line::from(vec![
            Span::raw("模式: "),
            Span::styled(mode_label, Style::default().fg(mode_color)),
        ]));

        // 房主端额外显示 NAT 状态 + dial multiaddr (给加入者复制).
        if self.is_host() {
            if let Some(reach) = nat {
                let (label, color) = match reach {
                    crate::net::p2p::host::NatReachability::Public(_) => {
                        ("Public (公网可达, 加入者可直连)", Color::Green)
                    }
                    crate::net::p2p::host::NatReachability::Private => {
                        ("Private (NAT 后, 加入者通过 relay 中转)", Color::Yellow)
                    }
                    crate::net::p2p::host::NatReachability::Unknown => {
                        ("Unknown (探测中, 等几秒)", Color::DarkGray)
                    }
                };
                lines.push(Line::from(vec![
                    Span::raw("NAT 状态: "),
                    Span::styled(label, Style::default().fg(color)),
                ]));
            }
            if let Some(addr) = dial_addr {
                let s = addr.to_string();
                let kind = if s.contains("/p2p-circuit") {
                    "通过 relay"
                } else {
                    "直连"
                };
                lines.push(Line::from(vec![
                    Span::raw(format!("加入用 ({}): ", kind)),
                    Span::styled(s, Style::default().fg(Color::Cyan)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    "加入用 multiaddr: (准备中, 等 listen 地址 + relay reservation)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
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
            let me_tag = if p.id == self.my_player_id() {
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
        let empty_label = match self.room_view.mode {
            crate::net::p2p::RoomMode::Standard => "(开局补 AI)",
            crate::net::p2p::RoomMode::ZeroTrust => "(等真人加入, ZeroTrust 不允许 AI)",
        };
        for i in 0..empty {
            lines.push(Line::from(Span::styled(
                format!("  - 空座位 {} {}", i + 1, empty_label),
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "配置",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        let cfg = &self.room_view.config;
        let len_label = match cfg.length {
            crate::engine::rules::LengthRule::Hanchan => "半庄战",
            crate::engine::rules::LengthRule::Tonpuusen => "东风战",
        };
        let thinking_label = cfg
            .thinking_time_secs
            .map(|s| format!("{} 秒", s))
            .unwrap_or_else(|| "不限时".into());
        let call_window_label = format!("{} 秒", cfg.call_window_secs);
        let entries: [(&str, String, bool); 7] = [
            ("赛制", len_label.to_string(), true),
            ("思考时长", thinking_label, true),
            ("鸣牌窗口", call_window_label, true),
            ("食断", bool_label(cfg.kuitan), cfg.kuitan),
            ("赤宝牌", bool_label(cfg.aka_dora), cfg.aka_dora),
            ("一发", bool_label(cfg.ippatsu), cfg.ippatsu),
            ("里宝牌", bool_label(cfg.ura_dora), cfg.ura_dora),
        ];
        for (key, val, on) in &entries {
            let val_color = match *key {
                "赛制" | "思考时长" | "鸣牌窗口" => Color::Yellow,
                _ => {
                    if *on {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }
                }
            };
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}: ", key)),
                Span::styled(val.clone(), Style::default().fg(val_color)),
            ]));
        }

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
            hints.push("C 改配置".into());
            hints.push("Enter 开始游戏 (空座位补 AI)".into());
        }
        hints.push("L 离开房间".into());
        hints.push("Esc 回主菜单".into());
        lines.push(Line::from(Span::styled(
            hints.join("  ·  "),
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);

        // EditConfigModal 叠加在最上层.
        if let Some(modal) = &self.editing_config {
            let theme = self.theme_kind.theme();
            modal.render(f.buffer_mut(), area, &theme);
        }
    }

    fn is_host(&self) -> bool {
        self.room_view.host_id == self.my_player_id()
    }
}

fn bool_label(v: bool) -> String {
    if v { "开".into() } else { "关".into() }
}
