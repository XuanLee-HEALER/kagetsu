//! 协议层手牌状态机 (M5.A.0).
//!
//! 协议 4-7 共用基础: 跟踪每个玩家的"密文手牌账本", 包含:
//! - **drawn**: 协议 2 摸到的 deck_index → plaintext (仅摸牌方知 plaintext, 其他人镜像里 None)
//! - **discarded**: 协议 4 已弃位置 (此时 plaintext 全公开)
//! - **melds**: 协议 5 副露 (Chi/Pon/Kan 公开手牌)
//! - **concealed_kans**: 协议 6 暗杠 (deck_indices 公开 + 1 监督方知 plaintext)
//!
//! ## 用法
//! 在零信任模式下, 每个玩家本地维护 N 个 [`HandState`] 镜像 (自己 + 其他 N-1 人).
//! 协议 2/4/5/6 的 announcement 来到时各方独立 transition 自己的镜像.
//! 共识 = 所有人的镜像应保持一致 (非 byzantine 时).
//!
//! ## 验证范围
//! 本模块仅做"密文 ↔ 明文 ↔ 索引"密码学层一致性 + 集合 ownership 验证. **牌型
//! 合法性** (3 张连续 / 相同等) 留给 application 层 [`crate::engine::state`].

use std::collections::HashMap;
use thiserror::Error;

use super::Curve;

/// 一组牌副露 (协议 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallType {
    /// 吃: 3 张连续 (1 张来自 from_player 弃牌, 2 张自己手牌)
    Chi,
    /// 碰: 3 张相同
    Pon,
    /// 大明杠: 4 张相同 (1 张弃牌, 3 张手牌). 加杠 (3 已碰 + 1 自摸) 走另一个 enum.
    Kan,
}

impl CallType {
    pub fn expected_indices_count(self) -> usize {
        match self {
            CallType::Chi | CallType::Pon => 3,
            CallType::Kan => 4,
        }
    }
}

/// 一次副露记录 (协议 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeldRecord {
    pub call_type: CallType,
    /// 副露的 deck_indices (含来自 from_player 的弃牌索引). 顺序: 自己手牌位置在前, from_player 弃牌位置在后.
    pub deck_indices: Vec<usize>,
    /// 对应明文.
    pub plaintexts: Vec<Curve>,
    /// 鸣谁的弃牌 (Chi/Pon/Kan 都有).
    pub from_player: usize,
    /// from_player 弃牌位置在 deck_indices 中的索引 (e.g. 末尾).
    pub from_position_in_meld: usize,
}

/// 一次暗杠记录 (协议 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcealedKanRecord {
    /// 4 个 deck_indices.
    pub deck_indices: [usize; 4],
    /// 选定的被动监督玩家.
    pub monitor_player: usize,
}

/// 单玩家手牌状态.
#[derive(Debug, Clone, Default)]
pub struct HandState {
    /// 协议 2 摸过的位置. value = Some(plaintext) 仅自己 HandState, 其他人镜像 None.
    drawn: HashMap<usize, Option<Curve>>,
    /// 协议 4 已弃位置 (plaintext 全员可见).
    discarded: HashMap<usize, Curve>,
    /// 协议 5 副露.
    melds: Vec<MeldRecord>,
    /// 协议 6 暗杠.
    concealed_kans: Vec<ConcealedKanRecord>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HandStateError {
    #[error("位置 {index} 已经在 drawn 集合内 (重复摸牌)")]
    AlreadyDrawn { index: usize },
    #[error("位置 {index} 不在玩家手牌内 (从未摸过)")]
    NotDrawn { index: usize },
    #[error("位置 {index} 已经弃过 / 鸣过, 不在手牌内")]
    NotInHand { index: usize },
    #[error("位置 {index} 已经在 discarded 集合内 (重复弃牌)")]
    AlreadyDiscarded { index: usize },
    #[error("plaintext 长度 {got} 跟 indices 长度 {expected} 不一致")]
    SizeMismatch { got: usize, expected: usize },
    #[error("CallType {expected:?} 期望 {expected_count} 个 indices, 实际 {got}")]
    WrongCallSize {
        expected: CallType,
        expected_count: usize,
        got: usize,
    },
    #[error("from_position_in_meld {pos} 越界 (deck_indices 长 {len})")]
    FromPositionOutOfRange { pos: usize, len: usize },
    #[error("声明从玩家 {from_player} 弃牌 index={from_index} 鸣, 但该 index 不在 from_player discarded 集合")]
    FromDiscardNotFound {
        from_player: usize,
        from_index: usize,
    },
}

impl HandState {
    pub fn new() -> Self {
        Self::default()
    }

