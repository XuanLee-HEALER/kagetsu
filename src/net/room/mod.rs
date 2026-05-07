//! RoomActor — 持权威 GameEngine + 处理玩家命令.
//!
//! ## 责任
//! - 接受玩家加入 / ready / 开始游戏
//! - 接收玩家动作 (Discard/Riichi/Pon/...) 并验证, 调 [`GameEngine`] mutator
//! - 给每个 client 投影 [`GameStateView`] (隐藏他家手牌)
//! - 房主修改房间配置
//! - 玩家离开 / 断线
//!
//! ## 设计
//! - 单 task, `mpsc::UnboundedReceiver<RoomCmd>` 收命令, 在每次命令处理后
//!   推进游戏状态 (调 `advance_game`) 并广播
//! - 鸣牌窗口 + 思考时长 timer 留 Phase 9. Phase 3 的 InGame 简化为:
//!   - AwaitDiscard 等玩家动作 (人类玩家或 AI)
//!   - AwaitCalls 直接 advance_turn (无人响应)
//! - AI 决策由 [`net::ai_seat`] 在 Phase 8 完整实现, Phase 3 暂用 default
//!   action (摸切 + 不和)

use std::collections::HashMap;

use rand::Rng;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use uuid::Uuid;

use crate::engine::domain::meld::Seat;
use crate::engine::rules::GameRules;
use crate::game_engine::GameEngine;
use crate::net::protocol::{ClientMsg, NetAction, PlayerSlot, RoomLifecycle, RoomView, ServerMsg};

// ============================================================================
// 公开 API
// ============================================================================

/// 创建一个新 RoomActor 并 spawn 到当前 tokio runtime.
/// 返回的 [`RoomHandle`] 可发 [`RoomCmd`] 给 actor.
///
/// 默认 mode = [`RoomMode::Standard`] (房主权威). ZeroTrust 模式用
/// [`spawn_room_with_mode`].
pub fn spawn_room(host_nickname: String, config: GameRules) -> RoomHandle {
    spawn_room_with_seed(host_nickname, config, None)
}

/// 创建带指定信任模式的 RoomActor (M5.B.2).
///
/// `mode = Standard`: 跟 [`spawn_room`] 一致, 房主权威.
/// `mode = ZeroTrust`: 当前 commit 仅记录 mode, 后续 M5.B.3+ 集成协议 1
/// 联合洗牌 + 协议 2/3 摸牌 / 揭示.
pub fn spawn_room_with_mode(
    host_nickname: String,
    config: GameRules,
    mode: crate::net::p2p::RoomMode,
) -> RoomHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut actor = RoomActor::new_with_rx(host_nickname, config, rx, tx.clone(), None);
    actor.mode = mode;
    tokio::spawn(actor.run());
    RoomHandle { tx }
}

/// 同 [`spawn_room`], 但允许测试注入固定 seed (None = 启动时随机).
///
/// 注入的 seed 用于:
/// - `game_seed` (整庄 seed). 局 seed = `seed ^ round_index`
/// - 局内随机 (洗牌) 决定性可复现
pub fn spawn_room_with_seed(
    host_nickname: String,
    config: GameRules,
    seed: Option<u64>,
) -> RoomHandle {
    spawn_room_advanced(host_nickname, config, seed, None)
}

/// 全选项版 spawn, 测试用. `call_window_ms_override` 缩短鸣牌窗口加快测试.
pub fn spawn_room_advanced(
    host_nickname: String,
    config: GameRules,
    seed: Option<u64>,
    call_window_ms_override: Option<u64>,
) -> RoomHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut actor = RoomActor::new_with_rx(host_nickname, config, rx, tx.clone(), seed);
    if let Some(ms) = call_window_ms_override {
        actor.call_window_ms = ms;
    }
    tokio::spawn(actor.run());
    RoomHandle { tx }
}

#[derive(Clone)]
pub struct RoomHandle {
    pub tx: UnboundedSender<RoomCmd>,
}

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("房间已满")]
    RoomFull,
    #[error("房间已开局, 不接受新玩家")]
    AlreadyInGame,
    #[error("token 无效或对应玩家未离开")]
    InvalidReconnectToken,
}

pub struct JoinResult {
    pub player_id: u32,
    pub reconnect_token: Uuid,
    pub room: RoomView,
}

