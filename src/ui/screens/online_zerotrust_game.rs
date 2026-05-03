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

/// UI table 镜像 — ZeroTrustGameState 从 MpEvent 累积出的可渲染状态.
/// 不重复 actor 内部 Table (那个 actor 持有, 不跨线程), UI 自己保留必要的视图.
#[derive(Debug, Default, Clone)]
pub struct UiTable {
    /// 4 家弃牌池: 各家弃过的 tile_id (按 discard 顺序).
    pub discarded_pools: [Vec<usize>; 4],
    /// 自家手牌 (deck_index → tile_id). 摸 + 弃 + 鸣 + 杠时增删.
    pub own_drawn_in_hand: std::collections::BTreeMap<u32, usize>,
    /// 自家副露 (Pon/Chi/Minkan/Ankan). 简化为 (call_type, tile_ids) tuple.
    pub own_melds: Vec<(crate::mental_poker::wire::WireCallType, Vec<usize>)>,
    /// 已揭示的 dora indicator tile_id 列表.
    pub dora_indicators: Vec<usize>,
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
    /// 累积的事件 banner.
    pub event_log: Vec<String>,
    /// UI table 镜像 (从 MpEvent 累积).
    pub ui_table: UiTable,
    /// 自家手牌 cursor (own_drawn_in_hand 中 BTreeMap 的索引位置, 0..N).
    pub hand_cursor: usize,
    /// 下一次 TriggerDraw 用的 deck_index counter. 简化 wall pointer:
    /// 收到 DrawComplete / DiscardApplied / CallApplied 时取 max.
    pub next_deck_index: u32,

