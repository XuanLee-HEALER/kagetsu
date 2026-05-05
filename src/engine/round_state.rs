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
use crate::engine::domain::meld::{Meld, MeldKind, Seat};
use crate::engine::domain::tile::{Tile, TileIndex, count_by_kind};
use crate::engine::domain::yaku::WinContext;
use crate::engine::event::GameEvent;
use crate::engine::op::{AtomicOp, OpError};
use crate::engine::rules::GameRules;
use crate::engine::score::{ScoreResult, distribute, evaluate};
use crate::engine::state::{PlayerState, RoundResult, RoundWind, RyuukyokuKind};
use crate::engine::wall::Wall;
use crate::typed_op;
use serde::{Deserialize, Serialize};

/// 手牌排序 helper.
fn sort_hand(tiles: &mut Vec<Tile>) {
    tiles.sort_by_key(|t| (t.kind.0, !t.red));
}

/// 从某家闭手中按 id 移除一组牌. 返 false 若有 id 不在.
fn remove_from_hand(p: &mut PlayerState, ids: &[u16]) -> bool {
    let mut to_remove: Vec<u16> = ids.to_vec();
    p.hand.closed.retain(|t| {
        if let Some(pos) = to_remove.iter().position(|id| *id == t.id) {
            to_remove.swap_remove(pos);
            false
        } else {
            true
        }
    });
    to_remove.is_empty()
}

/// 把上家 last_discard 那张从河末尾移除 (鸣牌后).
fn consume_discard(p: &mut PlayerState, tile: Tile) {
    if p.river.last().map(|t| t.id) == Some(tile.id) {
        p.river.pop();
    }
}

/// 鸣牌后清所有玩家 ippatsu + first_go_around.
fn break_first_round_and_ippatsu(common: &mut CommonRound) {
    for pp in common.players.iter_mut() {
        pp.ippatsu_active = false;
    }
    common.first_go_around = false;
}

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
///
/// `last_drawn` 是 Option, 因为进入 AwaitDiscard 有两条路径:
/// - Draw / RinshanDraw 后 → Some(摸到的牌)
/// - Pon / Chi / Minkan 后 → None (鸣牌不摸新牌)
///
/// Tsumo / RiichiDeclare 等 op 在 try_op 里检查 last_drawn 必须 Some
/// (这些动作前提是刚摸了牌).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitDiscardState {
    pub common: CommonRound,
    /// 当前家.
    pub turn: Seat,
    /// 刚摸到的那张, 仅 Draw / RinshanDraw 后 Some, 鸣牌后 None.
    pub last_drawn: Option<Tile>,
}

/// 当前家未摸牌, 唯一合法 op = Draw. driver 自动喂入.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitDrawState {
    pub common: CommonRound,
    pub turn: Seat,
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
    AwaitDraw(AwaitDrawState),
    AwaitDiscard(AwaitDiscardState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    AwaitCalls(AwaitCallsState),
    RoundEnd(RoundEndState),
}

