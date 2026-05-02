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
