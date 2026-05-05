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

use crate::engine::domain::decompose::decompose;
use crate::engine::domain::meld::{MeldKind, Seat};
use crate::engine::domain::tile::{Tile, TileIndex, count_by_kind};
use crate::engine::domain::yaku::WinContext;
use crate::engine::op::{AtomicOp, OpError};
use crate::engine::rules::GameRules;
use crate::engine::score::{ScoreResult, evaluate};
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

impl CommonRound {
    /// 自风以亲家相对位置决定: 亲家=东, 下家=南, 对家=西, 上家=北.
    pub fn seat_wind_of(&self, s: Seat) -> TileIndex {
        let offset = (s.index() + 4 - self.dealer.index()) % 4;
        match offset {
            0 => TileIndex::EAST,
            1 => TileIndex::SOUTH,
            2 => TileIndex::WEST,
            _ => TileIndex::NORTH,
        }
    }
}

// ============================================================
// Validity helpers — 多个 typed state 的 try_op 共用
// ============================================================

/// AwaitDiscard 阶段判当前家是否可自摸. 返 ScoreResult 表示可以.
fn try_tsumo(state: &AwaitDiscardState) -> Option<ScoreResult> {
    let p = &state.common.players[state.turn.index()];
    let last = state.last_drawn;
    let counts = count_by_kind(&p.hand.closed);
    let r = decompose(&counts, &p.hand.melds, last.kind);
    if r.is_empty() {
        return None;
    }
    let menzen = p.hand.is_menzen();
    let fully = p.hand.is_fully_concealed();
    let ctx = WinContext {
        decomposition: &r[0],
        seat_wind: state.common.seat_wind_of(state.turn),
        round_wind: state.common.round_wind.tile(),
        winning_tile: last.kind,
        is_tsumo: true,
        is_riichi: p.riichi,
        is_double_riichi: p.double_riichi,
        is_ippatsu: p.ippatsu_active,
        is_haitei: state.common.wall.remaining() == 0,
        is_houtei: false,
        is_rinshan: false,
        is_chankan: false,
        is_tenhou: state.common.first_go_around && state.turn == state.common.dealer,
        is_chiihou: state.common.first_go_around && state.turn != state.common.dealer,
        is_renhou: false,
        menzen,
        fully_concealed: fully,
        dora_count: 0,
        aka_count: 0,
        ura_dora_count: 0,
        rules: &state.common.rules,
    };
    evaluate(&ctx, &p.hand.melds)
}

/// AwaitCalls 阶段判某家是否能荣和 last_discard.
fn try_ron(state: &AwaitCallsState, who: Seat) -> Option<ScoreResult> {
    let (from, tile) = state.last_discard;
    if from == who {
        return None;
    }
    let p = &state.common.players[who.index()];
    let mut counts = count_by_kind(&p.hand.closed);
    counts[tile.kind.0 as usize] += 1;
    let r = decompose(&counts, &p.hand.melds, tile.kind);
    if r.is_empty() {
        return None;
    }
    let menzen = p.hand.is_menzen();
    let fully = p.hand.is_fully_concealed();
    let ctx = WinContext {
        decomposition: &r[0],
        seat_wind: state.common.seat_wind_of(who),
        round_wind: state.common.round_wind.tile(),
        winning_tile: tile.kind,
        is_tsumo: false,
        is_riichi: p.riichi,
        is_double_riichi: p.double_riichi,
        is_ippatsu: p.ippatsu_active,
        is_haitei: false,
        is_houtei: state.common.wall.remaining() == 0,
        is_rinshan: false,
        is_chankan: false,
        is_tenhou: false,
        is_chiihou: false,
        is_renhou: state.common.first_go_around && who != state.common.dealer,
        menzen,
        fully_concealed: fully,
        dora_count: 0,
        aka_count: 0,
        ura_dora_count: 0,
        rules: &state.common.rules,
    };
    evaluate(&ctx, &p.hand.melds)
}

/// 切此 tile 后是否仍听牌 (用于 RiichiDeclare 判断 / AwaitRiichiDiscard 验证).
fn is_tenpai_after_discard(player: &PlayerState, tile_id: u16) -> bool {
    let pos = player.hand.closed.iter().position(|t| t.id == tile_id);
    let Some(pos) = pos else { return false };
    let mut counts = count_by_kind(&player.hand.closed);
    let tile = player.hand.closed[pos];
    counts[tile.kind.0 as usize] -= 1;
    !crate::engine::domain::decompose::tenpai_tiles(&counts, &player.hand.melds).is_empty()
}