    // --- 查询接口 ---

    /// 该 deck_index 是否在玩家手牌内 (drawn ∧ ¬discarded ∧ ¬melded ∧ ¬concealed_kanned).
    pub fn has_in_hand(&self, deck_index: usize) -> bool {
        self.drawn.contains_key(&deck_index)
            && !self.discarded.contains_key(&deck_index)
            && !self.is_melded(deck_index)
            && !self.is_concealed_kanned(deck_index)
    }

    pub fn has_all_in_hand(&self, indices: &[usize]) -> bool {
        indices.iter().all(|i| self.has_in_hand(*i))
    }

    pub fn drawn_indices(&self) -> impl Iterator<Item = &usize> {
        self.drawn.keys()
    }

    pub fn discarded_indices(&self) -> impl Iterator<Item = &usize> {
        self.discarded.keys()
    }

    pub fn melds(&self) -> &[MeldRecord] {
        &self.melds
    }

    pub fn concealed_kans(&self) -> &[ConcealedKanRecord] {
        &self.concealed_kans
    }

    pub fn discarded_plaintext(&self, deck_index: usize) -> Option<&Curve> {
        self.discarded.get(&deck_index)
    }

    /// 玩家自己 view 下, 该 deck_index 摸到的 plaintext.
    pub fn drawn_plaintext(&self, deck_index: usize) -> Option<&Curve> {
        self.drawn.get(&deck_index).and_then(|o| o.as_ref())
    }

    fn is_melded(&self, deck_index: usize) -> bool {
        self.melds
            .iter()
            .any(|m| m.deck_indices.contains(&deck_index))
    }

    fn is_concealed_kanned(&self, deck_index: usize) -> bool {
        self.concealed_kans
            .iter()
            .any(|k| k.deck_indices.contains(&deck_index))
    }

    // --- transition: 协议 2 摸牌 ---

    /// 记录摸牌. plaintext = Some(...) 仅在玩家自己镜像, None 在其他人镜像.
    pub fn record_draw(
        &mut self,
        deck_index: usize,
        plaintext: Option<Curve>,
    ) -> Result<(), HandStateError> {
        if self.drawn.contains_key(&deck_index) {
            return Err(HandStateError::AlreadyDrawn { index: deck_index });
        }
        self.drawn.insert(deck_index, plaintext);
        Ok(())
    }

    // --- transition: 协议 4 弃牌 ---

    /// 记录玩家自己的弃牌. caller 应保证 deck_index 在 drawn ∧ 不在 discarded/melded/concealed_kan.
    pub fn record_discard(
        &mut self,
        deck_index: usize,
        plaintext: Curve,
    ) -> Result<(), HandStateError> {
        if !self.drawn.contains_key(&deck_index) {
            return Err(HandStateError::NotDrawn { index: deck_index });
        }
        if self.discarded.contains_key(&deck_index) {
            return Err(HandStateError::AlreadyDiscarded { index: deck_index });
        }
        if self.is_melded(deck_index) || self.is_concealed_kanned(deck_index) {
            return Err(HandStateError::NotInHand { index: deck_index });
        }
        self.discarded.insert(deck_index, plaintext);
        Ok(())
    }

    // --- transition: 协议 5 鸣牌 ---

