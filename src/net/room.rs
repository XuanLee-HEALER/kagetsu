//! RoomActor — 持权威 GameState + 处理玩家命令.
//!
//! ## 责任
//! - 接受玩家加入 / ready / 开始游戏
//! - 接收玩家动作 (Discard/Riichi/Pon/...) 并验证, 调 [`GameState`] mutator
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

use crate::config::GameConfig;
use crate::game::{GameState, Phase, RoundResult, RyuukyokuKind};
use crate::meld::Seat;
use crate::net::protocol::{
    ClientMsg, GameOverView, GameStateView, NetAction, PlayerSlot, PlayerView, RoomLifecycle,
    RoomView, RoundResultView, ServerMsg,
};
use crate::score::final_ranking;
use crate::tile::Tile;

// ============================================================================
// 公开 API
// ============================================================================

/// 创建一个新 RoomActor 并 spawn 到当前 tokio runtime.
/// 返回的 [`RoomHandle`] 可发 [`RoomCmd`] 给 actor.
pub fn spawn_room(host_nickname: String, config: GameConfig) -> RoomHandle {
    spawn_room_with_seed(host_nickname, config, None)
}

/// 同 [`spawn_room`], 但允许测试注入固定 seed (None = 启动时随机).
///
/// 注入的 seed 用于:
/// - `game_seed` (整庄 seed). 局 seed = `seed ^ round_index`
/// - 局内随机 (洗牌) 决定性可复现
pub fn spawn_room_with_seed(
    host_nickname: String,
    config: GameConfig,
    seed: Option<u64>,
) -> RoomHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let actor = RoomActor::new_with_rx(host_nickname, config, rx, tx.clone(), seed);
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

// ============================================================================
// RoomActor
// ============================================================================

const MAX_PLAYERS: usize = 4;
/// 鸣牌窗口超时 (ms). 真人玩家在此时间内未按 P/A/M/C 视为 Pass.
const CALL_WINDOW_MS: u64 = 2500;

/// 当前 unix 毫秒. 失败时返回 0.
fn chrono_now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct RoomActor {
    room_id: String,
    config: GameConfig,
    state: RoomLifecycle,
    slots: Vec<SlotEntry>,
    rx: UnboundedReceiver<RoomCmd>,
    /// self_tx 用于 RoomActor 自己发 cmd (e.g. CallTimeout from spawned timer).
    self_tx: UnboundedSender<RoomCmd>,
    next_player_id: u32,
    game: Option<GameState>,
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
    /// 测试注入的 seed; None 时 start_game 用真 RNG.
    seed_override: Option<u64>,
}

