//! 玩家局内状态 ([`PlayerState`]).
//!
//! 4 家各自一份, 由 [`crate::engine::round_state::CommonRound::players`] 数组持有.
//! 跨 [`crate::engine::round_state::RoundState`] 各 phase 共享.

use crate::engine::domain::hand::Hand;
use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::Tile;
use serde::{Deserialize, Serialize};

/// 单家在一局内的全部状态.
///
/// 包含手牌 / 弃牌河 / 分数 / 立直状态 / 一发标志. 局间 *不持续* (新局开始时
/// reset_round 清, 仅 score 通过 [`MatchState`] → `init_round` 重新注入).
///
/// [`MatchState`]: crate::engine::match_state::MatchState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    /// 该家固定座位 (East / South / West / North).
    pub seat: Seat,
    /// 手牌 (闭手 + 副露).
    pub hand: Hand,
    /// 弃牌河 (河 / 捨牌 / Sutehai). 按弃牌时间从早到晚排序.
    /// 振听 (Furiten) 判定基于本字段 (但当前 engine 未实现 furiten).
    pub river: Vec<Tile>,
    /// 持点 (持ち点 / Mochiten). 整庄初始 = `rules.starting_score`, 局间通过
    /// [`crate::engine::match_state::match_apply`] 更新.
    pub score: i32,
    /// 是否已立直 (立直 / Riichi). 立直方扣 1000 进供托池, 限定摸切, 翻里宝牌.
    pub riichi: bool,
    /// 是否双立直 (W立直 / Daburi / Daburu Riichi).
    /// 第一巡内 (`first_go_around=true`) 立直自动升级为双立直, 给 +1 番.
    pub double_riichi: bool,
    /// *一发* (一発 / Ippatsu) 标志是否仍生效.
    ///
    /// 立直后下一巡内若和了 = 一发役. 中间若鸣牌 (任意家鸣) 或自家杠则失效.
    /// engine 在每次鸣牌 op 后清所有玩家的此 flag.
    pub ippatsu_active: bool,
    /// 刚摸到尚未切的那张牌. 仅 `AwaitDiscard` / `AwaitRiichiDiscard` 有意义.
    /// 鸣牌后 = `None` (鸣牌不摸新牌).
    pub last_drawn: Option<Tile>,
    /// 立直宣告牌在 [`river`] 中的索引. 立直时切的那张牌 90° 横置展示用 (UI
    /// 提示玩家该家已立直). `None` = 未立直.
    ///
    /// [`river`]: PlayerState::river
    pub riichi_river_idx: Option<usize>,
}

impl PlayerState {
    /// 起始 state. score = MatchState 注入, 其它字段空 / false.
    pub fn new(seat: Seat, score: i32) -> Self {
        Self {
            seat,
            hand: Hand::new(),
            river: Vec::new(),
            score,
            riichi: false,
            double_riichi: false,
            ippatsu_active: false,
            last_drawn: None,
            riichi_river_idx: None,
        }
    }

    /// 局间 reset (保留 seat + score, 清局内状态).
    /// 注: 当前 init_round 直接 `PlayerState::new` 重新构造, 此方法是 legacy 兼容.
    pub fn reset_round(&mut self) {
        self.hand = Hand::new();
        self.river.clear();
        self.riichi = false;
        self.double_riichi = false;
        self.ippatsu_active = false;
        self.last_drawn = None;
        self.riichi_river_idx = None;
    }

    /// 暗手当前张数. 通常:
    /// - 局开始: 13
    /// - 摸牌后: 14
    /// - 杠后岭上摸前: 13 (暗杠 4 张移到 melds 后)
    pub fn closed_count(&self) -> usize {
        self.hand.closed.len()
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
    fn new_initializes_default_state() {
        let p = PlayerState::new(Seat::East, 25000);
        assert_eq!(p.seat, Seat::East);
        assert_eq!(p.score, 25000);
        assert_eq!(p.hand.closed.len(), 0);
        assert_eq!(p.hand.melds.len(), 0);
        assert!(p.river.is_empty());
        assert!(!p.riichi);
        assert!(!p.double_riichi);
        assert!(!p.ippatsu_active);
        assert!(p.last_drawn.is_none());
        assert!(p.riichi_river_idx.is_none());
    }

    #[test]
    fn closed_count_reflects_hand() {
        let mut p = PlayerState::new(Seat::South, 25000);
        assert_eq!(p.closed_count(), 0);
        p.hand.closed.push(t(0, 0));
        p.hand.closed.push(t(1, 1));
        assert_eq!(p.closed_count(), 2);
    }

    #[test]
    fn reset_round_keeps_seat_and_score_clears_round_state() {
        let mut p = PlayerState::new(Seat::West, 30000);
        // 撒满局内状态.
        p.hand.closed.push(t(5, 5));
        p.river.push(t(2, 2));
        p.riichi = true;
        p.double_riichi = true;
        p.ippatsu_active = true;
        p.last_drawn = Some(t(0, 9));
        p.riichi_river_idx = Some(3);

        p.reset_round();

        // 局间保留:
        assert_eq!(p.seat, Seat::West);
        assert_eq!(p.score, 30000);
        // 局内 reset:
        assert_eq!(p.hand.closed.len(), 0);
        assert!(p.river.is_empty());
        assert!(!p.riichi);
        assert!(!p.double_riichi);
        assert!(!p.ippatsu_active);
        assert!(p.last_drawn.is_none());
        assert!(p.riichi_river_idx.is_none());
    }
}