    /// 记录副露. 验证: 自己手牌部分在 drawn 且未弃未鸣未暗杠, from_position 那位的 from_player
    /// 必须有一个 record_discard 落在 deck_indices[from_position] (caller 校验).
    /// 本函数仅检查内部一致性 + 自己手牌 ownership.
    pub fn record_meld(&mut self, meld: MeldRecord) -> Result<(), HandStateError> {
        if meld.deck_indices.len() != meld.plaintexts.len() {
            return Err(HandStateError::SizeMismatch {
                got: meld.plaintexts.len(),
                expected: meld.deck_indices.len(),
            });
        }
        let expected = meld.call_type.expected_indices_count();
        if meld.deck_indices.len() != expected {
            return Err(HandStateError::WrongCallSize {
                expected: meld.call_type,
                expected_count: expected,
                got: meld.deck_indices.len(),
            });
        }
        if meld.from_position_in_meld >= meld.deck_indices.len() {
            return Err(HandStateError::FromPositionOutOfRange {
                pos: meld.from_position_in_meld,
                len: meld.deck_indices.len(),
            });
        }
        // 验证除 from_position 外的 indices 都在自己手牌内.
        for (i, idx) in meld.deck_indices.iter().enumerate() {
            if i == meld.from_position_in_meld {
                continue; // 来自 from_player 的弃牌, caller 单独验
            }
            if !self.has_in_hand(*idx) {
                return Err(HandStateError::NotInHand { index: *idx });
            }
        }
        self.melds.push(meld);
        Ok(())
    }

    // --- transition: 协议 6 暗杠 ---

    /// 记录暗杠. 验证: 4 个 deck_indices 都在自己手牌内.
    pub fn record_concealed_kan(
        &mut self,
        kan: ConcealedKanRecord,
    ) -> Result<(), HandStateError> {
        for idx in &kan.deck_indices {
            if !self.has_in_hand(*idx) {
                return Err(HandStateError::NotInHand { index: *idx });
            }
        }
        self.concealed_kans.push(kan);
        Ok(())
    }
}

/// 全桌账本: N 个玩家的 HandState 集合 + 牌山大小 sanity.
///
/// 玩家本地维护一份 [`Table`] 镜像作 ground truth, 协议事件 transition 同步.
#[derive(Debug, Clone)]
pub struct Table {
    pub n_players: usize,
    pub wall_size: usize,
    pub hands: Vec<HandState>,
}

impl Table {
    pub fn new(n_players: usize, wall_size: usize) -> Self {
        Self {
            n_players,
            wall_size,
            hands: vec![HandState::new(); n_players],
        }
    }

    pub fn hand(&self, player: usize) -> &HandState {
        &self.hands[player]
    }

    pub fn hand_mut(&mut self, player: usize) -> &mut HandState {
        &mut self.hands[player]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    #[test]
    fn record_draw_and_query() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        let pt = Curve::rand(rng);
        h.record_draw(5, Some(pt)).unwrap();
        assert!(h.has_in_hand(5));
        assert_eq!(h.drawn_plaintext(5), Some(&pt));
    }

    #[test]
    fn record_draw_duplicate_rejected() {
        let mut h = HandState::new();
        h.record_draw(5, None).unwrap();
        assert_eq!(
            h.record_draw(5, None),
            Err(HandStateError::AlreadyDrawn { index: 5 })
        );
    }

    #[test]
    fn discard_must_be_drawn_first() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        let pt = Curve::rand(rng);
        assert_eq!(
            h.record_discard(5, pt),
            Err(HandStateError::NotDrawn { index: 5 })
        );
    }