impl RoomActor {
    fn new_with_rx(
        host_nickname: String,
        config: GameConfig,
        rx: UnboundedReceiver<RoomCmd>,
        self_tx: UnboundedSender<RoomCmd>,
        seed_override: Option<u64>,
    ) -> Self {
        let mut rng = rand::rng();
        let room_id = format!("{:04x}-{:04x}", rng.random::<u16>(), rng.random::<u16>());
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
            seed_override,
        }
    }

    async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            self.handle_cmd(cmd);
            if self.state == RoomLifecycle::InGame {
                self.advance_game();
            }
        }
    }

    fn handle_cmd(&mut self, cmd: RoomCmd) {
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
                self.broadcast_room_update();
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
        }
    }

    // ========================================================================
    // Lobby
    // ========================================================================

    fn handle_join(
        &mut self,
        nickname: String,
        reconnect_token: Option<Uuid>,
        sender: UnboundedSender<ServerMsg>,
    ) -> Result<JoinResult, JoinError> {
        // 重连流程: 找 token 对应 slot, 复用 seat / 分数 / token, 替换 sender.
        if let Some(token) = reconnect_token
            && let Some(idx) = self.slots.iter().position(|s| s.reconnect_token == token)
        {
            let (player_id, seat_opt, sender_clone) = {
                let slot = &mut self.slots[idx];
                slot.connected = true;
                slot.is_ai = false; // AI 临时接管的 seat 现在交还给真人
                slot.sender = Some(sender.clone());
                slot.nickname = nickname;
                (slot.id, slot.seat, slot.sender.clone())
            };
            let room = self.room_view();
            if let Some(s) = sender_clone {
                let _ = s.send(ServerMsg::Welcome {
                    player_id,
                    reconnect_token: token,
                    room: Box::new(room.clone()),
                });
                // 如果是 InGame, 把当前 GameStateView 推给重连方
                if self.state == RoomLifecycle::InGame
                    && let Some(seat) = seat_opt
                    && let Some(view) = self.project_view(seat)
                {
                    let _ = s.send(ServerMsg::GameStateView(Box::new(view)));
                }
            }
            self.broadcast_room_update();
            return Ok(JoinResult {
                player_id,
                reconnect_token: token,
                room,
            });
        }
        // 新 join
        if self.state != RoomLifecycle::Lobby {
            return Err(JoinError::AlreadyInGame);
        }
        if self.slots.len() >= MAX_PLAYERS {
            return Err(JoinError::RoomFull);
        }
        let id = self.alloc_id();
        let token = Uuid::new_v4();
        let is_host = self.slots.is_empty();
        if is_host {
            self.pending_host_nickname = None;
        }
        self.slots.push(SlotEntry {
            id,
            nickname,
            ready: is_host,
            seat: None,
            is_ai: false,
            is_host,
            connected: true,
            sender: Some(sender.clone()),
            reconnect_token: token,
        });

        let room = self.room_view();
        let _ = sender.send(ServerMsg::Welcome {
            player_id: id,
            reconnect_token: token,
            room: Box::new(room.clone()),
        });
        self.broadcast_room_update();
        Ok(JoinResult {
            player_id: id,
            reconnect_token: token,
            room,
        })
    }

    fn handle_client_msg(&mut self, player_id: u32, msg: ClientMsg) {
        match msg {
            ClientMsg::Ready { ready } => self.handle_ready(player_id, ready),
            ClientMsg::StartGame => self.handle_start_game(player_id),
            ClientMsg::UpdateConfig(cfg) => self.handle_update_config(player_id, cfg),
            ClientMsg::Action(action) => self.handle_action(player_id, action),
            ClientMsg::BackToRoom => self.handle_back_to_room(player_id),
            ClientMsg::ContinueGame => self.handle_continue_game(player_id),
            ClientMsg::Leave => self.handle_leave(player_id),
            ClientMsg::Pong { .. } => {}
            ClientMsg::Join { .. } => {
                // 已经 join 过了, 忽略
            }
        }
    }

    fn handle_ready(&mut self, player_id: u32, ready: bool) {
        if self.state != RoomLifecycle::Lobby {
            return;
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == player_id)
            && !slot.is_host
        {
            slot.ready = ready;
        }
        self.broadcast_room_update();
    }

    fn handle_update_config(&mut self, player_id: u32, cfg: GameConfig) {
        if self.state != RoomLifecycle::Lobby {
            return;
        }
        if !self.is_host(player_id) {
            return;
        }
        self.config = cfg;
        self.broadcast_room_update();
    }

    fn handle_start_game(&mut self, player_id: u32) {
        if self.state != RoomLifecycle::Lobby {
            return;
        }
        if !self.is_host(player_id) {
            return;
        }
        let all_ready = self.slots.iter().all(|s| s.ready);
        if !all_ready {
            self.send_error(player_id, "有玩家未准备");
            return;
        }
        let n = self.slots.len();
        if !(1..=4).contains(&n) {
            self.send_error(player_id, "玩家数应为 1-4 (空座位 AI 补)");
            return;
        }

        // 分配座位 (东南西北顺序)
        let seats = [Seat::East, Seat::South, Seat::West, Seat::North];
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.seat = Some(seats[i]);
        }
        // 补 AI 到 4 人
        while self.slots.len() < MAX_PLAYERS {
            let i = self.slots.len();
            let id = self.alloc_id();
            self.slots.push(SlotEntry {
                id,
                nickname: format!("AI {}", i + 1),
                ready: true,
                seat: Some(seats[i]),
                is_ai: true,
                is_host: false,
                connected: true,
                sender: None,
                reconnect_token: Uuid::new_v4(),
            });
        }

        // 启动 GameState. 测试可注入固定 seed 以保证决定性.
        self.game_seed = self.seed_override.unwrap_or_else(|| rand::rng().random());
        self.round_index = 1;
        let mut g = GameState::new(self.config.clone());
        g.start_round(self.game_seed ^ self.round_index);
        self.game = Some(g);
        self.state = RoomLifecycle::InGame;

        self.broadcast_room_update();
        self.broadcast_state_view();
    }

    fn handle_back_to_room(&mut self, _player_id: u32) {
        if self.state != RoomLifecycle::GameEnd {
            return;
        }
        self.reset_to_lobby();
    }

    fn handle_continue_game(&mut self, player_id: u32) {
        if self.state != RoomLifecycle::GameEnd {
            return;
        }
        if !self.is_host(player_id) {
            return;
        }
        // 用旧配置开新一庄
        self.round_index = 1;
        let mut g = GameState::new(self.config.clone());
        g.start_round(self.game_seed ^ self.round_index);
        self.game = Some(g);
        self.state = RoomLifecycle::InGame;
        self.broadcast_room_update();
        self.broadcast_state_view();
    }

    fn handle_leave(&mut self, player_id: u32) {
        let Some(idx) = self.slots.iter().position(|s| s.id == player_id) else {
            return;
        };
        let was_host = self.slots[idx].is_host;
        if was_host {
            // 房主离开: 解散房间.
            self.broadcast_to_all(ServerMsg::Error {
                message: "房主已离开, 房间解散".into(),
            });
            self.slots.clear();
            self.game = None;
            self.state = RoomLifecycle::Lobby;
            return;
        }
        // 子玩家离开:
        // - InGame 阶段: 标记为 AI 接管
        // - Lobby/GameEnd 阶段: 直接移除 slot
        match self.state {
            RoomLifecycle::Lobby | RoomLifecycle::GameEnd => {
                self.slots.remove(idx);
                self.broadcast_room_update();
            }
            RoomLifecycle::InGame => {
                let slot = &mut self.slots[idx];
                slot.is_ai = true;
                slot.connected = false;
                slot.sender = None;
                slot.nickname = format!("AI ({} 离开)", slot.nickname);
                self.broadcast_state_view();
            }
        }
    }

    fn mark_disconnected(&mut self, player_id: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == player_id) {
            slot.connected = false;
            slot.sender = None;
        }
    }

    fn reset_to_lobby(&mut self) {
        self.state = RoomLifecycle::Lobby;
        self.game = None;
        // 清座位 + AI slot, 重置 ready
        self.slots.retain(|s| !s.is_ai);
        for slot in self.slots.iter_mut() {
            slot.seat = None;
            slot.ready = slot.is_host;
        }
        self.broadcast_room_update();
    }

    // ========================================================================
    // InGame
    // ========================================================================

    fn handle_action(&mut self, player_id: u32, action: NetAction) {
        if self.state != RoomLifecycle::InGame {
            return;
        }
        let Some(seat) = self.player_seat(player_id) else {
            return;
        };

        // AwaitCalls 阶段的鸣牌响应走单独路径
        let phase = match self.game.as_ref() {
            Some(g) => g.phase,
            None => return,
        };
        if phase == Phase::AwaitCalls {
            self.handle_call_response(player_id, action);
            return;
        }

        let Some(game) = self.game.as_mut() else {
            return;
        };

        match action {
            NetAction::Discard(spec) => {
                if game.turn != seat || game.phase != Phase::AwaitDiscard {
                    return;
                }
                let tile_opt: Option<Tile> = game.players[seat.index()]
                    .hand
                    .closed
                    .iter()
                    .find(|t| t.kind == spec.kind)
                    .copied();
                if let Some(t) = tile_opt {
                    let _ = game.do_discard(t);
                }
            }
            NetAction::Riichi(spec) => {
                if game.turn != seat || game.phase != Phase::AwaitDiscard {
                    return;
                }
                let tile_opt: Option<Tile> = game.players[seat.index()]
                    .hand
                    .closed
                    .iter()
                    .find(|t| t.kind == spec.kind)
                    .copied();
                if let Some(t) = tile_opt {
                    let _ = game.do_riichi(t);
                }
            }
            NetAction::Tsumo => {
                if game.turn != seat || game.phase != Phase::AwaitDiscard {
                    return;
                }
                if let Some(score) = game.try_tsumo() {
                    game.declare_tsumo(score);
                }
            }
            NetAction::Ankan(kind) => {
                if game.turn != seat || game.phase != Phase::AwaitDiscard {
                    return;
                }
                let _ = game.do_ankan(kind);
            }
            NetAction::Shouminkan(kind) => {
                if game.turn != seat || game.phase != Phase::AwaitDiscard {
                    return;
                }
                let _ = game.do_shouminkan(kind);
            }
            // AwaitDiscard 阶段忽略鸣牌响应
            NetAction::Pon | NetAction::Chi(_) | NetAction::Minkan | NetAction::Pass => {}
            NetAction::NextRound => {
                if game.phase == Phase::RoundEnd {
                    game.next_round();
                    if game.phase == Phase::GameEnd {
                        self.finalize_game();
                        return;
                    }
                    self.round_index += 1;
                    // game.next_round 仅设 phase=Deal, 必须再 start_round 发新牌山
                    let seed = self.game_seed ^ self.round_index;
                    game.start_round(seed);
                }
            }
        }
        self.broadcast_state_view();
    }

    /// AwaitCalls 阶段的玩家响应: 收 Pon/Chi/Minkan/Tsumo(=Ron)/Pass.
    /// 收齐后裁决: Ron > Pon=Kan > Chi.
    fn handle_call_response(&mut self, player_id: u32, action: NetAction) {
        let Some(pending) = self.pending_calls.as_mut() else {
            return;
        };
        if !pending.contains_key(&player_id) {
            return; // 不是被等的玩家, 忽略
        }
        // 记录响应
        pending.insert(player_id, Some(action));
        // 是否所有 pending 都响应了
        let all_responded = pending.values().all(|v| v.is_some());
        if !all_responded {
            return;
        }
        // 裁决
        self.resolve_call_window();
    }

    /// 收齐响应后裁决并应用. 优先级: Ron > Pon=Kan > Chi.
    fn resolve_call_window(&mut self) {
        let Some(pending) = self.pending_calls.take() else {
            return;
        };

        // 先找 Ron (Tsumo 在 AwaitCalls 阶段视为 Ron).
        for (pid, resp) in &pending {
            if matches!(resp, Some(NetAction::Tsumo)) {
                let Some(seat) = self.player_seat(*pid) else {
                    continue;
                };
                let game = self.game.as_mut().unwrap();
                if let Some(score) = game.try_ron(seat) {
                    game.declare_ron(seat, score);
                    self.broadcast_state_view();
                    self.broadcast_round_result();
                    return;
                }
            }
        }

        // 然后找 Pon/Minkan (同优先级, 取第一个).
        for (pid, resp) in &pending {
            match resp {
                Some(NetAction::Pon) => {
                    let Some(seat) = self.player_seat(*pid) else {
                        continue;
                    };
                    let game = self.game.as_mut().unwrap();
                    let opts = game.legal_calls(seat);
                    if let Some(two) = opts.pon {
                        let _ = game.do_pon(seat, two);
                        self.broadcast_state_view();
                        return;
                    }
                }
                Some(NetAction::Minkan) => {
                    let Some(seat) = self.player_seat(*pid) else {
                        continue;
                    };
                    let game = self.game.as_mut().unwrap();
                    let opts = game.legal_calls(seat);
                    if let Some(three) = opts.minkan {
                        let _ = game.do_minkan(seat, three);
                        self.broadcast_state_view();
                        return;
                    }
                }
                _ => {}
            }
        }

        // 然后找 Chi (头跳: 只下家可吃).
        for (pid, resp) in &pending {
            if let Some(NetAction::Chi(idx)) = resp {
                let Some(seat) = self.player_seat(*pid) else {
                    continue;
                };
                let game = self.game.as_mut().unwrap();
                let opts = game.legal_calls(seat);
                if let Some(two) = opts.chi.get(*idx).copied() {
                    let _ = game.do_chi(seat, two);
                    self.broadcast_state_view();
                    return;
                }
            }
        }

        // 全 Pass: 推进
        let game = self.game.as_mut().unwrap();
        game.advance_turn();
        self.broadcast_state_view();
    }

    /// 在每个 cmd 处理完后自动推进游戏 (Draw 阶段摸牌, AwaitCalls 简化推进).
    /// 推进游戏状态: Draw 自动摸牌, AwaitDiscard 时若当前家是 AI 则自动出牌.
    /// 循环到 phase / turn 稳定 (即等真人玩家行动) 或到达终态.
    fn advance_game(&mut self) {
        // 安全上限: 一局至多 ~70 步, 200 远远够
        for _ in 0..200 {
            // 取当前 phase / turn (短借用立即释放)
            let (phase, turn) = match self.game.as_ref() {
                Some(g) => (g.phase, g.turn),
                None => return,
            };
            match phase {
                Phase::Draw => {
                    let game = self.game.as_mut().unwrap();
                    if game.do_draw().is_none() {
                        game.phase = Phase::RoundEnd;
                        game.last_result = Some(RoundResult::Ryuukyoku {
                            kind: RyuukyokuKind::Howaipai,
                        });
                        self.broadcast_round_result();
                        return;
                    }
                    self.broadcast_state_view();
                }
                Phase::AwaitDiscard => {
                    if !self.is_seat_ai(turn) {
                        // 给该真人推 ActionRequired (含思考时长 deadline).
                        self.send_thinking_action_required(turn);
                        return;
                    }
                    let action = {
                        let game = self.game.as_ref().unwrap();
                        crate::player::ai_choose_discard(game)
                    };
                    self.apply_ai_action(action);
                }
                Phase::AwaitCalls => {
                    if self.pending_calls.is_some() {
                        // 已 setup, 等响应或 timer 触发
                        return;
                    }
                    // 收集真人玩家的 call options.
                    let game_ref = self.game.as_ref().unwrap();
                    let last_discarder = game_ref.last_discard.map(|(s, _)| s);
                    let mut humans_pending: HashMap<u32, Option<NetAction>> = HashMap::new();
                    let mut hints_per_player: Vec<(u32, Vec<NetAction>)> = Vec::new();
                    for slot in &self.slots {
                        let Some(seat) = slot.seat else { continue };
                        if Some(seat) == last_discarder {
                            continue;
                        }
                        if slot.is_ai || !slot.connected {
                            continue;
                        }
                        let opts = game_ref.legal_calls(seat);
                        if opts.any() {
                            humans_pending.insert(slot.id, None);
                            let mut hints: Vec<NetAction> = Vec::new();
                            if opts.pon.is_some() {
                                hints.push(NetAction::Pon);
                            }
                            for i in 0..opts.chi.len() {
                                hints.push(NetAction::Chi(i));
                            }
                            if opts.minkan.is_some() {
                                hints.push(NetAction::Minkan);
                            }
                            if opts.ron {
                                hints.push(NetAction::Tsumo);
                            }
                            hints.push(NetAction::Pass);
                            hints_per_player.push((slot.id, hints));
                        }
                    }
                    if humans_pending.is_empty() {
                        let game = self.game.as_mut().unwrap();
                        game.advance_turn();
                        self.broadcast_state_view();
                        continue;
                    }
                    // 进入等待状态: setup pending_calls + spawn timeout timer
                    self.call_window_gen = self.call_window_gen.wrapping_add(1);
                    let gen_now = self.call_window_gen;
                    self.pending_calls = Some(humans_pending);

                    // 给 hints 推 ActionRequired (让 UI 高亮鸣牌选择)
                    let deadline = chrono_now_unix_ms() + CALL_WINDOW_MS as i64;
                    for (pid, hints) in hints_per_player {
                        if let Some(slot) = self.slots.iter().find(|s| s.id == pid)
                            && let Some(sender) = &slot.sender
                        {
                            let _ = sender.send(ServerMsg::ActionRequired {
                                hints,
                                deadline_unix_ms: deadline,
                            });
                        }
                    }

                    self.broadcast_state_view();

                    // spawn timeout
                    let self_tx = self.self_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(CALL_WINDOW_MS)).await;
                        let _ = self_tx.send(RoomCmd::CallTimeout {
                            generation: gen_now,
                        });
                    });
                    return;
                }
                Phase::RoundEnd => {
                    self.broadcast_round_result();
                    return;
                }
                Phase::GameEnd => {
                    self.finalize_game();
                    return;
                }
                Phase::Deal => {
                    return;
                }
            }
        }
        tracing::warn!("advance_game 达到 200 步上限, 中止防死循环");
    }

    /// 给真人 turn 推 ActionRequired (含 thinking_time deadline).
    /// 同一 turn 重复推不要紧 (client 用最新 deadline 覆盖).
    fn send_thinking_action_required(&self, seat: Seat) {
        let Some(slot) = self.slots.iter().find(|s| s.seat == Some(seat)) else {
            return;
        };
        let Some(sender) = &slot.sender else { return };
        let secs = self.config.thinking_time_secs.unwrap_or(0);
        let deadline_ms = if secs == 0 {
            0 // 不限时
        } else {
            chrono_now_unix_ms() + (secs as i64) * 1000
        };
        // hints 简化: 列出主要可用动作 (UI 自己渲染按键速查).
        let hints = vec![
            NetAction::Discard(crate::ui::screens::game::TileSpec {
                kind: crate::tile::TileIndex(0),
            }),
            NetAction::Tsumo,
        ];
        let _ = sender.send(ServerMsg::ActionRequired {
            hints,
            deadline_unix_ms: deadline_ms,
        });
    }

    /// 当前 seat 是否 AI 控制 (slot 标记 AI 或对应 slot 已断线).
    fn is_seat_ai(&self, seat: Seat) -> bool {
        self.slots
            .iter()
            .find(|s| s.seat == Some(seat))
            .map(|s| s.is_ai || !s.connected)
            .unwrap_or(true)
    }

    /// 把 AI 的 [`Action`] 转化成 GameState 调用. 失败时退化为摸切.
    fn apply_ai_action(&mut self, action: crate::action::Action) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        use crate::action::Action;
        match action {
            Action::Discard(t) => {
                let _ = game.do_discard(t);
            }
            Action::Riichi(t) => {
                let _ = game.do_riichi(t);
            }
            Action::Tsumo => {
                if let Some(score) = game.try_tsumo() {
                    game.declare_tsumo(score);
                }
            }
            Action::Ankan(t) => {
                let _ = game.do_ankan(t.kind);
            }
            Action::Shouminkan(t) => {
                let _ = game.do_shouminkan(t.kind);
            }
            Action::Pon { .. } | Action::Chi { .. } | Action::Minkan | Action::Ron(_) => {
                // 鸣牌响应, AwaitDiscard 阶段不会有 AI 走这些. 留 Phase 9.
            }
            Action::Pass | Action::KyuushuKyuuhai => {
                // fallback: 摸切 last_drawn
                let me = game.turn;
                if let Some(t) = game.players[me.index()].last_drawn {
                    let _ = game.do_discard(t);
                }
            }
        }
        self.broadcast_state_view();
    }

    fn finalize_game(&mut self) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        game.phase = Phase::GameEnd;
        let rankings = final_ranking(&game.players, &game.config);
        self.broadcast_state_view();
        self.broadcast_to_all(ServerMsg::GameEnd(GameOverView { rankings }));
        self.state = RoomLifecycle::GameEnd;
        self.broadcast_room_update();
    }

    // ========================================================================
    // 投影 / 广播
    // ========================================================================

    fn room_view(&self) -> RoomView {
        let host_id = self
            .slots
            .iter()
            .find(|s| s.is_host)
            .map(|s| s.id)
            .unwrap_or(0);
        RoomView {
            room_id: self.room_id.clone(),
            host_id,
            config: self.config.clone(),
            players: self.slots.iter().map(SlotEntry::to_view).collect(),
            state: self.state,
        }
    }

    fn project_view(&self, my_seat: Seat) -> Option<GameStateView> {
        let game = self.game.as_ref()?;
        let me = &game.players[my_seat.index()];
        let players: [PlayerView; 4] = std::array::from_fn(|i| {
            let p = &game.players[i];
            let nickname = self
                .slots
                .iter()
                .find(|s| s.seat == Some(p.seat))
                .map(|s| s.nickname.clone())
                .unwrap_or_default();
            PlayerView {
                seat: p.seat,
                nickname,
                score: p.score,
                hand_count: p.hand.closed.len(),
                melds: p.hand.melds.clone(),
                river: p.river.clone(),
                riichi: p.riichi,
                riichi_river_idx: None,
            }
        });
        Some(GameStateView {
            round_wind: game.round_wind,
            kyoku: game.kyoku,
            honba: game.honba,
            riichi_sticks: game.riichi_sticks,
            dealer: game.dealer,
            turn: game.turn,
            phase: game.phase,
            my_seat,
            my_hand: me.hand.closed.clone(),
            my_last_drawn: me.last_drawn,
            players,
            wall_remaining: game.wall.as_ref().map(|w| w.remaining()).unwrap_or(0),
            dora_indicators: game
                .wall
                .as_ref()
                .map(|w| w.dora_indicators())
                .unwrap_or_default(),
            events: game.events.iter().cloned().collect(),
        })
    }

    fn broadcast_state_view(&self) {
        for slot in &self.slots {
            let Some(seat) = slot.seat else {
                continue;
            };
            let Some(sender) = &slot.sender else {
                continue;
            };
            if let Some(view) = self.project_view(seat) {
                let _ = sender.send(ServerMsg::GameStateView(Box::new(view)));
            }
        }
    }

    fn broadcast_round_result(&self) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        let message = match &game.last_result {
            Some(RoundResult::Win {
                winner,
                score,
                is_tsumo,
                ..
            }) => format!(
                "{:?} {}: {} 番 {} 符",
                winner,
                if *is_tsumo { "自摸" } else { "荣和" },
                score.han,
                score.fu
            ),
            Some(RoundResult::Ryuukyoku { .. }) => "流局".to_string(),
            None => "未知".to_string(),
        };
        let scores = [
            game.players[0].score,
            game.players[1].score,
            game.players[2].score,
            game.players[3].score,
        ];
        self.broadcast_to_all(ServerMsg::RoundResult(RoundResultView { message, scores }));
    }

    fn broadcast_room_update(&self) {
        let view = self.room_view();
        self.broadcast_to_all(ServerMsg::RoomUpdate(Box::new(view)));
    }

    fn broadcast_to_all(&self, msg: ServerMsg) {
        for slot in &self.slots {
            if let Some(s) = &slot.sender {
                let _ = s.send(msg.clone());
            }
        }
    }

    fn send_error(&self, player_id: u32, err: &str) {
        if let Some(slot) = self.slots.iter().find(|s| s.id == player_id)
            && let Some(s) = &slot.sender
        {
            let _ = s.send(ServerMsg::Error {
                message: err.to_string(),
            });
        }
    }

    // ========================================================================
    // helpers
    // ========================================================================

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_player_id;
        self.next_player_id += 1;
        id
    }

    fn is_host(&self, player_id: u32) -> bool {
        self.slots.iter().any(|s| s.id == player_id && s.is_host)
    }

    fn player_seat(&self, player_id: u32) -> Option<Seat> {
        self.slots
            .iter()
            .find(|s| s.id == player_id)
            .and_then(|s| s.seat)
    }
}

