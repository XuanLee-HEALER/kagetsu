//! 零信任模式 (ZeroTrust) 网络协议层 — Mental Poker per-player actor (M5.B).
//!
//! ## 架构跟 LAN/Standard 的区别
//!
//! ### Standard (现有 [`crate::net::room::RoomActor`])
//! ```text
//! Player 0 (host)         Player 1, 2, 3
//!   ┌───────────────┐       ┌──────────┐
//!   │  RoomActor    │ ◄──── │ Client   │
//!   │  GameState    │  ws   │ (UI only)│
//!   │  (authority)  │ ────► │          │
//!   └───────────────┘       └──────────┘
//! ```
//! 房主权威, 客户端只是 UI. 玩家信任房主不作弊.
//!
//! ### ZeroTrust (本模块 [`MpPlayerActor`])
//! ```text
//! Player 0          Player 1          Player 2          Player 3
//! ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
//! │MpPlayer  │◄──►│MpPlayer  │◄──►│MpPlayer  │◄──►│MpPlayer  │
//! │ Actor    │ P2P│ Actor    │ P2P│ Actor    │ P2P│ Actor    │
//! │ +sk_0    │    │ +sk_1    │    │ +sk_2    │    │ +sk_3    │
//! │ +mirror  │    │ +mirror  │    │ +mirror  │    │ +mirror  │
//! │  Table   │    │  Table   │    │  Table   │    │  Table   │
//! └──────────┘     └──────────┘     └──────────┘     └──────────┘
//! ```
//! 4 个对等 actor, 各持自己 sk_i + 本地 GameState 镜像. 任一方对状态
//! 不一致即检测作弊. 协议 1-7 (mental poker) 驱动同步.
//!
//! ## 模块结构
//! - [`actor`] MpPlayerActor 主 actor + run loop
//! - [`cmd`] MpRoomCmd / MpEvent (actor 边界消息)
//! - [`phase`] MpPhase 阶段枚举 + transition 规则
//!
//! ## 当前 commit (M5.B.3) 范围
//! 仅架构 scaffold + lifecycle 基础. 具体协议集成留后续:
//! - M5.B.4 协议 0/1 (keygen + 联合洗牌)
//! - M5.B.5 协议 2/3 (摸牌 + 揭示)
//! - M5.B.6 MpGameState + 协议 4-7
//! - M5.B.7 4 actor 本地 mpsc e2e
//! - M5.B.8 P2P libp2p 桥接
//! - M5.B.9 UI 接入

pub mod actor;
pub mod cmd;
pub mod phase;

pub use actor::{MpPlayerActor, MpPlayerHandle, spawn_mp_player};
pub use cmd::{MpEvent, MpRoomCmd};
pub use phase::MpPhase;