    #[test]
    fn discard_double_rejected() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        let pt = Curve::rand(rng);
        h.record_draw(5, Some(pt)).unwrap();
        h.record_discard(5, pt).unwrap();
        assert_eq!(
            h.record_discard(5, pt),
            Err(HandStateError::AlreadyDiscarded { index: 5 })
        );
    }

    #[test]
    fn discarded_not_in_hand() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        let pt = Curve::rand(rng);
        h.record_draw(5, Some(pt)).unwrap();
        h.record_discard(5, pt).unwrap();
        assert!(!h.has_in_hand(5));
    }

    #[test]
    fn meld_consumes_indices_from_hand() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        for i in 0..3 {
            h.record_draw(i, Some(Curve::rand(rng))).unwrap();
        }
        let pts: Vec<Curve> = (0..3).map(|_| Curve::rand(rng)).collect();
        let meld = MeldRecord {
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 99], // 99 是来自 from_player 弃牌
            plaintexts: pts,
            from_player: 2,
            from_position_in_meld: 2,
        };
        h.record_meld(meld).unwrap();
        // 0, 1 现在不在 hand 里
        assert!(!h.has_in_hand(0));
        assert!(!h.has_in_hand(1));
    }

    #[test]
    fn meld_size_mismatch_rejected() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        h.record_draw(0, Some(Curve::rand(rng))).unwrap();
        let meld = MeldRecord {
            call_type: CallType::Pon, // 期望 3
            deck_indices: vec![0, 1], // 只 2 个
            plaintexts: vec![Curve::rand(rng), Curve::rand(rng)],
            from_player: 1,
            from_position_in_meld: 1,
        };
        assert!(matches!(
            h.record_meld(meld),
            Err(HandStateError::WrongCallSize { .. })
        ));
    }

    #[test]
    fn meld_indices_plaintexts_length_mismatch() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        h.record_draw(0, Some(Curve::rand(rng))).unwrap();
        h.record_draw(1, Some(Curve::rand(rng))).unwrap();
        let meld = MeldRecord {
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 99],
            plaintexts: vec![Curve::rand(rng), Curve::rand(rng)], // 2 个 vs 3 个
            from_player: 2,
            from_position_in_meld: 2,
        };
        assert!(matches!(
            h.record_meld(meld),
            Err(HandStateError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn meld_index_not_drawn_rejected() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        let meld = MeldRecord {
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 99], // 0, 1 从未 record_draw
            plaintexts: vec![Curve::rand(rng); 3],
            from_player: 2,
            from_position_in_meld: 2,
        };
        assert!(matches!(
            h.record_meld(meld),
            Err(HandStateError::NotInHand { .. })
        ));
    }

    #[test]
    fn concealed_kan_records_4_indices() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        for i in 0..4 {
            h.record_draw(i, Some(Curve::rand(rng))).unwrap();
        }
        let kan = ConcealedKanRecord {
            deck_indices: [0, 1, 2, 3],
            monitor_player: 2,
        };
        h.record_concealed_kan(kan).unwrap();
        // 4 张全部从 hand 移出
        for i in 0..4 {
            assert!(!h.has_in_hand(i));
        }
    }

    #[test]
    fn concealed_kan_index_not_drawn_rejected() {
        let mut h = HandState::new();
        let kan = ConcealedKanRecord {
            deck_indices: [0, 1, 2, 3],
            monitor_player: 1,
        };
        assert!(matches!(
            h.record_concealed_kan(kan),
            Err(HandStateError::NotInHand { .. })
        ));
    }

    #[test]
    fn meld_then_discard_same_index_rejected() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        for i in 0..3 {
            h.record_draw(i, Some(Curve::rand(rng))).unwrap();
        }
        let meld = MeldRecord {
            call_type: CallType::Pon,
            deck_indices: vec![0, 1, 99],
            plaintexts: vec![Curve::rand(rng); 3],
            from_player: 2,
            from_position_in_meld: 2,
        };
        h.record_meld(meld).unwrap();
        // 现在尝试 discard index 0 — 应失败 (已 melded)
        let pt = Curve::rand(rng);
        assert!(matches!(
            h.record_discard(0, pt),
            Err(HandStateError::NotInHand { index: 0 })
        ));
    }

    #[test]
    fn table_init_n_hands() {
        let table = Table::new(4, 136);
        assert_eq!(table.hands.len(), 4);
        assert_eq!(table.wall_size, 136);
    }

    #[test]
    fn has_all_in_hand_partial() {
        let rng = &mut test_rng();
        let mut h = HandState::new();
        h.record_draw(0, Some(Curve::rand(rng))).unwrap();
        h.record_draw(2, Some(Curve::rand(rng))).unwrap();
        assert!(h.has_all_in_hand(&[0]));
        assert!(h.has_all_in_hand(&[0, 2]));
        assert!(!h.has_all_in_hand(&[0, 1])); // 1 没摸过
    }
}
