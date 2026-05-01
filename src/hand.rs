//! 手牌容器.

use crate::meld::Meld;
use crate::tile::{TILE_KINDS, Tile, count_by_kind};

#[derive(Debug, Clone, Default)]
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
