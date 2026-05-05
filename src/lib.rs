//! tui-majo: 终端日本麻将
//!
//! 模块概览:
//! - [`engine`] 计算库 (含 domain 静态类型 + state 状态机 + score 计分等)
//! - [`config`] 软件级用户偏好 (主题等, 持久化 prefs.toml)
//! - [`ai`]     AI 决策 + 超时默认 + Controller 类型
//! - [`mental_poker`] 零信任模式底层密码学 (M4 起)
//! - [`net`]    libp2p 网络层
//! - [`ui`]     ratatui 渲染层

pub mod ai;
pub mod config;
#[cfg(feature = "dev-tools")]
pub mod dev;
pub mod engine;
/// Legacy GameState — 已从 engine 剥离, 留作过渡: 外部代码 (UI / net /
/// ai / recorder) 仍引用旧 GameState 类型. 阶段 6 各调用方迁移到
/// RoundState/MatchState 后此模块整体删除.
#[deprecated(note = "Use crate::engine::RoundState / MatchState instead.")]
pub mod legacy_state;
pub mod mental_poker;
pub mod net;
pub mod ui;