pub enum RoomCmd {
    /// 玩家加入. `sender` 是给这个 client 发 ServerMsg 的 channel.
    Join {
        nickname: String,
        reconnect_token: Option<Uuid>,
        sender: UnboundedSender<ServerMsg>,
        ack: oneshot::Sender<Result<JoinResult, JoinError>>,
    },
    /// 玩家发来 ClientMsg.
    PlayerMsg { player_id: u32, msg: ClientMsg },
    /// 玩家断线 (transport 层检测到).
    Disconnect { player_id: u32 },
    /// 鸣牌窗口超时 (内部 timer 触发).
    /// `expected_round`/`expected_kyoku` 防止过期 timer 影响后续局.
    CallTimeout { generation: u64 },
    /// 重连 grace 期 (30s) 满, 检查 slot 是否仍未重连. 是 → 永久转 AI.
    ReconnectGraceTimeout { player_id: u32 },
    /// M5.D.2: 设置房主自己的 libp2p PeerId. spawn_p2p_listener 拿到
    /// local_peer_id 后调一次, RoomActor 把它关联到 host slot.
    SetLocalPeerId { peer_id_bytes: Vec<u8> },
    /// M5.D.2: 关联 player_id 到 libp2p PeerId 字节. host_swarm_task 在
    /// process_pending_join 完成 ClientMsg::Join 时调一次.
    AssociatePeer {
        player_id: u32,
        peer_id_bytes: Vec<u8>,
    },
}

// ============================================================================
// SlotEntry — 房间内一个座位
// ============================================================================

struct SlotEntry {
    id: u32,
    nickname: String,
    ready: bool,
    seat: Option<Seat>,
    is_ai: bool,
    is_host: bool,
    connected: bool,
    sender: Option<UnboundedSender<ServerMsg>>,
    reconnect_token: Uuid,
    /// 断线时的时刻. None = 在线; Some(t) = 进入 reconnect grace 期 (30s
    /// 内重连可恢复 sender + connected). grace 满后被 timer 触发, 转 AI 接管.
    disconnected_at: Option<std::time::Instant>,
}

impl SlotEntry {
    fn to_view(&self) -> PlayerSlot {
        PlayerSlot {
            id: self.id,
            nickname: self.nickname.clone(),
            ready: self.ready,
            seat: self.seat,
            is_ai: self.is_ai,
            is_host: self.is_host,
            connected: self.connected,
        }
    }
}

/// 断线后等多久转 AI 接管. 期间客户端可用 reconnect_token 重连恢复.
const RECONNECT_GRACE_SECS: u64 = 30;

// ============================================================================
// RoomActor
// ============================================================================

const MAX_PLAYERS: usize = 4;

/// 计算 ZeroTrust session_label = SHA-256(room_id || sorted_peer_ids).
///
/// 4 方独立算结果一致 (peer_ids 排序保证). 用作 mental poker 协议 1
/// cut-and-choose Fiat-Shamir transcript 一部分, 防 cross-session replay.
fn compute_session_label(room_id: &str, peer_ids: &[Vec<u8>]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&Vec<u8>> = peer_ids.iter().collect();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(b"tui-majo/mp/session/v1\0");
    h.update(room_id.as_bytes());
    h.update([0u8]);
    for pid in &sorted {
        h.update((pid.len() as u32).to_le_bytes());
        h.update(pid);
    }
    h.finalize().to_vec()
}

/// 当前 unix 毫秒. 失败时返回 0.
fn chrono_now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct RoomActor {
    room_id: String,
    config: GameRules,
    state: RoomLifecycle,
    slots: Vec<SlotEntry>,
    rx: UnboundedReceiver<RoomCmd>,
    /// self_tx 用于 RoomActor 自己发 cmd (e.g. CallTimeout from spawned timer).
    self_tx: UnboundedSender<RoomCmd>,
    next_player_id: u32,
    game: Option<GameEngine>,
    /// 整庄 seed (开局时随机).
    game_seed: u64,
    /// 局序号 (1-based, 用于 game_seed ^ round_index).
    round_index: u64,
    /// 房主创建时占位的 nickname; 房主真正加入后替换.
    pending_host_nickname: Option<String>,
    /// AwaitCalls 阶段, 等待真人玩家响应 (Pon/Chi/Minkan/Ron/Pass).
    /// HashMap<player_id, response>, None = 待响应.
    /// 收齐后或裁决后清空.
    pending_calls: Option<HashMap<u32, Option<NetAction>>>,
    /// 鸣牌窗口 generation, 每次 setup 自增, timer 触发时校验避免过期影响.
    call_window_gen: u64,
    /// 鸣牌窗口超时 ms. 派生自 GameRules.call_window_secs, 测试可 override.
    call_window_ms: u64,
    /// 测试注入的 seed; None 时 start_game 用真 RNG.
    seed_override: Option<u64>,
    /// 房间信任模式 (M5.B.2). Standard = 房主权威 (现状); ZeroTrust = 对等
    /// mental poker (M5.B.3+ 集成协议 1/2/3 + 协议 4-7 announcement).
    /// lobby phase 房主可改, 开局后冻结.
    mode: crate::net::p2p::RoomMode,
    /// M5.D.2: player_id → libp2p PeerId 字节的反查表. host_swarm_task 在
    /// 玩家 Join 完成时调 AssociatePeer 注入; spawn_p2p_listener 启动后调
    /// SetLocalPeerId 注入 host 自己的. ZeroTrust 模式 start_zerotrust_game
    /// 用此表填 MpStart.all_peer_ids.
    player_peers: HashMap<u32, Vec<u8>>,
    /// M5.D.2: 暂存的 host local_peer_id. SetLocalPeerId 时存这, 一旦 host
    /// slot 加入就关联到 player_peers; 反之 host 已 Join 时直接覆盖.
    pending_host_peer_id: Option<Vec<u8>>,
}

