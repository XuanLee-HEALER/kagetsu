//! 牌山 + 王牌 + dora 指示牌.
//!
//! 一副牌共 136 张, 王牌固定 14 张:
//! - 4 张岭上(rinshan)
//! - 5 对 dora 指示牌(上层为表 dora, 下层为里 dora)
//!
//! 活牌山可摸 `136 - 14 - 13×4 = 70` 张.

use crate::tile::{Tile, standard_set};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;

const DEAD_WALL_LEN: usize = 14;
const RINSHAN_LEN: usize = 4;
const DORA_INDICATORS_MAX: usize = 5;

pub struct Wall {
    /// 活牌山(从尾部摸: pop()).
    live: Vec<Tile>,
    /// 王牌区(共 14 张). 索引约定:
    /// `[0..4]` 岭上(从 0 开始消耗),
    /// `[4..14]` dora 区, 偶数 index = 表 dora 表牌, 奇数 = 对应里 dora.
    dead: Vec<Tile>,
    rinshan_used: usize,
    dora_revealed: usize,
}

impl Wall {
    pub fn shuffled(seed: u64, with_aka: bool) -> Self {
        let mut tiles = standard_set();
        if with_aka {
            // 把每花色的某张 5 标记为赤: 5m id 第一张, 5p, 5s.
            // 5m kind = 4, 5p kind = 13, 5s kind = 22.
            for &kind_idx in &[4u8, 13, 22] {
                if let Some(t) = tiles
                    .iter_mut()
                    .find(|t| t.kind.0 == kind_idx && !t.red)
                {
                    t.red = true;
                }
            }
        }

        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        tiles.shuffle(&mut rng);

        // 取最后 14 张作王牌.
        let dead = tiles.split_off(tiles.len() - DEAD_WALL_LEN);

        Self {
            live: tiles,
            dead,
            rinshan_used: 0,
            dora_revealed: 1, // 配牌后立即翻第一张表 dora.
        }
    }

    /// 摸一张活牌(从尾部).
    pub fn draw(&mut self) -> Option<Tile> {
        self.live.pop()
    }

    /// 杠后从岭上摸牌.
    pub fn rinshan_draw(&mut self) -> Option<Tile> {
        if self.rinshan_used >= RINSHAN_LEN {
            return None;
        }
        let t = self.dead[self.rinshan_used];
        self.rinshan_used += 1;
        Some(t)
    }

    /// 揭开新一张表 dora 指示牌(在杠成立后调用).
    pub fn reveal_next_dora(&mut self) {
        if self.dora_revealed < DORA_INDICATORS_MAX {
            self.dora_revealed += 1;
        }
    }

    /// 当前已揭开的表 dora 指示牌列表.
    pub fn dora_indicators(&self) -> Vec<Tile> {
        (0..self.dora_revealed)
            .map(|i| self.dead[RINSHAN_LEN + i * 2])
            .collect()
    }

    /// 里 dora 指示牌(立直和牌时才公开).
    pub fn ura_dora_indicators(&self) -> Vec<Tile> {
        (0..self.dora_revealed)
            .map(|i| self.dead[RINSHAN_LEN + i * 2 + 1])
            .collect()
    }

    /// 活牌山剩余张数.
    pub fn remaining(&self) -> usize {
        self.live.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_initial_state() {
        let w = Wall::shuffled(42, true);
        assert_eq!(w.live.len(), 136 - 14);
        assert_eq!(w.dead.len(), 14);
        assert_eq!(w.dora_indicators().len(), 1);
        assert_eq!(w.remaining(), 122);
    }

    #[test]
    fn rinshan_capped_at_4() {
        let mut w = Wall::shuffled(42, false);
        for _ in 0..4 {
            assert!(w.rinshan_draw().is_some());
        }
        assert!(w.rinshan_draw().is_none());
    }
}
