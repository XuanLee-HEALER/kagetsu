//! 协议 4: 弃牌 announcement (M5.A.1).
//!
//! 玩家弃一张已摸到的牌. 协议 2 摸牌时该位置 plaintext 已对该玩家唯一确定,
//! 弃牌时直接广播 (deck_index, plaintext). 其他人验证: deck_index 在该玩家
//! drawn ∧ ¬discarded ∧ ¬melded ∧ ¬concealed_kanned. plaintext 一致性留协议
//! 7 和牌时全手牌核对 (此时其他人没看过该 plaintext, 信任 → 后续审计).
//!
//! 无新 ZK 证明.

use thiserror::Error;

use super::protocol_state::{HandStateError, Table};
use super::Curve;

/// 弃牌广播包.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscardAnnouncement {
    pub player: usize,
    pub deck_index: usize,
    pub plaintext: Curve,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscardError {
    #[error("玩家 {player} 不在 table (n_players={n})")]
    UnknownPlayer { player: usize, n: usize },
    #[error("hand state 错误: {0}")]
    Hand(#[from] HandStateError),
}

impl DiscardAnnouncement {
    /// 验证 + apply 到 Table.
    pub fn apply(&self, table: &mut Table) -> Result<(), DiscardError> {
        if self.player >= table.n_players {
            return Err(DiscardError::UnknownPlayer {
                player: self.player,
                n: table.n_players,
            });
        }
        let hand = table.hand_mut(self.player);
        hand.record_discard(self.deck_index, self.plaintext)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn discard_apply_updates_table() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        // 玩家 0 先摸, 然后弃
        table.hand_mut(0).record_draw(5, Some(pt)).unwrap();
        let ann = DiscardAnnouncement {
            player: 0,
            deck_index: 5,
            plaintext: pt,
        };
        ann.apply(&mut table).unwrap();
        assert!(!table.hand(0).has_in_hand(5));
        assert_eq!(table.hand(0).discarded_plaintext(5), Some(&pt));
    }

    #[test]
    fn discard_unknown_player_rejected() {
        let mut table = Table::new(4, 136);
        let ann = DiscardAnnouncement {
            player: 99,
            deck_index: 5,
            plaintext: Curve::default(),
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(DiscardError::UnknownPlayer { .. })
        ));
    }

    #[test]
    fn discard_not_drawn_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let ann = DiscardAnnouncement {
            player: 0,
            deck_index: 5,
            plaintext: Curve::rand(rng),
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(DiscardError::Hand(HandStateError::NotDrawn { .. }))
        ));
    }

    #[test]
    fn discard_double_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        table.hand_mut(0).record_draw(5, Some(pt)).unwrap();
        let ann = DiscardAnnouncement {
            player: 0,
            deck_index: 5,
            plaintext: pt,
        };
        ann.apply(&mut table).unwrap();
        assert!(matches!(
            ann.apply(&mut table),
            Err(DiscardError::Hand(HandStateError::AlreadyDiscarded { .. }))
        ));
    }

    /// 玩家 1 不能弃玩家 0 的位置 (因为 player 1 没摸过那个 deck_index).
    #[test]
    fn discard_other_player_index_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        table.hand_mut(0).record_draw(5, Some(pt)).unwrap();
        let ann = DiscardAnnouncement {
            player: 1, // 不是 0
            deck_index: 5,
            plaintext: pt,
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(DiscardError::Hand(HandStateError::NotDrawn { index: 5 }))
        ));
    }
}
