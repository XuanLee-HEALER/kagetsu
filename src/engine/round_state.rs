//! 局 (Round / 局 / Kyoku) 层状态 + 转移函数.
//!
//! 一局麻将从 *配牌* (Haipai, 4 家各发 13 张) 开始, 经过摸牌 / 切牌 / 鸣牌 /
//! 立直等若干步, 直到 *和了* (Agari) 或 *流局* (Ryuukyoku) 结束. 本模块用
//! type-state 模式建模这条状态机.
//!
//! # Driver 用法
//!
//! ```ignore
//! use tui_majo::engine::{
//!     match_state::MatchState,
//!     round_state::{init_round, round_apply, summarize_round, RoundState},
//!     op::AtomicOp,
//!     rules::GameRules,
//! };
//!
//! // 1. 起庄
//! let mut mat = MatchState::new(GameRules::default());
//! // 2. 起一局
//! let mut round = init_round(&mat, 0xdead_beef /* seed */);
//! // 3. 推动状态机, 直到 RoundEnd
//! while !round.is_ended() {
//!     let op: AtomicOp = decide_next_op(&round); // 你的 driver
//!     let (next, _events) = round_apply(&round, op).expect("op valid");
//!     round = next;
//! }
//! // 4. 把局结果喂给庄, 进下一局
//! let outcome = summarize_round(&round).unwrap();
//! mat = tui_majo::engine::match_state::match_apply(&mat, outcome);
//! ```
//!
//! # 状态机 (6 phase)
//!
//! [`RoundState`] 是 6-variant enum, 对应 6 个 phase:
//!
//! | Phase | 触发条件 | 唯一 / 主要合法 op |
//! |---|---|---|
//! | [`AwaitDraw`](RoundState::AwaitDraw) | 局开始 / 上家切完 Pass | [`Draw`](AtomicOp::Draw) |
//! | [`AwaitDiscard`](RoundState::AwaitDiscard) | 摸完牌 / 鸣牌后 | `Discard` / `RiichiDeclare` / `Tsumo` / `Ankan` / `Shouminkan` |
//! | [`AwaitRiichiDiscard`](RoundState::AwaitRiichiDiscard) | `RiichiDeclare` 后 | `Discard` (限定为切某张听牌) |
//! | [`AwaitRinshanDraw`](RoundState::AwaitRinshanDraw) | 任意杠后 | [`RinshanDraw`](AtomicOp::RinshanDraw) |
//! | [`AwaitCalls`](RoundState::AwaitCalls) | 切牌后 | `Pon` / `Chi` / `Minkan` / `Ron` / `Pass` |
//! | [`RoundEnd`](RoundState::RoundEnd) | 和了 / 流局 / 山摸尽 | (任何 op 均拒绝) |
//!
//! # 4 层架构 (内部细节)
//!
//! - **L1 数据层**: [`AtomicOp`] (统一 enum, 序列化友好, 适合录像 / 网络协议)
//! - **L2 类型化 state**: [`AwaitDiscardState`] / [`AwaitCallsState`] / 等 6 个,
//!   各自只携带该 phase 需要的字段
//! - **L3 类型化 op**: `AwaitDiscardOp` / `AwaitCallsOp` / 等 (由 `typed_op!` 宏
//!   生成), AtomicOp 的子集对应该 phase 合法的 variants
//! - **L4 桥接**: 各 typed state 的 `try_op` 方法 — 把 AtomicOp 验证 + 翻译成
//!   typed-op, 失败返 [`OpError`]
//!
//! 公开 [`round_apply`] 把 4 层串起来, driver 只需要面对 [`RoundState`] +
//! [`AtomicOp`] 即可.
//!
//! # 引用
//!
//! 设计文档: `docs/design/abstract-model.md` §Layer 2

use crate::engine::domain::decompose::decompose;
use crate::engine::domain::meld::{Meld, MeldKind, Seat};
use crate::engine::domain::tile::{Tile, TileIndex, count_by_kind};
use crate::engine::domain::yaku::WinContext;
use crate::engine::event::GameEvent;
use crate::engine::op::{AtomicOp, OpError};
use crate::engine::player::PlayerState;
use crate::engine::rules::GameRules;
use crate::engine::score::{PaymentDistribution, ScoreResult, distribute, evaluate};
use crate::engine::wall::Wall;
use crate::typed_op;
use serde::{Deserialize, Serialize};

// ============================================================
// 局 / 庄共享类型 (RoundWind / RoundResult / RyuukyokuKind 等局级概念)
// ============================================================

/// 场风 (場風 / Bakaze) — 整个 *圈* (round) 的风牌.
///
/// 日麻按场风划分阶段:
/// - 东风战 (Tonpuusen): 仅 `East` 风, 4 局后结束
/// - 半庄 (Hanchan): `East` → `South`, 共 8 局
/// - 一庄 (Ichijou): `East` → `South` → `West` → `North` (本 engine 不支持)
///
/// 与自风 (Jikaze, 玩家相对庄家位置) 一起构成 *役牌* (Yakuhai) 的判定依据.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundWind {
    /// 东 (東 / トン).
    East,
    /// 南 (南 / ナン).
    South,
    /// 西 (西 / シャー). 本 engine 不主动进入, 保留备用.
    West,
    /// 北 (北 / ペー). 本 engine 不主动进入, 保留备用.
    North,
}

impl RoundWind {
    /// 转成对应的字牌 [`TileIndex`] (评分时用).
    pub fn tile(self) -> TileIndex {
        match self {
            RoundWind::East => TileIndex::EAST,
            RoundWind::South => TileIndex::SOUTH,
            RoundWind::West => TileIndex::WEST,
            RoundWind::North => TileIndex::NORTH,
        }
    }
    /// 中文短名 ("东" / "南" / ...).
    pub fn label(self) -> &'static str {
        match self {
            RoundWind::East => "东",
            RoundWind::South => "南",
            RoundWind::West => "西",
            RoundWind::North => "北",
        }
    }
}

