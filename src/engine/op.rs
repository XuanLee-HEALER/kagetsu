//! AtomicOp + OpError — engine 计算的算子代数.
//!
//! AtomicOp 是 engine 暴露给外部的统一算子集合 (单一 enum, 所有外部 driver
//! 都基于它). OpError 是 engine 拒绝 op 时的结构化原因.
//!
//! 见 docs/design/abstract-model.md.
//!
//! ## 设计选择: variant 全用 unit 或 named fields, 不用位置参数
//!
//! 例如 `Discard { tile: Tile }` 而非 `Discard(Tile)`. 原因:
//! declarative macro 没法对位置参数生成 fresh binding ident, 用 named fields
//! 让 `typed_op!` 宏实现极简. 序列化格式也更清晰.

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::{Tile, TileIndex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 局内不可分动作. engine 唯一接受的 event 类型.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomicOp {
    // ─── 引擎自动 (driver 据 state 自然推断, 也作为录像中的显式条目) ───
    /// 摸普通牌 (从牌山尾部 pop).
    Draw,
    /// 杠后从岭上摸.
    RinshanDraw,

    // ─── AwaitDiscard / AwaitRiichiDiscard 阶段算子 ───
    /// 切牌. tile 必须在当前家手中 (含 last_drawn).
    Discard { tile: Tile },
    /// 立直宣告. 不切牌, 仅设置 riichi flag + 扣 1000 入池. 之后 phase=AwaitRiichiDiscard,
    /// 唯一合法下一 op = Discard.
    RiichiDeclare,
    /// 自摸宣告.
    Tsumo,
    /// 暗杠. kind 必须在当前家手中有 4 张同 kind.
    Ankan { kind: TileIndex },
    /// 加杠. kind 必须有副露刻子 + 自手第 4 张.
    Shouminkan { kind: TileIndex },

    // ─── AwaitCalls 阶段算子 ───
    /// 碰. who = 鸣方, hand_tile_ids = 鸣方手里出的两张 (id 唯一定位).
    Pon { who: Seat, hand_tile_ids: [u16; 2] },
    /// 吃. 同上, who 必须是上家.
    Chi { who: Seat, hand_tile_ids: [u16; 2] },
    /// 明杠.
    Minkan {
        who: Seat,
        hand_tile_ids: [u16; 3],
    },
    /// 荣和.
    Ron { who: Seat },
    /// 跳过整个鸣牌窗口 (没人响应), 推进到下家摸牌.
    Pass,
}

/// AtomicOp 的 variant kind, 用于 OpError 报错 (不带数据).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomicOpKind {
    Draw,
    RinshanDraw,
    Discard,
    RiichiDeclare,
    Tsumo,
    Ankan,
    Shouminkan,
    Pon,
    Chi,
    Minkan,
    Ron,
    Pass,
}

impl AtomicOp {
    pub fn kind(&self) -> AtomicOpKind {
        match self {
            AtomicOp::Draw => AtomicOpKind::Draw,
            AtomicOp::RinshanDraw => AtomicOpKind::RinshanDraw,
            AtomicOp::Discard { .. } => AtomicOpKind::Discard,
            AtomicOp::RiichiDeclare => AtomicOpKind::RiichiDeclare,
            AtomicOp::Tsumo => AtomicOpKind::Tsumo,
            AtomicOp::Ankan { .. } => AtomicOpKind::Ankan,
            AtomicOp::Shouminkan { .. } => AtomicOpKind::Shouminkan,
            AtomicOp::Pon { .. } => AtomicOpKind::Pon,
            AtomicOp::Chi { .. } => AtomicOpKind::Chi,
            AtomicOp::Minkan { .. } => AtomicOpKind::Minkan,
            AtomicOp::Ron { .. } => AtomicOpKind::Ron,
            AtomicOp::Pass => AtomicOpKind::Pass,
        }
    }
}

/// Phase 的 variant kind, 用于 OpError 报错.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseKind {
    AwaitDraw,
    AwaitDiscard,
    AwaitRiichiDiscard,
    AwaitRinshanDraw,
    AwaitCalls,
    RoundEnd,
}

/// engine 拒绝 op 的结构化原因.
///
/// 不是"计算错误" — engine 计算本身永远不该出错 (那是 bug, 应 panic).
/// OpError variant 是 mahjong 规则书的反面陈述, caller 喂的 op 在当前 state 下无意义时返回.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum OpError {
    // ─── 数据级 (op 引用了 state 里不存在的东西) ───
    #[error("手中无 id={0} 的牌")]
    TileNotInHand(u16),
    #[error("当前无 last_discard, 无法响应")]
    NoLastDiscard,
    #[error("当前家手中无 4 张 {0:?} 同 kind 牌, 不能暗杠")]
    InsufficientForAnkan(TileIndex),
    #[error("当前家无 {0:?} 同 kind 的副露刻子, 不能加杠")]
    NoMatchingPonForShouminkan(TileIndex),
    #[error("吃: 给定的两张牌 + 弃牌不构成连续顺子")]
    ChiNotASequence,
    #[error("明杠: 三张手牌 kind 不匹配弃牌")]
    MinkanKindMismatch,
    #[error("碰: 两张手牌 kind 不匹配弃牌")]
    PonKindMismatch,

    // ─── 规则级 (op 违反 mahjong 规则) ───
    #[error("立直方必须摸切")]
    RiichiMustTsumogiri,
    #[error("有副露不能立直")]
    NotMenzen,
    #[error("切此牌后未听牌, 不能立直")]
    NotTenpaiForRiichi,
    #[error("分数 < 1000, 不能立直")]
    InsufficientScore,
    #[error("牌山剩余 < 4, 不能立直")]
    InsufficientWall,
    #[error("立直后不能 {0:?}")]
    DisallowedWhileRiichi(AtomicOpKind),
    #[error("已立直, 不能重复立直")]
    AlreadyRiichi,
    #[error("不能碰自己的弃牌")]
    PonOwnDiscard,
    #[error("吃只能从上家")]
    ChiNotFromUpper,
    #[error("自摸 / 荣和 但牌型不和")]
    NotWinning,
    #[error("和了但无役")]
    NoYaku,

    // ─── Phase 错配 (type-state 内部 try_op 大部分编译期消, 这里是 runtime 兜底) ───
    #[error("op {op_kind:?} 在 phase {phase_kind:?} 不合法")]
    IllegalForPhase {
        op_kind: AtomicOpKind,
        phase_kind: PhaseKind,
    },

    // ─── 边界态 ───
    #[error("局已结束, 不接受任何 op")]
    AlreadyEnded,
}

