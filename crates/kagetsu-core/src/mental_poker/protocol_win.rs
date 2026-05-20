//! 协议 7: 自摸 / 荣和 announcement (M5.A.4).
//!
//! 和牌玩家广播完整手牌明文 + 所有 deck_indices + 已揭示 dora indicator. 其他
//! 3 人验证:
//! 1. winning_tile_index 自摸时是协议 2 玩家自己摸到的, 荣和时是某玩家协议 4
//!    最近一次弃牌的 index (caller 校验).
//! 2. hand_indices 全部在 player 的 drawn ∧ ¬discarded 集合, 或在 player 的
//!    melds / concealed_kans 中.
//! 3. plaintexts 跟 indices 一一对应, 且未冲突 (没用同一 index 两次).
//! 4. dora_indicator_plaintexts 跟之前协议 3 揭示的一致 (caller 校验, 此模块
//!    只接受 caller 给的 trusted indicator list).
//!
//! 牌型 / yaku / 算分留 application 层 (yaku.rs).

use std::collections::HashSet;
use thiserror::Error;

use super::Curve;
use super::protocol_state::Table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinType {
    /// 自摸: winning_tile = 玩家自己刚摸的.
    Tsumo,
    /// 荣和: winning_tile = 来自 from_player 刚弃的.
    Ron { from_player: usize },
}

/// 和牌广播包.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinAnnouncement {
    pub player: usize,
    pub win_type: WinType,
    /// 完整手牌的 deck_indices (含 melds 和 concealed_kans 已经记账过的).
    /// 包含 winning_tile_index. 顺序: caller 决定.
    pub hand_indices: Vec<usize>,
    /// 跟 hand_indices 一一对应的 plaintexts.
    pub hand_plaintexts: Vec<Curve>,
    /// 和的那张牌的 deck_index (在 hand_indices 内).
    pub winning_tile_index: usize,
    /// 协议 3 已揭示的 dora indicator plaintexts (开局 dora + 杠 dora).
    pub dora_plaintexts: Vec<Curve>,
    /// 立直时里 dora plaintexts (协议 3 揭示, 仅立直玩家).
    pub uradoor_plaintexts: Option<Vec<Curve>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WinError {
    #[error("玩家 {player} 不在 table (n_players={n})")]
    UnknownPlayer { player: usize, n: usize },
    #[error("hand_indices 长 {got} 跟 hand_plaintexts 长 {expected} 不一致")]
    SizeMismatch { got: usize, expected: usize },
    #[error("winning_tile_index {index} 不在 hand_indices")]
    WinningNotInHand { index: usize },
    #[error("hand_indices 含重复 index {index}")]
    DuplicateIndex { index: usize },
    #[error("玩家未持有 deck_index={index} (不在 drawn / melded / concealed_kanned)")]
    NotOwned { index: usize },
    #[error(
        "Ron 声明从玩家 {from} 弃牌 index={index} 和, 但该 index 不在 from_player discarded 集合"
    )]
    RonFromDiscardNotFound { from: usize, index: usize },
    #[error("Tsumo: winning_tile_index {index} 不在自己 drawn 集合 (玩家未摸过)")]
    TsumoNotDrawn { index: usize },
    #[error("Ron: from_player {from} 跟 winner {player} 相同")]
    SelfRon { player: usize, from: usize },
    #[error("Ron: from_player {from} 不在 table (n_players={n})")]
    UnknownFromPlayer { from: usize, n: usize },
}