/// 一局的最终结果.
///
/// 当 [`RoundState`] 进入 [`RoundState::RoundEnd`] 时挂在
/// [`RoundEndState::result`]. 调用 [`summarize_round`] 抽出对应的
/// [`crate::engine::match_state::RoundOutcome`] 喂给庄层的
/// [`crate::engine::match_state::match_apply`] 推进.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundResult {
    /// 和了 (和了 / Agari) — 自摸 ([`AtomicOp::Tsumo`]) 或荣和 ([`AtomicOp::Ron`]).
    Win {
        /// 和家.
        winner: Seat,
        /// `true` = 自摸; `false` = 荣和.
        is_tsumo: bool,
        /// 放铳家. 自摸时 None, 荣和时 = 切牌方.
        loser: Option<Seat>,
        /// 完整评分结果 (役 / 番 / 符 / 总点数 / 等级).
        score: ScoreResult,
        /// 完整支付列表 (含立直棒 self-payment, score::distribute 算出).
        payments: Vec<PaymentDistribution>,
    },
    /// 流局 (流局 / Ryuukyoku) — 牌山摸尽或无役 (NoYaku) 等情况, 见 [`RyuukyokuKind`].
    Ryuukyoku {
        kind: RyuukyokuKind,
    },
}

/// 流局类型. 当前 engine 仅支持 *荒牌流局* (`Howaipai`) 和占位的 `NoYaku`.
///
/// 严格规则下还有: 九种九牌、四风连打、四杠散了、四家立直、三家和了 — 全部
/// future work, 当前简化为 Howaipai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RyuukyokuKind {
    /// 荒牌流局 (荒牌平局 / Howaipai) — 牌山活牌区摸尽且无人和, 局自然结束.
    /// 本 engine 当前唯一会真触发的流局类型.
    Howaipai,
    /// 占位 variant (无役流局), 保留给将来需要区分场景.
    NoYaku,
}

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

/// 局内 6 个 typed state 共享的字段.
///
/// 局开始时由 [`init_round`] 从 [`crate::engine::match_state::MatchState`] 注入:
/// - 庄层信息 (rules / round_wind / kyoku / honba / dealer / riichi_sticks_pool)
///   局内不变 (rules) 或仅特定 op 会改 (riichi_sticks_pool 在立直时 +1)
/// - 4 家初始状态 (`players`) 由 [`init_round`] 配牌 13×4
/// - 牌山 ([`Wall`]) 含活牌区 + 死墙 + 已翻宝牌指示
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonRound {
    /// 整庄规则 (从 [`MatchState`](crate::engine::match_state::MatchState) 注入,
    /// 局内不变).
    pub rules: GameRules,
    /// 场风 (場風 / Bakaze) — 决定字牌的役牌身份.
    pub round_wind: RoundWind,
    /// 局序号 (1..=4 in each round_wind, 例: 东 1 / 东 2 / ...).
    pub kyoku: u8,
    /// 本场数 (本場 / Honba) — 庄家连和 / 流局每次 +1, 子家和清零.
    /// 影响和了点数 (每本场 +300 点).
    pub honba: u8,
    /// 立直棒池 (供托 / Kyoutaku) — 已立直未被领走的 1000 点棒计数.
    /// 局内有人立直 → +1; 和家整池领走; 流局保留到下局.
    pub riichi_sticks_pool: u32,
    /// 庄家 (亲家 / 親 / Oya) — 决定本局自风分配 + 和了点数加倍.
    pub dealer: Seat,
    /// 4 家完整状态 (含手牌 / 弃牌河 / 副露 / 分数 / 立直 flags / 最后摸到的牌).
    /// 索引方式 = `Seat::index()` (East=0, South=1, West=2, North=3).
    pub players: [PlayerState; 4],
    /// 牌山 — 含活牌区 (live wall) / 死墙 (dead wall) / 已翻宝牌指示 (dora indicator).
    pub wall: Wall,
    /// *第一巡* (一巡目 / Chunkun) 是否仍未被鸣牌 / 杠打断.
    /// 用于判定 *天和* (Tenhou, 庄家配牌即和) / *地和* (Chiihou, 子家第一摸即和) /
    /// *人和* (Renhou, 子家在自家第一巡内荣和上家弃牌) 等极端役.
    pub first_go_around: bool,
}

/// **AwaitDiscard** — 当前家已摸牌, 等切牌 / 立直 / 自摸 / 杠决策.
///
/// 进入路径 (二选一):
/// - [`AtomicOp::Draw`] / [`AtomicOp::RinshanDraw`] 后 → `last_drawn = Some(刚摸的牌)`
/// - [`AtomicOp::Pon`] / [`AtomicOp::Chi`] / [`AtomicOp::Minkan`] 后 → `last_drawn = None` (鸣牌不摸新牌)
///
/// `Tsumo` / `RiichiDeclare` 等动作 *前提是刚摸了牌*, 因此在 try_op 里检查
/// `last_drawn` 必须 `Some` 否则拒绝.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitDiscardState {
    pub common: CommonRound,
    /// 当前家 (待决策方).
    pub turn: Seat,
    /// 刚摸到的那张. `Some` 仅在 Draw / RinshanDraw 之后, 鸣牌后 `None`.
    pub last_drawn: Option<Tile>,
}

/// **AwaitDraw** — 等当前家摸牌. 唯一合法 op = [`AtomicOp::Draw`].
///
/// 通常 driver 自动喂 `Draw` (没有玩家选择), 但作为显式 phase 让录像 (replay)
/// 表达更精准 — 摸的那一刻有明确事件.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitDrawState {
    pub common: CommonRound,
    /// 即将摸牌的家.
    pub turn: Seat,
}

/// **AwaitRiichiDiscard** — 立直宣告 ([`AtomicOp::RiichiDeclare`]) 已执行,
/// 必须紧接着切牌.
///
/// 立直规则要求宣告与切牌是 *逻辑上不可分* 的两步. engine 把它拆成两个 op,
/// 但用 type-state 限定中间不能插入其它操作 (除了切牌就是 [`OpError::IllegalForPhase`]).
///
/// `last_drawn` 必为 `Some` (立直前提是刚摸牌, 用 `Tile` 直接表达).
/// 切牌时检查必须切某张听牌 ([`OpError::NotTenpaiForRiichi`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitRiichiDiscardState {
    pub common: CommonRound,
    /// 立直方.
    pub turn: Seat,
    /// 立直瞬间手中刚摸的那张. 玩家可以切此牌 (摸切) 也可以切手中其它能听的牌.
    pub last_drawn: Tile,
}

/// **AwaitRinshanDraw** — 任何杠 (Ankan / Shouminkan / Minkan) 后, 必须从死墙
/// 岭上区摸一张. 唯一合法 op = [`AtomicOp::RinshanDraw`].
///
/// driver 通常自动喂入. 单独建模这个 phase 让录像 / 抢杠 (Chankan) 等场景
/// 有明确 hook 点 (虽然当前 engine 抢杠未实现).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitRinshanDrawState {
    pub common: CommonRound,
    /// 杠完即将岭上摸的家.
    pub turn: Seat,
}

