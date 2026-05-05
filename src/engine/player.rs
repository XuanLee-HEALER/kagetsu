//! 单个玩家在一局内的全部状态.
//!
//! 跨 RoundState 各 phase 共享, 由 CommonRound 持有 `[PlayerState; 4]`.

use crate::engine::domain::hand::Hand;
use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::Tile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub seat: Seat,
    pub hand: Hand,
    pub river: Vec<Tile>,
    pub score: i32,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu_active: bool,
    pub last_drawn: Option<Tile>,
    /// 立直宣告牌在 river 中的索引 (UI 用 90° 横置). None = 未立直.
    pub riichi_river_idx: Option<usize>,
}

impl PlayerState {
    pub fn new(seat: Seat, score: i32) -> Self {
        Self {
            seat,
            hand: Hand::new(),
            river: Vec::new(),
            score,
            riichi: false,
            double_riichi: false,
            ippatsu_active: false,
            last_drawn: None,
            riichi_river_idx: None,
        }
    }

    pub fn reset_round(&mut self) {
        self.hand = Hand::new();
        self.river.clear();
        self.riichi = false;
        self.double_riichi = false;
        self.ippatsu_active = false;
        self.last_drawn = None;
        self.riichi_river_idx = None;
    }

    /// 返回 13 (含暗杠时仍为 13 + 杠的 1 张) 或 14 (摸牌后).
    pub fn closed_count(&self) -> usize {
        self.hand.closed.len()
    }
}