    /// MpPlayerActor handle. None = spawn 失败.
    actor: Option<MpPlayerHandle>,
    _bridge: Option<MpBridgeHandle>,
    _inbound_forward: Option<JoinHandle<()>>,
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
            ui_table: UiTable::default(),
            hand_cursor: 0,
            next_deck_index: 0,
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
                self.ui_table.own_drawn_in_hand.insert(deck_index, tile_id);
                self.next_deck_index = self.next_deck_index.max(deck_index + 1);
                self.event_log
                    .push(format!("Drew deck[{deck_index}] = tile {tile_id}"));
            }
            MpEvent::RevealComplete {
                deck_index,
                tile_id,
            } => {
                self.ui_table.dora_indicators.push(tile_id);
                self.next_deck_index = self.next_deck_index.max(deck_index + 1);
                self.event_log
                    .push(format!("Revealed deck[{deck_index}] = tile {tile_id}"));
            }
            MpEvent::DiscardApplied {
                player,
                deck_index,
                tile_id,
            } => {
                if (player as usize) < 4 {
                    self.ui_table.discarded_pools[player as usize].push(tile_id);
                }
                if player == self.args.own_index {
                    self.ui_table.own_drawn_in_hand.remove(&deck_index);
                    let len = self.ui_table.own_drawn_in_hand.len();
                    if self.hand_cursor >= len && self.hand_cursor > 0 {
                        self.hand_cursor = len.saturating_sub(1);
                    }
                }
                self.next_deck_index = self.next_deck_index.max(deck_index + 1);
                self.event_log.push(format!(
                    "Player {player} discarded deck[{deck_index}] (tile {tile_id})"
                ));
            }
            MpEvent::CallApplied {
                player,
                from_player,
                deck_indices,
                tile_ids,
                call_type,
            } => {
                if player == self.args.own_index {
                    for &di in &deck_indices {
                        self.ui_table.own_drawn_in_hand.remove(&di);
                    }
                    self.ui_table.own_melds.push((call_type, tile_ids.clone()));
                    let len = self.ui_table.own_drawn_in_hand.len();
                    if self.hand_cursor >= len && self.hand_cursor > 0 {
                        self.hand_cursor = len.saturating_sub(1);
                    }
                }
                if let Some(&max_di) = deck_indices.iter().max() {
                    self.next_deck_index = self.next_deck_index.max(max_di + 1);
                }
                self.event_log.push(format!(
                    "Player {player} called {call_type:?} from {from_player}"
                ));
            }
            MpEvent::ConcealedKanApplied {
                player,
                deck_indices,
                ..
            } => {
                if player == self.args.own_index {
                    for &di in &deck_indices {
                        self.ui_table.own_drawn_in_hand.remove(&di);
                    }
                    let len = self.ui_table.own_drawn_in_hand.len();
                    if self.hand_cursor >= len && self.hand_cursor > 0 {
                        self.hand_cursor = len.saturating_sub(1);
                    }
                }
                if let Some(&max_di) = deck_indices.iter().max() {
                    self.next_deck_index = self.next_deck_index.max(max_di + 1);
                }
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

    /// UI 触发摸下一张牌. wall pointer 用 next_deck_index counter.
    pub fn trigger_draw_next(&self) {
        self.send_cmd(MpRoomCmd::TriggerDraw {
            deck_index: self.next_deck_index,
        });
    }

    /// UI 触发弃当前 cursor 指的牌.
    pub fn discard_cursor(&self) {
        let Some((deck_index, _)) = self.ui_table.own_drawn_in_hand.iter().nth(self.hand_cursor)
        else {
            return;
        };
        self.send_cmd(MpRoomCmd::Discard {
            deck_index: *deck_index,
        });
    }

    /// UI 触发揭示下一张 dora indicator.
    pub fn trigger_reveal_next(&self) {
        self.send_cmd(MpRoomCmd::TriggerReveal {
            deck_index: self.next_deck_index,
        });
    }

    pub fn cursor_left(&mut self) {
        if self.hand_cursor > 0 {
            self.hand_cursor -= 1;
        }
    }

    pub fn cursor_right(&mut self) {
        let max = self.ui_table.own_drawn_in_hand.len().saturating_sub(1);
        if self.hand_cursor < max {
            self.hand_cursor += 1;
        }
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('D') if self.phase == MpPhase::Playing => {
                self.trigger_draw_next();
                None
            }
            KeyCode::Char(' ') | KeyCode::Enter if self.phase == MpPhase::Playing => {
                self.discard_cursor();
                None
            }
            KeyCode::Char('r') | KeyCode::Char('R') if self.phase == MpPhase::Playing => {
                self.trigger_reveal_next();
                None
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H')
                if self.phase == MpPhase::Playing =>
            {
                self.cursor_left();
                None
            }
            KeyCode::Right | KeyCode::Char('j') | KeyCode::Char('J')
                if self.phase == MpPhase::Playing =>
            {
                self.cursor_right();
                None
            }
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
                Constraint::Length(3),  // 标题
                Constraint::Length(3),  // 协议进度
                Constraint::Length(10), // 4 家弃牌池
                Constraint::Length(5),  // 自家手牌
                Constraint::Min(3),     // 事件日志
                Constraint::Length(3),  // 状态 banner
                Constraint::Length(3),  // 操作提示
            ])
            .split(area);

        // 标题
        let dora_str = if self.ui_table.dora_indicators.is_empty() {
            String::new()
        } else {
            let s: Vec<String> = self
                .ui_table
                .dora_indicators
                .iter()
                .map(|&t| tile_label(t, self.args.deck_size))
                .collect();
            format!(" · 宝牌指示: {}", s.join(" "))
        };
        let title = Paragraph::new(format!(
            "ZeroTrust · own_index={} · phase={:?}{dora_str}",
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

        // 协议进度
        let progress_text = match self.phase {
            MpPhase::KeyExchange => "等待 4 方 keygen + Schnorr DLEQ 验证...".to_string(),
            MpPhase::Shuffling => format!(
                "联合洗牌中 · {} / {} 轮 (每轮 cut-and-choose proof 验证)",
                self.shuffle_progress.0, self.shuffle_progress.1
            ),
            MpPhase::Playing => format!(
                "游戏进行中 · 牌山指针 = {} / {}",
                self.next_deck_index, self.args.deck_size
            ),
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
        f.render_widget(progress, chunks[1]);

        // 4 家弃牌池
        let pool_lines: Vec<Line> = (0..4)
            .map(|i| {
                let me = if i == self.args.own_index as usize {
                    "→ "
                } else {
                    "  "
                };
                let tiles_str: Vec<String> = self.ui_table.discarded_pools[i]
                    .iter()
                    .map(|&t| tile_label(t, self.args.deck_size))
                    .collect();
                Line::from(vec![
                    Span::styled(
                        format!("{me}player[{i}] ({:>2}): ", tiles_str.len()),
                        Style::default().fg(if i == self.args.own_index as usize {
                            theme.accent
                        } else {
                            theme.fg
                        }),
                    ),
                    Span::raw(tiles_str.join(" ")),
                ])
            })
            .collect();
        let pools = Paragraph::new(pool_lines)
            .style(Style::default().fg(theme.fg).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("弃牌池")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(pools, chunks[2]);

        // 自家手牌 + cursor
        let hand_entries: Vec<(u32, usize)> = self
            .ui_table
            .own_drawn_in_hand
            .iter()
            .map(|(&di, &tid)| (di, tid))
            .collect();
        let mut hand_spans: Vec<Span> = Vec::with_capacity(hand_entries.len() * 2);
        for (i, (di, tid)) in hand_entries.iter().enumerate() {
            let label = tile_label(*tid, self.args.deck_size);
            let style = if i == self.hand_cursor {
                Style::default()
                    .fg(theme.bg)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg).bg(theme.bg)
            };
            hand_spans.push(Span::styled(format!("[{label}#{di}]"), style));
            hand_spans.push(Span::raw(" "));
        }
        let melds_str: Vec<String> = self
            .ui_table
            .own_melds
            .iter()
            .map(|(ct, ids)| {
                let labels: Vec<String> = ids
                    .iter()
                    .map(|&t| tile_label(t, self.args.deck_size))
                    .collect();
                format!("{ct:?}({})", labels.join(""))
            })
            .collect();
        let hand_lines = vec![
            Line::from(hand_spans),
            Line::from(format!("副露: {}", melds_str.join(" "))),
        ];
        let hand = Paragraph::new(hand_lines)
            .style(Style::default().fg(theme.fg).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("自家手牌")
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(hand, chunks[3]);

        // 事件日志
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
        f.render_widget(log, chunks[4]);

        // 状态 banner
        let banner_text = if self.message.is_empty() {
            if self.actor.is_some() {
                "actor + bridge 已 spawn".to_string()
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
        f.render_widget(banner, chunks[5]);

        // 操作提示
        let hint_text = match self.phase {
            MpPhase::Playing => {
                "D: 摸下张  Space/Enter: 弃 cursor  R: 揭示 dora  ←/→: 移动 cursor  Esc/L: 离开"
            }
            _ => "Esc / L: 离开",
        };
        let hint = Paragraph::new(hint_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.dim).bg(theme.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().bg(theme.bg)),
            );
        f.render_widget(hint, chunks[6]);
    }
}

/// tile_id → 显示标签. 生产 deck_size=136 时按 standard_set 顺序映射 mahjong
/// label (1m..9m, 1p..9p, 1s..9s, 东南西北白发中); 测试 deck_size<136 时直接
/// 显示 t{id}.
fn tile_label(tile_id: usize, deck_size: u32) -> String {
    if deck_size == 136 {
        let kind = (tile_id / 4) as u8;
        if kind < 9 {
            format!("{}m", kind + 1)
        } else if kind < 18 {
            format!("{}p", kind - 8)
        } else if kind < 27 {
            format!("{}s", kind - 17)
        } else {
            ["东", "南", "西", "北", "白", "发", "中"]
                .get((kind - 27) as usize)
                .copied()
                .unwrap_or("?")
                .to_string()
        }
    } else {
        format!("t{tile_id}")
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

    /// **M5.E.0 ZeroTrustGameState gameplay e2e**: 4 screen 跑到 Playing 后,
    /// screen[0] trigger_draw_next() → 等 DrawComplete → discard_cursor() →
    /// 验证 4 screen 的 ui_table.discarded_pools[0] 长度 = 1, screen[0]
    /// 的 own_drawn_in_hand 减回 0.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn zerotrust_screen_draw_then_discard_updates_ui_table() {
        use crate::mental_poker::wire::MentalPokerMsg;
        use crate::net::p2p::mp_swarm::SwarmCommand;
        use crate::net::session::NetSession;
        use libp2p::PeerId;
        use std::collections::HashMap;
        use std::time::Duration;
        use tokio::sync::mpsc::unbounded_channel;
        use uuid::Uuid;

        const N: usize = 4;

        fn fake_peer_id(seed: u8) -> PeerId {
            let mut bytes = [0u8; 32];
            bytes[0] = seed;
            let kp = libp2p::identity::Keypair::ed25519_from_bytes(bytes).expect("kp");
            PeerId::from(&kp.public())
        }
        let peer_ids: Vec<PeerId> = (0..N as u8).map(fake_peer_id).collect();
        let peer_to_idx: HashMap<PeerId, usize> =
            peer_ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();

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

        let session_label = vec![0x88u8; 32];
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

        // Step 1: 等 all in Playing
        let _ = tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                for s in &mut screens {
                    let _ = s.advance();
                }
                if screens.iter().all(|s| s.phase == MpPhase::Playing) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        for (i, s) in screens.iter().enumerate() {
            assert_eq!(s.phase, MpPhase::Playing, "screen {i} 应 Playing");
        }

        // Step 2: screen[0] 摸下一张 (deck_index = 0, next_deck_index 初始 0)
        screens[0].trigger_draw_next();

        // 等 screen[0].own_drawn_in_hand 含 deck[0]
        let _ = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                for s in &mut screens {
                    let _ = s.advance();
                }
                if !screens[0].ui_table.own_drawn_in_hand.is_empty() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert_eq!(
            screens[0].ui_table.own_drawn_in_hand.len(),
            1,
            "screen[0] 摸 1 张应入 own_drawn_in_hand"
        );
        let drew_tile_id = *screens[0]
            .ui_table
            .own_drawn_in_hand
            .values()
            .next()
            .unwrap();

        // Step 3: screen[0] 弃 cursor 指的牌
        screens[0].discard_cursor();

        // 等 4 screen 都收 DiscardApplied → ui_table.discarded_pools[0] 长度 = 1
        let _ = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                for s in &mut screens {
                    let _ = s.advance();
                }
                if screens
                    .iter()
                    .all(|s| s.ui_table.discarded_pools[0].len() == 1)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;

        for (i, s) in screens.iter().enumerate() {
            assert_eq!(
                s.ui_table.discarded_pools[0].len(),
                1,
                "screen {i} 应看到 player 0 弃 1 张"
            );
            assert_eq!(
                s.ui_table.discarded_pools[0][0], drew_tile_id,
                "screen {i} 看到的弃牌 tile_id 应跟 screen[0] 摸的一致"
            );
        }
        // screen[0] 自家手牌减回 0
        assert_eq!(
            screens[0].ui_table.own_drawn_in_hand.len(),
            0,
            "screen[0] 弃后 own_drawn_in_hand 应为空"
        );
    }

    #[test]
    fn tile_label_mahjong_for_full_deck() {
        assert_eq!(tile_label(0, 136), "1m");
        assert_eq!(tile_label(35, 136), "9m"); // 8 * 4 + 3
        assert_eq!(tile_label(36, 136), "1p");
        assert_eq!(tile_label(72, 136), "1s");
        assert_eq!(tile_label(108, 136), "东");
        assert_eq!(tile_label(132, 136), "中");
    }

    #[test]
    fn tile_label_t_for_test_deck() {
        assert_eq!(tile_label(0, 16), "t0");
        assert_eq!(tile_label(7, 16), "t7");
    }
}