impl RoomActor {
    pub(super) fn new_with_rx(
        host_nickname: String,
        config: GameRules,
        rx: UnboundedReceiver<RoomCmd>,
        self_tx: UnboundedSender<RoomCmd>,
        seed_override: Option<u64>,
    ) -> Self {
        let mut rng = rand::rng();
        let room_id = format!("{:04x}-{:04x}", rng.random::<u16>(), rng.random::<u16>());
        let call_window_ms = config.call_window_secs as u64 * 1000;
        Self {
            room_id,
            config,
            state: RoomLifecycle::Lobby,
            slots: Vec::with_capacity(MAX_PLAYERS),
            rx,
            self_tx,
            next_player_id: 1,
            game: None,
            game_seed: 0,
            round_index: 0,
            pending_host_nickname: Some(host_nickname),
            pending_calls: None,
            call_window_gen: 0,
            call_window_ms,
            seed_override,
            mode: crate::net::p2p::RoomMode::Standard,
            player_peers: HashMap::new(),
            pending_host_peer_id: None,
        }
    }

    pub(super) async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            self.handle_cmd(cmd);
            if self.state == RoomLifecycle::InGame {
                self.advance_game();
            }
        }
    }

    pub(super) fn handle_cmd(&mut self, cmd: RoomCmd) {
        match cmd {
            RoomCmd::Join {
                nickname,
                reconnect_token,
                sender,
                ack,
            } => {
                let result = self.handle_join(nickname, reconnect_token, sender);
                let _ = ack.send(result);
            }
            RoomCmd::PlayerMsg { player_id, msg } => {
                self.handle_client_msg(player_id, msg);
            }
            RoomCmd::Disconnect { player_id } => {
                self.mark_disconnected(player_id);
            }
            RoomCmd::ReconnectGraceTimeout { player_id } => {
                self.on_reconnect_grace_timeout(player_id);
            }
            RoomCmd::CallTimeout { generation } => {
                // 过期 timer (玩家已响应或已进入下一回合), 忽略
                if generation != self.call_window_gen {
                    return;
                }
                if self.pending_calls.is_none() {
                    return;
                }
                tracing::debug!("call window timeout, generation={generation}");
                self.resolve_call_window();
            }
            RoomCmd::SetLocalPeerId { peer_id_bytes } => {
                self.pending_host_peer_id = Some(peer_id_bytes.clone());
                // 已 join 的 host slot 立刻关联
                if let Some(host_id) = self.slots.iter().find(|s| s.is_host).map(|s| s.id) {
                    self.player_peers.insert(host_id, peer_id_bytes);
                }
            }
            RoomCmd::AssociatePeer {
                player_id,
                peer_id_bytes,
            } => {
                self.player_peers.insert(player_id, peer_id_bytes);
            }
        }
    }

    // ========================================================================
    // Lobby
    // ========================================================================

    pub(super) fn alloc_id(&mut self) -> u32 {
        let id = self.next_player_id;
        self.next_player_id += 1;
        id
    }

    pub(super) fn is_host(&self, player_id: u32) -> bool {
        self.slots.iter().any(|s| s.id == player_id && s.is_host)
    }

    pub(super) fn player_seat(&self, player_id: u32) -> Option<Seat> {
        self.slots
            .iter()
            .find(|s| s.id == player_id)
            .and_then(|s| s.seat)
    }
}

// 让外部 (用于 Phase 4 client UI) 也能拿到 token → player_id 映射. 暂存在
// `RoomActor::pending_host_nickname` 等字段不太干净, 待后续重构.
mod game;
#[allow(dead_code)]
// 拆分子模块 (impl RoomActor 在 sibling file 内, 共享私有字段访问).
mod lobby;
mod projection;
mod zerotrust;

fn _api_silence_warnings(_x: HashMap<Uuid, u32>) {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests;