/// declarative macro: 给定 AtomicOp 的子集, 生成 typed-op enum + try_from_atomic +
/// From<TypedOp> for AtomicOp.
///
/// 所有 variant 形式必须是 unit (无字段) 或 named fields (无位置参数).
/// 这是有意的设计 (位置参数会让 declarative macro 复杂得多).
///
/// 用法:
/// ```ignore
/// typed_op! {
///     AwaitDiscardOp from AtomicOp accepts {
///         Discard { tile: Tile },
///         RiichiDeclare,
///         Tsumo,
///         Ankan { kind: TileIndex },
///         Shouminkan { kind: TileIndex },
///     }
///     for_phase AwaitDiscard;
/// }
/// ```
///
/// 展开生成:
/// 1. `enum AwaitDiscardOp { ... }` (variant 与 AtomicOp 同名 + 同字段)
/// 2. `impl AwaitDiscardOp { fn try_from_atomic(op: AtomicOp) -> Result<Self, OpError> }`
///    — 列出的 variant 翻译, 其它返 `OpError::IllegalForPhase`.
/// 3. `impl From<AwaitDiscardOp> for AtomicOp` — 反向转换 (录像复用).
#[macro_export]
macro_rules! typed_op {
    (
        $name:ident from AtomicOp accepts {
            $(
                $variant:ident $( { $($field:ident : $ty:ty),* $(,)? } )?
            ),* $(,)?
        }
        for_phase $phase:ident;
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
        pub enum $name {
            $(
                $variant $( { $($field: $ty),* } )?
            ),*
        }

        impl $name {
            /// AtomicOp → typed-op. 不属于本 phase 接受的 variant 返
            /// `OpError::IllegalForPhase`. 不做数据级 / 规则级检查 (那由 typed state 的
            /// try_op 在调本函数后追加).
            pub fn try_from_atomic(
                op: $crate::engine::op::AtomicOp,
            ) -> ::std::result::Result<Self, $crate::engine::op::OpError> {
                use $crate::engine::op::{AtomicOp, OpError, PhaseKind};
                let kind = op.kind();
                match op {
                    $(
                        AtomicOp::$variant $( { $($field),* } )?
                        => Ok(Self::$variant $( { $($field),* } )?),
                    )*
                    _ => Err(OpError::IllegalForPhase {
                        op_kind: kind,
                        phase_kind: PhaseKind::$phase,
                    }),
                }
            }
        }

        impl ::std::convert::From<$name> for $crate::engine::op::AtomicOp {
            fn from(op: $name) -> Self {
                match op {
                    $(
                        $name::$variant $( { $($field),* } )?
                        => $crate::engine::op::AtomicOp::$variant $( { $($field),* } )?,
                    )*
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试用 typed-op (mahjong 用例稍后实际场景定义).
    typed_op! {
        TestDiscardOp from AtomicOp accepts {
            Discard { tile: Tile },
            Tsumo,
            Pass,
        }
        for_phase AwaitDiscard;
    }

    fn t(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    #[test]
    fn try_from_atomic_accepts_listed_variants() {
        let r = TestDiscardOp::try_from_atomic(AtomicOp::Discard { tile: t(0, 0) });
        assert!(matches!(r, Ok(TestDiscardOp::Discard { .. })));

        let r = TestDiscardOp::try_from_atomic(AtomicOp::Tsumo);
        assert!(matches!(r, Ok(TestDiscardOp::Tsumo)));
    }

    #[test]
    fn try_from_atomic_rejects_unlisted_variants() {
        let r = TestDiscardOp::try_from_atomic(AtomicOp::RiichiDeclare);
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: AtomicOpKind::RiichiDeclare,
                phase_kind: PhaseKind::AwaitDiscard,
            })
        ));

        let r = TestDiscardOp::try_from_atomic(AtomicOp::Pon {
            who: Seat::East,
            hand_tile_ids: [0, 1],
        });
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: AtomicOpKind::Pon,
                ..
            })
        ));
    }

    #[test]
    fn from_typed_op_back_to_atomic() {
        let typed = TestDiscardOp::Discard { tile: t(5, 10) };
        let atomic: AtomicOp = typed.into();
        assert_eq!(atomic, AtomicOp::Discard { tile: t(5, 10) });
    }

    #[test]
    fn op_kind_matches_variant() {
        assert_eq!(AtomicOp::Draw.kind(), AtomicOpKind::Draw);
        assert_eq!(
            AtomicOp::Discard { tile: t(0, 0) }.kind(),
            AtomicOpKind::Discard
        );
        assert_eq!(
            AtomicOp::Pon {
                who: Seat::East,
                hand_tile_ids: [0, 1]
            }
            .kind(),
            AtomicOpKind::Pon
        );
    }
}
