//! 牌 (Tile / 牌 / Hai) 定义.
//!
//! 一副日麻牌 136 张:
//! - **数牌** (数牌 / 数牌 / Suupai): 万子 / 筒子 / 索子 各 9 种 × 4 = 108 张
//! - **字牌** (字牌 / 字牌 / Jihai): 风牌 (东南西北) 4 种 + 三元牌 (白發中) 3 种 = 7 种 × 4 = 28 张
//!
//! *赤宝牌* (赤ドラ / Aka-Dora): 各花色的 5 各替换 1 张为红色版本 (5m / 5p / 5s).
//! 实战常加 3 张赤五, 总数仍 136 但每副有 3 张红 5.
//!
//! # Tile vs TileIndex
//!
//! - [`TileIndex`] = "牌的种类" (0..34), 不区分同 kind 的 4 张. 用于评分 / 役判定.
//! - [`Tile`] = "具体某一张" (带 `id` 唯一标识, `red` 标志赤宝牌). 用于鸣牌 /
//!   弃牌 / 录像 — 需要精确定位 *哪一张* 时.

use serde::{Deserialize, Serialize};

/// 花色 (Suit).
///
/// 数牌 3 种 (Man / Pin / Sou) + 字牌 2 种 (Wind / Dragon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Suit {
    /// 万子 (萬子 / マンズ / Manzu) — 1m..9m.
    Man,
    /// 筒子 (筒子 / ピンズ / Pinzu) — 1p..9p.
    Pin,
    /// 索子 (索子 / ソウズ / Souzu) — 1s..9s.
    Sou,
    /// 风牌 (風牌 / カゼハイ / Kazehai) — 东南西北 (Ton/Nan/Sha/Pee).
    Wind,
    /// 三元牌 (三元牌 / サンゲンパイ / Sangenpai) — 白發中 (Haku/Hatsu/Chun).
    Dragon,
}

/// 牌种类索引 (0..34). **同 kind 的 4 张共用同一 TileIndex**.
///
/// 编码:
///
/// | 范围      | 含义            |
/// |-----------|-----------------|
/// | 0..9      | 1m..9m (万子)   |
/// | 9..18     | 1p..9p (筒子)   |
/// | 18..27    | 1s..9s (索子)   |
/// | 27..31    | 东 / 南 / 西 / 北 |
/// | 31..34    | 白 / 發 / 中    |
///
/// 常量 [`TileIndex::EAST`] / [`TileIndex::HAKU`] 等提供命名访问.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileIndex(pub u8);

/// 一张具体的牌. 带唯一 `id` 用于区分同 kind 的 4 张, 区分红 5 vs 普通 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tile {
    /// 种类 (0..34).
    pub kind: TileIndex,
    /// 是否赤宝牌 (红色版本). 仅 5m / 5p / 5s 可能为 true.
    /// 红 5 算 1 番宝牌但牌种仍是普通 5.
    pub red: bool,
    /// 牌山中唯一 id (0..136). 用于精确定位某张牌 — 例: 切牌 / 鸣牌时确定
    /// 是 4 张 5m 中的哪一张, 或区分两张普通 5m vs 一张红 5m.
    pub id: u16,
}

pub const TILE_KINDS: usize = 34;

impl TileIndex {
    pub const EAST: TileIndex = TileIndex(27);
    pub const SOUTH: TileIndex = TileIndex(28);
    pub const WEST: TileIndex = TileIndex(29);
    pub const NORTH: TileIndex = TileIndex(30);
    pub const HAKU: TileIndex = TileIndex(31);
    pub const HATSU: TileIndex = TileIndex(32);
    pub const CHUN: TileIndex = TileIndex(33);

    pub fn suit(self) -> Suit {
        match self.0 {
            0..=8 => Suit::Man,
            9..=17 => Suit::Pin,
            18..=26 => Suit::Sou,
            27..=30 => Suit::Wind,
            31..=33 => Suit::Dragon,
            _ => panic!("invalid tile index {}", self.0),
        }
    }

