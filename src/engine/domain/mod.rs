//! 领域层 — 静态类型 + 纯算法.
//!
//! 这一层不持有可变状态, 不依赖任何业务模块 (engine/ai/net/ui).
//! 任何模块都可依赖 domain.
//!
//! - [`tile`]      牌张定义 (34 种, 每种 4 张, 加 3 张赤牌)
//! - [`meld`]      副露 (吃/碰/明杠/加杠/暗杠) + Seat
//! - [`hand`]      手牌容器
//! - [`action`]    玩家可执行的动作
//! - [`decompose`] 和牌型拆解 (4 面 1 雀头 / 七对子 / 国士)
//! - [`yaku`]      役种判定 (含古役)

pub mod action;
pub mod decompose;
pub mod hand;
pub mod meld;
pub mod tile;
pub mod yaku;
