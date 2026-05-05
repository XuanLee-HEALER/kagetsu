//! 算子代数 (`AtomicOp`) + 失败原因 (`OpError`).
//!
//! 这是 engine 公开的 *唯一* 输入面: 任何 driver (单机 UI、AI、网络对局、
//! 录像 replay) 都把"这一步要做什么"塞进 [`AtomicOp`], 喂给
//! [`crate::engine::round_state::round_apply`] 推动局向前.
//!
//! # 算子分类
//!
//! [`AtomicOp`] 12 个 variant 按所属 phase 分组:
//!
//! - **摸牌算子** ([`AtomicOp::Draw`] / [`AtomicOp::RinshanDraw`]) —
//!   通常 driver 在 `AwaitDraw` / `AwaitRinshanDraw` 阶段自动喂入,
//!   也可作为录像中的显式条目.
//! - **切牌阶段算子** ([`AtomicOp::Discard`] / [`AtomicOp::RiichiDeclare`] /
//!   [`AtomicOp::Tsumo`] / [`AtomicOp::Ankan`] / [`AtomicOp::Shouminkan`])
//!   — 当前家在 `AwaitDiscard` 阶段可选动作.
//! - **鸣牌窗口算子** ([`AtomicOp::Pon`] / [`AtomicOp::Chi`] /
//!   [`AtomicOp::Minkan`] / [`AtomicOp::Ron`] / [`AtomicOp::Pass`]) —
//!   `AwaitCalls` 阶段他家对刚弃出的牌如何响应.
//!
//! # 错误模型
//!
//! [`OpError`] 不是计算错误 — engine 计算本身永远不会出错 (那是 bug, 应 panic).
//! `OpError` 是 *输入合法性* 裁定: caller 喂的 op 在当前 state 下违反规则或
//! 引用不存在的实体. 三类:
//!
//! 1. **数据级** (例: [`OpError::TileNotInHand`]) — op 引用的 id 不存在
//! 2. **规则级** (例: [`OpError::NotMenzen`] 即"门前清不成立, 不能立直") —
//!    符合数据但违反日麻规则
//! 3. **Phase 错配** ([`OpError::IllegalForPhase`]) — 该 op 在该 phase 没有意义
//!
//! # 设计选择: 全用 unit 或 named fields, 不用位置参数
//!
//! 例如 `Discard { tile: Tile }` 而非 `Discard(Tile)`. 原因:
//! declarative macro 没法对位置参数生成 fresh binding ident, 用 named fields
//! 让 `typed_op!` 宏实现极简. 序列化格式也更清晰.
//!
//! # 引用
//!
//! 设计文档: `docs/design/abstract-model.md`

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::{Tile, TileIndex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 局内不可分动作 — engine 接受的唯一 op 类型.
///
/// 12 个 variant 覆盖一局麻将能发生的所有玩家/系统决策. 喂给
/// [`crate::engine::round_state::round_apply`] 推动 RoundState 前进.
///
/// # 序列化 / 录像
///
/// 实现了 `Serialize`/`Deserialize`, 可作为录像 (replay) 的最小条目 —
/// 完整局回放 = `Vec<AtomicOp>` + 局起手 seed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomicOp {
    // ─── 摸牌算子 (driver 自动, 也作为录像显式条目) ───
    /// 普通摸牌 (摸牌 / つも / Tsumo as 动词).
    ///
    /// 从牌山活牌区 (live wall) 末尾摸一张, 加入当前家手牌, 转 `AwaitDiscard`.
    /// 仅在 `AwaitDraw` phase 合法. 若 live wall 已耗尽 (`remaining()==0`)
    /// 则触发 *荒牌流局* (Howaipai), engine 自动转 `RoundEnd`.
    Draw,

    /// 岭上摸牌 (嶺上開花 / Rinshan).
    ///
    /// 杠 (Kan) 之后必须从死墙 (dead wall) 的岭上区摸一张, 同时翻新一枚
    /// 宝牌指示牌 (新ドラ / Shin-Dora). 仅在 `AwaitRinshanDraw` phase 合法.
    /// 若该张和了, 称作"岭上開花"役.
    RinshanDraw,

    // ─── AwaitDiscard / AwaitRiichiDiscard 阶段算子 ───
    /// 切牌 (打牌 / Dahai).
    ///
    /// `tile.id` 必须在当前家手牌中 (含刚摸到的 `last_drawn`).
    /// 切完后进 `AwaitCalls` 等其它 3 家响应窗口.
    ///
    /// 立直方 (riichi=true) 在 `AwaitDiscard` 必须摸切 (切刚摸的那张),
    /// 否则返 [`OpError::RiichiMustTsumogiri`].
    Discard {
        /// 要切的牌. 通过唯一 `id` 定位 (避免红宝牌 / 同 kind 多张的歧义).
        tile: Tile,
    },

    /// 立直宣告 (立直 / リーチ / Riichi).
    ///
    /// 此 op *不切牌*, 仅:
    /// 1. 设置 `riichi` flag, 玩家分数扣 1000
    /// 2. 把这枚 1000 点棒计入立直棒池 (供托 / Kyoutaku)
    /// 3. 转 `AwaitRiichiDiscard`, 唯一合法下一 op = `Discard`
    /// 4. 第一巡 (`first_go_around=true`) 立直自动升级为双立直 (W立直 / Daburi)
    ///
    /// 立直前提 (engine 检查): 门前清 (Menzen) + 切某张后听牌 +
    /// 分数 ≥ 1000 + 牌山剩余 ≥ 4. 不满足返对应 `OpError::*` variant.
    RiichiDeclare,

    /// 自摸和了 (自摸和 / Tsumo).
    ///
    /// 当前家用 `last_drawn` 这张牌完成和了. engine 自动:
    /// 1. 评分 (decompose + yaku detect + 番符)
    /// 2. 计算 payments (子家三家分摊 / 庄家平摊)
    /// 3. 立直棒池清算给和家
    /// 4. 转 `RoundEnd`
    ///
    /// 牌型不和或无役返 [`OpError::NotWinning`].
    Tsumo,

    /// 暗杠 (暗槓 / Ankan).
    ///
    /// 当前家手中 4 张同 kind 牌 (含 `last_drawn`) 直接副露成杠子.
    /// 暗杠不破坏门前清状态 (Menzen 仍 true), 立直方在严格规则下也可
    /// 暗杠 (但本 engine 简化: 立直后禁所有杠).
    ///
    /// 杠完进 `AwaitRinshanDraw` 等岭上摸. 翻新一枚宝牌指示牌 (新ドラ).
    Ankan {
        /// 杠的牌种 (4 张同 kind 的 kind).
        kind: TileIndex,
    },

    /// 加杠 (加槓 / 小明槓 / Shouminkan).
    ///
    /// 已有副露刻子 (Pon) + 自手第 4 张同 kind 时, 把那第 4 张加进副露成杠.
    /// 加杠破坏门前清, 立直后禁止. 是 *被抢杠* (Chankan / 槍槓) 唯一能成立的
    /// 时机: 抢杠和家在杠完之前用 `Ron` 截胡.
    Shouminkan {
        /// 已有副露 Pon 的 kind, 必须有自手第 4 张.
        kind: TileIndex,
    },

    // ─── AwaitCalls 阶段算子 ───
    /// 碰 (ポン / Pon).
    ///
    /// 鸣方手中 2 张同 kind 配上家弃牌组成刻子 (3 张同). 鸣后:
    /// - 鸣方副露刻子, 破坏门前清
    /// - turn 直接转给鸣方 (跳过中间各家), 进 `AwaitDiscard`
    /// - 全场 *一发* (Ippatsu) 标志清空 (鸣牌打断一发)
    /// - 第一巡标志清空
    ///
    /// 立直方不能碰 ([`OpError::DisallowedWhileRiichi`]).
    Pon {
        /// 鸣方. 必须 ≠ 切牌方 ([`OpError::PonOwnDiscard`]).
        who: Seat,
        /// 鸣方手里要出的两张 (用唯一 id 定位).
        hand_tile_ids: [u16; 2],
    },

    /// 吃 (チー / Chi).
    ///
    /// 鸣方手中 2 张配上家弃牌组成顺子 (3 张连续同花色). 仅 *上家* 可吃
    /// (即切牌方下家, [`OpError::ChiNotFromUpper`]).
    Chi {
        /// 鸣方. 必须 = `from.next()` (上家).
        who: Seat,
        /// 鸣方手里要出的两张 (`tile1` + `tile2` + 弃牌 = 顺子).
        hand_tile_ids: [u16; 2],
    },

    /// 明杠 (大明槓 / Minkan).
    ///
    /// 鸣方手中 3 张同 kind 配他家弃牌成杠子. 鸣后流程跟 `Pon` 类似,
    /// 但鸣完不直接 `AwaitDiscard` — 必须先岭上摸, 即 engine 自动让鸣方进
    /// `AwaitRinshanDraw`.
    Minkan {
        who: Seat,
        /// 鸣方手里要出的三张 (3 张同 kind).
        hand_tile_ids: [u16; 3],
    },

    /// 荣和 (栄和 / ロン / Ron).
    ///
    /// `who` 用切牌方刚弃的那张完成和了. 评分 / payments / 立直棒池清算
    /// 同 `Tsumo`, 但 loser = 切牌方独自支付.
    ///
    /// 振听 (Furiten) 检查在调用方实现 (engine 当前未 enforce).
    Ron {
        /// 和家. 必须 ≠ 切牌方 (放铳家不能荣和自己的弃牌).
        who: Seat,
    },

    /// 跳过 (パス / Pass).
    ///
    /// 表示 4 家都不响应当前弃牌, 鸣牌窗口关闭, turn 推到切牌方下家进
    /// `AwaitDraw`. 在 `AwaitCalls` phase *永远合法*.
    Pass,
}

