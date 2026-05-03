//! 副露(鸣牌)与座位.

use serde::{Deserialize, Serialize};

use crate::domain::tile::Tile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Seat {
    East,
    South,
    West,
    North,
}

impl Seat {
    pub const ALL: [Seat; 4] = [Seat::East, Seat::South, Seat::West, Seat::North];

    pub fn next(self) -> Seat {
        match self {
            Seat::East => Seat::South,
            Seat::South => Seat::West,
            Seat::West => Seat::North,
            Seat::North => Seat::East,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Seat::East => 0,
            Seat::South => 1,
            Seat::West => 2,
            Seat::North => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeldKind {
    /// 吃: 必为下家弃牌.
    Chi { tiles: [Tile; 3] },
    /// 碰.
    Pon { tiles: [Tile; 3] },
    /// 大明杠.
    Minkan { tiles: [Tile; 4] },
    /// 加杠(小明杠): 由已碰刻子加上自摸第四张.
    Shouminkan { tiles: [Tile; 4] },
    /// 暗杠.
    Ankan { tiles: [Tile; 4] },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meld {
    pub kind: MeldKind,
    /// 牌取自哪家(暗杠为 None).
    pub from: Option<Seat>,
}

impl Meld {
    pub fn is_concealed(&self) -> bool {
        matches!(self.kind, MeldKind::Ankan { .. })
    }

    pub fn is_kan(&self) -> bool {
        matches!(
            self.kind,
            MeldKind::Minkan { .. } | MeldKind::Shouminkan { .. } | MeldKind::Ankan { .. }
        )
    }

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
    use crate::domain::tile::TileIndex;

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
