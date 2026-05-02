//! 牌张定义
//!
//! 一副牌 136 张:
//! - 数牌(suupai): 万 9 × 4 + 筒 9 × 4 + 索 9 × 4 = 108
//! - 字牌(jihai): 风 4 × 4 + 三元 3 × 4 = 28
//!
//! 加 3 张赤宝牌时: 各花色 5 各替换 1 张为红色版本.
//!
//! 用 `TileIndex` 表示"种类"(0..34), 用 `Tile` 表示具体某一张(带 id, 用于区分赤五和重复种).

use serde::{Deserialize, Serialize};

/// 花色.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Suit {
    Man,    // 万
    Pin,    // 筒
    Sou,    // 索
    Wind,   // 风(东南西北)
    Dragon, // 三元(白发中)
}

/// 牌的种类索引 (0..34).
///
/// | 范围   | 含义        |
/// |-------|-------------|
/// | 0..9  | 1m..9m      |
/// | 9..18 | 1p..9p      |
/// |18..27 | 1s..9s      |
/// |27..31 | 东 南 西 北 |
/// |31..34 | 白 發 中    |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileIndex(pub u8);

/// 一张具体的牌.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tile {
    pub kind: TileIndex,
    pub red: bool,
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
}
