//! 座位 (Seat) + 副露 (Meld / 鳴き / 副露 / Furo).
//!
//! 麻将 4 家固定按 *逆时针* 排列: 东 → 南 → 西 → 北 → 东... [`Seat::next`]
//! 实现这个轮转. 副露指鸣牌后公开亮在桌上的牌组 ([`Meld`]) — 含吃 (Chi) /
//! 碰 (Pon) / 杠 (Kan, 三种).

use serde::{Deserialize, Serialize};

use crate::engine::domain::tile::Tile;

/// 4 家座位.
///
/// 顺序: `East → South → West → North → East...` ([`Seat::next`]).
/// 与日麻惯例一致 (逆时针, 即麻将桌上的左旋).
///
/// 索引: [`Seat::index`] 返回 0..=3, 用于数组寻址 (例: `players[seat.index()]`).
///
/// # 术语
///
/// - 当庄家 (亲家 / 親 / Oya) = 当前局的 `MatchState::dealer`
/// - 上家 (上家 / カミチャ / Kamicha) = 当前家的 *逆方向* 一家 (即 [`Seat::next`] 反向)
/// - 下家 (下家 / シモチャ / Shimocha) = `current.next()`
/// - 对家 (対面 / トイメン / Toimen) = `current.next().next()`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Seat {
    /// 东 (东家 / トン / Ton).
    East,
    /// 南 (南家 / ナン / Nan).
    South,
    /// 西 (西家 / シャー / Shaa).
    West,
    /// 北 (北家 / ペー / Pee).
    North,
}

impl Seat {
    /// 4 家全集合, 用于 `for seat in Seat::ALL { ... }` 遍历.
    pub const ALL: [Seat; 4] = [Seat::East, Seat::South, Seat::West, Seat::North];

    /// 下家 (Shimocha) — 麻将桌逆时针下一家.
    ///
    /// `East → South → West → North → East...`
    pub fn next(self) -> Seat {
        match self {
            Seat::East => Seat::South,
            Seat::South => Seat::West,
            Seat::West => Seat::North,
            Seat::North => Seat::East,
        }
    }

    /// 数组索引 (East=0, South=1, West=2, North=3).
    ///
    /// 用于 `players[seat.index()]` 等数组寻址.
    pub fn index(self) -> usize {
        match self {
            Seat::East => 0,
            Seat::South => 1,
            Seat::West => 2,
            Seat::North => 3,
        }
    }
}

/// 副露 (鳴き / Naki) 类型.
///
/// 5 个 variant 对应日麻 5 种合法副露:
/// - **顺子类** (Chi): 3 张连续同花色
/// - **刻子类** (Pon): 3 张同 kind
/// - **杠子类** (Minkan / Shouminkan / Ankan): 4 张同 kind, 来源不同
///
/// 各 variant `tiles` 数组长度匹配该副露的牌数 (3 或 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeldKind {
    /// 吃 (チー / Chi) — 顺子鸣牌. 3 张连续同花色 (例: 3m/4m/5m).
    /// 仅可吃 *上家* 弃牌, 因此 [`Meld::from`] = `current.next()` 反向 = 上家.
    Chi { tiles: [Tile; 3] },
    /// 碰 (ポン / Pon) — 刻子鸣牌. 3 张同 kind, 来自任意他家弃牌.
    Pon { tiles: [Tile; 3] },
    /// 大明杠 (大明槓 / Minkan) — 鸣方手中 3 张同 kind 配他家弃牌成 4 张杠子.
    Minkan { tiles: [Tile; 4] },
    /// 加杠 / 小明杠 (加槓 / 小明槓 / Shouminkan) — 已有副露 Pon 加自手第 4 张同 kind.
    /// 是 *被抢杠* (Chankan / 槍槓役) 的唯一时机.
    Shouminkan { tiles: [Tile; 4] },
    /// 暗杠 (暗槓 / Ankan) — 自手 4 张同 kind 直接副露成杠.
    /// 不破坏门前清 (Menzen) 状态.
    Ankan { tiles: [Tile; 4] },
}