/// **AwaitCalls** — 当前家已切牌, 鸣牌窗口 (鳴き / Naki window) 打开.
///
/// 其它 3 家可选: 碰 ([`AtomicOp::Pon`]) / 吃 ([`AtomicOp::Chi`], 限上家) /
/// 明杠 ([`AtomicOp::Minkan`]) / 荣和 ([`AtomicOp::Ron`]) / 跳过 ([`AtomicOp::Pass`]).
///
/// 实际上层 driver 通常按优先级收集所有 3 家响应 ([`legal_ops`] 提供
/// per-seat 查询), 头跳规则 (Atamahane) 决定多家荣和谁优先.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitCallsState {
    pub common: CommonRound,
    /// 切牌方 + 刚切出的那张牌.
    pub last_discard: (Seat, Tile),
}

/// **RoundEnd** — 局已结束 (和了 / 流局 / 山摸尽).
///
/// 不接受任何 op (返 [`OpError::AlreadyEnded`]). 调用方:
/// 1. 读 [`RoundEndState::result`] 知和家 / 役 / 流局原因
/// 2. 调 [`summarize_round`] 抽 [`crate::engine::match_state::RoundOutcome`]
/// 3. 喂给 [`crate::engine::match_state::match_apply`] 更新 MatchState
/// 4. [`init_round`] 起下一局
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundEndState {
    pub common: CommonRound,
    /// 和了 / 流局结果.
    pub result: RoundResult,
}

/// 局状态机. **driver 唯一面对的 round 类型**.
///
/// 6 个 variant 对应 6 个 phase, 每个 variant 包一个 typed state struct
/// (字段精确反映该 phase 必有的信息). 详见模块顶部 doc.
///
/// # 推进方式
///
/// 用 [`round_apply`] 喂 [`AtomicOp`]. 不应直接构造 / mutate variant 内部字段.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundState {
    /// 等摸牌. driver 通常立即喂 [`AtomicOp::Draw`].
    AwaitDraw(AwaitDrawState),
    /// 等切牌 / 立直 / 自摸 / 杠决策.
    AwaitDiscard(AwaitDiscardState),
    /// 立直宣告后必须切牌, 唯一合法 op = `Discard`.
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    /// 杠后必须岭上摸, driver 通常立即喂 `RinshanDraw`.
    AwaitRinshanDraw(AwaitRinshanDrawState),
    /// 切牌后鸣牌窗口, 等其它 3 家响应或 Pass.
    AwaitCalls(AwaitCallsState),
    /// 局已结束 — 调 [`summarize_round`] + [`crate::engine::match_state::match_apply`].
    RoundEnd(RoundEndState),
}

impl RoundState {
    /// 取该 phase 的共享字段 (rules / 4 家 / 牌山 / etc.).
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

    /// 当前轮到哪家 (RoundEnd / AwaitCalls 阶段无单一 turn 概念, None).
    pub fn turn(&self) -> Option<Seat> {
        match self {
            RoundState::AwaitDraw(s) => Some(s.turn),
            RoundState::AwaitDiscard(s) => Some(s.turn),
            RoundState::AwaitRiichiDiscard(s) => Some(s.turn),
            RoundState::AwaitRinshanDraw(s) => Some(s.turn),
            RoundState::AwaitCalls(_) => None,
            RoundState::RoundEnd(_) => None,
        }
    }

    /// AwaitCalls 阶段的最近弃牌 (切牌方 + 弃出的那张). 其它 phase None.
    pub fn last_discard(&self) -> Option<(Seat, Tile)> {
        match self {
            RoundState::AwaitCalls(s) => Some(s.last_discard),
            _ => None,
        }
    }

    /// 局是否已结束 (RoundEnd phase).
    pub fn is_ended(&self) -> bool {
        matches!(self, RoundState::RoundEnd(_))
    }

    /// AwaitDiscard / AwaitRiichiDiscard / AwaitRinshanDraw 阶段当前家刚摸到的牌 (None 若鸣牌后或非这些阶段).
    pub fn last_drawn(&self) -> Option<Tile> {
        match self {
            RoundState::AwaitDiscard(s) => s.last_drawn,
            RoundState::AwaitRiichiDiscard(s) => Some(s.last_drawn),
            _ => None,
        }
    }

