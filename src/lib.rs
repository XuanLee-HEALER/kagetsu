//! tui-majo: 终端日本麻将
//!
//! 模块概览 (重构进行中, 见 docs/discuss-module-refactor.md):
//! - [`domain`]  ✓ 静态类型 + 纯算法 (tile/meld/hand/action/decompose/yaku)
//! - [`engine`]  状态机 (C3 之后建立, 当前 wall/game/score 仍平铺)
//! - [`config`]  软件级用户偏好 (主题等, 持久化 prefs.toml)
//! - [`ai`]      AI 决策 (C5 之后建立, 当前 player 仍平铺)
//! - [`net`]     libp2p 网络层
//! - [`ui`]      ratatui 渲染层

pub mod config;
pub mod domain;
pub mod game;
pub mod net;
pub mod player;
pub mod score;
pub mod ui;
pub mod wall;