/// 当前家手牌中是否含某 id 的牌 (含 last_drawn 已 push 进 hand.closed).
fn hand_contains(player: &PlayerState, tile_id: u16) -> bool {
    player.hand.closed.iter().any(|t| t.id == tile_id)
}

// ============================================================
// L4: try_op — validity gate (phase + 数据级 + 规则级)
// ============================================================

impl AwaitDiscardState {
    /// 完整 validity gate. 通过后 op 一定能在 apply 内 total 执行 (输入域已 valid).
    pub fn try_op(&self, op: AtomicOp) -> Result<AwaitDiscardOp, OpError> {
        let typed = AwaitDiscardOp::try_from_atomic(op)?;
        let p = &self.common.players[self.turn.index()];

        match &typed {
            AwaitDiscardOp::Discard { tile } => {
                if !hand_contains(p, tile.id) {
                    return Err(OpError::TileNotInHand(tile.id));
                }
                if p.riichi && self.last_drawn.id != tile.id {
                    return Err(OpError::RiichiMustTsumogiri);
                }
            }
            AwaitDiscardOp::RiichiDeclare => {
                if p.riichi {
                    return Err(OpError::AlreadyRiichi);
                }
                if !p.hand.is_menzen() {
                    return Err(OpError::NotMenzen);
                }
                if p.score < 1000 {
                    return Err(OpError::InsufficientScore);
                }
                if self.common.wall.remaining() < 4 {
                    return Err(OpError::InsufficientWall);
                }
                // 至少有一张牌切完后听牌.
                let any_tenpai = p
                    .hand
                    .closed
                    .iter()
                    .any(|t| is_tenpai_after_discard(p, t.id));
                if !any_tenpai {
                    return Err(OpError::NotTenpaiForRiichi);
                }
            }
            AwaitDiscardOp::Tsumo => {
                if try_tsumo(self).is_none() {
                    // 不和牌 / 无役 — 区分困难, 统一返 NotWinning (decompose 失败 = 不和;
                    // decompose 成功但 evaluate=None = 无役). 上游用户能从 try_op 失败知不可点.
                    return Err(OpError::NotWinning);
                }
            }
            AwaitDiscardOp::Ankan { kind } => {
                if p.riichi {
                    // 立直后简化: 禁暗杠 (严格规则: 不变 wait 的暗杠允许; MVP 一刀切)
                    return Err(OpError::DisallowedWhileRiichi(
                        crate::engine::op::AtomicOpKind::Ankan,
                    ));
                }
                let counts = count_by_kind(&p.hand.closed);
                if counts[kind.0 as usize] < 4 {
                    return Err(OpError::InsufficientForAnkan(*kind));
                }
            }
            AwaitDiscardOp::Shouminkan { kind } => {
                if p.riichi {
                    return Err(OpError::DisallowedWhileRiichi(
                        crate::engine::op::AtomicOpKind::Shouminkan,
                    ));
                }
                let has_pon = p.hand.melds.iter().any(|m| {
                    matches!(&m.kind, MeldKind::Pon { tiles } if tiles[0].kind == *kind)
                });
                if !has_pon {
                    return Err(OpError::NoMatchingPonForShouminkan(*kind));
                }
                let counts = count_by_kind(&p.hand.closed);
                if counts[kind.0 as usize] < 1 {
                    return Err(OpError::InsufficientForAnkan(*kind));
                    // 复用 InsufficientForAnkan 表达"少了那张第 4 张" — 也可加 Shouminkan 专用 variant
                }
            }
        }
        Ok(typed)
    }
}

impl AwaitRiichiDiscardState {
    pub fn try_op(&self, op: AtomicOp) -> Result<AwaitRiichiDiscardOp, OpError> {
        let typed = AwaitRiichiDiscardOp::try_from_atomic(op)?;
        let p = &self.common.players[self.turn.index()];
        match &typed {
            AwaitRiichiDiscardOp::Discard { tile } => {
                if !hand_contains(p, tile.id) {
                    return Err(OpError::TileNotInHand(tile.id));
                }
                // 立直宣告时切的这张, 必须是切完后听牌的那张.
                if !is_tenpai_after_discard(p, tile.id) {
                    return Err(OpError::NotTenpaiForRiichi);
                }
            }
        }
        Ok(typed)
    }
}

impl AwaitRinshanDrawState {
    pub fn try_op(&self, op: AtomicOp) -> Result<AwaitRinshanDrawOp, OpError> {
        // 唯一合法 op = RinshanDraw, typed_op! 宏自动 reject 其它.
        AwaitRinshanDrawOp::try_from_atomic(op)
    }
}