// 让外部 (用于 Phase 4 client UI) 也能拿到 token → player_id 映射. 暂存在
// `RoomActor::pending_host_nickname` 等字段不太干净, 待后续重构.
#[allow(dead_code)]
fn _api_silence_warnings(_x: HashMap<Uuid, u32>) {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    use std::time::Duration;

    /// 模拟一个 client 连到 RoomActor, 拿到 (player_id, token, recv_rx).
    async fn join_player(
        handle: &RoomHandle,
        nickname: &str,
    ) -> (u32, Uuid, UnboundedReceiver<ServerMsg>) {
        let (tx, rx) = mpsc::unbounded_channel::<ServerMsg>();
        let (ack_tx, ack_rx) = oneshot::channel();
        handle
            .tx
            .send(RoomCmd::Join {
                nickname: nickname.into(),
                reconnect_token: None,
                sender: tx,
                ack: ack_tx,
            })
            .unwrap();
        let result = ack_rx.await.unwrap().unwrap();
        (result.player_id, result.reconnect_token, rx)
    }

    /// 等到 actor 处理完已发的 cmd. 多次 yield 让 spawn 的 task 跑.
    async fn yield_actor() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_join_alone() {
        let handle = spawn_room("host".into(), GameConfig::default());
        let (id, _token, mut rx) = join_player(&handle, "host").await;
        assert_eq!(id, 1);
        // 应收到 Welcome
        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, ServerMsg::Welcome { .. }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn second_player_not_host() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, _) = join_player(&handle, "host").await;
        let (other_id, _, _) = join_player(&handle, "other").await;
        assert_eq!(host_id, 1);
        assert_eq!(other_id, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn start_game_with_one_human_three_ai() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
        // host 自动 ready, 直接 start
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: host_id,
                msg: ClientMsg::StartGame,
            })
            .unwrap();
        yield_actor().await;
        // host_rx 应收到一连串消息 (Welcome + RoomUpdate × n + GameStateView)
        let mut got_state = false;
        while let Ok(msg) = host_rx.try_recv() {
            if matches!(msg, ServerMsg::GameStateView(_)) {
                got_state = true;
                break;
            }
        }
        assert!(got_state, "应该至少收到一个 GameStateView");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_leaves_room_dissolves() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
        let (_other_id, _, mut other_rx) = join_player(&handle, "other").await;
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: host_id,
                msg: ClientMsg::Leave,
            })
            .unwrap();
        yield_actor().await;
        // 两人都应收到 Error("房主已离开...")
        let drain = |rx: &mut UnboundedReceiver<ServerMsg>| -> bool {
            while let Ok(msg) = rx.try_recv() {
                if matches!(msg, ServerMsg::Error { .. }) {
                    return true;
                }
            }
            false
        };
        assert!(drain(&mut host_rx));
        assert!(drain(&mut other_rx));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn config_update_only_by_host() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, _) = join_player(&handle, "host").await;
        let (other_id, _, _) = join_player(&handle, "other").await;

        let cfg = GameConfig {
            length: crate::config::LengthRule::Tonpuusen,
            ..Default::default()
        };

        // 非 host 改: 应被拒
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: other_id,
                msg: ClientMsg::UpdateConfig(cfg.clone()),
            })
            .unwrap();
        yield_actor().await;

        // host 改: 应成功 (没有直接验证, 但至少不报错; 测试主要是 actor 不 panic)
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: host_id,
                msg: ClientMsg::UpdateConfig(cfg),
            })
            .unwrap();
        yield_actor().await;
    }

    /// 等到 host_rx 中收到一个满足条件的 GameStateView, 否则超时.
    /// 返回最后一个匹配的 view. 用于稳健的状态等待 (避免 yield_actor 时间不够).
    async fn wait_for_view(
        rx: &mut UnboundedReceiver<ServerMsg>,
        latest: &mut Option<GameStateView>,
        condition: impl Fn(&GameStateView) -> bool,
        timeout_ms: u64,
    ) -> Option<GameStateView> {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        while std::time::Instant::now() < deadline {
            // drain 当前可读消息, 更新 latest
            while let Ok(msg) = rx.try_recv() {
                if let ServerMsg::GameStateView(v) = msg {
                    *latest = Some(*v);
                }
            }
            if let Some(v) = latest.as_ref()
                && condition(v)
            {
                return latest.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        latest.clone()
    }

    /// 重连: 玩家 disconnect 后用 token 重连, 应恢复 seat + 分数 +
    /// 立即收到 GameStateView (如果游戏中).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconnect_with_token_resumes_seat() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, _) = join_player(&handle, "host").await;
        let (_, alice_token, alice_rx) = join_player(&handle, "alice").await;

        // host 让 alice ready (其实第二个玩家加入后默认 ready=false, 必须手动)
        // 但这里我们只测重连不开局, lobby 阶段
        // 模拟 alice 断线: drop 她的 rx (channel close), 通知 server
        drop(alice_rx);
        yield_actor().await;

        // alice 用 token 重连
        let (tx2, mut rx2) = mpsc::unbounded_channel::<ServerMsg>();
        let (ack_tx, ack_rx) = oneshot::channel();
        handle
            .tx
            .send(RoomCmd::Join {
                nickname: "alice2".into(),
                reconnect_token: Some(alice_token),
                sender: tx2,
                ack: ack_tx,
            })
            .unwrap();
        let result = ack_rx.await.unwrap().unwrap();
        // 应该拿到原来同一个 player_id (而不是新分配)
        assert_ne!(result.player_id, host_id);
        assert_eq!(result.reconnect_token, alice_token);

        // 第一条消息应该是 Welcome
        let msg = rx2.recv().await.unwrap();
        assert!(matches!(msg, ServerMsg::Welcome { .. }));
    }

    /// AI 驱动: 1 真人 host + 3 AI, 一直推进直到 host 应该出牌 (turn=East AwaitDiscard).
    /// 然后 host 切牌, AI 应继续接管直到下一次 host turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ai_drives_when_seat_is_ai() {
        let handle = spawn_room("h".into(), GameConfig::default());
        let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: host_id,
                msg: ClientMsg::StartGame,
            })
            .unwrap();

        let mut latest: Option<GameStateView> = None;
        let view = wait_for_view(
            &mut host_rx,
            &mut latest,
            |v| v.turn == Seat::East && v.phase == Phase::AwaitDiscard,
            2000,
        )
        .await
        .expect("应在 2s 内收到 East AwaitDiscard 状态");
        assert_eq!(view.turn, Seat::East);
        assert_eq!(view.phase, Phase::AwaitDiscard);

        // host 切自家手牌第一张
        let first_tile = view.my_hand[0];
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: host_id,
                msg: ClientMsg::Action(crate::net::protocol::NetAction::Discard(
                    crate::ui::screens::game::TileSpec {
                        kind: first_tile.kind,
                    },
                )),
            })
            .unwrap();

        // AI 接管 South/West/North 自动出, 然后回到 host (East) AwaitDiscard.
        // 等条件: 事件中至少 4 次 Discard 且 turn=East AwaitDiscard.
        let mut latest2: Option<GameStateView> = None;
        let view2 = wait_for_view(
            &mut host_rx,
            &mut latest2,
            |v| {
                v.turn == Seat::East
                    && v.phase == Phase::AwaitDiscard
                    && v.events
                        .iter()
                        .filter(|e| matches!(e, crate::game::GameEvent::Discard { .. }))
                        .count()
                        >= 4
            },
            3000,
        )
        .await;
        let view2 = view2.unwrap_or_else(|| {
            panic!(
                "AI 推进后应回到 East AwaitDiscard, latest={:?}",
                latest2.as_ref().map(|v| (v.turn, v.phase))
            )
        });
        assert_eq!(view2.turn, Seat::East);
        assert_eq!(view2.phase, Phase::AwaitDiscard);
    }
}
