//! ZeroTrust 模式游戏屏 (M5.D.1).
//!
//! 房间收到 [`crate::net::protocol::ServerMsg::MpStart`] 后切到此屏. spawn 自己的
//! [`crate::net::mp::actor::MpPlayerActor`] + [`crate::net::p2p::mp_bridge::spawn_mp_bridge`],
//! 用 [`crate::net::session::NetSession::mp_command_tx`] (走 SwarmTransport)
//! 跟 swarm 通信. 入站消息从 [`crate::net::session::NetSession::mp_inbound_rx`]
//! 通过 forward task 反查 PeerId → own_index 后 deliver 给 [`MpInbound`].
//!
//! UI 状态机 advance() drain MpEvent 累积:
//! - phase / shuffle progress
//! - 自家摸过的 (deck_index → tile_id) 反查表
//! - 各 actor table 镜像 (展示弃牌池 / 副露)
//!
//! 暂不实现 game action (Discard / Pon / Tsumo) — 完整 UI gameplay 留 M5.E.
//! 当前屏只 prove 端到端 wire-up: MpStart → spawn → keygen → shuffle → Playing.

use crossterm::event::{KeyCode, KeyEvent};
use libp2p::PeerId;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

use crate::net::mp::MpPhase;
use crate::net::mp::actor::{MpConfig, MpPlayerHandle, spawn_mp_player};
use crate::net::mp::cmd::{MpEvent, MpRoomCmd};
use crate::net::p2p::mp_bridge::{MpBridgeHandle, new_inbound_channel, spawn_mp_bridge};
use crate::net::p2p::mp_swarm::SwarmTransport;
use crate::net::session::NetSession;
use crate::ui::Transition;
use crate::ui::theme::ThemeKind;

/// MpStart 参数 — 跟 [`crate::net::protocol::ServerMsg::MpStart`] 字段一致.
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

    /// 当前 mp 协议 phase (KeyExchange → Shuffling → Playing → GameOver).
    pub phase: MpPhase,
    /// shuffle 进度 (completed / total). KeyExchange 阶段都是 0.
    pub shuffle_progress: (u32, u32),
    /// 累积的事件 banner (e.g. "Drew deck[5] = tile 12", "Player 1 discarded tile 3").
    pub event_log: Vec<String>,

    /// MpPlayerActor handle. None = spawn 失败 (e.g. NetSession.mp_command_tx 缺失).
    actor: Option<MpPlayerHandle>,
    /// mp_bridge handle (drop 时 abort).
    _bridge: Option<MpBridgeHandle>,
    /// inbound forward task handle (drop 时 abort).
    _inbound_forward: Option<JoinHandle<()>>,
    /// UI 侧 event drain rx.
    ui_event_rx: Option<UnboundedReceiver<MpEvent>>,
}

