//! 局 (Round) 层 — type-state 模式. 见 docs/design/abstract-model.md §Layer 2.
//!
//! 4 层架构:
//! - L1 数据层: AtomicOp (在 op.rs 定义)
//! - L2 类型化 state: AwaitDiscardState / AwaitRiichiDiscardState / ... 在本文件
//! - L3 类型化 op: 由 typed_op! 宏在本文件生成 (AwaitDiscardOp 等)
//! - L4 桥接: 各 typed state 的 try_op 方法 (在本文件 impl)
//!
//! RoundState enum 包装所有 typed state, 公开给外部用.
//!
//! ## 阶段 5a 状态: 类型骨架
//!
//! 本提交只定义 struct/enum 字段 + From 占位. try_op (5b) / typed apply (5c) /
//! 公开 round_apply 等 entry (5d) 待续.

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::Tile;
use crate::engine::rules::GameRules;
use crate::engine::state::{PlayerState, RoundResult, RoundWind};
use crate::engine::wall::Wall;
use crate::typed_op;
use serde::{Deserialize, Serialize};

/// 各 typed state 共享的局内字段. 抽出避免每个 state 重复.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonRound {
    /// 整庄规则 (从 MatchState 注入, 局内不变).
    pub rules: GameRules,
    /// 场风 (从 MatchState).
    pub round_wind: RoundWind,
    /// 局序号 (从 MatchState).
    pub kyoku: u8,
    /// 本场数 (从 MatchState).
    pub honba: u8,
    /// 立直棒池 (本局开局时 from MatchState, 局内有人立直会 +1).
    pub riichi_sticks_pool: u32,
    /// 庄家 (从 MatchState).
    pub dealer: Seat,
    /// 4 家完整 state (含 hand / river / melds / score / riichi flags / last_drawn).
    pub players: [PlayerState; 4],
    /// 牌山 (含活/死/dora_revealed).
    pub wall: Wall,
    /// 第一巡是否未被打断 (用于天和/地和等极端役).
    pub first_go_around: bool,
}

/// 等当前家做出 AwaitDiscard 阶段的某个决策 (切牌 / 立直宣告 / 自摸 / 暗杠 / 加杠).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitDiscardState {
    pub common: CommonRound,
    /// 当前家.
    pub turn: Seat,
    /// 刚摸到的那张. 类型保证 Some (由 phase 进入条件保证).
    pub last_drawn: Tile,
}

/// RiichiDeclare 已执行, 必须切牌. 唯一合法下一 op = Discard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitRiichiDiscardState {
    pub common: CommonRound,
    pub turn: Seat,
    pub last_drawn: Tile,
}

/// 杠 (明杠 / 暗杠 / 加杠) 刚执行, 必须摸岭上. 唯一合法下一 op = RinshanDraw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitRinshanDrawState {
    pub common: CommonRound,
    pub turn: Seat,
}

/// 当前家已切牌, 等其它玩家是否鸣 (Pon / Chi / Minkan / Ron) 或 Pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitCallsState {
    pub common: CommonRound,
    /// 切牌方 + 切的牌. 类型保证 Some.
    pub last_discard: (Seat, Tile),
}

/// 局已结束 (和 / 流局). 不接受任何 op. 持有 RoundResult 供 summarize_round 抽 RoundOutcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundEndState {
    pub common: CommonRound,
    pub result: RoundResult,
}

/// 公开 RoundState — 外部唯一看到的 round 类型. 内部按 phase 拆 typed state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundState {
    AwaitDiscard(AwaitDiscardState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    AwaitCalls(AwaitCallsState),
    RoundEnd(RoundEndState),
}

impl RoundState {
    pub fn common(&self) -> &CommonRound {
        match self {
            RoundState::AwaitDiscard(s) => &s.common,
            RoundState::AwaitRiichiDiscard(s) => &s.common,
            RoundState::AwaitRinshanDraw(s) => &s.common,
            RoundState::AwaitCalls(s) => &s.common,
            RoundState::RoundEnd(s) => &s.common,
        }
    }
}

// ============================================================
// Typed-op enum 由 typed_op! 宏生成
// ============================================================

typed_op! {
    AwaitDiscardOp from AtomicOp accepts {
        Discard { tile: crate::engine::domain::tile::Tile },
        RiichiDeclare,
        Tsumo,
        Ankan { kind: crate::engine::domain::tile::TileIndex },
        Shouminkan { kind: crate::engine::domain::tile::TileIndex },
    }
    for_phase AwaitDiscard;
}

typed_op! {
    AwaitRiichiDiscardOp from AtomicOp accepts {
        Discard { tile: crate::engine::domain::tile::Tile },
    }
    for_phase AwaitRiichiDiscard;
}

typed_op! {
    AwaitRinshanDrawOp from AtomicOp accepts {
        RinshanDraw,
    }
    for_phase AwaitRinshanDraw;
}

