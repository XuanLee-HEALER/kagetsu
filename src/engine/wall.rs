//! 牌山 + 王牌 + dora 指示牌.
//!
//! 一副牌共 136 张, 王牌固定 14 张:
//! - 4 张岭上(rinshan)
//! - 5 对 dora 指示牌(上层为表 dora, 下层为里 dora)
//!
//! 活牌山可摸 `136 - 14 - 13×4 = 70` 张.

use crate::engine::domain::tile::{Tile, standard_set};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

const DEAD_WALL_LEN: usize = 14;
const RINSHAN_LEN: usize = 4;
const DORA_INDICATORS_MAX: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
                if let Some(t) = tiles.iter_mut().find(|t| t.kind.0 == kind_idx && !t.red) {
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

    /// 测试 / replay 用: 用预定的活牌山 + 王牌区构造, 跳过随机洗牌.
    ///
    /// `live` 顺序 = 摸牌反向 (pop 从尾部, 所以 live\[len-1\] 是下一张被摸的牌).
    /// `dead` 必须 14 张, 索引约定:
    /// - \[0..4\] 岭上
    /// - \[4..14\] dora 区, 偶数表 dora / 奇数里 dora.
    ///
    /// `dora_revealed` ∈ \[1, 5\], 默认 1.
    pub fn from_components(live: Vec<Tile>, dead: Vec<Tile>, dora_revealed: usize) -> Self {
        assert_eq!(dead.len(), DEAD_WALL_LEN, "dead wall 必须 14 张");
        assert!(
            (1..=DORA_INDICATORS_MAX).contains(&dora_revealed),
            "dora_revealed 必须 ∈ [1, 5]"
        );
        Self {
            live,
            dead,
            rinshan_used: 0,
            dora_revealed,
        }
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

    #[test]
    fn from_components_preserves_state() {
        use crate::engine::domain::tile::TileIndex;
        let mk = |k: u8, id: u16| Tile {
            id,
            kind: TileIndex(k),
            red: false,
        };
        let live: Vec<Tile> = (0..70).map(|i| mk((i % 34) as u8, i as u16)).collect();
        let dead: Vec<Tile> = (70..84).map(|i| mk((i % 34) as u8, i as u16)).collect();
        let mut w = Wall::from_components(live, dead, 2);
        assert_eq!(w.remaining(), 70);
        assert_eq!(w.dora_indicators().len(), 2);
        // 摸 1 张应是 live 末尾 (id=69)
        assert_eq!(w.draw().unwrap().id, 69);
        assert_eq!(w.remaining(), 69);
        // rinshan 第 1 张
        assert_eq!(w.rinshan_draw().unwrap().id, 70);
    }

    /// 杠开后 dora 跟着翻, 但上限 5.
    #[test]
    fn reveal_next_dora_capped_at_5() {
        let mut w = Wall::shuffled(42, false);
        assert_eq!(w.dora_indicators().len(), 1);
        for expected in 2..=5 {
            w.reveal_next_dora();
            assert_eq!(w.dora_indicators().len(), expected);
        }
        // 第 5 张已翻, 再翻应 no-op.
        w.reveal_next_dora();
        assert_eq!(w.dora_indicators().len(), 5);
        // ura dora 长度跟 表 dora 一致 (立直和牌时全揭)
        assert_eq!(w.ura_dora_indicators().len(), 5);
    }

    /// ura_dora 跟 表 dora 张数一致, 但取的是 dead 区奇数索引 — 不应等于
    /// 同一张表 dora 指示牌 (除非两张随机刚好相同).
    #[test]
    fn ura_dora_indices_distinct_from_omote() {
        use crate::engine::domain::tile::TileIndex;
        // 用 from_components 显式控制 dead 区让 omote/ura 必定不同.
        let mk = |k: u8, id: u16| Tile {
            id,
            kind: TileIndex(k),
            red: false,
        };
        let live: Vec<Tile> = (0..70).map(|i| mk(0, i as u16)).collect();
        // dead[0..4] 岭上 / dead[4,6,8,10,12] 表 dora / dead[5,7,9,11,13] 里 dora
        // 用 kind 区分: 表 dora kind=1, 里 dora kind=2.
        let mut dead: Vec<Tile> = Vec::with_capacity(14);
        for i in 0..4 {
            dead.push(mk(0, (70 + i) as u16));
        }
        for i in 0..5 {
            dead.push(mk(1, (74 + i * 2) as u16)); // omote
            dead.push(mk(2, (75 + i * 2) as u16)); // ura
        }
        let w = Wall::from_components(live, dead, 5);
        for o in w.dora_indicators() {
            assert_eq!(o.kind.0, 1, "omote dora 应来自 kind=1 索引");
        }
        for u in w.ura_dora_indicators() {
            assert_eq!(u.kind.0, 2, "ura dora 应来自 kind=2 索引");
        }
    }

    /// with_aka = true 时, 5m / 5p / 5s 各恰 1 张赤.
    #[test]
    fn aka_dora_exactly_one_per_suit() {
        let w = Wall::shuffled(42, true);
        // 拼接 live + dead 看全副是否恰好 3 张赤.
        let all: Vec<&Tile> = w.live.iter().chain(w.dead.iter()).collect();
        let aka_count = all.iter().filter(|t| t.red).count();
        assert_eq!(aka_count, 3);
        // 每个 5 花色 (kind = 4 / 13 / 22) 恰 1 张赤.
        for &kind in &[4u8, 13, 22] {
            let cnt = all.iter().filter(|t| t.kind.0 == kind && t.red).count();
            assert_eq!(cnt, 1, "kind={kind} 期望恰 1 张赤, 实际 {cnt}");
        }
    }

    /// without aka 时全副无赤.
    #[test]
    fn no_aka_when_disabled() {
        let w = Wall::shuffled(42, false);
        let all: Vec<&Tile> = w.live.iter().chain(w.dead.iter()).collect();
        assert!(all.iter().all(|t| !t.red));
    }

    /// 全副 136 张, kind 分布 = 每 kind 4 张 (赤替换不增加张数).
    #[test]
    fn shuffled_total_136_distribution() {
        let w = Wall::shuffled(42, true);
        let all: Vec<&Tile> = w.live.iter().chain(w.dead.iter()).collect();
        assert_eq!(all.len(), 136);
        let mut counts = [0u8; 34];
        for t in &all {
            counts[t.kind.0 as usize] += 1;
        }
        for (k, &c) in counts.iter().enumerate() {
            assert_eq!(c, 4, "kind {k} 张数 {c} ≠ 4");
        }
    }

    /// draw 全摸完后返回 None.
    #[test]
    fn draw_exhaustion_returns_none() {
        let mut w = Wall::shuffled(42, false);
        let init = w.remaining();
        for _ in 0..init {
            assert!(w.draw().is_some());
        }
        assert_eq!(w.remaining(), 0);
        assert!(w.draw().is_none());
        assert!(w.draw().is_none()); // idempotent
    }

    /// 不同 seed 给不同顺序, 同 seed 给相同顺序 (deterministic).
    #[test]
    fn shuffled_is_deterministic_per_seed() {
        let w1 = Wall::shuffled(42, false);
        let w2 = Wall::shuffled(42, false);
        let w3 = Wall::shuffled(43, false);
        // 同 seed → 相同 live[0..5] ID 序列
        for i in 0..5 {
            assert_eq!(w1.live[i].id, w2.live[i].id, "同 seed 应一致");
        }
        // 不同 seed 应大概率不一样 (取 5 个比, 同概率 < 极小)
        let same_prefix = (0..5).all(|i| w1.live[i].id == w3.live[i].id);
        assert!(!same_prefix, "不同 seed 大概率给不同序列");
    }

    /// from_components 输入合法但 dora_revealed = 0 → panic.
    #[test]
    #[should_panic(expected = "dora_revealed")]
    fn from_components_rejects_zero_dora() {
        use crate::engine::domain::tile::TileIndex;
        let mk = |k: u8, id: u16| Tile {
            id,
            kind: TileIndex(k),
            red: false,
        };
        let live = (0..70).map(|i| mk(0, i)).collect();
        let dead = (70..84).map(|i| mk(0, i)).collect();
        let _ = Wall::from_components(live, dead, 0);
    }

    /// from_components dead 长度 ≠ 14 → panic.
    #[test]
    #[should_panic(expected = "dead wall 必须 14 张")]
    fn from_components_rejects_wrong_dead_size() {
        use crate::engine::domain::tile::TileIndex;
        let mk = |k: u8, id: u16| Tile {
            id,
            kind: TileIndex(k),
            red: false,
        };
        let live = (0..70).map(|i| mk(0, i)).collect();
        let dead = (70..82).map(|i| mk(0, i)).collect(); // 12 张
        let _ = Wall::from_components(live, dead, 1);
    }
}