impl ZeroTrustGameState {
    /// 构造屏并 spawn 协议层 actor + bridge.
    /// 失败 (NetSession 缺 mp 边带 / PeerId 解析失败) 时屏仍可显示, 但
    /// `actor` 为 None, message 写明原因.
    pub fn new(mut session: NetSession, args: MpStartArgs) -> Self {
        let mut state = Self {
            phase: MpPhase::KeyExchange,
            shuffle_progress: (0, 4),
            event_log: Vec::new(),
            actor: None,
            _bridge: None,
            _inbound_forward: None,
            ui_event_rx: None,
            theme_kind: ThemeKind::default(),
            message: String::new(),
            args: args.clone(),
            session: NetSession::from_channels(
                session.player_id,
                session.token,
                // Tricky: NetSession 不能直接 split, 我们暂时构造一个空的占位 (会立刻 swap).
                // 实际下面用 std::mem::swap 把 session 还原.
                tokio::sync::mpsc::unbounded_channel().0,
                tokio::sync::mpsc::unbounded_channel().1,
            ),
        };
        std::mem::swap(&mut state.session, &mut session);
        // 现在 state.session 持原 session, 我们继续从 state.session 取 mp 边带.

        // 解析 4 个 PeerId (生产期望 args.all_peer_ids 是真 libp2p PeerId 字节;
        // 当前 RoomActor.start_zerotrust_game 用 Uuid 字节占位 — 解析失败会 fallback)
        let peer_ids: Vec<PeerId> = state
            .args
            .all_peer_ids
            .iter()
            .map(|b| PeerId::from_bytes(b).unwrap_or_else(|_| PeerId::random()))
            .collect();

        // 派生 mp_topic = tui-majo/mp/<session_label hex 前 16 字节>/v1
        // 4 方独立算一致, 因 session_label 来自 RoomActor 的统一计算.
        let topic_id = hex_short(&state.args.session_label);
        let mp_topic = format!("tui-majo/mp/{}/v1", topic_id);

        // 拿 NetSession.mp_command_tx + take mp_inbound_rx
        let Some(mp_command_tx) = state.session.mp_command_tx.clone() else {
            state.message =
                "NetSession 缺 mp 边带 (Standard mode 或本地 Session, 无 P2P swarm)".into();
            return state;
        };
        let Some(mut mp_inbound_rx) = state.session.mp_inbound_rx.take() else {
            state.message = "NetSession.mp_inbound_rx 已被 take".into();
            return state;
        };

        // SwarmTransport
        let transport = SwarmTransport::new(mp_command_tx, mp_topic, peer_ids.clone());

        // spawn MpPlayerActor
        let cfg = MpConfig {
            own_index: state.args.own_index as usize,
            all_peer_ids: state.args.all_peer_ids.clone(),
            session_label: state.args.session_label.clone(),
            deck_size: state.args.deck_size as usize,
            cnc_k_rounds: state.args.cnc_k_rounds as usize,
        };
        let mut player = spawn_mp_player(cfg, None);
        let actor_cmd_tx = player.cmd_tx.clone();
        let actor_event_rx = player.take_event_rx().expect("event_rx");

        // event fan-out: actor_event_rx → bridge_event_rx + ui_event_rx
        let (bridge_event_tx, bridge_event_rx) = unbounded_channel::<MpEvent>();
        let (ui_event_tx, ui_event_rx) = unbounded_channel::<MpEvent>();
        tokio::spawn(async move {
            let mut rx = actor_event_rx;
            while let Some(ev) = rx.recv().await {
                let _ = bridge_event_tx.send(ev.clone());
                let _ = ui_event_tx.send(ev);
            }
        });

        // bridge 接 NetSession.mp_inbound_rx → MpInbound 反查 PeerId → idx
        let (mp_inbound, mp_inbound_rx_for_bridge) = new_inbound_channel();
        let peer_to_idx: std::collections::HashMap<PeerId, usize> =
            peer_ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        let inbound_forward = tokio::spawn(async move {
            while let Some((peer, msg)) = mp_inbound_rx.recv().await {
                let idx = peer_to_idx.get(&peer).copied();
                let _ = mp_inbound.deliver(idx, msg);
            }
        });

        let bridge = spawn_mp_bridge(
            transport,
            bridge_event_rx,
            actor_cmd_tx,
            mp_inbound_rx_for_bridge,
        );

        state.actor = Some(player);
        state._bridge = Some(bridge);
        state._inbound_forward = Some(inbound_forward);
        state.ui_event_rx = Some(ui_event_rx);
        state.event_log.push(format!(
            "MpPlayerActor spawned (own_index={})",
            state.args.own_index
        ));
        state
    }

    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme_kind = kind;
    }

    /// 发 MpRoomCmd 给 actor (UI 触发动作时用, e.g. TriggerDraw / Discard).
    /// actor 没起则 noop.
    pub fn send_cmd(&self, cmd: MpRoomCmd) {
        if let Some(a) = &self.actor {
            let _ = a.cmd_tx.send(cmd);
        }
    }

    /// drain server msg + actor event. 累积渲染状态 + log.
    pub fn advance(&mut self) -> Option<Transition> {
        while let Some(msg) = self.session.try_recv() {
            if let crate::net::protocol::ServerMsg::Error { message } = msg {
                self.message = message;
            }
        }
        if self.session.is_disconnected() && self.message.is_empty() {
            self.message = "连接断开".into();
        }

        let mut events: Vec<MpEvent> = Vec::new();
        if let Some(rx) = self.ui_event_rx.as_mut() {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            self.apply_event(ev);
        }
        None
    }

    fn apply_event(&mut self, ev: MpEvent) {
        const MAX_LOG: usize = 32;
        match ev {
            MpEvent::PhaseChanged { phase } => {
                self.phase = phase;
                self.event_log.push(format!("phase → {phase:?}"));
            }
            MpEvent::ShuffleProgress { completed, total } => {
                self.shuffle_progress = (completed, total);
            }
            MpEvent::DrawComplete {
                deck_index,
                tile_id,
                ..
            } => {
                self.event_log
                    .push(format!("Drew deck[{deck_index}] = tile {tile_id}"));
            }
            MpEvent::RevealComplete {
                deck_index,
                tile_id,
            } => {
                self.event_log
                    .push(format!("Revealed deck[{deck_index}] = tile {tile_id}"));
            }
            MpEvent::DiscardApplied {
                player,
                deck_index,
                tile_id,
            } => {
                self.event_log.push(format!(
                    "Player {player} discarded deck[{deck_index}] (tile {tile_id})"
                ));
            }
            MpEvent::CallApplied {
                player,
                from_player,
                ..
            } => {
                self.event_log
                    .push(format!("Player {player} called from {from_player}"));
            }
            MpEvent::ConcealedKanApplied { player, .. } => {
                self.event_log
                    .push(format!("Player {player} concealed kan"));
            }
            MpEvent::WinValidated {
                player, is_tsumo, ..
            } => {
                self.event_log.push(format!(
                    "Player {player} won ({})",
                    if is_tsumo { "Tsumo" } else { "Ron" }
                ));
            }
            MpEvent::ProtocolError { offender, reason } => {
                self.message = format!("协议错误 (offender={offender:?}): {reason}");
            }
            MpEvent::GameOver { reason } => {
                self.event_log.push(format!("GameOver: {reason}"));
            }
            MpEvent::OutboundMsg { .. } | MpEvent::MonitorVerified { .. } => {
                // 不渲染
            }
        }
        if self.event_log.len() > MAX_LOG {
            let drop_n = self.event_log.len() - MAX_LOG;
            self.event_log.drain(..drop_n);
        }
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
                Constraint::Length(3), // phase + shuffle 进度
                Constraint::Min(3),    // event log
                Constraint::Length(3), // 状态 banner
                Constraint::Length(3), // 操作提示
            ])
            .split(area);

        // 标题
        let title = Paragraph::new(format!(
            "ZeroTrust · own_index={} · phase={:?}",
            self.args.own_index, self.phase,
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

        // phase + shuffle 进度
        let progress_text = match self.phase {
            MpPhase::KeyExchange => "等待 4 方 keygen + Schnorr DLEQ 验证...".to_string(),
            MpPhase::Shuffling => format!(
                "联合洗牌中 · {} / {} 轮 (每轮 cut-and-choose proof 验证)",
                self.shuffle_progress.0, self.shuffle_progress.1
            ),
            MpPhase::Playing => "游戏进行中".to_string(),
            MpPhase::GameOver => "局已结束".to_string(),
        };
        let progress = Paragraph::new(progress_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.accent).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("协议进度")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(progress, chunks[2]);

        // event log
        let log_lines: Vec<Line> = self
            .event_log
            .iter()
            .rev()
            .take(20)
            .rev()
            .map(|s| Line::from(s.as_str()))
            .collect();
        let log = Paragraph::new(log_lines)
            .style(Style::default().fg(theme.fg).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("事件日志")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(log, chunks[3]);

        // 状态 banner
        let banner_text = if self.message.is_empty() {
            if self.actor.is_some() {
                "ZeroTrust 协议层已 spawn — actor + bridge 跑 mental poker 协议".to_string()
            } else {
                "ZeroTrust 协议层未启动 (NetSession 缺 mp 边带)".to_string()
            }
        } else {
            self.message.clone()
        };
        let banner_color = if self.message.is_empty() {
            theme.info
        } else {
            theme.danger
        };
        let banner = Paragraph::new(banner_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(banner_color).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("状态")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(banner, chunks[4]);

        // 操作提示
        let hint = Paragraph::new("Esc / L: 离开")
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.dim).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(hint, chunks[5]);
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

    /// **M5.D.1 in-memory mock e2e**: 4 个 ZeroTrustGameState 通过 dispatcher 模拟
    /// 4 进程跨 swarm 通信, 验证 UI 状态机 spawn actor → keygen → shuffle →
    /// transition Playing 一气呵成. dispatcher 跟 mp_swarm.rs 那个一致, 但这次
    /// 走 ZeroTrustGameState (UI 入口) → NetSession.mp_command_tx →
    /// SwarmCommand → dispatcher → NetSession.mp_inbound_rx → forward task →
    /// MpInbound → bridge → actor.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_zerotrust_screens_keygen_and_shuffle_via_swarm() {
        use crate::mental_poker::wire::MentalPokerMsg;
        use crate::net::p2p::mp_swarm::SwarmCommand;
        use crate::net::session::NetSession;
        use libp2p::PeerId;
        use std::collections::HashMap;
        use std::time::Duration;
        use tokio::sync::mpsc::unbounded_channel;
        use uuid::Uuid;

        const N: usize = 4;

        // 4 个真实 PeerId (从 ed25519 keypair 派生)
        fn fake_peer_id(seed: u8) -> PeerId {
            let mut bytes = [0u8; 32];
            bytes[0] = seed;
            let kp = libp2p::identity::Keypair::ed25519_from_bytes(bytes).expect("kp");
            PeerId::from(&kp.public())
        }
        let peer_ids: Vec<PeerId> = (0..N as u8).map(fake_peer_id).collect();
        let peer_to_idx: HashMap<PeerId, usize> =
            peer_ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();

        // 4 个 NetSession 各自带一对 mp 边带 channel
        let mut session_inbound_txs: Vec<
            tokio::sync::mpsc::UnboundedSender<(PeerId, MentalPokerMsg)>,
        > = Vec::with_capacity(N);
        let mut cmd_rxs = Vec::with_capacity(N);
        let mut sessions = Vec::with_capacity(N);
        for i in 0..N {
            let (out_tx, _out_rx) = unbounded_channel::<crate::net::protocol::ClientMsg>();
            let (_in_tx, in_rx) = unbounded_channel::<crate::net::protocol::ServerMsg>();
            let (mp_cmd_tx, mp_cmd_rx) = unbounded_channel::<SwarmCommand>();
            let (mp_in_tx, mp_in_rx) = unbounded_channel::<(PeerId, MentalPokerMsg)>();
            let session = NetSession::from_channels(i as u32, Uuid::new_v4(), out_tx, in_rx)
                .with_mp_handles(mp_cmd_tx, mp_in_rx);
            sessions.push(session);
            cmd_rxs.push(mp_cmd_rx);
            session_inbound_txs.push(mp_in_tx);
        }

        // 4 个 ZeroTrustGameState (用真 PeerId 字节作 args.all_peer_ids 让解析成功)
        let session_label = vec![0x42u8; 32];
        let mut screens = Vec::with_capacity(N);
        for (i, sess) in sessions.into_iter().enumerate() {
            let args = MpStartArgs {
                all_peer_ids: peer_ids.iter().map(|p| p.to_bytes()).collect(),
                own_index: i as u32,
                session_label: session_label.clone(),
                deck_size: 16,
                cnc_k_rounds: 8,
            };
            screens.push(ZeroTrustGameState::new(sess, args));
        }

        // dispatcher tasks: 4 个, 每个接一方 cmd_rx, 路由到对端 inbound
        for i in 0..N {
            let mut cmd_rx = cmd_rxs.remove(0);
            let inbound_txs = session_inbound_txs.clone();
            let peer_to_idx_clone = peer_to_idx.clone();
            let peer_ids_clone = peer_ids.clone();
            tokio::spawn(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        SwarmCommand::Broadcast { topic: _, msg } => {
                            for (idx, tx) in inbound_txs.iter().enumerate() {
                                if idx == i {
                                    continue;
                                }
                                let _ = tx.send((peer_ids_clone[i], msg.clone()));
                            }
                        }
                        SwarmCommand::Unicast { target, msg } => {
                            if let Some(&t_idx) = peer_to_idx_clone.get(&target) {
                                let _ = inbound_txs[t_idx].send((peer_ids_clone[i], msg));
                            }
                        }
                    }
                }
            });
        }

        // 跑直到所有 screen.phase 进 Playing 或超时
        let timeout = tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                for s in &mut screens {
                    let _ = s.advance();
                }
                if screens.iter().all(|s| s.phase == MpPhase::Playing) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;

        let phases: Vec<MpPhase> = screens.iter().map(|s| s.phase).collect();
        assert!(
            timeout.is_ok() && timeout.unwrap(),
            "4 ZeroTrustGameState 应全 transition 到 Playing, 实际 {phases:?}"
        );
        // 各 screen actor 应已 spawn
        for (i, s) in screens.iter().enumerate() {
            assert!(s.actor.is_some(), "screen {i} actor 应 spawned");
            assert!(
                s.event_log.iter().any(|l| l.contains("phase → Shuffling")),
                "screen {i} log 应含 Shuffling transition"
            );
            assert!(
                s.event_log.iter().any(|l| l.contains("phase → Playing")),
                "screen {i} log 应含 Playing transition"
            );
        }
    }
}