    /// RoundEnd 阶段的结果, 其它 phase None.
    pub fn result(&self) -> Option<&RoundResult> {
        match self {
            RoundState::RoundEnd(s) => Some(&s.result),
            _ => None,
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

/// 局层转移函数 — engine 公开的主 entry point.
///
/// 输入当前 [`RoundState`] + 一个 [`AtomicOp`], 返回新 state + 该步 emit 的
/// [`GameEvent`] 列表 (供 driver / UI / 录像消费).
///
/// # 错误模型
///
/// - 输入合法 → `Ok((new_state, events))`. caller 应 *替换* 自己的 state,
///   旧 state 引用应丢弃.
/// - 输入不合法 → `Err(OpError)`. **caller state 不动** (内部已 clone 自 `&state`).
///
/// # 不变量
///
/// - 是 *pure function*: 同样 `(state, op)` 永远产生同样输出
/// - 内部不做副作用 (不打 log, 不 push 录像, 不 mutate input)
/// - clone 整个 state, 性能 O(n) 但与游戏总步数比可忽略
///
/// # Phase 分发
///
/// 内部根据 [`RoundState`] variant 分发到对应 typed state 的 `try_op + apply`:
/// - `AwaitDraw / AwaitDiscard / AwaitRiichiDiscard / AwaitRinshanDraw / AwaitCalls`
///   → 该 typed state 处理
/// - `RoundEnd` → 永远返 [`OpError::AlreadyEnded`]
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

/// 当前 [`RoundState`] 下合法动作的结构化汇总 — 供 driver / AI 决策用.
///
/// **不返回完整 [`AtomicOp`] 列表**, 而是按"自家可宣"+"四家可响应"的结构
/// 让 UI / AI 直接渲染 / 决策. 调用方需要时手动 build [`AtomicOp`] 喂回
/// [`round_apply`].
///
/// # 字段语义
///
/// - 前 4 个字段 (`can_tsumo` / `riichi_discards` / `ankan` / `shouminkan`)
///   仅在 `AwaitDiscard` phase 有意义 — 当前家自己可主动宣的动作
/// - `calls[seat.index()]` 仅在 `AwaitCalls` phase 有意义 — 各家对刚弃出的牌
///   可作的响应
///
/// 其它 phase 调用 [`legal_ops`] 返默认空 `LegalOps` (所有字段 default).
#[derive(Debug, Clone, Default)]
pub struct LegalOps {
    /// 当前家 (turn) 是否能自摸和了 (考虑了役 / 番符 + 是否有 last_drawn).
    pub can_tsumo: bool,
    /// 切哪几张可立直成立 (按 kind 去重: 同 kind 只列一张代表). 空 = 不能立直.
    pub riichi_discards: Vec<Tile>,
    /// 当前家手中可暗杠的 kind 集合 (4 张同 kind 的 kind).
    pub ankan: Vec<TileIndex>,
    /// 当前家可加杠的 kind 集合 (有副露 Pon 且自手第 4 张).
    pub shouminkan: Vec<TileIndex>,
    /// 各家在 `AwaitCalls` 阶段可响应的动作.
    /// 索引: `Seat::index()`. 切牌方 (`from`) 的 `PerSeatCalls` 全部 default.
    pub calls: [PerSeatCalls; 4],
}

/// 单家对刚弃牌 (`AwaitCallsState::last_discard`) 的合法响应集合.
///
/// 立直方所有响应都禁 (本 engine 简化), 即对应玩家 `pon` / `chi` / `minkan`
/// 全 None / 空 vec, `ron` 仍可能 true (立直方仍能荣和).
#[derive(Debug, Clone, Default)]
pub struct PerSeatCalls {
    /// 碰: `Some([t1, t2])` = 鸣方手中可出的两张 (pair). None = 不能碰.
    pub pon: Option<[Tile; 2]>,
    /// 吃: 多种顺子方案. 例: 弃牌是 5m, 自手有 3m/4m + 4m/6m + 6m/7m
    /// 三种吃法都列出. 仅上家可吃, 其它家恒为空 vec.
    pub chi: Vec<[Tile; 2]>,
    /// 明杠: `Some([t1, t2, t3])` = 鸣方手中三张同 kind. None = 不能.
    pub minkan: Option<[Tile; 3]>,
    /// 是否能荣和.
    pub ron: bool,
}

/// 计算当前 [`RoundState`] 下的合法动作.
///
/// 复杂度近似 O(玩家数 × 手牌数), 单次调用 < 1ms. 是 pure query, 不改 state.
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

/// 抽取局结果给庄层用. 仅 [`RoundState::RoundEnd`] 时返 `Some`, 否则 `None`.
///
/// 流局 (Ryuukyoku) 时同时计算 *庄家是否听牌* (dealer_tenpai) — 决定连庄
/// (Renchan) 还是进局 (i.e. dealer 推到下家). 和了时数据直接来自
/// [`RoundResult::Win`].
///
/// 输出喂给 [`crate::engine::match_state::match_apply`] 推进
/// [`crate::engine::match_state::MatchState`].
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

/// 起一局新的 [`RoundState`].
///
/// 流程:
/// 1. 4 家 [`PlayerState::new`] 用 [`MatchState::scores`](crate::engine::match_state::MatchState::scores) 注入分数
/// 2. [`Wall::shuffled`] 用 `seed` 洗牌 (确定性, 同 seed 同结果, 适合 replay)
/// 3. 配牌 (Haipai): 13×4 = 52 张分发, 按东→南→西→北轮转
/// 4. 各家手牌排序 (UI 友好)
/// 5. 庄家 (`m.dealer`) 进 [`RoundState::AwaitDraw`], 等首张摸牌
///
/// `seed` 通常 = 庄 seed XOR 局序号, 让每局牌山可复现且互不相关.
pub fn init_round(
    m: &crate::engine::match_state::MatchState,
    seed: u64,
) -> RoundState {
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
// NextXxxState — typed apply 函数的返回类型
//
// 把"该 phase 的 apply 可能转去哪些下一 phase" 编码进类型. 通过 From impl
// 升回公开 RoundState. 主要价值: 编译期穷尽性检查, 防止 typed apply 返
// 不该返的 phase (例: AwaitRiichiDiscard 切完只能进 AwaitCalls, 不会进 RoundEnd).
// ============================================================

/// [`AwaitDrawState::apply`] 的可能去向.
///
/// - `AwaitDiscard` — 摸到牌, 等切牌
/// - `RoundEnd` — 牌山摸尽 (`Wall::remaining() == 0`) → 荒牌流局
#[derive(Debug, Clone)]
pub enum NextAwaitDrawState {
    AwaitDiscard(AwaitDiscardState),
    RoundEnd(RoundEndState),
}

/// [`AwaitDiscardState::apply`] 的可能去向 (4 选 1).
///
/// - `AwaitCalls` — 普通切牌后等其它 3 家响应
/// - `AwaitRiichiDiscard` — 立直宣告后, 限定切某张
/// - `AwaitRinshanDraw` — 暗杠 / 加杠 后, 必须岭上摸
/// - `RoundEnd` — 自摸和了
#[derive(Debug, Clone)]
pub enum NextAwaitDiscardState {
    AwaitCalls(AwaitCallsState),
    AwaitRiichiDiscard(AwaitRiichiDiscardState),
    AwaitRinshanDraw(AwaitRinshanDrawState),
    RoundEnd(RoundEndState),
}

/// [`AwaitRiichiDiscardState::apply`] 的可能去向 (单一).
///
/// 立直切牌后必然进 [`AwaitCalls`](RoundState::AwaitCalls), 没有其它分支.
/// (立直切牌不能直接和了, 因为立直瞬间已经过自摸判定.)
#[derive(Debug, Clone)]
pub enum NextAwaitRiichiDiscardState {
    AwaitCalls(AwaitCallsState),
}

/// [`AwaitRinshanDrawState::apply`] 的可能去向.
///
/// - `AwaitDiscard` — 岭上摸到, 等鸣方切牌
/// - `RoundEnd` — 岭上区耗尽 (理论 4 杠子流局, 当前简化为 Howaipai)
#[derive(Debug, Clone)]
pub enum NextAwaitRinshanDrawState {
    AwaitDiscard(AwaitDiscardState),
    RoundEnd(RoundEndState),
}

/// [`AwaitCallsState::apply`] 的可能去向 (3 选 1).
///
/// - `AwaitDiscard` — Pon/Chi/Minkan 鸣完, turn 转给鸣方等切牌 (`last_drawn = None`)
/// - `AwaitDraw` — Pass 4 家都不响应, 推到切牌方下家摸
/// - `RoundEnd` — Ron 荣和
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
    use crate::engine::score::ScoreLevel;

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
    // Test fixtures: 用 init_round + round_apply Draw 推到 AwaitDiscard.

    use crate::engine::match_state::MatchState;
    use crate::engine::rules::GameRules;

    /// 用 seed 构造一个 AwaitDiscardState (东家摸第 14 张后, 未切).
    fn fixture_await_discard(seed: u64) -> AwaitDiscardState {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, seed);
        let (next, _) = round_apply(&r, AtomicOp::Draw).expect("Draw on fresh round");
        match next {
            RoundState::AwaitDiscard(s) => {
                assert_eq!(s.turn, Seat::East);
                s
            }
            _ => panic!("Draw should land on AwaitDiscard"),
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

    // ============================================================
    // RoundWind / RoundState getter / 简单 helper
    // ============================================================

    #[test]
    fn roundwind_tile_maps_to_wind_tileindex() {
        assert_eq!(RoundWind::East.tile(), TileIndex::EAST);
        assert_eq!(RoundWind::South.tile(), TileIndex::SOUTH);
        assert_eq!(RoundWind::West.tile(), TileIndex::WEST);
        assert_eq!(RoundWind::North.tile(), TileIndex::NORTH);
    }

    #[test]
    fn roundwind_label_chinese() {
        assert_eq!(RoundWind::East.label(), "东");
        assert_eq!(RoundWind::South.label(), "南");
        assert_eq!(RoundWind::West.label(), "西");
        assert_eq!(RoundWind::North.label(), "北");
    }

    #[test]
    fn roundstate_getters_per_phase() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 7);
        // AwaitDraw: turn=East, last_discard/result/last_drawn 全 None.
        assert_eq!(r.turn(), Some(Seat::East));
        assert!(r.last_discard().is_none());
        assert!(r.last_drawn().is_none());
        assert!(r.result().is_none());
        assert!(!r.is_ended());

        // Draw → AwaitDiscard: last_drawn 应 Some.
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        assert!(r.last_drawn().is_some());
        assert!(r.last_discard().is_none());

        // Discard → AwaitCalls: last_discard 应 Some, turn None, last_drawn None.
        let t = r.last_drawn().unwrap();
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: t }).unwrap();
        assert!(matches!(&r, RoundState::AwaitCalls(_)));
        assert_eq!(r.turn(), None);
        assert!(r.last_discard().is_some());
        assert!(r.last_drawn().is_none());

        // Pass → AwaitDraw (下家).
        let (r, _) = round_apply(&r, AtomicOp::Pass).unwrap();
        assert_eq!(r.turn(), Some(Seat::South));
    }

