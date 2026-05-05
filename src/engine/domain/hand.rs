//! 手牌 (手牌 / Tehai) 容器.
//!
//! 一手牌分两部分:
//! - **暗手** (闭手 / 暗牌 / Closed): 未公开的牌, 含刚摸的那张
//! - **副露** (鳴き / 副露 / Furo): 已公开的鸣牌牌组 (Chi / Pon / Kan)

use crate::engine::domain::meld::Meld;
use crate::engine::domain::tile::{TILE_KINDS, Tile, count_by_kind};
use serde::{Deserialize, Serialize};

/// 一家的完整手牌.
///
/// # 数量约束
///
/// - 局开始: `closed` = 13 张, `melds` = 空
/// - 摸完牌后 (待切): `closed` = 14 张, `melds` 不变
/// - 切完牌后: `closed` = 13 张
/// - 每副露一组减少 closed 中相应张数, 加 1 个 `melds` 条目:
///   - Chi/Pon: closed -2, melds +1 (3 张)
///   - Kan (任意): closed -3 或 -4, melds +1 (4 张)
///
/// 整手牌净张数 = 13 (待切时 14), 与杠子数无关 (杠子有 4 张但只占 3 张"位置",
/// 因为杠后必摸岭上一张补回).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hand {
    /// 暗手 (Closed). 未公开的牌, 含刚摸尚未切出的那张. 排序按 `(kind, !red)`,
    /// 即同 kind 红 5 排前面.
    pub closed: Vec<Tile>,
    /// 副露列表 (按副露发生时间从早到晚排序).
    pub melds: Vec<Meld>,
}

impl Hand {
    /// 空手牌. 局开始前由 `init_round` 配 13 张牌.
    pub fn new() -> Self {
        Self::default()
    }

    /// 门前清 (門前清 / Menzen) — 没有 *他人来源* 的副露.
    ///
    /// 暗杠 (Ankan) 仍算门前清. 立直 / 平和 / 三色同顺等门前限定役需此 true.
    pub fn is_menzen(&self) -> bool {
        self.melds.iter().all(|m| m.is_concealed())
    }

    /// 完全闭手 (无任何 melds, 含暗杠也算破).
    ///
    /// 用于天和 (Tenhou) / 地和 (Chiihou) / 人和 (Renhou) 等极端役判定 —
    /// 这些役要求局开始就和, 不能有任何副露 (即使暗杠也不行, 因为暗杠后必摸岭上).
    pub fn is_fully_concealed(&self) -> bool {
        self.melds.is_empty()
    }

    /// 暗手按 kind 计数, 返 `[count_of_kind_0, count_of_kind_1, ..., count_of_kind_33]`.
    /// 不含副露. 每个 count ∈ 0..=4.
    pub fn closed_counts(&self) -> [u8; TILE_KINDS] {
        count_by_kind(&self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::meld::{Meld, MeldKind, Seat};
    use crate::engine::domain::tile::TileIndex;

    fn t(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    #[test]
    fn empty_hand_is_menzen_and_fully_concealed() {
        let h = Hand::new();
        assert!(h.is_menzen());
        assert!(h.is_fully_concealed());
        assert_eq!(h.closed.len(), 0);
        assert_eq!(h.melds.len(), 0);
    }

    #[test]
    fn pon_breaks_menzen_and_fully_concealed() {
        let h = Hand {
            closed: vec![],
            melds: vec![Meld {
                kind: MeldKind::Pon {
                    tiles: [t(0, 0), t(0, 1), t(0, 2)],
                },
                from: Some(Seat::South),
            }],
        };
        assert!(!h.is_menzen());
        assert!(!h.is_fully_concealed());
    }

    /// 暗杠保留 menzen 但不 fully_concealed (因为有 melds, 即使暗杠).
    #[test]
    fn ankan_keeps_menzen_but_not_fully_concealed() {
        let h = Hand {
            closed: vec![],
            melds: vec![Meld {
                kind: MeldKind::Ankan {
                    tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
                },
                from: None,
            }],
        };
        assert!(h.is_menzen(), "暗杠仍门清");
        assert!(!h.is_fully_concealed(), "有 meld 即非 fully concealed");
    }

    #[test]
    fn closed_counts_aggregates() {
        let h = Hand {
            closed: vec![t(0, 0), t(0, 1), t(5, 2), t(33, 3)],
            melds: vec![],
        };
        let c = h.closed_counts();
        assert_eq!(c[0], 2);
        assert_eq!(c[5], 1);
        assert_eq!(c[33], 1);
        assert_eq!(c[10], 0);
    }

    #[test]
    fn mixed_melds_menzen_iff_only_ankan() {
        // 1 暗杠 + 1 碰 → 不门清
        let h = Hand {
            closed: vec![],
            melds: vec![
                Meld {
                    kind: MeldKind::Ankan {
                        tiles: [t(0, 0), t(0, 1), t(0, 2), t(0, 3)],
                    },
                    from: None,
                },
                Meld {
                    kind: MeldKind::Pon {
                        tiles: [t(5, 0), t(5, 1), t(5, 2)],
                    },
                    from: Some(Seat::West),
                },
            ],
        };
        assert!(!h.is_menzen());
    }
}
