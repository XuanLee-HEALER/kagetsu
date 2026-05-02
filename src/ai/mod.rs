//! AI 决策 + 玩家控制类型.
//!
//! 当前 AI 是占位实现 (摸切 + 能和就和), 足够推动游戏运行.
//! 后续可加更复杂策略 (牌效率/防守/听牌选择等), 按子模块组织:
//! `dummy.rs` (当前) / `mcts.rs` / `defensive.rs` / `tenhou_clone.rs` ...
//!
//! - [`controller`] Controller enum (Human / DummyAi)
//! - [`dummy`]      占位 AI 决策 (摸切 / 荣和 / 不鸣牌)
//! - [`timeout`]    超时默认动作 (与 AI 区分: 超时不自动和)

pub mod controller;
pub mod dummy;
pub mod timeout;
