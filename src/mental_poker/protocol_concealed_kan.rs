//! 协议 6 选项 C: 暗杠揭示给 1 监督方 (M5.A.3).
//!
//! ## scratch.md 选项
//! - A: 暗杠强制揭示 — 违反规则
//! - B: ZK-SNARK 证明 "已摸密文集合中存在 4 张相同 tile_index 的牌, 不揭示具体哪 4 张"
//!      工程量极大 (PLONK / Halo2 框架), 留给 M6.
//! - **C (本模块, MVP)**: 暗杠时仅揭示给 1 个被动监督玩家. 监督方验证 4 个
//!   tile_index 相同, 否则上报作弊. 不密码学严格 (监督方可串通), 但工程简单.
//!
//! ## 协议
//! 1. 玩家 X 决定暗杠, 选 monitor_player m (≠ X)
//! 2. X 公开广播 (deck_indices: [usize; 4], monitor=m): 让所有人知道有暗杠存在
//! 3. X 私发给 m: plaintexts: [Curve; 4]
//! 4. m 验证: indices 都在 X 的 drawn ∧ 未弃未鸣 (所有人都能验); plaintexts 4 张
//!    tile_index 相同 (m 自己验, application 层 Tile mapping 比较)
//! 5. m 异常时上报作弊事件
//!
//! 本模块只做协议层 transition + 自己手牌 ownership 验证. 4 张相同 tile_index
//! 的判断留 application 层.

use thiserror::Error;

use super::Curve;
use super::protocol_state::{ConcealedKanRecord, HandStateError, Table};

/// 公开部分: 让所有人知道有暗杠.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcealedKanAnnouncement {
    pub player: usize,
    pub deck_indices: [usize; 4],
    pub monitor_player: usize,
}

/// 私发给 monitor 的部分.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcealedKanReveal {
    pub plaintexts: [Curve; 4],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConcealedKanError {
    #[error("玩家 {player} 不在 table (n_players={n})")]
    UnknownPlayer { player: usize, n: usize },
    #[error("monitor {monitor} 不在 table (n_players={n})")]
    UnknownMonitor { monitor: usize, n: usize },
    #[error("monitor 必须 != player (player={player})")]
    SelfMonitor { player: usize },
    #[error("hand state 错误: {0}")]
    Hand(#[from] HandStateError),
}

impl ConcealedKanAnnouncement {
    /// 公开部分 apply 到 Table — 把 4 张 indices 移出 hand.
    pub fn apply(&self, table: &mut Table) -> Result<(), ConcealedKanError> {
        let n = table.n_players;
        if self.player >= n {
            return Err(ConcealedKanError::UnknownPlayer {
                player: self.player,
                n,
            });
        }
        if self.monitor_player >= n {
            return Err(ConcealedKanError::UnknownMonitor {
                monitor: self.monitor_player,
                n,
            });
        }
        if self.monitor_player == self.player {
            return Err(ConcealedKanError::SelfMonitor {
                player: self.player,
            });
        }
        let kan = ConcealedKanRecord {
            deck_indices: self.deck_indices,
            monitor_player: self.monitor_player,
        };
        table.hand_mut(self.player).record_concealed_kan(kan)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn concealed_kan_apply_moves_indices_out_of_hand() {
        let rng = &mut test_rng();
        let mut table = Table::new(4, 136);
        for i in 0..4 {
            table
                .hand_mut(0)
                .record_draw(i, Some(Curve::rand(rng)))
                .unwrap();
        }
        let ann = ConcealedKanAnnouncement {
            player: 0,
            deck_indices: [0, 1, 2, 3],
            monitor_player: 2,
        };
        ann.apply(&mut table).unwrap();
        for i in 0..4 {
            assert!(!table.hand(0).has_in_hand(i));
        }
        assert_eq!(table.hand(0).concealed_kans().len(), 1);
        assert_eq!(table.hand(0).concealed_kans()[0].monitor_player, 2);
    }

    #[test]
    fn self_monitor_rejected() {
        let mut table = Table::new(4, 136);
        let ann = ConcealedKanAnnouncement {
            player: 0,
            deck_indices: [0, 1, 2, 3],
            monitor_player: 0,
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(ConcealedKanError::SelfMonitor { player: 0 })
        ));
    }

    #[test]
    fn unknown_monitor_rejected() {
        let mut table = Table::new(4, 136);
        let ann = ConcealedKanAnnouncement {
            player: 0,
            deck_indices: [0, 1, 2, 3],
            monitor_player: 99,
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(ConcealedKanError::UnknownMonitor { monitor: 99, .. })
        ));
    }

    #[test]
    fn concealed_kan_index_not_drawn_rejected() {
        let mut table = Table::new(4, 136);
        let ann = ConcealedKanAnnouncement {
            player: 0,
            deck_indices: [0, 1, 2, 3], // 没摸过
            monitor_player: 2,
        };
        assert!(matches!(
            ann.apply(&mut table),
            Err(ConcealedKanError::Hand(HandStateError::NotInHand { .. }))
        ));
    }
}