typed_op! {
    AwaitCallsOp from AtomicOp accepts {
        Pon { who: Seat, hand_tile_ids: [u16; 2] },
        Chi { who: Seat, hand_tile_ids: [u16; 2] },
        Minkan { who: Seat, hand_tile_ids: [u16; 3] },
        Ron { who: Seat },
        Pass,
    }
    for_phase AwaitCalls;
}

// ============================================================
// NextXxxState — 各 typed state 转移目标的 enum, 供 typed apply 返回
// 阶段 5c 实现具体转移逻辑, 这里先占位
// ============================================================

/// AwaitDiscard 转移可能去向: Calls (普通切) / RiichiDiscard (立直宣告) /
/// RinshanDraw (暗杠/加杠) / RoundEnd (自摸).
#[derive(Debug, Clone)]
pub enum NextAwaitDiscardState {
    AwaitCalls(AwaitCallsState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    RoundEnd(RoundEndState),
}

/// AwaitRiichiDiscard 转移可能去向: Calls (切牌后等鸣).
#[derive(Debug, Clone)]
pub enum NextAwaitRiichiDiscardState {
    AwaitCalls(AwaitCallsState),
}

/// AwaitRinshanDraw 转移可能去向: AwaitDiscard (摸完岭上) / RoundEnd (岭上摸到导致流局, 罕见).
#[derive(Debug, Clone)]
pub enum NextAwaitRinshanDrawState {
    AwaitDiscard(AwaitDiscardState),
    RoundEnd(RoundEndState),
}

/// AwaitCalls 转移可能去向: AwaitDiscard (Pon/Chi/Minkan 鸣完接切) /
/// RoundEnd (Ron) / 下家 Draw 后的 AwaitDiscard (Pass + 下家摸完).
#[derive(Debug, Clone)]
pub enum NextAwaitCallsState {
    AwaitDiscard(AwaitDiscardState),
    RoundEnd(RoundEndState),
}

// ============================================================
// From impls — 把各 NextXxxState 升回公开 RoundState
// ============================================================

impl From<NextAwaitDiscardState> for RoundState {
    fn from(n: NextAwaitDiscardState) -> Self {
        match n {
            NextAwaitDiscardState::AwaitCalls(s) => RoundState::AwaitCalls(s),
            NextAwaitDiscardState::AwaitRiichiDiscard(s) => RoundState::AwaitRiichiDiscard(s),
            NextAwaitDiscardState::AwaitRinshanDraw(s) => RoundState::AwaitRinshanDraw(s),
            NextAwaitDiscardState::RoundEnd(s) => RoundState::RoundEnd(s),
        }
    }
}

impl From<NextAwaitRiichiDiscardState> for RoundState {
    fn from(n: NextAwaitRiichiDiscardState) -> Self {
        match n {
            NextAwaitRiichiDiscardState::AwaitCalls(s) => RoundState::AwaitCalls(s),
        }
    }
}

impl From<NextAwaitRinshanDrawState> for RoundState {
    fn from(n: NextAwaitRinshanDrawState) -> Self {
        match n {
            NextAwaitRinshanDrawState::AwaitDiscard(s) => RoundState::AwaitDiscard(s),
            NextAwaitRinshanDrawState::RoundEnd(s) => RoundState::RoundEnd(s),
        }
    }
}

impl From<NextAwaitCallsState> for RoundState {
    fn from(n: NextAwaitCallsState) -> Self {
        match n {
            NextAwaitCallsState::AwaitDiscard(s) => RoundState::AwaitDiscard(s),
            NextAwaitCallsState::RoundEnd(s) => RoundState::RoundEnd(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::op::{AtomicOp, AtomicOpKind, OpError, PhaseKind};

    #[test]
    fn typed_op_macro_generates_correctly() {
        // AwaitDiscardOp 接受 Discard / RiichiDeclare / Tsumo / Ankan / Shouminkan
        let op = AtomicOp::RiichiDeclare;
        let r = AwaitDiscardOp::try_from_atomic(op);
        assert!(matches!(r, Ok(AwaitDiscardOp::RiichiDeclare)));

        // AwaitDiscardOp 拒绝 Pon
        let op = AtomicOp::Pon {
            who: Seat::East,
            hand_tile_ids: [0, 1],
        };
        let r = AwaitDiscardOp::try_from_atomic(op);
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: AtomicOpKind::Pon,
                phase_kind: PhaseKind::AwaitDiscard,
            })
        ));
    }

    #[test]
    fn await_riichi_discard_op_only_discard() {
        let r = AwaitRiichiDiscardOp::try_from_atomic(AtomicOp::Tsumo);
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: AtomicOpKind::Tsumo,
                phase_kind: PhaseKind::AwaitRiichiDiscard,
            })
        ));
    }

    #[test]
    fn await_calls_op_accepts_call_variants() {
        let op = AtomicOp::Pass;
        let r = AwaitCallsOp::try_from_atomic(op);
        assert!(matches!(r, Ok(AwaitCallsOp::Pass)));

        let op = AtomicOp::Ron { who: Seat::South };
        let r = AwaitCallsOp::try_from_atomic(op);
        assert!(matches!(
            r,
            Ok(AwaitCallsOp::Ron { who: Seat::South })
        ));
    }
}
