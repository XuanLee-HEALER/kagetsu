//! 计算库 — domain 类型 + 状态机 + 计分. 零外部感知 (不知道 ui/ai/net/recorder 存在).
//!
//! 子模块:
//! - [`domain`] 静态类型 + 纯算法 (tile/meld/hand/action/decompose/yaku)
//! - [`rules`]  GameRules — 一庄规则参数 (开局 freeze)
//! - [`wall`]   Wall — 牌山 + 王牌 + dora
//! - [`state`]  GameState + PlayerState + RoundResult
//! - [`phase`]  Phase — 状态机阶段
//! - [`event`]  GameEvent — 局内动作事件
//! - [`score`]  番符计算 + 点数分配

pub mod domain;
pub mod event;
pub mod match_state;
pub mod op;
pub mod phase;
pub mod round_state;
pub mod rules;
pub mod score;
pub mod state;
pub mod wall;

// 公开 re-export — 外部 (ai / ui / net / dev) 直接写 `tui_majo::engine::Tile` 等,
// 不暴露 engine::domain:: 路径细节.
pub use domain::action::Action;
pub use domain::hand::Hand;
pub use domain::meld::{Meld, MeldKind, Seat};
pub use domain::tile::{Tile, TileIndex};
pub use domain::yaku::{Yaku, YakuhaiKind};
pub use match_state::{MatchState, RoundOutcome, match_apply};
pub use op::{AtomicOp, AtomicOpKind, OpError, PhaseKind};
pub use round_state::{
    AwaitCallsState, AwaitDiscardState, AwaitDrawState, AwaitRiichiDiscardState,
    AwaitRinshanDrawState, CommonRound, LegalOps, PerSeatCalls, RoundEndState, RoundState,
    init_round, legal_ops, round_apply, summarize_round,
};
