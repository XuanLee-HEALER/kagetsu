//! 状态机层 — GameState + 规则参数 + 计分.
//!
//! 依赖 [`crate::domain`] (类型 + 算法), 不依赖 ai/net/ui/config.
//!
//! - [`rules`] GameRules — 一庄规则参数 (开局 freeze)
//! - [`wall`]  Wall — 牌山 + 王牌 + dora
//! - [`state`] GameState + PlayerState + RoundResult (主体)
//! - [`phase`] Phase — 状态机阶段
//! - [`event`] GameEvent — 局内动作日志 (UI 用)
//! - [`score`] 番符计算 + 点数分配

pub mod event;
pub mod phase;
pub mod rules;
pub mod score;
pub mod state;
pub mod wall;
