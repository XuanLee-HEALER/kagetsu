//! 手牌容器.

use crate::domain::meld::Meld;
use crate::domain::tile::{TILE_KINDS, Tile, count_by_kind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hand {
    /// 暗手部分(未鸣牌的部分),包括摸到尚未切的那张.
    pub closed: Vec<Tile>,
    /// 副露.
    pub melds: Vec<Meld>,
}

impl Hand {
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否门清(无副露; 暗杠在判定特定役时另行处理).
    pub fn is_menzen(&self) -> bool {
        self.melds.iter().all(|m| m.is_concealed())
    }

    /// 是否完全无副露(包括暗杠).用于天和等极端役.
    pub fn is_fully_concealed(&self) -> bool {
        self.melds.is_empty()
    }

    pub fn closed_counts(&self) -> [u8; TILE_KINDS] {
        count_by_kind(&self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::meld::{Meld, MeldKind, Seat};
    use crate::domain::tile::TileIndex;

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