/// [`AtomicOp`] 的 variant 标签 (枚举 ID, 不带数据).
///
/// 用于 [`OpError::IllegalForPhase`] 等需要描述 op 类型但不需要 payload 的场景.
/// `AtomicOp::kind()` 返回对应 `AtomicOpKind`.
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
    /// 返回本 op 对应的 variant 标签.
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

/// 局内 phase 的 variant 标签, 与 [`crate::engine::round_state::RoundState`] 一一对应.
///
/// 用于 [`OpError::IllegalForPhase`] 等错误描述. engine 内 type-state 设计意味着
/// 大部分 phase 错配在编译期就被消除 — 这里是 runtime 兜底, 给 driver 一个
/// 统一报错格式.
///
/// # 6 个 phase
///
/// - `AwaitDraw` — 等当前家摸牌, 唯一合法 op = `Draw`
/// - `AwaitDiscard` — 当前家已摸牌, 选: 切牌 / 立直 / 自摸 / 杠
/// - `AwaitRiichiDiscard` — 立直宣告后, 必须切刚摸的牌 (摸切)
/// - `AwaitRinshanDraw` — 杠后, 必须岭上摸
/// - `AwaitCalls` — 切牌后等其它 3 家鸣 / 荣和 / 跳过
/// - `RoundEnd` — 局已结束, 不接受任何 op
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseKind {
    AwaitDraw,
    AwaitDiscard,
    AwaitRiichiDiscard,
    AwaitRinshanDraw,
    AwaitCalls,
    RoundEnd,
}