impl WinAnnouncement {
    /// 验证 announcement 跟当前 Table state 一致. 不修改 table (和牌是终止
    /// 事件, 不再 transition). 返回 Ok 则 caller 可以走 application 层 yaku 算分.
    pub fn validate(&self, table: &Table) -> Result<(), WinError> {
        let n = table.n_players;
        if self.player >= n {
            return Err(WinError::UnknownPlayer {
                player: self.player,
                n,
            });
        }
        if self.hand_indices.len() != self.hand_plaintexts.len() {
            return Err(WinError::SizeMismatch {
                got: self.hand_indices.len(),
                expected: self.hand_plaintexts.len(),
            });
        }
        // 检查重复 index
        let mut seen: HashSet<usize> = HashSet::new();
        for idx in &self.hand_indices {
            if !seen.insert(*idx) {
                return Err(WinError::DuplicateIndex { index: *idx });
            }
        }
        // winning_tile_index 必须在 hand_indices 内
        if !seen.contains(&self.winning_tile_index) {
            return Err(WinError::WinningNotInHand {
                index: self.winning_tile_index,
            });
        }

        let hand = table.hand(self.player);

        // hand_indices 中的每个 index 必须 owned (drawn ∨ melded ∨ concealed_kan).
        // 但**已弃牌不算**, 因为弃出去的不是手牌. 只有 winning_tile 在 Ron 时
        // 是来自 from_player 的弃牌 — 对 winner 来说不是 drawn, 但 announcement
        // 里它仍属于和牌部分.
        for idx in &self.hand_indices {
            if *idx == self.winning_tile_index {
                continue; // 单独处理
            }
            let drawn_in_hand = hand.drawn_indices().any(|i| i == idx);
            let in_meld = hand.melds().iter().any(|m| m.deck_indices.contains(idx));
            let in_kan = hand
                .concealed_kans()
                .iter()
                .any(|k| k.deck_indices.contains(idx));
            if !(drawn_in_hand || in_meld || in_kan) {
                return Err(WinError::NotOwned { index: *idx });
            }
        }

        // winning_tile_index 验证.
        match self.win_type {
            WinType::Tsumo => {
                // 自己 drawn 集合内.
                if !hand.drawn_indices().any(|i| *i == self.winning_tile_index) {
                    return Err(WinError::TsumoNotDrawn {
                        index: self.winning_tile_index,
                    });
                }
            }
            WinType::Ron { from_player } => {
                if from_player >= n {
                    return Err(WinError::UnknownFromPlayer {
                        from: from_player,
                        n,
                    });
                }
                if from_player == self.player {
                    return Err(WinError::SelfRon {
                        player: self.player,
                        from: from_player,
                    });
                }
                if table
                    .hand(from_player)
                    .discarded_plaintext(self.winning_tile_index)
                    .is_none()
                {
                    return Err(WinError::RonFromDiscardNotFound {
                        from: from_player,
                        index: self.winning_tile_index,
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::protocol_discard::DiscardAnnouncement;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    /// 自摸: hand 全在 drawn, winning_tile 也在 drawn.
    #[test]
    fn tsumo_validates() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pts: Vec<Curve> = (0..14).map(|_| Curve::rand(rng)).collect();
        for (i, pt) in pts.iter().enumerate() {
            table.hand_mut(0).record_draw(i, Some(*pt)).unwrap();
        }
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Tsumo,
            hand_indices: (0..14).collect(),
            hand_plaintexts: pts,
            winning_tile_index: 13,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        win.validate(&table).unwrap();
    }

    /// Tsumo 但 winning_tile 没摸过 → TsumoNotDrawn.
    #[test]
    fn tsumo_winning_not_drawn_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        // 摸 0..13 共 13 张, 不含 99
        for i in 0..13 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Tsumo,
            hand_indices: (0..13).chain(std::iter::once(99)).collect(),
            hand_plaintexts: vec![Curve::rand(rng); 14],
            winning_tile_index: 99, // 没摸过 (NotOwned 先 catch)
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        // winning_tile_index 没在 owned 集合里 (drawn / meld / kan) → TsumoNotDrawn
        // (winning 走单独路径不走 NotOwned 分支)
        assert!(matches!(
            win.validate(&table),
            Err(WinError::TsumoNotDrawn { index: 99 })
        ));
    }

    /// Ron: winning_tile 是 from_player 弃牌, 不在自己 drawn.
    #[test]
    fn ron_validates() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        let pts: Vec<Curve> = (0..13).map(|_| Curve::rand(rng)).collect();
        for (i, pt) in pts.iter().enumerate() {
            table.hand_mut(0).record_draw(i, Some(*pt)).unwrap();
        }
        // 玩家 1 摸 + 弃 99 (winning tile)
        let winning_pt = Curve::rand(rng);
        table.hand_mut(1).record_draw(99, Some(winning_pt)).unwrap();
        DiscardAnnouncement {
            player: 1,
            deck_index: 99,
            plaintext: winning_pt,
        }
        .apply(&mut table)
        .unwrap();

        let mut hand_indices: Vec<usize> = (0..13).collect();
        hand_indices.push(99);
        let mut hand_plaintexts = pts;
        hand_plaintexts.push(winning_pt);
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Ron { from_player: 1 },
            hand_indices,
            hand_plaintexts,
            winning_tile_index: 99,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        win.validate(&table).unwrap();
    }

    /// Ron: from_player 没弃过 winning_tile → fail.
    #[test]
    fn ron_no_discard_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        for i in 0..13 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        let mut hand_indices: Vec<usize> = (0..13).collect();
        hand_indices.push(99);
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Ron { from_player: 1 },
            hand_indices,
            hand_plaintexts: vec![Curve::rand(rng); 14],
            winning_tile_index: 99, // 玩家 1 没弃过
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        assert!(matches!(
            win.validate(&table),
            Err(WinError::RonFromDiscardNotFound { from: 1, index: 99 })
        ));
    }

    /// 自和 (Self-Ron) 拒绝.
    #[test]
    fn self_ron_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        for i in 0..13 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        let mut hand_indices: Vec<usize> = (0..13).collect();
        hand_indices.push(99);
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Ron { from_player: 0 }, // self
            hand_indices,
            hand_plaintexts: vec![Curve::rand(rng); 14],
            winning_tile_index: 99,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        assert!(matches!(
            win.validate(&table),
            Err(WinError::SelfRon { .. })
        ));
    }

    /// hand_indices 重复 → fail.
    #[test]
    fn duplicate_index_rejected() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        for i in 0..13 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Tsumo,
            hand_indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 12], // 12 重复
            hand_plaintexts: vec![Curve::rand(rng); 14],
            winning_tile_index: 12,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        assert!(matches!(
            win.validate(&table),
            Err(WinError::DuplicateIndex { index: 12 })
        ));
    }

    /// hand_indices 中含 melded 的 indices: 走 melds / concealed_kans 路径 valid.
    /// 副露 1 副 (3 张) + 暗手 11 张 = 14 张和牌型.
    #[test]
    fn melded_indices_in_hand_valid() {
        use crate::mental_poker::protocol_state::{CallType, MeldRecord};
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        // 玩家 0 摸 0/1 (将被 meld 走)
        table
            .hand_mut(0)
            .record_draw(0, Some(Curve::rand(rng)))
            .unwrap();
        table
            .hand_mut(0)
            .record_draw(1, Some(Curve::rand(rng)))
            .unwrap();
        // mock 一个 meld 用 0/1/99 (99 from player 1 弃)
        let meld = MeldRecord {
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 99],
            plaintexts: vec![Curve::rand(rng); 3],
            from_player: 1,
            from_position_in_meld: 2,
        };
        table.hand_mut(0).record_meld(meld).unwrap();
        // 玩家 0 此后摸 11 张暗手 (2..13)
        for i in 2..13 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        // 和牌 14 张 = melded 3 (0, 1, 99) + drawn 11 (2..13) = 14
        let mut hand_indices: Vec<usize> = vec![0, 1, 99];
        hand_indices.extend(2..13);
        assert_eq!(hand_indices.len(), 14);
        let win = WinAnnouncement {
            player: 0,
            win_type: WinType::Tsumo,
            hand_indices,
            hand_plaintexts: vec![Curve::rand(rng); 14],
            winning_tile_index: 12, // 在 drawn (2..13)
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        win.validate(&table).unwrap();
    }
}
