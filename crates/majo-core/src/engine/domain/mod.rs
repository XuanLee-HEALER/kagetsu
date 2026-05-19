//! 领域层 — 静态类型 + 纯算法.
//!
//! 不持有可变状态, 不依赖任何业务模块. domain 是日麻规则的"静态部分":
//! 牌长什么样, 怎么组成手牌, 哪些组合算面子, 哪些组合得役 — 不涉及"局怎么走".
//!
//! 任何 engine 子模块 (含 round_state / match_state / score) 都可依赖 domain,
//! domain 内部不能反向依赖.
//!
//! # 子模块
//!
//! - [`tile`]      — 牌张定义 (34 种 [`TileIndex`], 每种 4 张 [`Tile`], 加 3 张赤五)
//! - [`meld`]      — 座位 [`Seat`] (4 家) + 副露 [`Meld`] (吃 / 碰 / 杠 三种)
//! - [`hand`]      — 手牌容器 [`Hand`] (闭手 + 副露)
//! - [`action`]    — [`Action`] enum: UI / AI 决策中间表示 (会翻成 AtomicOp)
//! - [`decompose`] — 和牌型分解算法 (4 面子+雀头 / 七对子 / 国士无双)
//! - [`yaku`]      — 役种判定 ([`Yaku`] enum + [`detect_yaku`])
//!
//! [`Tile`]: tile::Tile
//! [`TileIndex`]: tile::TileIndex
//! [`Seat`]: meld::Seat
//! [`Meld`]: meld::Meld
//! [`Hand`]: hand::Hand
//! [`Action`]: action::Action
//! [`Yaku`]: yaku::Yaku
//! [`detect_yaku`]: yaku::detect_yaku

pub mod action;
pub mod decompose;
pub mod hand;
pub mod meld;
pub mod tile;
pub mod yaku;