/// [`crate::engine::round_state::round_apply`] 拒绝 op 的结构化原因.
///
/// 哲学: **不是计算错误**. engine 内部计算 (洗牌、决麻形、算番符) 永远成功 —
/// 那些是 total function. `OpError` 是 caller 喂的 op 在当前 state 下没意义,
/// 是 *输入合法性裁定*. 三类:
///
/// 1. **数据级** — op 引用 state 里不存在的实体 (`TileNotInHand` / `NoLastDiscard` / ...)
/// 2. **规则级** — 数据合理但违反日麻规则 (`NotMenzen` / `RiichiMustTsumogiri` / ...)
/// 3. **Phase 错配** + **边界态** — `IllegalForPhase` / `AlreadyEnded`
///
/// 所有 variant 都实现 `Display` (via `thiserror`), 直接 `format!("{}", err)`
/// 给用户友好提示.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum OpError {
    // ─── 数据级 ───
    /// op 引用的牌 `id` 不在玩家手中. 通常是 driver / replay 用了过期 snapshot.
    #[error("手中无 id={0} 的牌")]
    TileNotInHand(u16),

    /// 在 `AwaitCalls` 阶段但内部 last_discard = None — 理论上不可能 (是 bug).
    /// 留作防御性 variant.
    #[error("当前无 last_discard, 无法响应")]
    NoLastDiscard,

    /// 暗杠 (Ankan): 当前家手中没有 4 张同 kind 的指定牌种.
    #[error("当前家手中无 4 张 {0:?} 同 kind 牌, 不能暗杠")]
    InsufficientForAnkan(TileIndex),

    /// 加杠 (Shouminkan): 当前家没有该 kind 的副露刻子, 或没有第 4 张同 kind 在自手.
    #[error("当前家无 {0:?} 同 kind 的副露刻子, 不能加杠")]
    NoMatchingPonForShouminkan(TileIndex),

    /// 吃 (Chi): `hand_tile_ids` 两张配弃牌不构成连续 3 张顺子 (同花色 + 数字连续).
    #[error("吃: 给定的两张牌 + 弃牌不构成连续顺子")]
    ChiNotASequence,

    /// 明杠 (Minkan): 三张手牌 kind 跟弃牌 kind 不一致 (理论上 driver 不会犯,
    /// 防御 variant).
    #[error("明杠: 三张手牌 kind 不匹配弃牌")]
    MinkanKindMismatch,

    /// 碰 (Pon): 两张手牌 kind 跟弃牌 kind 不一致.
    #[error("碰: 两张手牌 kind 不匹配弃牌")]
    PonKindMismatch,

    // ─── 规则级 ───
    /// 立直方在 `AwaitDiscard` 阶段必须切刚摸的那张 (摸切 / Tsumogiri).
    /// 切手中其它牌返本错.
    #[error("立直方必须摸切")]
    RiichiMustTsumogiri,

    /// 立直前提之一: 门前清 (Menzen, 即没有副露 Pon/Chi/Minkan; 暗杠不破).
    /// 已副露的玩家不能立直.
    #[error("有副露不能立直")]
    NotMenzen,

    /// 立直前提之一: 切某张后必须听牌 (Tenpai). 任何牌切完都不听就不能立直.
    #[error("切此牌后未听牌, 不能立直")]
    NotTenpaiForRiichi,

    /// 立直前提之一: 玩家分数 ≥ 1000 (要扣立直棒).
    #[error("分数 < 1000, 不能立直")]
    InsufficientScore,

    /// 立直前提之一: 牌山活牌区 (live wall) 剩余 ≥ 4 张 (保证一巡).
    #[error("牌山剩余 < 4, 不能立直")]
    InsufficientWall,

    /// 立直方在某些 op 上被禁止 (本 engine 简化: 禁所有杠 / 鸣牌). 严格规则下
    /// 不变 wait 的暗杠允许; 留作 future 扩展.
    #[error("立直后不能 {0:?}")]
    DisallowedWhileRiichi(AtomicOpKind),

    /// 重复立直 (玩家已 riichi=true 时再喂 `RiichiDeclare`).
    #[error("已立直, 不能重复立直")]
    AlreadyRiichi,

    /// 自家不能碰自己的弃牌 (理论上不可能因为切牌方在 `AwaitCalls` 不是当前家;
    /// 防御 variant).
    #[error("不能碰自己的弃牌")]
    PonOwnDiscard,

    /// 吃只能从上家 (即 `who == from.next()`). 跳家或对家吃返本错.
    #[error("吃只能从上家")]
    ChiNotFromUpper,

    /// 自摸 / 荣和: 牌型 decompose 不出和了型 (4 面子 + 1 雀头 / 国士无双 / 七对子).
    /// 包含 *无役* 子情形 (decompose 成功但 yaku 检测返 None).
    #[error("自摸 / 荣和 但牌型不和")]
    NotWinning,

    /// 和了但无役 (役 / Yaku) — 日麻规则要求至少 1 役才能和.
    /// 注: 当前实现合并到 `NotWinning`, 此 variant 保留备用.
    #[error("和了但无役")]
    NoYaku,

    // ─── Phase 错配 ───
    /// op 在当前 phase 没有意义 (例: `AwaitDraw` 阶段塞 `Discard`).
    #[error("op {op_kind:?} 在 phase {phase_kind:?} 不合法")]
    IllegalForPhase {
        op_kind: AtomicOpKind,
        phase_kind: PhaseKind,
    },

    // ─── 边界态 ───
    /// 局已结束 ([`crate::engine::round_state::RoundState::RoundEnd`]).
    /// 任何 op 都拒绝, 调用方应改用 [`crate::engine::round_state::summarize_round`]
    /// + [`crate::engine::match_state::match_apply`] 推进到下一局.
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
