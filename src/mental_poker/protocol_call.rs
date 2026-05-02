//! 协议 5: 鸣牌 announcement (吃/碰/明杠) — M5.A.2.
//!
//! 鸣牌玩家把暗手牌中的某些牌变成明牌副露. 广播 3 (吃/碰) 或 4 (明杠) 张牌的
//! 明文 + 它们对应的密文索引. 其他人验证:
//! 1. 自己手牌部分的 deck_indices 都在 player 的 drawn ∧ ¬discarded ∧ ¬melded.
//! 2. 1 张来自 from_player 的 from_deck_index, 该位置在 from_player 的 discarded
//!    集合 (协议 4 已弃过), plaintext 跟 discarded 那张一致.
//! 3. plaintexts 跟 indices 一一对应 (size + 元素一致).
//!
//! **牌型合法性** (3 张连续 / 相同 / 4 张相同) 留 application 层 (yaku.rs).
//! 本模块只做密文-明文-索引一致性 + 集合 ownership.
//!
//! 无新 ZK 证明.

use thiserror::Error;

use super::protocol_state::{CallType, HandStateError, MeldRecord, Table};
use super::Curve;

/// 鸣牌广播包.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallAnnouncement {
    pub player: usize,
    pub call_type: CallType,
    pub deck_indices: Vec<usize>,
    pub plaintexts: Vec<Curve>,
    /// 鸣谁的弃牌. 必须 != player.
    pub from_player: usize,
    /// from_player 弃牌位置 = deck_indices[from_position_in_meld].
    pub from_position_in_meld: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CallError {
    #[error("玩家 {player} 不在 table (n_players={n})")]
    UnknownPlayer { player: usize, n: usize },
    #[error("from_player {from} 不在 table (n_players={n})")]
    UnknownFromPlayer { from: usize, n: usize },
    #[error("不能鸣自己的弃牌 (player={player} == from_player)")]
    SelfCall { player: usize },
    #[error("from_position {pos} 越界 (deck_indices 长 {len})")]
    FromPositionOutOfRange { pos: usize, len: usize },
    #[error("from_player {from} 没弃过 deck_index={index} (协议 4 没记录)")]
    FromDiscardNotFound { from: usize, index: usize },
    #[error("from_player 弃牌的 plaintext 跟 announcement 不一致 (deck_index={index})")]
    FromPlaintextMismatch { index: usize },
    #[error("hand state 错误: {0}")]
    Hand(#[from] HandStateError),
}

impl CallAnnouncement {
    /// 验证 + apply 到 Table.
    pub fn apply(&self, table: &mut Table) -> Result<(), CallError> {
        let n = table.n_players;
        if self.player >= n {
            return Err(CallError::UnknownPlayer {
                player: self.player,
                n,
            });
        }
        if self.from_player >= n {
            return Err(CallError::UnknownFromPlayer {
                from: self.from_player,
                n,
            });
        }
        if self.player == self.from_player {
            return Err(CallError::SelfCall {
                player: self.player,
            });
        }
        if self.from_position_in_meld >= self.deck_indices.len() {
            return Err(CallError::FromPositionOutOfRange {
                pos: self.from_position_in_meld,
                len: self.deck_indices.len(),
            });
        }

        // 验证 from_player 的弃牌历史含此 index, plaintext 一致.
        let from_index = self.deck_indices[self.from_position_in_meld];
        let from_pt = self.plaintexts[self.from_position_in_meld];
        let from_hand = table.hand(self.from_player);
        let recorded = from_hand
            .discarded_plaintext(from_index)
            .ok_or(CallError::FromDiscardNotFound {
                from: self.from_player,
                index: from_index,
            })?;
        if *recorded != from_pt {
            return Err(CallError::FromPlaintextMismatch { index: from_index });
        }

        // apply 到 player 的 hand
        let meld = MeldRecord {
            call_type: self.call_type,
            deck_indices: self.deck_indices.clone(),
            plaintexts: self.plaintexts.clone(),
            from_player: self.from_player,
            from_position_in_meld: self.from_position_in_meld,
        };
        table.hand_mut(self.player).record_meld(meld)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::protocol_discard::DiscardAnnouncement;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    /// 玩家 1 弃一张 → 玩家 0 碰这张.
    #[test]
    fn pon_call_after_discard() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        // 玩家 0 摸 indices 0, 1
        let p0_pt = Curve::rand(rng);
        table.hand_mut(0).record_draw(0, Some(p0_pt)).unwrap();
        table.hand_mut(0).record_draw(1, Some(p0_pt)).unwrap();
        // 玩家 1 摸 + 弃 index 50, plaintext = p0_pt (碰需要相同 plaintext)
        table.hand_mut(1).record_draw(50, Some(p0_pt)).unwrap();
        DiscardAnnouncement {
            player: 1,
            deck_index: 50,
            plaintext: p0_pt,
        }
        .apply(&mut table)
        .unwrap();
        // 玩家 0 碰: deck_indices [0, 1, 50], 50 是 from_player 1 的弃牌
        let call = CallAnnouncement {
            player: 0,
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![p0_pt; 3],
            from_player: 1,
            from_position_in_meld: 2,
        };
        call.apply(&mut table).unwrap();
        // 玩家 0 的 0/1 现在不在 hand 里
        assert!(!table.hand(0).has_in_hand(0));
        assert!(!table.hand(0).has_in_hand(1));
        assert_eq!(table.hand(0).melds().len(), 1);
    }

    #[test]
    fn self_call_rejected() {
        let mut table = Table::new(4, 136);
        let call = CallAnnouncement {
            player: 1,
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 2],
            plaintexts: vec![Curve::default(); 3],
            from_player: 1, // self
            from_position_in_meld: 2,
        };
        assert!(matches!(
            call.apply(&mut table),
            Err(CallError::SelfCall { player: 1 })
        ));
    }

    #[test]
    fn from_discard_not_found_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        table.hand_mut(0).record_draw(0, Some(pt)).unwrap();
        table.hand_mut(0).record_draw(1, Some(pt)).unwrap();
        // from_player 1 没弃过 50
        let call = CallAnnouncement {
            player: 0,
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![pt; 3],
            from_player: 1,
            from_position_in_meld: 2,
        };
        assert!(matches!(
            call.apply(&mut table),
            Err(CallError::FromDiscardNotFound { from: 1, index: 50 })
        ));
    }

    #[test]
    fn from_plaintext_mismatch_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        let other_pt = Curve::rand(rng);
        table.hand_mut(0).record_draw(0, Some(pt)).unwrap();
        table.hand_mut(0).record_draw(1, Some(pt)).unwrap();
        table.hand_mut(1).record_draw(50, Some(pt)).unwrap();
        DiscardAnnouncement {
            player: 1,
            deck_index: 50,
            plaintext: pt,
        }
        .apply(&mut table)
        .unwrap();
        // call 时声称 from plaintext 是 other_pt (跟实际 discarded 不符)
        let call = CallAnnouncement {
            player: 0,
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![pt, pt, other_pt], // index 2 (= from_position) 错
            from_player: 1,
            from_position_in_meld: 2,
        };
        assert!(matches!(
            call.apply(&mut table),
            Err(CallError::FromPlaintextMismatch { index: 50 })
        ));
    }

    #[test]
    fn kan_size_4_required() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pt = Curve::rand(rng);
        for i in 0..3 {
            table.hand_mut(0).record_draw(i, Some(pt)).unwrap();
        }
        table.hand_mut(1).record_draw(50, Some(pt)).unwrap();
        DiscardAnnouncement {
            player: 1,
            deck_index: 50,
            plaintext: pt,
        }
        .apply(&mut table)
        .unwrap();
        // 用 Kan 但只 3 个 indices
        let call = CallAnnouncement {
            player: 0,
            call_type: CallType::Kan,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![pt; 3],
            from_player: 1,
            from_position_in_meld: 2,
        };
        // CallType::Kan 期望 4 个 → WrongCallSize.
        assert!(matches!(
            call.apply(&mut table),
            Err(CallError::Hand(HandStateError::WrongCallSize { .. }))
        ));
    }

    #[test]
    fn chi_3_consecutive_size_3() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        // 玩家 0 摸 0, 1 (mock 连续 tile)
        let pt0 = Curve::rand(rng);
        let pt1 = Curve::rand(rng);
        let pt2 = Curve::rand(rng);
        table.hand_mut(0).record_draw(0, Some(pt0)).unwrap();
        table.hand_mut(0).record_draw(1, Some(pt1)).unwrap();
        table.hand_mut(1).record_draw(50, Some(pt2)).unwrap();
        DiscardAnnouncement {
            player: 1,
            deck_index: 50,
            plaintext: pt2,
        }
        .apply(&mut table)
        .unwrap();
        let call = CallAnnouncement {
            player: 0,
            call_type: CallType::Chi,
            deck_indices: vec![0, 1, 50],
            plaintexts: vec![pt0, pt1, pt2],
            from_player: 1,
            from_position_in_meld: 2,
        };
        call.apply(&mut table).unwrap();
        assert_eq!(table.hand(0).melds().len(), 1);
    }
}