/// 一个副露牌组 (Meld) — `kind` + 来源.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meld {
    /// 副露类型 + 包含的牌.
    pub kind: MeldKind,
    /// 牌取自哪家. 暗杠 (Ankan) 没有来源 = `None`, 其它都 `Some`.
    pub from: Option<Seat>,
}

impl Meld {
    /// 是否暗副露 (Concealed). 仅暗杠 (Ankan) 算.
    ///
    /// 暗杠不破坏门前清, 影响役判定 (例: 三暗刻可含暗杠).
    pub fn is_concealed(&self) -> bool {
        matches!(self.kind, MeldKind::Ankan { .. })
    }

    /// 是否杠子. Minkan / Shouminkan / Ankan 均 true.
    pub fn is_kan(&self) -> bool {
        matches!(
            self.kind,
            MeldKind::Minkan { .. } | MeldKind::Shouminkan { .. } | MeldKind::Ankan { .. }
        )
    }

    /// 副露包含的所有牌 (3 或 4 张).
    pub fn tiles(&self) -> &[Tile] {
        match &self.kind {
            MeldKind::Chi { tiles } | MeldKind::Pon { tiles } => tiles,
            MeldKind::Minkan { tiles }
            | MeldKind::Shouminkan { tiles }
            | MeldKind::Ankan { tiles } => tiles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::tile::TileIndex;

    fn t(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    #[test]
    fn seat_next_cycles() {
        assert_eq!(Seat::East.next(), Seat::South);
        assert_eq!(Seat::South.next(), Seat::West);
        assert_eq!(Seat::West.next(), Seat::North);
        assert_eq!(Seat::North.next(), Seat::East);
    }

    #[test]
    fn seat_index_matches_all_array() {
        for (i, s) in Seat::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn seat_serde_roundtrip() {
        for s in Seat::ALL {
            let json = serde_json::to_string(&s).unwrap();
            let back: Seat = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn ankan_is_concealed_others_not() {
        let chi = Meld {
            kind: MeldKind::Chi {
                tiles: [t(0, 0), t(1, 1), t(2, 2)],
            },
            from: Some(Seat::North),
        };
        assert!(!chi.is_concealed());
        let pon = Meld {
            kind: MeldKind::Pon {
                tiles: [t(0, 0), t(0, 1), t(0, 2)],
            },
            from: Some(Seat::South),
        };
        assert!(!pon.is_concealed());
        let minkan = Meld {
            kind: MeldKind::Minkan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            from: Some(Seat::West),
        };
        assert!(!minkan.is_concealed());
        let shouminkan = Meld {
            kind: MeldKind::Shouminkan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            from: Some(Seat::West),
        };
        assert!(!shouminkan.is_concealed());
        let ankan = Meld {
            kind: MeldKind::Ankan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            from: None,
        };
        assert!(ankan.is_concealed());
    }

    #[test]
    fn is_kan_only_for_kan_variants() {
        let chi = Meld {
            kind: MeldKind::Chi {
                tiles: [t(0, 0), t(1, 1), t(2, 2)],
            },
            from: Some(Seat::North),
        };
        assert!(!chi.is_kan());
        let pon = Meld {
            kind: MeldKind::Pon {
                tiles: [t(0, 0), t(0, 1), t(0, 2)],
            },
            from: Some(Seat::South),
        };
        assert!(!pon.is_kan());
        for kk in [
            MeldKind::Minkan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            MeldKind::Shouminkan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            MeldKind::Ankan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
        ] {
            let m = Meld {
                kind: kk,
                from: None,
            };
            assert!(m.is_kan());
        }
    }

    #[test]
    fn tiles_returns_correct_slice() {
        let chi = Meld {
            kind: MeldKind::Chi {
                tiles: [t(0, 0), t(1, 1), t(2, 2)],
            },
            from: Some(Seat::North),
        };
        assert_eq!(chi.tiles().len(), 3);
        let kan = Meld {
            kind: MeldKind::Minkan {
                tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
            },
            from: Some(Seat::West),
        };
        assert_eq!(kan.tiles().len(), 4);
    }
}