    /// 数牌返回 1..=9, 字牌返回 None.
    pub fn rank(self) -> Option<u8> {
        match self.0 {
            0..=8 => Some(self.0 + 1),
            9..=17 => Some(self.0 - 9 + 1),
            18..=26 => Some(self.0 - 18 + 1),
            _ => None,
        }
    }

    pub fn is_suupai(self) -> bool {
        self.0 < 27
    }

    pub fn is_honor(self) -> bool {
        self.0 >= 27
    }

    pub fn is_terminal(self) -> bool {
        matches!(self.rank(), Some(1 | 9))
    }

    /// 幺九牌: 1, 9, 字牌.
    pub fn is_yaochuu(self) -> bool {
        self.is_terminal() || self.is_honor()
    }

    /// 中张: 数牌 2-8.
    pub fn is_simple(self) -> bool {
        matches!(self.rank(), Some(2..=8))
    }

    pub fn is_wind(self) -> bool {
        (27..=30).contains(&self.0)
    }

    pub fn is_dragon(self) -> bool {
        (31..=33).contains(&self.0)
    }

    /// 绿一色用牌: 索 2/3/4/6/8 + 發.
    pub fn is_green(self) -> bool {
        matches!(self.0, 19 | 20 | 21 | 23 | 25 | 32)
    }

    /// dora 表牌指向的下一张(即 dora).
    /// 1m..9m -> 2m..1m, 1p..9p -> 2p..1p, 索同, 东南西北循环, 白發中循环.
    pub fn next_dora(self) -> TileIndex {
        let n = self.0;
        let next = match n {
            0..=8 => (n + 1) % 9,
            9..=17 => 9 + (n - 9 + 1) % 9,
            18..=26 => 18 + (n - 18 + 1) % 9,
            27..=30 => 27 + (n - 27 + 1) % 4,
            31..=33 => 31 + (n - 31 + 1) % 3,
            _ => panic!("invalid tile index {n}"),
        };
        TileIndex(next)
    }

    /// 短文本表示: "1m" / "5pr" / "東" / "中".
    pub fn short(self) -> String {
        match self.0 {
            0..=8 => format!("{}m", self.0 + 1),
            9..=17 => format!("{}p", self.0 - 9 + 1),
            18..=26 => format!("{}s", self.0 - 18 + 1),
            27 => "東".into(),
            28 => "南".into(),
            29 => "西".into(),
            30 => "北".into(),
            31 => "白".into(),
            32 => "發".into(),
            33 => "中".into(),
            _ => "?".into(),
        }
    }
}

/// 标准 136 张牌(无赤). 顺序: 1m×4, 2m×4, ..., 中×4.
pub fn standard_set() -> Vec<Tile> {
    let mut tiles = Vec::with_capacity(136);
    let mut id: u16 = 0;
    for k in 0..TILE_KINDS as u8 {
        for _ in 0..4 {
            tiles.push(Tile {
                kind: TileIndex(k),
                red: false,
                id,
            });
            id += 1;
        }
    }
    tiles
}