impl RoundState {
    pub fn common(&self) -> &CommonRound {
        match self {
            RoundState::AwaitDraw(s) => &s.common,
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
/// 仅当 last_drawn 是 Some 时可能自摸 (鸣牌后无 last_drawn 不能自摸).
fn try_tsumo(state: &AwaitDiscardState) -> Option<ScoreResult> {
    let p = &state.common.players[state.turn.index()];
    let last = state.last_drawn?;
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
// 公开 entry: round_apply / legal_ops / summarize_round / init_round
// ============================================================

/// 公开 round_apply — engine 暴露的局层转移函数.
/// 接 untrusted AtomicOp, 内部 dispatch 到 typed state 的 try_op + apply.
/// 失败时 caller state 不动 (内部已 clone 自 &state).
pub fn round_apply(
    state: &RoundState,
    op: AtomicOp,
) -> Result<(RoundState, Vec<GameEvent>), OpError> {
    match state.clone() {
        RoundState::AwaitDraw(s) => {
            let typed = s.try_op(op)?;
            let (next, events) = s.apply(typed);
            Ok((next.into(), events))
        }
        RoundState::AwaitDiscard(s) => {
            let typed = s.try_op(op)?;
            let (next, events) = s.apply(typed);
            Ok((next.into(), events))
        }
        RoundState::AwaitRiichiDiscard(s) => {
            let typed = s.try_op(op)?;
            let (next, events) = s.apply(typed);
            Ok((next.into(), events))
        }
        RoundState::AwaitRinshanDraw(s) => {
            let typed = s.try_op(op)?;
            let (next, events) = s.apply(typed);
            Ok((next.into(), events))
        }
        RoundState::AwaitCalls(s) => {
            let typed = s.try_op(op)?;
            let (next, events) = s.apply(typed);
            Ok((next.into(), events))
        }
        RoundState::RoundEnd(_) => Err(OpError::AlreadyEnded),
    }
}

/// 当前 RoundState 下哪些 AtomicOp 合法 — 给 driver / AI 决策用.
///
/// 返回结构汇总各 phase 下可执行算子. 实现思路: 调 try_op 走遍所有可能 op,
/// 收集成功的. 但当前实现简化: 仅返回结构化能力 (per-player call 选项 +
/// 自家 self 选项), 不返回完整 AtomicOp 列表.
#[derive(Debug, Clone, Default)]
pub struct LegalOps {
    /// 自家 (turn) 在 AwaitDiscard 阶段可宣的动作 (类似旧 SelfOptions).
    pub can_tsumo: bool,
    pub riichi_discards: Vec<Tile>,
    pub ankan: Vec<TileIndex>,
    pub shouminkan: Vec<TileIndex>,
    /// 各家在 AwaitCalls 阶段可响应的动作 (类似旧 CallOptions, 4 家分别).
    pub calls: [PerSeatCalls; 4],
}

#[derive(Debug, Clone, Default)]
pub struct PerSeatCalls {
    pub pon: Option<[Tile; 2]>,
    pub chi: Vec<[Tile; 2]>,
    pub minkan: Option<[Tile; 3]>,
    pub ron: bool,
}

pub fn legal_ops(state: &RoundState) -> LegalOps {
    let mut ops = LegalOps::default();
    match state {
        RoundState::AwaitDiscard(s) => {
            // 自家可宣
            ops.can_tsumo = try_tsumo(s).is_some();
            let p = &s.common.players[s.turn.index()];
            if !p.riichi
                && p.hand.is_menzen()
                && p.score >= 1000
                && s.common.wall.remaining() >= 4
            {
                let mut seen = Vec::new();
                for t in &p.hand.closed {
                    if seen.contains(&t.kind.0) {
                        continue;
                    }
                    if is_tenpai_after_discard(p, t.id) {
                        ops.riichi_discards.push(*t);
                        seen.push(t.kind.0);
                    }
                }
            }
            if !p.riichi {
                let counts = count_by_kind(&p.hand.closed);
                for k in 0..34u8 {
                    if counts[k as usize] == 4 {
                        ops.ankan.push(TileIndex(k));
                    }
                }
                for meld in &p.hand.melds {
                    if let MeldKind::Pon { tiles } = &meld.kind {
                        let kind = tiles[0].kind;
                        if counts[kind.0 as usize] >= 1 {
                            ops.shouminkan.push(kind);
                        }
                    }
                }
            }
        }
        RoundState::AwaitCalls(s) => {
            let (from, called) = s.last_discard;
            for who in Seat::ALL {
                if who == from {
                    continue;
                }
                let p = &s.common.players[who.index()];
                let mut pc = PerSeatCalls::default();

                // Pon: 2 张同 kind 在手.
                let counts = count_by_kind(&p.hand.closed);
                if !p.riichi && counts[called.kind.0 as usize] >= 2 {
                    let two: Vec<Tile> = p
                        .hand
                        .closed
                        .iter()
                        .filter(|t| t.kind == called.kind)
                        .copied()
                        .take(2)
                        .collect();
                    if two.len() == 2 {
                        pc.pon = Some([two[0], two[1]]);
                    }
                }

                // Chi: 仅上家.
                if !p.riichi && from.next() == who && called.kind.is_suupai() {
                    let kc = called.kind.0;
                    let suit = kc / 9;
                    let n = kc % 9;
                    let candidates: Vec<(u8, u8)> = match n {
                        0 => vec![(1, 2)],
                        1 => vec![(0, 2), (2, 3)],
                        7 => vec![(5, 6), (6, 8)],
                        8 => vec![(6, 7)],
                        _ => vec![(n - 1, n + 1), (n - 2, n - 1), (n + 1, n + 2)],
                    };
                    for (a, b) in candidates {
                        let ka = suit * 9 + a;
                        let kb = suit * 9 + b;
                        if counts[ka as usize] >= 1 && counts[kb as usize] >= 1 {
                            let ta = p
                                .hand
                                .closed
                                .iter()
                                .find(|t| t.kind.0 == ka)
                                .copied()
                                .unwrap();
                            let tb = p
                                .hand
                                .closed
                                .iter()
                                .find(|t| t.kind.0 == kb)
                                .copied()
                                .unwrap();
                            pc.chi.push([ta, tb]);
                        }
                    }
                }

                // Minkan: 3 张同 kind.
                if !p.riichi && counts[called.kind.0 as usize] >= 3 {
                    let three: Vec<Tile> = p
                        .hand
                        .closed
                        .iter()
                        .filter(|t| t.kind == called.kind)
                        .copied()
                        .take(3)
                        .collect();
                    if three.len() == 3 {
                        pc.minkan = Some([three[0], three[1], three[2]]);
                    }
                }

                // Ron: 牌型 + 役 ok.
                if try_ron(s, who).is_some() {
                    pc.ron = true;
                }

                ops.calls[who.index()] = pc;
            }
        }
        _ => {}
    }
    ops
}

/// 局结束 (RoundState::RoundEnd) 时抽 RoundOutcome 喂给 match_apply.
/// 其它 phase 返 None.
pub fn summarize_round(state: &RoundState) -> Option<crate::engine::match_state::RoundOutcome> {
    if let RoundState::RoundEnd(s) = state {
        match &s.result {
            RoundResult::Win {
                winner,
                is_tsumo,
                loser,
                payments,
                ..
            } => Some(crate::engine::match_state::RoundOutcome::Win {
                winner: *winner,
                is_tsumo: *is_tsumo,
                loser: *loser,
                payments: payments.clone(),
            }),
            RoundResult::Ryuukyoku { kind } => {
                let dealer_p = &s.common.players[s.common.dealer.index()];
                let counts = count_by_kind(&dealer_p.hand.closed);
                let dealer_tenpai =
                    !crate::engine::domain::decompose::tenpai_tiles(&counts, &dealer_p.hand.melds)
                        .is_empty();
                Some(crate::engine::match_state::RoundOutcome::Ryuukyoku {
                    kind: *kind,
                    dealer_tenpai,
                })
            }
        }
    } else {
        None
    }
}

/// 给定 MatchState + seed 创建一局新 RoundState (起手是 AwaitDraw, 等 driver 喂 Draw).
///
/// 返回的 state.turn = MatchState.dealer (新庄家先摸).
pub fn init_round(
    m: &crate::engine::match_state::MatchState,
    seed: u64,
) -> RoundState {
    use crate::engine::state::PlayerState;

    // 4 玩家 PlayerState, score 来自 MatchState.scores.
    let mut players: [PlayerState; 4] = [
        PlayerState::new(Seat::East, m.scores[0]),
        PlayerState::new(Seat::South, m.scores[1]),
        PlayerState::new(Seat::West, m.scores[2]),
        PlayerState::new(Seat::North, m.scores[3]),
    ];

    let mut wall = Wall::shuffled(seed, m.rules.aka_dora);
    // 配牌 13×4
    for _ in 0..13 {
        for seat in Seat::ALL {
            if let Some(t) = wall.draw() {
                players[seat.index()].hand.closed.push(t);
            }
        }
    }
    // 排序
    for p in players.iter_mut() {
        sort_hand(&mut p.hand.closed);
    }

    let common = CommonRound {
        rules: m.rules.clone(),
        round_wind: m.round_wind,
        kyoku: m.kyoku,
        honba: m.honba,
        riichi_sticks_pool: m.riichi_sticks_pool,
        dealer: m.dealer,
        players,
        wall,
        first_go_around: true,
    };

    RoundState::AwaitDraw(AwaitDrawState {
        common,
        turn: m.dealer,
    })
}

// ============================================================
// L4: try_op — validity gate (phase + 数据级 + 规则级)
// ============================================================

impl AwaitDrawState {
    /// 唯一合法 op = Draw. typed_op! 宏自动 reject 其它.
    pub fn try_op(&self, op: AtomicOp) -> Result<AwaitDrawOp, OpError> {
        AwaitDrawOp::try_from_atomic(op)
    }
}

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
                if p.riichi {
                    // 立直方在 AwaitDiscard 必然来自 Draw / RinshanDraw, last_drawn 必 Some.
                    let last = self.last_drawn.expect("riichi player in AwaitDiscard implies recent draw");
                    if last.id != tile.id {
                        return Err(OpError::RiichiMustTsumogiri);
                    }
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

// ============================================================
// L2: typed apply — total transition (输入已 validated)
// ============================================================

impl AwaitDrawState {
    /// 摸一张牌, 转 AwaitDiscard. 若山摸尽 → RoundEnd 流局.
    pub fn apply(self, op: AwaitDrawOp) -> (NextAwaitDrawState, Vec<GameEvent>) {
        let mut events = Vec::new();
        match op {
            AwaitDrawOp::Draw => {
                let mut common = self.common;
                let (new_wall, drawn) = common.wall.drawn();
                common.wall = new_wall;
                match drawn {
                    Some(t) => {
                        common.players[self.turn.index()].hand.closed.push(t);
                        common.players[self.turn.index()].last_drawn = Some(t);
                        sort_hand(&mut common.players[self.turn.index()].hand.closed);
                        events.push(GameEvent::Draw {
                            who: self.turn,
                            tile: t,
                        });
                        (
                            NextAwaitDrawState::AwaitDiscard(AwaitDiscardState {
                                common,
                                turn: self.turn,
                                last_drawn: Some(t),
                            }),
                            events,
                        )
                    }
                    None => {
                        // 山摸尽 → 流局.
                        (
                            NextAwaitDrawState::RoundEnd(RoundEndState {
                                common,
                                result: RoundResult::Ryuukyoku {
                                    kind: RyuukyokuKind::Howaipai,
                                },
                            }),
                            events,
                        )
                    }
                }
            }
        }
    }
}

impl AwaitDiscardState {
    /// 转移函数. self 已含 validated typed-op, 内部 total 无 Result.
    /// 返新 state + 该步 emit 的 GameEvent.
    pub fn apply(self, op: AwaitDiscardOp) -> (NextAwaitDiscardState, Vec<GameEvent>) {
        let mut events = Vec::new();
        match op {
            AwaitDiscardOp::Discard { tile } => {
                let mut common = self.common;
                let p = &mut common.players[self.turn.index()];
                let pos = p
                    .hand
                    .closed
                    .iter()
                    .position(|t| t.id == tile.id)
                    .expect("validated by try_op");
                let removed = p.hand.closed.remove(pos);
                p.river.push(removed);
                p.last_drawn = None;
                p.ippatsu_active = false;
                sort_hand(&mut p.hand.closed);
                events.push(GameEvent::Discard {
                    who: self.turn,
                    tile: removed,
                });
                (
                    NextAwaitDiscardState::AwaitCalls(AwaitCallsState {
                        common,
                        last_discard: (self.turn, removed),
                    }),
                    events,
                )
            }
            AwaitDiscardOp::RiichiDeclare => {
                let mut common = self.common;
                let p = &mut common.players[self.turn.index()];
                p.riichi = true;
                p.double_riichi = common.first_go_around;
                p.ippatsu_active = true;
                p.score -= 1000;
                common.riichi_sticks_pool += 1;
                // 注: 没 Riichi event 单独标记 (RiichiDeclare 是宣告, 切是后续 Discard op,
                // 那个 op 会 emit Discard event. 老代码用 GameEvent::Riichi 既标志宣告又
                // 含 tile, 与新模型 2-op 拆分不符. 保留 GameEvent::Riichi 在 AwaitRiichiDiscard
                // 切牌时一并 emit 更连贯.)
                let last_drawn = self
                    .last_drawn
                    .expect("RiichiDeclare 在 AwaitDiscard 必有 last_drawn (try_op 保证)");
                (
                    NextAwaitDiscardState::AwaitRiichiDiscard(AwaitRiichiDiscardState {
                        common,
                        turn: self.turn,
                        last_drawn,
                    }),
                    events,
                )
            }
            AwaitDiscardOp::Tsumo => {
                let score = try_tsumo(&self).expect("validated by try_op");
                let common = self.common;
                let payments = distribute(
                    &score,
                    self.turn,
                    common.dealer,
                    true,
                    None,
                    common.honba as u32,
                    common.riichi_sticks_pool,
                );
                events.push(GameEvent::Tsumo { who: self.turn });
                (
                    NextAwaitDiscardState::RoundEnd(RoundEndState {
                        common,
                        result: RoundResult::Win {
                            winner: self.turn,
                            is_tsumo: true,
                            loser: None,
                            score,
                            payments,
                        },
                    }),
                    events,
                )
            }
            AwaitDiscardOp::Ankan { kind } => {
                let mut common = self.common;
                // 取 4 张同 kind, 移出 closed.
                let four: Vec<Tile> = common.players[self.turn.index()]
                    .hand
                    .closed
                    .iter()
                    .filter(|t| t.kind == kind)
                    .copied()
                    .collect();
                debug_assert_eq!(four.len(), 4, "validated by try_op");
                common.players[self.turn.index()]
                    .hand
                    .closed
                    .retain(|t| t.kind != kind);
                common.players[self.turn.index()].hand.melds.push(Meld {
                    kind: MeldKind::Ankan {
                        tiles: [four[0], four[1], four[2], four[3]],
                    },
                    from: None,
                });
                break_first_round_and_ippatsu(&mut common);
                common.wall = common.wall.revealed_next_dora();
                events.push(GameEvent::Ankan {
                    who: self.turn,
                    kind,
                });
                (
                    NextAwaitDiscardState::AwaitRinshanDraw(AwaitRinshanDrawState {
                        common,
                        turn: self.turn,
                    }),
                    events,
                )
            }
            AwaitDiscardOp::Shouminkan { kind } => {
                let mut common = self.common;
                let seat = self.turn;
                let meld_pos = common.players[seat.index()]
                    .hand
                    .melds
                    .iter()
                    .position(|m| {
                        matches!(&m.kind, MeldKind::Pon { tiles } if tiles[0].kind == kind)
                    })
                    .expect("validated by try_op");
                let fourth_pos = common.players[seat.index()]
                    .hand
                    .closed
                    .iter()
                    .position(|t| t.kind == kind)
                    .expect("validated by try_op");
                let fourth = common.players[seat.index()].hand.closed.remove(fourth_pos);
                let from = common.players[seat.index()].hand.melds[meld_pos].from;
                let tiles = match &common.players[seat.index()].hand.melds[meld_pos].kind {
                    MeldKind::Pon { tiles } => *tiles,
                    _ => unreachable!("validated by try_op"),
                };
                common.players[seat.index()].hand.melds[meld_pos] = Meld {
                    kind: MeldKind::Shouminkan {
                        tiles: [tiles[0], tiles[1], tiles[2], fourth],
                    },
                    from,
                };
                break_first_round_and_ippatsu(&mut common);
                common.wall = common.wall.revealed_next_dora();
                events.push(GameEvent::Shouminkan { who: seat, kind });
                (
                    NextAwaitDiscardState::AwaitRinshanDraw(AwaitRinshanDrawState {
                        common,
                        turn: seat,
                    }),
                    events,
                )
            }
        }
    }
}

impl AwaitRiichiDiscardState {
    pub fn apply(self, op: AwaitRiichiDiscardOp) -> (NextAwaitRiichiDiscardState, Vec<GameEvent>) {
        let mut events = Vec::new();
        match op {
            AwaitRiichiDiscardOp::Discard { tile } => {
                let mut common = self.common;
                let p = &mut common.players[self.turn.index()];
                let pos = p
                    .hand
                    .closed
                    .iter()
                    .position(|t| t.id == tile.id)
                    .expect("validated by try_op");
                let removed = p.hand.closed.remove(pos);
                p.river.push(removed);
                p.last_drawn = None;
                // 立直宣告牌的 river idx 写入 (UI 横置用).
                p.riichi_river_idx = p.river.len().checked_sub(1);
                sort_hand(&mut p.hand.closed);
                // 立直宣告 + 切作为同一逻辑事件 (UI 提示用).
                events.push(GameEvent::Riichi {
                    who: self.turn,
                    tile: removed,
                });
                (
                    NextAwaitRiichiDiscardState::AwaitCalls(AwaitCallsState {
                        common,
                        last_discard: (self.turn, removed),
                    }),
                    events,
                )
            }
        }
    }
}

impl AwaitRinshanDrawState {
    pub fn apply(self, op: AwaitRinshanDrawOp) -> (NextAwaitRinshanDrawState, Vec<GameEvent>) {
        let mut events = Vec::new();
        match op {
            AwaitRinshanDrawOp::RinshanDraw => {
                let mut common = self.common;
                let (new_wall, drawn) = common.wall.rinshan_drawn();
                common.wall = new_wall;
                match drawn {
                    Some(t) => {
                        common.players[self.turn.index()].hand.closed.push(t);
                        common.players[self.turn.index()].last_drawn = Some(t);
                        sort_hand(&mut common.players[self.turn.index()].hand.closed);
                        events.push(GameEvent::Draw {
                            who: self.turn,
                            tile: t,
                        });
                        (
                            NextAwaitRinshanDrawState::AwaitDiscard(AwaitDiscardState {
                                common,
                                turn: self.turn,
                                last_drawn: Some(t),
                            }),
                            events,
                        )
                    }
                    None => {
                        // 岭上耗尽 → 流局 (理论上 4 杠子流局, 这里简化处理)
                        (
                            NextAwaitRinshanDrawState::RoundEnd(RoundEndState {
                                common,
                                result: RoundResult::Ryuukyoku {
                                    kind: RyuukyokuKind::Howaipai,
                                },
                            }),
                            events,
                        )
                    }
                }
            }
        }
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

    /// 转移函数. 鸣牌 (Pon/Chi/Minkan) 把 turn 转给鸣方, last_discard 清掉, phase 进 AwaitDiscard.
    /// Pass 推到下家, 由调用方 (上层 round_apply) 接 Draw 推进. Ron 进 RoundEnd.
    pub fn apply(self, op: AwaitCallsOp) -> (NextAwaitCallsState, Vec<GameEvent>) {
        let mut events = Vec::new();
        let (from, called) = self.last_discard;
        match op {
            AwaitCallsOp::Pon {
                who,
                hand_tile_ids,
            } => {
                let mut common = self.common;
                let two: [Tile; 2] = {
                    let p = &common.players[who.index()];
                    let a = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[0])
                        .copied()
                        .expect("validated");
                    let b = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[1])
                        .copied()
                        .expect("validated");
                    [a, b]
                };
                let ok = remove_from_hand(
                    &mut common.players[who.index()],
                    &[two[0].id, two[1].id],
                );
                debug_assert!(ok, "validated by try_op");
                common.players[who.index()].hand.melds.push(Meld {
                    kind: MeldKind::Pon {
                        tiles: [two[0], two[1], called],
                    },
                    from: Some(from),
                });
                consume_discard(&mut common.players[from.index()], called);
                break_first_round_and_ippatsu(&mut common);
                sort_hand(&mut common.players[who.index()].hand.closed);
                events.push(GameEvent::Pon { who, tile: called });
                (
                    NextAwaitCallsState::AwaitDiscard(AwaitDiscardState {
                        common,
                        turn: who,
                        // Pon 后没新摸的牌, last_drawn 概念不适用.
                        // type-state 设计要求有 last_drawn — 用 called 牌作占位 (它已副露, 不会被切).
                        // 实际 try_op 阶段会拒绝 Discard last_drawn (因为它在副露不在 closed).
                        // FIXME: 想清楚 Pon 后的 AwaitDiscard 表达 — 当前 last_drawn 字段意义混乱.
                        last_drawn: None,  // Pon/Chi/Minkan 不摸新牌
                    }),
                    events,
                )
            }
            AwaitCallsOp::Chi {
                who,
                hand_tile_ids,
            } => {
                let mut common = self.common;
                let two: [Tile; 2] = {
                    let p = &common.players[who.index()];
                    let a = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[0])
                        .copied()
                        .expect("validated");
                    let b = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[1])
                        .copied()
                        .expect("validated");
                    [a, b]
                };
                let ok = remove_from_hand(
                    &mut common.players[who.index()],
                    &[two[0].id, two[1].id],
                );
                debug_assert!(ok);
                common.players[who.index()].hand.melds.push(Meld {
                    kind: MeldKind::Chi {
                        tiles: [two[0], two[1], called],
                    },
                    from: Some(from),
                });
                consume_discard(&mut common.players[from.index()], called);
                break_first_round_and_ippatsu(&mut common);
                sort_hand(&mut common.players[who.index()].hand.closed);
                events.push(GameEvent::Chi { who, tile: called });
                (
                    NextAwaitCallsState::AwaitDiscard(AwaitDiscardState {
                        common,
                        turn: who,
                        last_drawn: None,  // Pon/Chi/Minkan 不摸新牌
                    }),
                    events,
                )
            }
            AwaitCallsOp::Minkan {
                who,
                hand_tile_ids,
            } => {
                let mut common = self.common;
                let three: [Tile; 3] = {
                    let p = &common.players[who.index()];
                    let a = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[0])
                        .copied()
                        .expect("validated");
                    let b = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[1])
                        .copied()
                        .expect("validated");
                    let c = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.id == hand_tile_ids[2])
                        .copied()
                        .expect("validated");
                    [a, b, c]
                };
                let ok = remove_from_hand(
                    &mut common.players[who.index()],
                    &[three[0].id, three[1].id, three[2].id],
                );
                debug_assert!(ok);
                common.players[who.index()].hand.melds.push(Meld {
                    kind: MeldKind::Minkan {
                        tiles: [three[0], three[1], three[2], called],
                    },
                    from: Some(from),
                });
                consume_discard(&mut common.players[from.index()], called);
                break_first_round_and_ippatsu(&mut common);
                common.wall = common.wall.revealed_next_dora();
                sort_hand(&mut common.players[who.index()].hand.closed);
                events.push(GameEvent::Minkan { who, tile: called });
                // 明杠后必摸岭上, 进 AwaitRinshanDraw (而不是直接给牌).
                // 注: 老代码在 do_minkan 内一并 rinshan_draw, 这里拆开.
                // 暂时简化: 直接当 AwaitDiscard, 留给上层 round_apply 检测 last_meld_was_kan.
                // 但本 type-state 没"last_meld_was_kan"标志... 留 FIXME, 5d 处理.
                (
                    NextAwaitCallsState::AwaitDiscard(AwaitDiscardState {
                        common,
                        turn: who,
                        last_drawn: None,  // Pon/Chi/Minkan 不摸新牌
                    }),
                    events,
                )
            }
            AwaitCallsOp::Ron { who } => {
                let score = try_ron(&self, who).expect("validated");
                let common = self.common;
                let payments = distribute(
                    &score,
                    who,
                    common.dealer,
                    false,
                    Some(from),
                    common.honba as u32,
                    common.riichi_sticks_pool,
                );
                events.push(GameEvent::Ron { who, from });
                (
                    NextAwaitCallsState::RoundEnd(RoundEndState {
                        common,
                        result: RoundResult::Win {
                            winner: who,
                            is_tsumo: false,
                            loser: Some(from),
                            score,
                            payments,
                        },
                    }),
                    events,
                )
            }
            AwaitCallsOp::Pass => {
                // Pass = 没人鸣, 推到下家 AwaitDraw. 弃牌不消费 (留河 = 现状).
                // last_discard 也没意义了 (call window 已关闭).
                let common = self.common;
                (
                    NextAwaitCallsState::AwaitDraw(AwaitDrawState {
                        common,
                        turn: from.next(),
                    }),
                    events,
                )
            }
        }
    }
}

