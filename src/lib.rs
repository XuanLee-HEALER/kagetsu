//! tui-majo: 终端日本麻将
//!
//! 模块概览:
//! - [`domain`] 静态类型 + 纯算法 (tile/meld/hand/action/decompose/yaku)
//! - [`engine`] 状态机 (rules/wall/state/phase/event/score)
//! - [`config`] 软件级用户偏好 (主题等, 持久化 prefs.toml)
//! - [`ai`]     AI 决策 + 超时默认 + Controller 类型
//! - [`net`]    libp2p 网络层
//! - [`ui`]     ratatui 渲染层

pub mod ai;
pub mod config;
pub mod domain;
pub mod engine;
pub mod net;
pub mod ui;
