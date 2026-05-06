//! room/lobby — RoomActor 拆分模块: Lobby 阶段命令处理 (join / ready / config / leave / disconnect / grace)
//!
//! impl block 直接读 super::RoomActor 私有字段 (Rust 子 module 可访问 parent 私有).

use uuid::Uuid;
use tokio::sync::mpsc::UnboundedSender;
use crate::engine::domain::meld::Seat;
use crate::engine::rules::GameRules;
use crate::game_engine::GameEngine;
use crate::net::protocol::{
    ClientMsg, RoomLifecycle, ServerMsg,
};
use super::*;

impl RoomActor {
    pub(super) fn handle_join(
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
                slot.disconnected_at = None; // 退出 grace 期
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
                disconnected_at: None,
        });
        // M5.D.2: host slot 若已有 pending_host_peer_id (spawn_p2p_listener 先发的)
        // 立即关联. 否则等 SetLocalPeerId 后处理.
        if is_host && let Some(peer_bytes) = self.pending_host_peer_id.clone() {
            self.player_peers.insert(id, peer_bytes);
        }

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
    pub(super) fn handle_client_msg(&mut self, player_id: u32, msg: ClientMsg) {
        match msg {
            ClientMsg::Ready { ready } => self.handle_ready(player_id, ready),
            ClientMsg::StartGame => self.handle_start_game(player_id),
            ClientMsg::UpdateRules(cfg) => self.handle_update_config(player_id, cfg),
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
    pub(super) fn handle_ready(&mut self, player_id: u32, ready: bool) {
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
    pub(super) fn handle_update_config(&mut self, player_id: u32, cfg: GameRules) {
        if self.state != RoomLifecycle::Lobby {
            return;
        }
        if !self.is_host(player_id) {
            return;
        }
        self.config = cfg;
        self.broadcast_room_update();
    }
    pub(super) fn handle_start_game(&mut self, player_id: u32) {
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

        // ZeroTrust 模式: mental poker 协议要求 4 独立 sk holder, 真"AI 补"
        // 在零信任假设下做不到 (AI 的 sk 由房主控制 → 不再零信任).
        // Fallback 策略: n < 4 真人时, 自动降级为 Standard 模式 + AI 补,
        // 让用户能正常开局; 房主仍持权威 GameState. UI 应提示降级.
        if self.mode == crate::net::p2p::RoomMode::ZeroTrust {
            if n == MAX_PLAYERS {
                return self.start_zerotrust_game();
            }
            // 降级为 Standard + AI 补. 通知玩家.
            tracing::info!(
                "ZeroTrust 不足 4 真人 ({n}/{MAX_PLAYERS}), 自动降级为 Standard + AI 补"
            );
            self.broadcast_to_all(ServerMsg::Error {
                message: format!(
                    "ZeroTrust 不足 4 真人 ({n}/4), 已自动降级为 Standard 模式 + AI 补足座位"
                ),
            });
            self.mode = crate::net::p2p::RoomMode::Standard;
            // 落到下面 Standard 启动流程 (分配座位 + AI 补 + GameEngine).
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
                disconnected_at: None,
            });
        }

        // 启动 GameState. 测试可注入固定 seed 以保证决定性.
        self.game_seed = self.seed_override.unwrap_or_else(|| rand::rng().random());
        self.round_index = 1;
        let mut g = GameEngine::new(self.config.clone());
        g.start_round(self.game_seed ^ self.round_index);
        self.game = Some(g);
        self.state = RoomLifecycle::InGame;

        self.broadcast_room_update();
        self.broadcast_state_view();
    }

    /// ZeroTrust 模式开局 (M5.B.8). 给 4 真人玩家各发一条 [`ServerMsg::MpStart`],
    /// 各自 spawn MpPlayerActor 接管协议层. RoomActor 进 InGame 状态但不再
    /// 处理 ClientMsg::Action — game 命令走 P2P (mental poker 消息).
    ///
    /// 调用前已 verify: state=Lobby, is_host, all_ready, n=4.
    pub(super) fn handle_back_to_room(&mut self, _player_id: u32) {
        if self.state != RoomLifecycle::GameEnd {
            return;
        }
        self.reset_to_lobby();
    }
    pub(super) fn handle_continue_game(&mut self, player_id: u32) {
        if self.state != RoomLifecycle::GameEnd {
            return;
        }
        if !self.is_host(player_id) {
            return;
        }
        // 用旧配置开新一庄
        self.round_index = 1;
        let mut g = GameEngine::new(self.config.clone());
        g.start_round(self.game_seed ^ self.round_index);
        self.game = Some(g);
        self.state = RoomLifecycle::InGame;
        self.broadcast_room_update();
        self.broadcast_state_view();
    }
    pub(super) fn handle_leave(&mut self, player_id: u32) {
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
    pub(super) fn mark_disconnected(&mut self, player_id: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == player_id) {
            // 进入 reconnect grace 期: sender 清掉 (无法继续 push 消息),
            // connected 仍标 true (UI 显示"等待重连"). 客户端持 reconnect_token
            // 可在 RECONNECT_GRACE_SECS 秒内重连恢复.
            slot.sender = None;
            slot.disconnected_at = Some(std::time::Instant::now());
        }
        // 启 grace timer: 满 30s 后回送 ReconnectGraceTimeout 让 actor 检查
        // 是否需要永久转 AI.
        let self_tx = self.self_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(RECONNECT_GRACE_SECS)).await;
            let _ = self_tx.send(RoomCmd::ReconnectGraceTimeout { player_id });
        });
        self.broadcast_room_update();
    }

    /// grace timer 触发: 检查 slot 是否仍未重连. 是 → 永久标记 disconnected
    /// (connected=false), is_seat_ai 视为 AI, advance_game 让 AI 接管.
    pub(super) fn on_reconnect_grace_timeout(&mut self, player_id: u32) {
        let mut transitioned = false;
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == player_id) {
            if slot.disconnected_at.is_some() && slot.sender.is_none() {
                slot.connected = false;
                slot.disconnected_at = None;
                transitioned = true;
            }
        }
        if transitioned {
            self.broadcast_room_update();
            if self.state == RoomLifecycle::InGame {
                self.advance_game();
            }
        }
    }
    pub(super) fn reset_to_lobby(&mut self) {
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
}
