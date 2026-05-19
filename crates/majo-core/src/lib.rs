//! majo-core: 终端日本麻将的非渲染逻辑层.
//!
//! 拆分自原 `tui-majo` 单 crate, 不依赖 ratatui / crossterm. 同时被 `tui-majo`
//! (TUI 客户端) 和 `web-majo` (web 节点) 复用.
//!
//! 模块概览:
//! - [`engine`]      计算库 (pure functional: domain + RoundState/MatchState 状态机 + score)
//! - [`game_engine`] UI/AI/net 共用的 engine 包装 (持 RoundState + MatchState, 累积 events)
//! - [`config`]      软件级用户偏好 (主题等, 持久化 prefs.toml)
//! - [`ai`]          AI 决策 + 超时默认 + Controller 类型
//! - [`mental_poker`] 零信任模式底层密码学
//! - [`net`]         libp2p 网络层 + 协议消息

pub mod ai;
pub mod config;
#[cfg(feature = "dev-tools")]
pub mod dev;
pub mod engine;
pub mod game_engine;
pub mod mental_poker;
pub mod net;