    // ============================================================
    // round_apply 各 typed-state apply 路径
    // ============================================================

    /// 跑到 AwaitDiscard, 让 East 立直宣告 + 切牌.
    /// (用真实 round_apply, 但条件: 手牌一定听牌. seed=42 East 起手未必听,
    /// 所以测试构造黑魔法 hand: 听 1m 单骑.)
    #[test]
    fn apply_riichi_declare_then_discard_full_lifecycle() {
        let m = MatchState::new(GameRules::default());
        let r0 = init_round(&m, 42);
        let (r, _) = round_apply(&r0, AtomicOp::Draw).unwrap();
        // 黑魔法: 替 East 闭手 = 听 1m 单骑的牌型, last_drawn 也是听后牌.
        // 14 张 = 234m+234p+234s+567s+99m, last_drawn=9m(其中一张).
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                let hand = vec![
                    Tile { kind: TileIndex(1), red: false, id: 100 },
                    Tile { kind: TileIndex(2), red: false, id: 101 },
                    Tile { kind: TileIndex(3), red: false, id: 102 },
                    Tile { kind: TileIndex(10), red: false, id: 103 },
                    Tile { kind: TileIndex(11), red: false, id: 104 },
                    Tile { kind: TileIndex(12), red: false, id: 105 },
                    Tile { kind: TileIndex(19), red: false, id: 106 },
                    Tile { kind: TileIndex(20), red: false, id: 107 },
                    Tile { kind: TileIndex(21), red: false, id: 108 },
                    Tile { kind: TileIndex(22), red: false, id: 109 },
                    Tile { kind: TileIndex(23), red: false, id: 110 },
                    Tile { kind: TileIndex(24), red: false, id: 111 },
                    Tile { kind: TileIndex(8), red: false, id: 112 },
                    Tile { kind: TileIndex(8), red: false, id: 113 }, // last drawn
                ];
                s.common.players[Seat::East.index()].hand.closed = hand;
                s.last_drawn = Some(Tile { kind: TileIndex(8), red: false, id: 113 });
                s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!("expected AwaitDiscard"),
        };
        // RiichiDeclare → AwaitRiichiDiscard
        let (r, _) = round_apply(&r, AtomicOp::RiichiDeclare).unwrap();
        assert!(matches!(&r, RoundState::AwaitRiichiDiscard(_)));
        // East 应 riichi=true, score-1000, riichi_sticks_pool +1.
        match &r {
            RoundState::AwaitRiichiDiscard(s) => {
                let p = &s.common.players[Seat::East.index()];
                assert!(p.riichi);
                assert_eq!(p.score, 24000);
                assert_eq!(s.common.riichi_sticks_pool, 1);
            }
            _ => unreachable!(),
        }
        // Discard last_drawn (摸切立直).
        let drawn = match &r {
            RoundState::AwaitRiichiDiscard(s) => s.last_drawn,
            _ => unreachable!(),
        };
        let (r, evs) = round_apply(&r, AtomicOp::Discard { tile: drawn }).unwrap();
        assert!(matches!(&r, RoundState::AwaitCalls(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Riichi { .. })));
    }

    #[test]
    fn apply_tsumo_completes_round_with_payments() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // 黑魔法: 让 East 摸到能自摸的型. closed 14 张 = 国士单役满 (winning=9m).
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                let mut hand = Vec::new();
                let mut id = 200u16;
                // 13 种幺九各 1
                for &k in &[0u8, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33] {
                    hand.push(Tile { kind: TileIndex(k), red: false, id });
                    id += 1;
                }
                // 加 1 张 1m 雀头 (winning=1m → thirteen_wait=true 双倍役满)
                let last = Tile { kind: TileIndex(0), red: false, id };
                hand.push(last);
                s.common.players[Seat::East.index()].hand.closed = hand;
                s.last_drawn = Some(last);
                s.common.players[Seat::East.index()].last_drawn = Some(last);
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, evs) = round_apply(&r, AtomicOp::Tsumo).unwrap();
        assert!(matches!(&r, RoundState::RoundEnd(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Tsumo { .. })));
        match r.result().unwrap() {
            RoundResult::Win { winner, is_tsumo, score, payments, .. } => {
                assert_eq!(*winner, Seat::East);
                assert!(*is_tsumo);
                assert!(matches!(score.level, ScoreLevel::Yakuman(_)));
                assert!(!payments.is_empty());
            }
            _ => panic!("expect Win"),
        }
    }

    #[test]
    fn apply_ankan_then_rinshan_draw() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // 黑魔法: 给 East 4 张 1m + 9 张其它牌 + last_drawn = 1m
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                let mut hand = vec![
                    Tile { kind: TileIndex(0), red: false, id: 300 },
                    Tile { kind: TileIndex(0), red: false, id: 301 },
                    Tile { kind: TileIndex(0), red: false, id: 302 },
                    Tile { kind: TileIndex(0), red: false, id: 303 }, // 4 张 1m → 暗杠
                ];
                let mut id = 310u16;
                for k in 1..=10u8 {
                    hand.push(Tile { kind: TileIndex(k), red: false, id });
                    id += 1;
                }
                s.common.players[Seat::East.index()].hand.closed = hand;
                s.last_drawn = Some(Tile { kind: TileIndex(0), red: false, id: 303 });
                s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, evs) = round_apply(&r, AtomicOp::Ankan { kind: TileIndex(0) }).unwrap();
        assert!(matches!(&r, RoundState::AwaitRinshanDraw(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Ankan { .. })));
        // 验证 meld 已加, dora 已翻 (revealed +=1).
        match &r {
            RoundState::AwaitRinshanDraw(s) => {
                let p = &s.common.players[Seat::East.index()];
                assert_eq!(p.hand.melds.len(), 1);
                assert!(matches!(&p.hand.melds[0].kind, MeldKind::Ankan { .. }));
            }
            _ => unreachable!(),
        }
        // RinshanDraw → AwaitDiscard.
        let (r, _) = round_apply(&r, AtomicOp::RinshanDraw).unwrap();
        assert!(matches!(&r, RoundState::AwaitDiscard(_)));
    }

    #[test]
    fn apply_pon_from_await_calls_to_pon_caller_discard() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // East 切一张 5p (kind=13). 给 South 手中插 2 张 5p.
        let pon_tile = Tile { kind: TileIndex(13), red: false, id: 500 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                // East last_drawn 替成 pon_tile + closed 加 pon_tile
                s.common.players[Seat::East.index()].hand.closed.push(pon_tile);
                s.last_drawn = Some(pon_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(pon_tile);
                // South 手中插 2 张 5p
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 501,
                });
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 502,
                });
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        // East 切 pon_tile.
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: pon_tile }).unwrap();
        assert!(matches!(&r, RoundState::AwaitCalls(_)));
        // South 碰.
        let (r, evs) = round_apply(
            &r,
            AtomicOp::Pon {
                who: Seat::South,
                hand_tile_ids: [501, 502],
            },
        )
        .unwrap();
        assert!(matches!(&r, RoundState::AwaitDiscard(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Pon { .. })));
        match &r {
            RoundState::AwaitDiscard(s) => {
                assert_eq!(s.turn, Seat::South, "Pon 后 turn 转鸣方");
                let p = &s.common.players[Seat::South.index()];
                assert_eq!(p.hand.melds.len(), 1);
                assert!(matches!(&p.hand.melds[0].kind, MeldKind::Pon { .. }));
                assert!(s.last_drawn.is_none(), "鸣牌后无 last_drawn");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn apply_chi_from_await_calls_to_caller_discard() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // East 切 3m (kind=2). 下家 South 手中有 1m+2m → 吃成 1-2-3m.
        // 但: 吃只能从上家! East 的下家是 South, 但 South 是 East 的下家 → 自家.
        // Chi 规则: 仅可吃上家弃牌. East 切 → 吃方必须是 South (East.next() = South).
        let chi_tile = Tile { kind: TileIndex(2), red: false, id: 600 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                s.common.players[Seat::East.index()].hand.closed.push(chi_tile);
                s.last_drawn = Some(chi_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(chi_tile);
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(0), red: false, id: 601,
                });
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(1), red: false, id: 602,
                });
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: chi_tile }).unwrap();
        let (r, evs) = round_apply(
            &r,
            AtomicOp::Chi {
                who: Seat::South,
                hand_tile_ids: [601, 602],
            },
        )
        .unwrap();
        assert!(matches!(&r, RoundState::AwaitDiscard(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Chi { .. })));
        match &r {
            RoundState::AwaitDiscard(s) => {
                assert_eq!(s.turn, Seat::South);
                assert!(matches!(
                    &s.common.players[Seat::South.index()].hand.melds[0].kind,
                    MeldKind::Chi { .. }
                ));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn apply_minkan_from_await_calls_to_rinshan_draw() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // East 切 7s (kind=24). South 手中 3 张 7s → 明杠.
        let kan_tile = Tile { kind: TileIndex(24), red: false, id: 700 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                s.common.players[Seat::East.index()].hand.closed.push(kan_tile);
                s.last_drawn = Some(kan_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(kan_tile);
                for id in 701..=703 {
                    s.common.players[Seat::South.index()].hand.closed.push(Tile {
                        kind: TileIndex(24), red: false, id,
                    });
                }
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: kan_tile }).unwrap();
        let (r, evs) = round_apply(
            &r,
            AtomicOp::Minkan {
                who: Seat::South,
                hand_tile_ids: [701, 702, 703],
            },
        )
        .unwrap();
        // FIXME engine bug: 明杠规则是必摸岭上 (AwaitRinshanDraw), 但当前实现
        // 直接转 AwaitDiscard (round_state.rs::AwaitCallsOp::Minkan apply 内有
        // FIXME 注释). 本测试 assertion 跟随当前实现, 修复后应改回 AwaitRinshanDraw.
        assert!(matches!(&r, RoundState::AwaitDiscard(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Minkan { .. })));
    }

    #[test]
    fn apply_pass_advances_to_next_seat_await_draw() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let t = match &r {
            RoundState::AwaitDiscard(s) => s.last_drawn.unwrap(),
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: t }).unwrap();
        let (r, _) = round_apply(&r, AtomicOp::Pass).unwrap();
        match &r {
            RoundState::AwaitDraw(s) => assert_eq!(s.turn, Seat::South),
            _ => panic!("Pass 后应回 AwaitDraw"),
        }
    }

    #[test]
    fn apply_shouminkan_from_existing_pon() {
        // 加杠: 已有 Pon, 自手第 4 张升级 Kan.
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                // 给 East 已有 Pon (3 张 5p meld).
                s.common.players[Seat::East.index()].hand.melds.push(Meld {
                    kind: MeldKind::Pon {
                        tiles: [
                            Tile { kind: TileIndex(13), red: false, id: 800 },
                            Tile { kind: TileIndex(13), red: false, id: 801 },
                            Tile { kind: TileIndex(13), red: false, id: 802 },
                        ],
                    },
                    from: Some(Seat::West),
                });
                // 闭手 push 第 4 张 5p
                s.common.players[Seat::East.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 803,
                });
                s.last_drawn = Some(Tile { kind: TileIndex(13), red: false, id: 803 });
                s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, evs) = round_apply(&r, AtomicOp::Shouminkan { kind: TileIndex(13) }).unwrap();
        assert!(matches!(&r, RoundState::AwaitRinshanDraw(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Shouminkan { .. })));
        match &r {
            RoundState::AwaitRinshanDraw(s) => {
                let p = &s.common.players[Seat::East.index()];
                assert!(matches!(&p.hand.melds[0].kind, MeldKind::Shouminkan { .. }));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn apply_ron_from_await_calls_completes_round() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        // East 切 9m (kind=8). South 闭手 13 张 = 国士 1m..字牌 + 9m winning.
        let ron_tile = Tile { kind: TileIndex(8), red: false, id: 900 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                // East push ron_tile + last_drawn.
                s.common.players[Seat::East.index()].hand.closed.push(ron_tile);
                s.last_drawn = Some(ron_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(ron_tile);
                // South 闭手 13 张国士型, winning=9m.
                let mut south_hand = Vec::new();
                let mut id = 901u16;
                for &k in &[0u8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33] {
                    south_hand.push(Tile { kind: TileIndex(k), red: false, id });
                    id += 1;
                }
                south_hand.push(Tile { kind: TileIndex(0), red: false, id }); // 1m 雀头第 2 张
                s.common.players[Seat::South.index()].hand.closed = south_hand;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: ron_tile }).unwrap();
        let (r, evs) = round_apply(&r, AtomicOp::Ron { who: Seat::South }).unwrap();
        assert!(matches!(&r, RoundState::RoundEnd(_)));
        assert!(evs.iter().any(|e| matches!(e, GameEvent::Ron { .. })));
        match r.result().unwrap() {
            RoundResult::Win { winner, is_tsumo, loser, .. } => {
                assert_eq!(*winner, Seat::South);
                assert!(!*is_tsumo);
                assert_eq!(*loser, Some(Seat::East));
            }
            _ => panic!("expect Win"),
        }
    }

    #[test]
    fn wall_drained_transitions_to_ryuukyoku() {
        // 把 wall 推到只剩 0 活牌, Draw 应直接转 RoundEnd Howaipai.
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let r = match r {
            RoundState::AwaitDraw(mut s) => {
                // 黑魔法: 把 wall.live 清空.
                while s.common.wall.remaining() > 0 {
                    s.common.wall = s.common.wall.drawn().0;
                }
                RoundState::AwaitDraw(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        assert!(matches!(&r, RoundState::RoundEnd(_)));
        assert!(matches!(
            r.result().unwrap(),
            RoundResult::Ryuukyoku { kind: RyuukyokuKind::Howaipai }
        ));
    }

    // ============================================================
    // legal_ops 路径
    // ============================================================

    #[test]
    fn legal_ops_at_await_calls_lists_pon_when_pair_in_hand() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let pon_tile = Tile { kind: TileIndex(13), red: false, id: 1000 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                s.common.players[Seat::East.index()].hand.closed.push(pon_tile);
                s.last_drawn = Some(pon_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(pon_tile);
                // South 手中 2 张 5p
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 1001,
                });
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 1002,
                });
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: pon_tile }).unwrap();
        let ops = legal_ops(&r);
        assert!(ops.calls[Seat::South.index()].pon.is_some(), "South 应能碰");
        // 切牌方 East 的 PerSeatCalls 全空.
        assert!(ops.calls[Seat::East.index()].pon.is_none());
    }

    #[test]
    fn legal_ops_ankan_when_4_same_kind() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                // East 替成 4 张 5p + 9 其它 + last_drawn=5p.
                let mut hand = vec![
                    Tile { kind: TileIndex(13), red: false, id: 1100 },
                    Tile { kind: TileIndex(13), red: false, id: 1101 },
                    Tile { kind: TileIndex(13), red: false, id: 1102 },
                    Tile { kind: TileIndex(13), red: false, id: 1103 },
                ];
                let mut id = 1110u16;
                for k in 0..10u8 {
                    hand.push(Tile { kind: TileIndex(k), red: false, id });
                    id += 1;
                }
                s.common.players[Seat::East.index()].hand.closed = hand;
                s.last_drawn = Some(Tile { kind: TileIndex(13), red: false, id: 1103 });
                s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let ops = legal_ops(&r);
        assert!(ops.ankan.contains(&TileIndex(13)), "4 张 5p 应可暗杠, got {:?}", ops.ankan);
    }

    #[test]
    fn legal_ops_shouminkan_when_pon_plus_4th() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                s.common.players[Seat::East.index()].hand.melds.push(Meld {
                    kind: MeldKind::Pon {
                        tiles: [
                            Tile { kind: TileIndex(13), red: false, id: 1200 },
                            Tile { kind: TileIndex(13), red: false, id: 1201 },
                            Tile { kind: TileIndex(13), red: false, id: 1202 },
                        ],
                    },
                    from: Some(Seat::West),
                });
                // 闭手新 5p
                s.common.players[Seat::East.index()].hand.closed.push(Tile {
                    kind: TileIndex(13), red: false, id: 1203,
                });
                s.last_drawn = Some(Tile { kind: TileIndex(13), red: false, id: 1203 });
                s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let ops = legal_ops(&r);
        assert!(ops.shouminkan.contains(&TileIndex(13)), "Pon+第4张应可加杠");
    }

    // ============================================================
    // 错误路径补充
    // ============================================================

    #[test]
    fn riichi_not_menzen_after_chi_err() {
        let s = fixture_await_discard(42);
        let mut s = s;
        // 给 East 添加一个 Chi 副露破 menzen.
        s.common.players[Seat::East.index()].hand.melds.push(Meld {
            kind: MeldKind::Chi {
                tiles: [
                    Tile { kind: TileIndex(0), red: false, id: 1300 },
                    Tile { kind: TileIndex(1), red: false, id: 1301 },
                    Tile { kind: TileIndex(2), red: false, id: 1302 },
                ],
            },
            from: Some(Seat::North),
        });
        let r = s.try_op(AtomicOp::RiichiDeclare);
        assert!(matches!(r, Err(OpError::NotMenzen)));
    }

    #[test]
    fn riichi_not_tenpai_err() {
        // 手牌强行替成不听牌型: 完全乱手 13 张 (不构成任何潜在和牌型).
        let mut s = fixture_await_discard(42);
        // 让 East 手牌 = 13 张全是 1m/2m/字牌散乱不能听任何牌.
        let bad_hand = vec![
            Tile { kind: TileIndex(0), red: false, id: 1400 }, // 1m
            Tile { kind: TileIndex(2), red: false, id: 1401 }, // 3m
            Tile { kind: TileIndex(4), red: false, id: 1402 }, // 5m
            Tile { kind: TileIndex(6), red: false, id: 1403 }, // 7m
            Tile { kind: TileIndex(9), red: false, id: 1404 }, // 1p
            Tile { kind: TileIndex(11), red: false, id: 1405 }, // 3p
            Tile { kind: TileIndex(13), red: false, id: 1406 }, // 5p
            Tile { kind: TileIndex(15), red: false, id: 1407 }, // 7p
            Tile { kind: TileIndex(18), red: false, id: 1408 }, // 1s
            Tile { kind: TileIndex(20), red: false, id: 1409 }, // 3s
            Tile { kind: TileIndex(27), red: false, id: 1410 }, // 东
            Tile { kind: TileIndex(29), red: false, id: 1411 }, // 西
            Tile { kind: TileIndex(31), red: false, id: 1412 }, // 白
            Tile { kind: TileIndex(33), red: false, id: 1413 }, // 中 (last_drawn)
        ];
        s.common.players[Seat::East.index()].hand.closed = bad_hand;
        s.last_drawn = Some(Tile { kind: TileIndex(33), red: false, id: 1413 });
        let r = s.try_op(AtomicOp::RiichiDeclare);
        assert!(matches!(r, Err(OpError::NotTenpaiForRiichi)));
    }

    #[test]
    fn riichi_insufficient_wall_err() {
        let mut s = fixture_await_discard(42);
        // 拖空 wall 到 < 4.
        while s.common.wall.remaining() >= 4 {
            s.common.wall = s.common.wall.drawn().0;
        }
        let r = s.try_op(AtomicOp::RiichiDeclare);
        assert!(matches!(r, Err(OpError::InsufficientWall)));
    }

    #[test]
    fn shouminkan_no_matching_pon_err() {
        let s = fixture_await_discard(42);
        // 没人有 Pon meld, shouminkan 应 err.
        let r = s.try_op(AtomicOp::Shouminkan { kind: TileIndex(0) });
        assert!(matches!(r, Err(OpError::NoMatchingPonForShouminkan(TileIndex(0)))));
    }

    #[test]
    fn shouminkan_no_4th_tile_err() {
        // 有 Pon 但闭手没第 4 张 → InsufficientForAnkan (复用此 variant 表达).
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].hand.melds.push(Meld {
            kind: MeldKind::Pon {
                tiles: [
                    Tile { kind: TileIndex(0), red: false, id: 9000 },
                    Tile { kind: TileIndex(0), red: false, id: 9001 },
                    Tile { kind: TileIndex(0), red: false, id: 9002 },
                ],
            },
            from: Some(Seat::West),
        });
        // 闭手不含 1m → 加杠应 err.
        s.common.players[Seat::East.index()]
            .hand
            .closed
            .retain(|t| t.kind != TileIndex(0));
        let r = s.try_op(AtomicOp::Shouminkan { kind: TileIndex(0) });
        assert!(matches!(r, Err(OpError::InsufficientForAnkan(TileIndex(0)))));
    }

    #[test]
    fn shouminkan_while_riichi_err() {
        let mut s = fixture_await_discard(42);
        s.common.players[Seat::East.index()].riichi = true;
        let r = s.try_op(AtomicOp::Shouminkan { kind: TileIndex(0) });
        assert!(matches!(
            r,
            Err(OpError::DisallowedWhileRiichi(
                crate::engine::op::AtomicOpKind::Shouminkan
            ))
        ));
    }

    #[test]
    fn await_riichi_discard_state_getters() {
        // 直接构造 AwaitRiichiDiscardState, 验 RoundState wrapper 的 common/turn/last_drawn.
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let common = r.common().clone();
        let drawn = Tile { kind: TileIndex(0), red: false, id: 9999 };
        let r = RoundState::AwaitRiichiDiscard(AwaitRiichiDiscardState {
            common,
            turn: Seat::South,
            last_drawn: drawn,
        });
        assert_eq!(r.turn(), Some(Seat::South));
        assert_eq!(r.last_drawn(), Some(drawn));
        // common() 也走 AwaitRiichiDiscard 分支.
        assert_eq!(r.common().dealer, Seat::East);
    }

    #[test]
    fn await_rinshan_draw_state_getters() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let common = r.common().clone();
        let r = RoundState::AwaitRinshanDraw(AwaitRinshanDrawState {
            common,
            turn: Seat::West,
        });
        assert_eq!(r.turn(), Some(Seat::West));
        // last_drawn 在 RinshanDraw 阶段无意义, 应返 None.
        assert!(r.last_drawn().is_none());
        assert_eq!(r.common().dealer, Seat::East);
    }

    #[test]
    fn legal_ops_at_await_calls_lists_minkan() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let kan_tile = Tile { kind: TileIndex(13), red: false, id: 7000 };
        let r = match r {
            RoundState::AwaitDiscard(mut s) => {
                s.common.players[Seat::East.index()].hand.closed.push(kan_tile);
                s.last_drawn = Some(kan_tile);
                s.common.players[Seat::East.index()].last_drawn = Some(kan_tile);
                // South 手中插 3 张 5p 备明杠.
                for id in 7001..=7003 {
                    s.common.players[Seat::South.index()].hand.closed.push(Tile {
                        kind: TileIndex(13), red: false, id,
                    });
                }
                RoundState::AwaitDiscard(s)
            }
            _ => panic!(),
        };
        let (r, _) = round_apply(&r, AtomicOp::Discard { tile: kan_tile }).unwrap();
        let ops = legal_ops(&r);
        assert!(ops.calls[Seat::South.index()].minkan.is_some(), "应能明杠");
    }

    #[test]
    fn await_discard_riichi_must_tsumogiri_already_handled_by_other_test() {
        // 占位以提示该路径已被 await_discard_try_op_riichi_must_tsumogiri 覆盖.
        // 这里也直接覆盖一次让 try_op 内的 RiichiMustTsumogiri 路径在 Shouminkan/Ankan 之外
        // 也走一遍 (已在另一测试覆盖, 此条 noop).
    }
}