/// 把一组牌按种类索引计数为长度 34 的数组.
pub fn count_by_kind(tiles: &[Tile]) -> [u8; TILE_KINDS] {
    let mut c = [0u8; TILE_KINDS];
    for t in tiles {
        c[t.kind.0 as usize] += 1;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_set_has_136_tiles() {
        assert_eq!(standard_set().len(), 136);
    }

    #[test]
    fn next_dora_wraps() {
        assert_eq!(TileIndex(8).next_dora(), TileIndex(0)); // 9m -> 1m
        assert_eq!(TileIndex(30).next_dora(), TileIndex(27)); // 北 -> 东
        assert_eq!(TileIndex(33).next_dora(), TileIndex(31)); // 中 -> 白
    }

    #[test]
    fn yaochuu_classification() {
        assert!(TileIndex(0).is_yaochuu()); // 1m
        assert!(TileIndex(8).is_yaochuu()); // 9m
        assert!(!TileIndex(4).is_yaochuu()); // 5m
        assert!(TileIndex(27).is_yaochuu()); // 东
        assert!(TileIndex(33).is_yaochuu()); // 中
    }

    #[test]
    fn suit_classification() {
        assert_eq!(TileIndex(0).suit(), Suit::Man);
        assert_eq!(TileIndex(8).suit(), Suit::Man);
        assert_eq!(TileIndex(9).suit(), Suit::Pin);
        assert_eq!(TileIndex(17).suit(), Suit::Pin);
        assert_eq!(TileIndex(18).suit(), Suit::Sou);
        assert_eq!(TileIndex(26).suit(), Suit::Sou);
        assert_eq!(TileIndex(27).suit(), Suit::Wind);
        assert_eq!(TileIndex(30).suit(), Suit::Wind);
        assert_eq!(TileIndex(31).suit(), Suit::Dragon);
        assert_eq!(TileIndex(33).suit(), Suit::Dragon);
    }

    #[test]
    #[should_panic(expected = "invalid tile index")]
    fn suit_out_of_range_panics() {
        let _ = TileIndex(99).suit();
    }

    #[test]
    fn rank_returns_1_to_9_for_suupai() {
        assert_eq!(TileIndex(0).rank(), Some(1)); // 1m
        assert_eq!(TileIndex(8).rank(), Some(9)); // 9m
        assert_eq!(TileIndex(9).rank(), Some(1)); // 1p
        assert_eq!(TileIndex(17).rank(), Some(9));
        assert_eq!(TileIndex(18).rank(), Some(1)); // 1s
        assert_eq!(TileIndex(26).rank(), Some(9));
        assert_eq!(TileIndex(27).rank(), None); // 字牌
        assert_eq!(TileIndex(33).rank(), None);
    }

    #[test]
    fn is_suupai_vs_honor() {
        for k in 0..27 {
            assert!(TileIndex(k).is_suupai());
            assert!(!TileIndex(k).is_honor());
        }
        for k in 27..34 {
            assert!(!TileIndex(k).is_suupai());
            assert!(TileIndex(k).is_honor());
        }
    }

    #[test]
    fn is_terminal_only_1_or_9() {
        assert!(TileIndex(0).is_terminal()); // 1m
        assert!(TileIndex(8).is_terminal()); // 9m
        assert!(TileIndex(9).is_terminal()); // 1p
        assert!(TileIndex(26).is_terminal()); // 9s
        assert!(!TileIndex(1).is_terminal()); // 2m
        assert!(!TileIndex(7).is_terminal()); // 8m
        assert!(!TileIndex(27).is_terminal()); // 东 (字牌不是 terminal)
    }

    #[test]
    fn is_simple_only_2_to_8() {
        for k in 0..27 {
            let r = TileIndex(k).rank().unwrap();
            assert_eq!(TileIndex(k).is_simple(), (2..=8).contains(&r));
        }
        // 字牌不是 simple
        for k in 27..34 {
            assert!(!TileIndex(k).is_simple());
        }
    }

    #[test]
    fn is_wind_and_dragon() {
        for k in 27..=30 {
            assert!(TileIndex(k).is_wind());
            assert!(!TileIndex(k).is_dragon());
        }
        for k in 31..=33 {
            assert!(!TileIndex(k).is_wind());
            assert!(TileIndex(k).is_dragon());
        }
        // 数牌都不是
        assert!(!TileIndex(0).is_wind());
        assert!(!TileIndex(0).is_dragon());
    }

    /// 绿一色用牌: 索 2/3/4/6/8 + 發.
    #[test]
    fn is_green_set() {
        // 索 2/3/4/6/8 = TileIndex 19/20/21/23/25
        for &k in &[19u8, 20, 21, 23, 25, 32] {
            assert!(TileIndex(k).is_green(), "kind={k} 应为绿");
        }
        // 索 1/5/7/9 不是
        for &k in &[18u8, 22, 24, 26] {
            assert!(!TileIndex(k).is_green(), "kind={k} 不应为绿");
        }
        // 万 / 筒 / 风 / 白中 不是
        assert!(!TileIndex(0).is_green());
        assert!(!TileIndex(9).is_green());
        assert!(!TileIndex(27).is_green());
        assert!(!TileIndex(31).is_green());
        assert!(!TileIndex(33).is_green());
    }

    #[test]
    fn next_dora_within_suit() {
        assert_eq!(TileIndex(0).next_dora(), TileIndex(1)); // 1m -> 2m
        assert_eq!(TileIndex(7).next_dora(), TileIndex(8)); // 8m -> 9m
        // 风牌 wind 4 项内循环 (东南西北 → 东)
        assert_eq!(TileIndex(27).next_dora(), TileIndex(28)); // 东 -> 南
        assert_eq!(TileIndex(28).next_dora(), TileIndex(29));
        assert_eq!(TileIndex(29).next_dora(), TileIndex(30));
        // 三元 3 项循环
        assert_eq!(TileIndex(31).next_dora(), TileIndex(32)); // 白 -> 發
        assert_eq!(TileIndex(32).next_dora(), TileIndex(33)); // 發 -> 中
    }

    #[test]
    fn short_text_format() {
        assert_eq!(TileIndex(0).short(), "1m");
        assert_eq!(TileIndex(4).short(), "5m");
        assert_eq!(TileIndex(13).short(), "5p");
        assert_eq!(TileIndex(22).short(), "5s");
        assert_eq!(TileIndex(27).short(), "東");
        assert_eq!(TileIndex(30).short(), "北");
        assert_eq!(TileIndex(31).short(), "白");
        assert_eq!(TileIndex(33).short(), "中");
    }

    #[test]
    fn count_by_kind_aggregates() {
        let tiles = standard_set();
        let cnts = count_by_kind(&tiles);
        // 标准 set 每个 kind 4 张
        for c in &cnts {
            assert_eq!(*c, 4);
        }
        assert_eq!(cnts.iter().map(|c| *c as usize).sum::<usize>(), 136);
    }

    #[test]
    fn count_by_kind_partial() {
        let tiles = vec![
            Tile {
                kind: TileIndex(0),
                red: false,
                id: 0,
            },
            Tile {
                kind: TileIndex(0),
                red: false,
                id: 1,
            },
            Tile {
                kind: TileIndex(33),
                red: false,
                id: 2,
            },
        ];
        let cnts = count_by_kind(&tiles);
        assert_eq!(cnts[0], 2);
        assert_eq!(cnts[33], 1);
        assert_eq!(cnts[5], 0);
    }

    #[test]
    fn standard_set_each_kind_has_unique_ids() {
        let tiles = standard_set();
        // 每 kind 4 张 id 不同
        for k in 0..34u8 {
            let ids: Vec<u16> = tiles
                .iter()
                .filter(|t| t.kind.0 == k)
                .map(|t| t.id)
                .collect();
            assert_eq!(ids.len(), 4);
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 4, "kind={k} ids 应不重复");
        }
    }

    #[test]
    fn standard_set_has_no_aka() {
        let tiles = standard_set();
        assert!(tiles.iter().all(|t| !t.red));
    }

    #[test]
    fn tile_index_serde_roundtrip() {
        let t = TileIndex(15);
        let s = serde_json::to_string(&t).unwrap();
        let back: TileIndex = serde_json::from_str(&s).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn yaochuu_includes_all_terminals_and_honors() {
        let yaochuu_kinds: Vec<u8> = (0..34u8).filter(|&k| TileIndex(k).is_yaochuu()).collect();
        // 1m 9m 1p 9p 1s 9s + 7 字牌 = 13
        assert_eq!(yaochuu_kinds.len(), 13);
    }
}