impl AwaitCallsState {
    pub fn try_op(&self, op: AtomicOp) -> Result<AwaitCallsOp, OpError> {
        let typed = AwaitCallsOp::try_from_atomic(op)?;
        let (from, called_tile) = self.last_discard;

        match &typed {
            AwaitCallsOp::Pon { who, hand_tile_ids } => {
                if *who == from {
                    return Err(OpError::PonOwnDiscard);
                }
                let p = &self.common.players[who.index()];
                if p.riichi {
                    return Err(OpError::DisallowedWhileRiichi(
                        crate::engine::op::AtomicOpKind::Pon,
                    ));
                }
                for id in hand_tile_ids {
                    if !hand_contains(p, *id) {
                        return Err(OpError::TileNotInHand(*id));
                    }
                }
                // 必须 3 张同 kind.
                let kinds: Vec<_> = hand_tile_ids
                    .iter()
                    .filter_map(|id| p.hand.closed.iter().find(|t| t.id == *id))
                    .map(|t| t.kind)
                    .collect();
                if kinds.len() != 2 || !kinds.iter().all(|k| *k == called_tile.kind) {
                    return Err(OpError::PonKindMismatch);
                }
            }
            AwaitCallsOp::Chi { who, hand_tile_ids } => {
                if from.next() != *who {
                    return Err(OpError::ChiNotFromUpper);
                }
                let p = &self.common.players[who.index()];
                if p.riichi {
                    return Err(OpError::DisallowedWhileRiichi(
                        crate::engine::op::AtomicOpKind::Chi,
                    ));
                }
                for id in hand_tile_ids {
                    if !hand_contains(p, *id) {
                        return Err(OpError::TileNotInHand(*id));
                    }
                }
                // 必须组成顺子.
                let tiles_in: Vec<_> = hand_tile_ids
                    .iter()
                    .filter_map(|id| p.hand.closed.iter().find(|t| t.id == *id))
                    .map(|t| t.kind.0)
                    .collect();
                if tiles_in.len() != 2 {
                    return Err(OpError::ChiNotASequence);
                }
                let mut three = [called_tile.kind.0, tiles_in[0], tiles_in[1]];
                three.sort();
                let valid_seq = TileIndex(three[0]).is_suupai()
                    && three[0] / 9 == three[2] / 9
                    && three[1] == three[0] + 1
                    && three[2] == three[0] + 2;
                if !valid_seq {
                    return Err(OpError::ChiNotASequence);
                }
            }
            AwaitCallsOp::Minkan { who, hand_tile_ids } => {
                if *who == from {
                    return Err(OpError::PonOwnDiscard); // 复用: 不能明杠自家弃牌
                }
                let p = &self.common.players[who.index()];
                if p.riichi {
                    return Err(OpError::DisallowedWhileRiichi(
                        crate::engine::op::AtomicOpKind::Minkan,
                    ));
                }
                for id in hand_tile_ids {
                    if !hand_contains(p, *id) {
                        return Err(OpError::TileNotInHand(*id));
                    }
                }
                let kinds: Vec<_> = hand_tile_ids
                    .iter()
                    .filter_map(|id| p.hand.closed.iter().find(|t| t.id == *id))
                    .map(|t| t.kind)
                    .collect();
                if kinds.len() != 3 || !kinds.iter().all(|k| *k == called_tile.kind) {
                    return Err(OpError::MinkanKindMismatch);
                }
            }
            AwaitCallsOp::Ron { who } => {
                if try_ron(self, *who).is_none() {
                    return Err(OpError::NotWinning);
                }
            }
            AwaitCallsOp::Pass => {
                // 始终合法.
            }
        }
        Ok(typed)
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

    // ─── try_op validity gate 测试 ───
    //
    // Test fixtures: 用现有 GameState::new + start_round 构造合法初始 GameState,
    // 抽出 PlayerState/Wall 等组装到新 RoundState. 这是 stage 5b 的临时桥, 阶段 5d
    // 写完 init_round 后改用那个.

    use crate::engine::rules::GameRules;
    use crate::engine::state::GameState;

    /// 用 seed 构造一个 AwaitDiscardState (东家摸第 14 张后, 未切).
    fn fixture_await_discard(seed: u64) -> AwaitDiscardState {
        let mut g = GameState::new(GameRules::default());
        g.start_round(seed);
        // 让 East 摸一张
        let drawn = g.do_draw().expect("wall not empty");
        assert_eq!(g.turn, Seat::East);
        let common = CommonRound {
            rules: g.rules.clone(),
            round_wind: g.round_wind,
            kyoku: g.kyoku,
            honba: g.honba,
            riichi_sticks_pool: g.riichi_sticks as u32,
            dealer: g.dealer,
            players: g.players.clone(),
            wall: g.wall.clone().expect("wall set"),
            first_go_around: g.first_go_around,
        };
        AwaitDiscardState {
            common,
            turn: g.turn,
            last_drawn: drawn,
        }
    }

    #[test]
    fn await_discard_try_op_discard_in_hand_ok() {
        let s = fixture_await_discard(42);
        let some_tile = s.last_drawn;
        let r = s.try_op(AtomicOp::Discard { tile: some_tile });
        assert!(matches!(r, Ok(AwaitDiscardOp::Discard { .. })));
    }

    #[test]
    fn await_discard_try_op_discard_not_in_hand_err() {
        let s = fixture_await_discard(42);
        let fake_tile = Tile {
            kind: TileIndex(0),
            red: false,
            id: 9999, // not in any hand
        };
        let r = s.try_op(AtomicOp::Discard { tile: fake_tile });
        assert!(matches!(r, Err(OpError::TileNotInHand(9999))));
    }

    #[test]
    fn await_discard_try_op_pon_phase_mismatch() {
        let s = fixture_await_discard(42);
        let r = s.try_op(AtomicOp::Pon {
            who: Seat::East,
            hand_tile_ids: [0, 1],
        });
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: crate::engine::op::AtomicOpKind::Pon,
                phase_kind: crate::engine::op::PhaseKind::AwaitDiscard,
            })
        ));
    }

    #[test]
    fn await_discard_try_op_riichi_no_score_err() {
        let mut s = fixture_await_discard(42);
        // 把 East 分数砸到 < 1000.
        s.common.players[Seat::East.index()].score = 500;
        let r = s.try_op(AtomicOp::RiichiDeclare);
        assert!(matches!(r, Err(OpError::InsufficientScore)));
    }

    #[test]
    fn await_discard_try_op_riichi_already_err() {
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].riichi = true;
        let r = s.try_op(AtomicOp::RiichiDeclare);
        assert!(matches!(r, Err(OpError::AlreadyRiichi)));
    }

    #[test]
    fn await_discard_try_op_ankan_insufficient_tiles_err() {
        let s = fixture_await_discard(42);
        // 大概率 hand 不会 4 张同 kind, fixture seed=42 应该不会触发.
        let r = s.try_op(AtomicOp::Ankan { kind: TileIndex(0) });
        // 要么 InsufficientForAnkan 要么 DisallowedWhileRiichi (后者只在 riichi=true 时), 这里 East 未立直.
        assert!(matches!(
            r,
            Err(OpError::InsufficientForAnkan(TileIndex(0)))
        ));
    }

    #[test]
    fn await_discard_try_op_ankan_while_riichi_err() {
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].riichi = true;
        let r = s.try_op(AtomicOp::Ankan { kind: TileIndex(0) });
        assert!(matches!(
            r,
            Err(OpError::DisallowedWhileRiichi(
                crate::engine::op::AtomicOpKind::Ankan
            ))
        ));
    }

    #[test]
    fn await_discard_try_op_riichi_must_tsumogiri() {
        let mut s = fixture_await_discard(42);
        let last_drawn_id = s.last_drawn.id;
        s.common.players[Seat::East.index()].riichi = true;
        // 选一张不是 last_drawn 的牌.
        let other_tile = s
            .common
            .players[Seat::East.index()]
            .hand
            .closed
            .iter()
            .find(|t| t.id != last_drawn_id)
            .copied()
            .expect("hand has tiles");
        let r = s.try_op(AtomicOp::Discard { tile: other_tile });
        assert!(matches!(r, Err(OpError::RiichiMustTsumogiri)));
    }

    #[test]
    fn await_riichi_discard_try_op_only_discard() {
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].riichi = true;
        // 转成 AwaitRiichiDiscardState
        let ard = AwaitRiichiDiscardState {
            common: s.common.clone(),
            turn: s.turn,
            last_drawn: s.last_drawn,
        };
        let r = ard.try_op(AtomicOp::Tsumo);
        assert!(matches!(
            r,
            Err(OpError::IllegalForPhase {
                op_kind: crate::engine::op::AtomicOpKind::Tsumo,
                phase_kind: crate::engine::op::PhaseKind::AwaitRiichiDiscard,
            })
        ));
    }
}
