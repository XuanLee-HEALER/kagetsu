//! room/projection — RoomActor 拆分模块: GameStateView / RoomView 投影 + broadcast (该 module 内不改 state)
//!
//! impl block 直接读 super::RoomActor 私有字段 (Rust 子 module 可访问 parent 私有).

use super::*;
use crate::engine::domain::meld::Seat;
use crate::engine::round_state::RoundResult;
use crate::net::protocol::{
    GameStateView, PlayerView, RoomLifecycle, RoomView, RoundResultView, ServerMsg,
};

impl RoomActor {
    pub(super) fn room_view(&self) -> RoomView {
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
            mode: self.mode,
        }
    }
    pub(super) fn project_view(&self, my_seat: Seat) -> Option<GameStateView> {
        let game = self.game.as_ref()?;
        let players_arr = game.players();
        let me = &players_arr[my_seat.index()];
        let players: [PlayerView; 4] = std::array::from_fn(|i| {
            let p = &players_arr[i];
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
                riichi_river_idx: p.riichi_river_idx,
            }
        });
        Some(GameStateView {
            round_wind: game.round_wind(),
            kyoku: game.kyoku(),
            honba: game.honba(),
            riichi_sticks: game.riichi_sticks(),
            dealer: game.dealer(),
            turn: game.turn(),
            phase: game.phase(),
            my_seat,
            my_hand: me.hand.closed.clone(),
            my_last_drawn: me.last_drawn,
            players,
            wall_remaining: game.wall_remaining(),
            dora_indicators: game.dora_indicators(),
            events: game.events.iter().cloned().collect(),
        })
    }
    pub(super) fn broadcast_state_view(&self) {
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
    pub(super) fn broadcast_round_result(&self) {
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
        let players = game.players();
        let scores = [
            players[0].score,
            players[1].score,
            players[2].score,
            players[3].score,
        ];
        self.broadcast_to_all(ServerMsg::RoundResult(RoundResultView { message, scores }));
    }
    pub(super) fn broadcast_room_update(&self) {
        let view = self.room_view();
        self.broadcast_to_all(ServerMsg::RoomUpdate(Box::new(view)));
        self.publish_lobby_dyn_state();
    }

    /// 把当前 (真人玩家数, lifecycle) 推到 lobby_watch (供 host_swarm_task
    /// publish_lobby 用). lobby_watch=None (未起 P2P listener) 时空操作.
    pub(super) fn publish_lobby_dyn_state(&self) {
        let Some(tx) = self.lobby_watch.as_ref() else {
            return;
        };
        let players = self.slots.iter().filter(|s| !s.is_ai).count() as u8;
        let lifecycle = match self.state {
            RoomLifecycle::Lobby => "lobby",
            RoomLifecycle::InGame => "in_game",
            RoomLifecycle::GameEnd => "game_end",
        };
        let _ = tx.send_replace(crate::net::p2p::host::LobbyDynState {
            players,
            lifecycle: lifecycle.into(),
        });
    }
    pub(super) fn broadcast_to_all(&self, msg: ServerMsg) {
        for slot in &self.slots {
            if let Some(s) = &slot.sender {
                let _ = s.send(msg.clone());
            }
        }
    }
    pub(super) fn send_error(&self, player_id: u32, err: &str) {
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
}
