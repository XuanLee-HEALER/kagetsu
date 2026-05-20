//! 日麻 (Riichi Mahjong) 计算库 — pure functional, 零外部感知.
//!
//! engine 是 `kagetsu` 的核心计算层, 给外部 driver (单机 UI / AI / 网络对局 /
//! 录像 replay) 消费. **engine 不知道任何外部实体存在** — 没有 logging, 没有
//! 副作用, 不引用 ui/ai/net/recorder 任何模块. 行为像 SQLite 给 SQL 客户端用.
//!
//! # 三层 fold 抽象
//!
//! 一个完整比赛 = 三层 fold:
//!
//! ```text
//! match_state = ROUNDS.fold(match_apply, init_match)        // 庄层 (Match)
//! round_state = OPS.try_fold(round_apply, init_round)        // 局层 (Round)
//! atomic_op = 局内单一不可分动作 (Discard / Pon / Tsumo / etc.)  // 操作层
//! ```
//!
//! - **庄** (Match / 半庄 / 东风战): 一次完整比赛, 含多局. 见 [`match_state`].
//! - **局** (Round / 局 / 一局): 从配牌到和了/流局. 见 [`round_state`].
//! - **算子** (AtomicOp / 操作): driver 给 engine 喂的动作单元. 见 [`op`].
//!
//! # 快速上手
//!
//! ```ignore
//! use kagetsu_tui::engine::{
//!     match_state::{MatchState, match_apply},
//!     round_state::{init_round, round_apply, summarize_round, RoundState, legal_ops},
//!     op::AtomicOp,
//!     rules::GameRules,
//! };
//!
//! // 1. 起庄 (整场比赛初始化)
//! let mut mat = MatchState::new(GameRules::default());
//!
//! while !mat.ended {
//!     // 2. 起一局 (配牌)
//!     let mut round = init_round(&mat, /* seed */ 0xc0ffee);
//!
//!     // 3. 推动局内状态机直到 RoundEnd
//!     while !round.is_ended() {
//!         let legal = legal_ops(&round);
//!         let op: AtomicOp = your_decide(&round, &legal);
//!         let (next, _events) = round_apply(&round, op).expect("op valid");
//!         round = next;
//!     }
//!
//!     // 4. 庄层推进 (更新分数 / 庄家 / 本场 / 检测整庄结束)
//!     let outcome = summarize_round(&round).unwrap();
//!     mat = match_apply(&mat, outcome);
//! }
//!
//! // 5. 整庄结束, 算最终排名
//! let rankings = kagetsu_tui::engine::score::final_ranking(/* ... */);
//! ```
//!
//! # 错误模型
//!
//! engine 内部计算 *不会失败* (所有算法是 total function). 唯一的失败路径是
//! [`OpError`] — caller 喂的 [`AtomicOp`] 在当前 [`RoundState`] 下违反规则.
//! 这是 *输入合法性裁定*, 不是 bug.
//!
//! # 子模块
//!
//! ## 核心 API (driver 直接消费)
//!
//! - [`op`] — [`AtomicOp`] + [`OpError`]: engine 唯一的输入面
//! - [`round_state`] — [`RoundState`] + [`round_apply`] + [`legal_ops`]: 局层
//! - [`match_state`] — [`MatchState`] + [`match_apply`]: 庄层
//! - [`score`] — 评分 + 点数分配 + 终局排名
//! - [`event`] — [`GameEvent`]: round_apply emit 的事件流
//!
//! ## 基础类型 (domain)
//!
//! - [`domain::tile`] — [`Tile`] / [`TileIndex`] / [`Suit`]: 牌
//! - [`domain::meld`] — [`Seat`] / [`Meld`] / [`MeldKind`]: 座位 + 副露
//! - [`domain::hand`] — [`Hand`]: 手牌容器
//! - [`domain::action`] — [`Action`]: UI/AI 决策中间表示
//! - [`domain::decompose`] — 牌型分解 (4 面子+雀头 / 七对子 / 国士)
//! - [`domain::yaku`] — 役 (Yaku) + 役牌种 + WinContext
//!
//! ## 配置 / 杂项
//!
//! - [`rules`] — [`GameRules`] (整庄规则参数, 开局 freeze)
//! - [`wall`] — [`Wall`] (牌山 + 死墙 + 宝牌)
//! - [`player`] — [`PlayerState`] (单家局内状态)
//! - [`phase`] — [`Phase`] (legacy 4-phase 表示, UI 用)
//!
//! [`AtomicOp`]: op::AtomicOp
//! [`OpError`]: op::OpError
//! [`RoundState`]: round_state::RoundState
//! [`MatchState`]: match_state::MatchState
//! [`round_apply`]: round_state::round_apply
//! [`match_apply`]: match_state::match_apply
//! [`legal_ops`]: round_state::legal_ops
//! [`GameEvent`]: event::GameEvent
//! [`Tile`]: domain::tile::Tile
//! [`TileIndex`]: domain::tile::TileIndex
//! [`Suit`]: domain::tile::Suit
//! [`Seat`]: domain::meld::Seat
//! [`Meld`]: domain::meld::Meld
//! [`MeldKind`]: domain::meld::MeldKind
//! [`Hand`]: domain::hand::Hand
//! [`Action`]: domain::action::Action
//! [`GameRules`]: rules::GameRules
//! [`Wall`]: wall::Wall
//! [`PlayerState`]: player::PlayerState
//! [`Phase`]: phase::Phase

pub mod domain;
pub mod event;
pub mod match_state;
pub mod op;
pub mod phase;
pub mod player;
pub mod round_state;
pub mod rules;
pub mod score;
pub mod wall;

// 公开 re-export — 外部 (ai / ui / net / dev) 直接写 `kagetsu_tui::engine::Tile` 等,
// 不暴露 engine::domain:: 路径细节.
pub use domain::action::Action;
pub use domain::hand::Hand;
pub use domain::meld::{Meld, MeldKind, Seat};
pub use domain::tile::{Tile, TileIndex};
pub use domain::yaku::{Yaku, YakuhaiKind};
pub use match_state::{MatchState, RoundOutcome, match_apply};
pub use op::{AtomicOp, AtomicOpKind, OpError, PhaseKind};
pub use player::PlayerState;
pub use round_state::{
    AwaitCallsState, AwaitDiscardState, AwaitDrawState, AwaitRiichiDiscardState,
    AwaitRinshanDrawState, CommonRound, LegalOps, PerSeatCalls, RoundEndState, RoundResult,
    RoundState, RoundWind, RyuukyokuKind, init_round, legal_ops, round_apply, summarize_round,
};
