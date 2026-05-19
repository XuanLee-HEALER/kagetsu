//! room/game — RoomActor 拆分模块: InGame 阶段动作 (handle_action / advance_game / call_window / AI 推进)
//!
//! impl block 直接读 super::RoomActor 私有字段 (Rust 子 module 可访问 parent 私有).

use super::*;
use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::Tile;
use crate::engine::phase::Phase;
use crate::engine::score::final_ranking;
use crate::net::protocol::{GameOverView, NetAction, RoomLifecycle, ServerMsg};
use std::collections::HashMap;

impl RoomActor {
    pub(super) fn handle_action(&mut self, player_id: u32, action: NetAction) {
        if self.state != RoomLifecycle::InGame {
            return;
        }
        let Some(seat) = self.player_seat(player_id) else {
            return;
        };

        // AwaitCalls 阶段的鸣牌响应走单独路径
        let phase = match self.game.as_ref() {
            Some(g) => g.phase(),
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
                if game.turn() != seat || game.phase() != Phase::AwaitDiscard {
                    return;
                }
                let tile_opt: Option<Tile> = game.players()[seat.index()]
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
                if game.turn() != seat || game.phase() != Phase::AwaitDiscard {
                    return;
                }
                let tile_opt: Option<Tile> = game.players()[seat.index()]
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
                if game.turn() != seat || game.phase() != Phase::AwaitDiscard {
                    return;
                }
                if let Some(score) = game.try_tsumo() {
                    game.declare_tsumo(score);
                }
            }
            NetAction::Ankan(kind) => {
                if game.turn() != seat || game.phase() != Phase::AwaitDiscard {
                    return;
                }
                let _ = game.do_ankan(kind);
            }
            NetAction::Shouminkan(kind) => {
                if game.turn() != seat || game.phase() != Phase::AwaitDiscard {
                    return;
                }
                let _ = game.do_shouminkan(kind);
            }
            // AwaitDiscard 阶段忽略鸣牌响应
            NetAction::Pon | NetAction::Chi(_) | NetAction::Minkan | NetAction::Pass => {}
            NetAction::NextRound => {
                if game.phase() == Phase::RoundEnd {
                    game.next_round();
                    if game.phase() == Phase::GameEnd {
                        self.finalize_game();
                        return;
                    }
                    self.round_index += 1;
                    // next_round 仅推进 MatchState; 仍需 start_round 发新牌山
                    let seed = self.game_seed ^ self.round_index;
                    game.start_round(seed);
                }
            }
        }
        self.broadcast_state_view();
    }

    /// AwaitCalls 阶段的玩家响应: 收 Pon/Chi/Minkan/Tsumo(=Ron)/Pass.
    /// 收齐后裁决: Ron > Pon=Kan > Chi.
    pub(super) fn handle_call_response(&mut self, player_id: u32, action: NetAction) {
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
    pub(super) fn resolve_call_window(&mut self) {
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
    pub(super) fn advance_game(&mut self) {
        // 安全上限: 一局至多 ~70 步, 200 远远够
        for _ in 0..200 {
            // 取当前 phase / turn (短借用立即释放)
            let (phase, turn) = match self.game.as_ref() {
                Some(g) => (g.phase(), g.turn()),
                None => return,
            };
            match phase {
                Phase::Draw => {
                    let game = self.game.as_mut().unwrap();
                    // do_draw 返 None 时, engine 已自动转 RoundEnd (荒牌流局).
                    if game.do_draw().is_none() {
                        if game.phase() == Phase::RoundEnd {
                            // 推 phase=RoundEnd 的 GameStateView 让 client 看到流局, 然后结算.
                            self.broadcast_state_view();
                            self.broadcast_round_result();
                            return;
                        }
                        // 极端兜底: 不应发生 (engine 摸尽必转 RoundEnd).
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
                        crate::ai::dummy::ai_choose_discard(&game.round)
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
                    let last_discarder = game_ref.last_discard().map(|(s, _)| s);
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
                    let window_ms = self.call_window_ms;
                    let deadline = chrono_now_unix_ms() + window_ms as i64;
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
                        tokio::time::sleep(std::time::Duration::from_millis(window_ms)).await;
                        let _ = self_tx.send(RoomCmd::CallTimeout {
                            generation: gen_now,
                        });
                    });
                    return;
                }
                Phase::RoundEnd => {
                    // 推 GameStateView 让 client 看到 phase=RoundEnd (用于 UI 转
                    // "按 N 进下一局" 状态), 然后推 RoundResultView 显示结算.
                    self.broadcast_state_view();
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

    /// 给真人 turn 推 ActionRequired (含 thinking_time deadline + 真实 hints).
    /// 同一 turn 重复推不要紧 (client 用最新 deadline 覆盖).
    pub(super) fn send_thinking_action_required(&self, seat: Seat) {
        let Some(slot) = self.slots.iter().find(|s| s.seat == Some(seat)) else {
            return;
        };
        let Some(sender) = &slot.sender else { return };
        let secs = self.config.thinking_time_secs.unwrap_or(0);
        let deadline_ms = if secs == 0 {
            0
        } else {
            chrono_now_unix_ms() + (secs as i64) * 1000
        };
        // 从 GameEngine 真实算可宣动作: 自摸 / 立直 / 暗杠 / 加杠. 切牌总是合法,
        // hints 始终至少含一条 Discard placeholder (UI 自己枚举具体手牌张).
        let mut hints: Vec<NetAction> = Vec::new();
        // Discard placeholder (kind 任意, UI 不读, 仅作"切牌动作合法"标志).
        hints.push(NetAction::Discard(crate::net::protocol::TileSpec {
            kind: crate::engine::domain::tile::TileIndex(0),
        }));
        if let Some(engine) = self.game.as_ref() {
            let opts = engine.legal_self_options();
            if opts.tsumo {
                hints.push(NetAction::Tsumo);
            }
            for tile in &opts.riichi_discards {
                hints.push(NetAction::Riichi(crate::net::protocol::TileSpec {
                    kind: tile.kind,
                }));
            }
            for kind in &opts.ankan {
                hints.push(NetAction::Ankan(*kind));
            }
            for kind in &opts.shouminkan {
                hints.push(NetAction::Shouminkan(*kind));
            }
        }
        let _ = sender.send(ServerMsg::ActionRequired {
            hints,
            deadline_unix_ms: deadline_ms,
        });
    }

    /// 当前 seat 是否 AI 控制. AI 接管条件:
    /// - `slot.is_ai = true` (开局补的 AI), 或
    /// - 永久断线 (connected=false 且 disconnected_at=None, grace 期已结束).
    ///
    /// grace 期内 (connected=false, disconnected_at=Some) **不**视为 AI, 游戏
    /// 暂停等真人重连.
    pub(super) fn is_seat_ai(&self, seat: Seat) -> bool {
        self.slots
            .iter()
            .find(|s| s.seat == Some(seat))
            .map(|s| s.is_ai || (!s.connected && s.disconnected_at.is_none()))
            .unwrap_or(true)
    }

    /// 把 AI 的 [`Action`] 转化成 GameEngine 调用. 失败时退化为摸切.
    pub(super) fn apply_ai_action(&mut self, action: crate::engine::domain::action::Action) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        use crate::engine::domain::action::Action;
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
                let me = game.turn();
                if let Some(t) = game.players()[me.index()].last_drawn {
                    let _ = game.do_discard(t);
                }
            }
        }
        self.broadcast_state_view();
    }
    pub(super) fn finalize_game(&mut self) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        // GameEngine.phase() == GameEnd 由 mat.ended + last_result.is_some() 自动推导,
        // next_round 触发 match_apply 已设 mat.ended; 此处不需要再写 phase 字段.
        let rankings = final_ranking(game.players(), game.rules());
        self.broadcast_state_view();
        self.broadcast_to_all(ServerMsg::GameEnd(GameOverView { rankings }));
        self.state = RoomLifecycle::GameEnd;
        self.broadcast_room_update();
    }

    // ========================================================================
    // 投影 / 广播
    // ========================================================================
}
