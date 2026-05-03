//! 玩家可执行的动作 (Action).
//!
//! 由 UI 或 AI 产生, 由 [`crate::game::GameState`] 消费.

use crate::domain::meld::Seat;
use crate::domain::tile::Tile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 普通切牌.
    Discard(Tile),
    /// 立直宣言+切牌.
    Riichi(Tile),
    /// 碰. tiles 为自手将与他家弃牌组成刻子的两张.
    Pon { tiles: [Tile; 2] },
    /// 吃. tiles 为自手将与下家弃牌组成顺子的两张.
    Chi { tiles: [Tile; 2] },
    /// 大明杠.
    Minkan,
    /// 暗杠(自摸第四张).
    Ankan(Tile),
    /// 加杠(已碰刻子加上自摸第四张).
    Shouminkan(Tile),
    /// 自摸和.
    Tsumo,
    /// 荣和(对 by 家弃牌).
    Ron(Seat),
    /// 九种九牌流局宣言.
    KyuushuKyuuhai,
    /// 跳过(对鸣牌/和牌机会放弃).
    Pass,
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
    fn action_equality_distinguishes_variants() {
        let d1 = Action::Discard(t(0, 0));
        let d2 = Action::Discard(t(0, 0));
        assert_eq!(d1, d2);
        let r = Action::Riichi(t(0, 0));
        assert_ne!(d1, r);
    }

    #[test]
    fn discard_distinguishes_tiles() {
        let a = Action::Discard(t(0, 0));
        let b = Action::Discard(t(1, 1));
        assert_ne!(a, b);
    }

    #[test]
    fn pon_with_different_tile_pairs_differ() {
        let p1 = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        let p2 = Action::Pon {
            tiles: [t(0, 2), t(0, 3)],
        };
        assert_ne!(p1, p2);
        // 同 tiles 同 id 相等
        let p3 = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        assert_eq!(p1, p3);
    }

    #[test]
    fn ron_records_target_seat() {
        let r1 = Action::Ron(crate::domain::meld::Seat::West);
        let r2 = Action::Ron(crate::domain::meld::Seat::North);
        assert_ne!(r1, r2);
    }

    #[test]
    fn pass_and_tsumo_are_unit() {
        assert_eq!(Action::Pass, Action::Pass);
        assert_eq!(Action::Tsumo, Action::Tsumo);
        assert_eq!(Action::KyuushuKyuuhai, Action::KyuushuKyuuhai);
        assert_eq!(Action::Minkan, Action::Minkan);
        assert_ne!(Action::Pass, Action::Tsumo);
    }

    #[test]
    fn ankan_carries_tile_kind() {
        let a1 = Action::Ankan(t(0, 0));
        let a2 = Action::Ankan(t(1, 0));
        assert_ne!(a1, a2);
    }

    #[test]
    fn action_clones_correctly() {
        let a = Action::Pon {
            tiles: [t(0, 0), t(0, 1)],
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
