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

        // M5.F.3: ZeroTrust 模式 + 不足 4 真人时, 提示等待 + 不暴露 Enter 开始按键.
        let n_players = self.room_view.players.len();
        let is_zerotrust = self.room_view.mode == crate::net::p2p::RoomMode::ZeroTrust;
        let zerotrust_short = is_zerotrust && n_players < 4;
        if zerotrust_short {
            lines.push(Line::from(Span::styled(
                format!("⏳ ZeroTrust 模式需 4 真人 ({}/4 已加入)", n_players),
                Style::default().fg(Color::Magenta),
            )));
        }

        lines.push(Line::from(""));
        let mut hints = vec!["R 切换准备".to_string()];
        if self.is_host() {
            hints.push("C 改配置".into());
            if zerotrust_short {
                hints.push(format!("(Enter 开始: 等 {} 名真人)", 4 - n_players));
            } else if is_zerotrust {
                hints.push("Enter 开始游戏 (ZeroTrust)".into());
            } else {
                hints.push("Enter 开始游戏 (空座位补 AI)".into());
            }
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
            let theme = crate::ui::theme::Theme::from_kind(self.theme_kind);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::GameRules;
    use crate::net::protocol::PlayerSlot;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn make_view(
        my_id: u32,
        host_id: u32,
        n_players: usize,
        state: RoomLifecycle,
        mode: crate::net::p2p::RoomMode,
    ) -> RoomView {
        let players: Vec<PlayerSlot> = (1..=n_players as u32)
            .map(|i| PlayerSlot {
                id: i,
                nickname: format!("p{i}"),
                ready: i == host_id,
                seat: None,
                is_ai: false,
                is_host: i == host_id,
                connected: true,
            })
            .collect();
        let _ = my_id;
        RoomView {
            room_id: "rid".into(),
            host_id,
            config: GameRules::default(),
            players,
            state,
            mode,
        }
    }

    fn make_state(
        my_id: u32,
        host_id: u32,
        n_players: usize,
    ) -> (
        OnlineRoomState,
        mpsc::UnboundedReceiver<ClientMsg>,
        mpsc::UnboundedSender<ServerMsg>,
    ) {
        let (out_tx, out_rx) = mpsc::unbounded_channel::<ClientMsg>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
        let session = NetSession::from_channels(my_id, Uuid::new_v4(), out_tx, in_rx);
        let view = make_view(
            my_id,
            host_id,
            n_players,
            RoomLifecycle::Lobby,
            crate::net::p2p::RoomMode::Standard,
        );
        let state = OnlineRoomState::new(session, view);
        (state, out_rx, in_tx)
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn keycode(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    // ============================================================================
    // bool_label / is_host
    // ============================================================================

    #[test]
    fn bool_label_open_close() {
        assert_eq!(bool_label(true), "开");
        assert_eq!(bool_label(false), "关");
    }

    #[test]
    fn is_host_true_when_player_id_matches_host() {
        let (s, _, _) = make_state(1, 1, 2);
        assert!(s.is_host());
    }

    #[test]
    fn is_host_false_when_not_host() {
        let (s, _, _) = make_state(2, 1, 2);
        assert!(!s.is_host());
    }

    // ============================================================================
    // handle_event 各按键
    // ============================================================================

    #[test]
    fn r_key_sends_ready_toggle() {
        let (mut s, mut out_rx, _) = make_state(2, 1, 2);
        s.handle_event(key('R'));
        let msg = out_rx.try_recv().expect("应发 ClientMsg::Ready");
        assert!(matches!(msg, ClientMsg::Ready { ready: true }));
    }

    #[test]
    fn r_key_toggles_off_when_already_ready() {
        let (mut s, mut out_rx, _) = make_state(1, 1, 2);
        // host id=1 默认 ready=true.
        s.handle_event(key('r'));
        let msg = out_rx.try_recv().expect("应发 Ready");
        assert!(matches!(msg, ClientMsg::Ready { ready: false }));
    }

    #[test]
    fn c_key_opens_config_modal_for_host() {
        let (mut s, _, _) = make_state(1, 1, 2);
        s.handle_event(key('C'));
        assert!(s.editing_config.is_some());
    }

    #[test]
    fn c_key_emits_message_for_non_host() {
        let (mut s, _, _) = make_state(2, 1, 2);
        s.handle_event(key('c'));
        assert!(s.editing_config.is_none());
        assert!(s.message.contains("房主"));
    }

    #[test]
    fn enter_sends_start_game_when_host() {
        let (mut s, mut out_rx, _) = make_state(1, 1, 2);
        s.handle_event(keycode(KeyCode::Enter));
        let msg = out_rx.try_recv().expect("应发 StartGame");
        assert!(matches!(msg, ClientMsg::StartGame));
    }

    #[test]
    fn enter_no_op_when_not_host() {
        let (mut s, mut out_rx, _) = make_state(2, 1, 2);
        s.handle_event(keycode(KeyCode::Enter));
        assert!(out_rx.try_recv().is_err());
    }

    #[test]
    fn space_acts_like_enter_for_host() {
        let (mut s, mut out_rx, _) = make_state(1, 1, 2);
        s.handle_event(key(' '));
        assert!(matches!(out_rx.try_recv(), Ok(ClientMsg::StartGame)));
    }

    #[test]
    fn l_key_returns_leave_request_confirm() {
        let (mut s, _, _) = make_state(1, 1, 2);
        let t = s.handle_event(key('L'));
        assert!(matches!(t, Some(Transition::RequestConfirm { .. })));
    }

    #[test]
    fn esc_returns_back_to_main_request_confirm() {
        let (mut s, _, _) = make_state(1, 1, 2);
        let t = s.handle_event(keycode(KeyCode::Esc));
        assert!(matches!(t, Some(Transition::RequestConfirm { .. })));
    }

    #[test]
    fn unhandled_key_is_noop() {
        let (mut s, _, _) = make_state(1, 1, 2);
        let t = s.handle_event(key('Z'));
        assert!(t.is_none());
    }

    // ============================================================================
    // editing_config modal 路径 — Save / Cancel
    // ============================================================================

    #[test]
    fn modal_cancel_clears_editing_config() {
        let (mut s, _, _) = make_state(1, 1, 2);
        s.editing_config = Some(EditConfigModal::new(s.room_view.config.clone()));
        s.handle_event(keycode(KeyCode::Esc));
        assert!(s.editing_config.is_none());
    }

    #[test]
    fn modal_save_sends_update_rules_and_closes_modal() {
        let (mut s, mut out_rx, _) = make_state(1, 1, 2);
        s.editing_config = Some(EditConfigModal::new(s.room_view.config.clone()));
        // EditConfigModal 内部 Enter Save (无修改也 Save 当前 cfg).
        s.handle_event(keycode(KeyCode::Enter));
        assert!(s.editing_config.is_none());
        // 应发 UpdateRules
        let mut got = false;
        while let Ok(msg) = out_rx.try_recv() {
            if matches!(msg, ClientMsg::UpdateRules(_)) {
                got = true;
            }
        }
        assert!(got);
        assert!(s.message.contains("配置"));
    }

    // ============================================================================
    // handle_msg 各 ServerMsg
    // ============================================================================

    #[test]
    fn welcome_replaces_room_view() {
        let (mut s, _, in_tx) = make_state(1, 1, 2);
        let new_view = make_view(
            1,
            1,
            3,
            RoomLifecycle::Lobby,
            crate::net::p2p::RoomMode::Standard,
        );
        in_tx
            .send(ServerMsg::Welcome {
                player_id: 1,
                reconnect_token: Uuid::new_v4(),
                room: Box::new(new_view),
            })
            .unwrap();
        let _ = s.advance();
        assert_eq!(s.room_view.players.len(), 3);
    }

    #[test]
    fn room_update_in_game_standard_triggers_enter_online_game() {
        let (mut s, _, in_tx) = make_state(1, 1, 2);
        let mut v = make_view(
            1,
            1,
            2,
            RoomLifecycle::InGame,
            crate::net::p2p::RoomMode::Standard,
        );
        v.state = RoomLifecycle::InGame;
        in_tx.send(ServerMsg::RoomUpdate(Box::new(v))).unwrap();
        let t = s.advance();
        assert!(matches!(t, Some(Transition::EnterOnlineGame)));
    }

    #[test]
    fn room_update_in_game_zerotrust_does_not_enter_standard_game() {
        let (mut s, _, in_tx) = make_state(1, 1, 2);
        let v = make_view(
            1,
            1,
            4,
            RoomLifecycle::InGame,
            crate::net::p2p::RoomMode::ZeroTrust,
        );
        in_tx.send(ServerMsg::RoomUpdate(Box::new(v))).unwrap();
        let t = s.advance();
        assert!(t.is_none(), "ZeroTrust 路径不该走 EnterOnlineGame");
    }

    #[test]
    fn game_state_view_routes_to_enter_online_game() {
        use crate::engine::domain::meld::Seat;
        use crate::net::protocol::PlayerView;
        let (mut s, _, in_tx) = make_state(1, 1, 2);
        let make_pv = |seat| PlayerView {
            seat,
            nickname: String::new(),
            score: 0,
            hand_count: 13,
            melds: Vec::new(),
            river: Vec::new(),
            riichi: false,
            riichi_river_idx: None,
        };
        let view = crate::net::protocol::GameStateView {
            round_wind: crate::engine::round_state::RoundWind::East,
            kyoku: 1,
            honba: 0,
            riichi_sticks: 0,
            dealer: Seat::East,
            turn: Seat::East,
            phase: crate::engine::phase::Phase::Draw,
            my_seat: Seat::East,
            my_hand: Vec::new(),
            my_last_drawn: None,
            players: [
                make_pv(Seat::East),
                make_pv(Seat::South),
                make_pv(Seat::West),
                make_pv(Seat::North),
            ],
            wall_remaining: 70,
            dora_indicators: Vec::new(),
            events: Vec::new(),
        };
        in_tx
            .send(ServerMsg::GameStateView(Box::new(view)))
            .unwrap();
        let t = s.advance();
        assert!(matches!(t, Some(Transition::EnterOnlineGame)));
    }

    #[test]
    fn mp_start_routes_to_zero_trust_game_with_args() {
        let (mut s, _, in_tx) = make_state(1, 1, 4);
        in_tx
            .send(ServerMsg::MpStart {
                all_peer_ids: vec![vec![1; 32], vec![2; 32], vec![3; 32], vec![4; 32]],
                own_index: 0,
                session_label: vec![7; 32],
                deck_size: 136,
                cnc_k_rounds: 80,
            })
            .unwrap();
        let t = s.advance();
        match t {
            Some(Transition::EnterZeroTrustGame { args }) => {
                assert_eq!(args.own_index, 0);
                assert_eq!(args.deck_size, 136);
                assert_eq!(args.cnc_k_rounds, 80);
                assert_eq!(args.session_label.len(), 32);
            }
            _ => panic!("应返回 EnterZeroTrustGame"),
        }
    }

    #[test]
    fn error_msg_sets_state_message() {
        let (mut s, _, in_tx) = make_state(1, 1, 2);
        in_tx
            .send(ServerMsg::Error {
                message: "boom".into(),
            })
            .unwrap();
        let _ = s.advance();
        assert_eq!(s.message, "boom");
    }

    #[test]
    fn advance_when_disconnected_sets_default_message() {
        let (mut s, out_rx, _) = make_state(1, 1, 2);
        // 关闭 in_tx 让 session 仍然 connected, 但关闭 out_tx 意味着 disconnected.
        // out_rx 是 receiver of out_tx, 关闭 receiver 让 session.out_tx.is_closed() 为 true.
        drop(out_rx);
        let t = s.advance();
        assert!(t.is_none());
        assert_eq!(s.message, "连接断开");
    }

    #[test]
    fn set_theme_updates_theme_kind() {
        let (mut s, _, _) = make_state(1, 1, 2);
        s.set_theme(ThemeKind::Light);
        assert_eq!(s.theme_kind, ThemeKind::Light);
    }

    // ============================================================================
    // render smoke
    // ============================================================================

    #[test]
    fn render_lobby_does_not_panic() {
        let (s, _out, _in) = make_state(1, 1, 2);
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area(), None, None)).unwrap();
    }

    #[test]
    fn render_with_nat_and_dial_addr_does_not_panic() {
        let (mut s, _out, _in) = make_state(1, 1, 4);
        s.message = "hello".into();
        let nat_pub = crate::net::p2p::host::NatReachability::Public(
            "/ip4/8.8.8.8/udp/4001/quic-v1".parse().unwrap(),
        );
        let dial: libp2p::Multiaddr = "/ip4/8.8.8.8/udp/4001/quic-v1/p2p-circuit".parse().unwrap();
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area(), Some(&nat_pub), Some(&dial)))
            .unwrap();
    }

    #[test]
    fn render_with_zerotrust_short_humans_does_not_panic() {
        let (mut s, _out, _in) = make_state(1, 1, 2);
        s.room_view.mode = crate::net::p2p::RoomMode::ZeroTrust;
        let nat_priv = crate::net::p2p::host::NatReachability::Private;
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area(), Some(&nat_priv), None))
            .unwrap();
    }

    #[test]
    fn render_with_modal_open_does_not_panic() {
        let (mut s, _out, _in) = make_state(1, 1, 2);
        s.editing_config = Some(EditConfigModal::new(s.room_view.config.clone()));
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area(), None, None)).unwrap();
    }

    #[test]
    fn render_with_nat_unknown_does_not_panic() {
        let (s, _out, _in) = make_state(1, 1, 1);
        let nat_unknown = crate::net::p2p::host::NatReachability::Unknown;
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area(), Some(&nat_unknown), None))
            .unwrap();
    }
}
