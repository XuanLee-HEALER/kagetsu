//! tui-majo: 终端日本麻将
//!
//! 模块概览:
//! - [`tile`]      牌张定义 (34 种, 每种 4 张, 加 3 张赤牌)
//! - [`meld`]      副露 (吃/碰/明杠/加杠/暗杠)
//! - [`hand`]      手牌容器
//! - [`wall`]      牌山 + 王牌 + dora
//! - [`decompose`] 和牌型拆解 (4面1雀头 / 七对子 / 国士)
//! - [`yaku`]      役种判定 (含古役)
//! - [`score`]     番符与点数计算
//! - [`action`]    玩家可执行的动作
//! - [`config`]    游戏规则配置
//! - [`game`]      游戏状态机
//! - [`player`]    玩家与 AI 接口
//! - [`ui`]        ratatui 渲染层

pub mod action;
pub mod config;
pub mod decompose;
pub mod game;
pub mod hand;
pub mod meld;
pub mod net;
pub mod player;
pub mod score;
pub mod tile;
pub mod ui;
pub mod wall;
pub mod yaku;