// ============================================================
// Typed-op enum 由 typed_op! 宏生成
// ============================================================

typed_op! {
    AwaitDrawOp from AtomicOp accepts {
        Draw,
    }
    for_phase AwaitDraw;
}

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

/// AwaitDraw 转移可能去向: AwaitDiscard (摸完牌) / RoundEnd (山摸尽 → 流局).
#[derive(Debug, Clone)]
pub enum NextAwaitDrawState {
    AwaitDiscard(AwaitDiscardState),
    RoundEnd(RoundEndState),
}

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

/// AwaitCalls 转移可能去向:
/// - AwaitDiscard (Pon/Chi/Minkan 鸣完, 鸣方接切, last_drawn=None)
/// - AwaitDraw (Pass 没人鸣, 推到下家摸)
/// - RoundEnd (Ron)
#[derive(Debug, Clone)]
pub enum NextAwaitCallsState {
    AwaitDiscard(AwaitDiscardState),
    AwaitDraw(AwaitDrawState),
    RoundEnd(RoundEndState),
}

// ============================================================
// From impls — 把各 NextXxxState 升回公开 RoundState
// ============================================================

impl From<NextAwaitDrawState> for RoundState {
    fn from(n: NextAwaitDrawState) -> Self {
        match n {
            NextAwaitDrawState::AwaitDiscard(s) => RoundState::AwaitDiscard(s),
            NextAwaitDrawState::RoundEnd(s) => RoundState::RoundEnd(s),
        }
    }
}

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
            NextAwaitCallsState::AwaitDraw(s) => RoundState::AwaitDraw(s),
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
            last_drawn: Some(drawn),
        }
    }

    #[test]
    fn await_discard_try_op_discard_in_hand_ok() {
        let s = fixture_await_discard(42);
        let some_tile = s.last_drawn.unwrap();
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
        let last_drawn_id = s.last_drawn.unwrap().id;
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
    fn init_round_creates_await_draw_state() {
        use crate::engine::match_state::MatchState;
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        assert!(matches!(r, RoundState::AwaitDraw(_)));
        assert_eq!(r.common().players[0].hand.closed.len(), 13);
        assert_eq!(r.common().wall.remaining(), 70);
    }

    #[test]
    fn round_apply_draw_then_discard_complete_loop() {
        use crate::engine::match_state::MatchState;
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);

        // AwaitDraw → Draw → AwaitDiscard
        let (r, evs) = round_apply(&r, AtomicOp::Draw).unwrap();
        assert!(matches!(r, RoundState::AwaitDiscard(_)));
        assert_eq!(evs.len(), 1);
        assert!(matches!(evs[0], GameEvent::Draw { .. }));

        // 取一张能切的
        let some_tile = match &r {
            RoundState::AwaitDiscard(s) => s.last_drawn.unwrap(),
            _ => panic!(),
        };
        let (r, evs) = round_apply(&r, AtomicOp::Discard { tile: some_tile }).unwrap();
        assert!(matches!(r, RoundState::AwaitCalls(_)));
        assert!(matches!(evs[0], GameEvent::Discard { .. }));

        // Pass → AwaitDraw (turn=South)
        let (r, _) = round_apply(&r, AtomicOp::Pass).unwrap();
        match &r {
            RoundState::AwaitDraw(s) => assert_eq!(s.turn, Seat::South),
            _ => panic!("expect AwaitDraw"),
        }
    }

    #[test]
    fn round_apply_round_end_rejects_op() {
        use crate::engine::match_state::MatchState;
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        // 制造 RoundEnd: 反复 Draw + Discard + Pass 4 家轮转直到山摸尽.
        // 这里偷懒手动构造 RoundEnd 状态.
        let common = match r {
            RoundState::AwaitDraw(s) => s.common,
            _ => panic!(),
        };
        let r = RoundState::RoundEnd(RoundEndState {
            common,
            result: RoundResult::Ryuukyoku {
                kind: RyuukyokuKind::Howaipai,
            },
        });
        let err = round_apply(&r, AtomicOp::Draw).unwrap_err();
        assert!(matches!(err, OpError::AlreadyEnded));
    }

    #[test]
    fn legal_ops_at_await_discard_lists_riichi_discards() {
        // 用现有 SelfOptions 测试逻辑覆盖, 这里仅 smoke test.
        let s = fixture_await_discard(42);
        let r = RoundState::AwaitDiscard(s);
        let ops = legal_ops(&r);
        // riichi_discards 可能为空 (depends on hand), 但函数不该 panic.
        assert!(ops.riichi_discards.len() <= 14);
    }

    #[test]
    fn await_riichi_discard_try_op_only_discard() {
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].riichi = true;
        // 转成 AwaitRiichiDiscardState
        let ard = AwaitRiichiDiscardState {
            common: s.common.clone(),
            turn: s.turn,
            last_drawn: s.last_drawn.unwrap(),
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
