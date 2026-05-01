//! 多人对战 (LAN) 模块.
//!
//! - [`protocol`] 网络协议消息定义 (client ↔ server)
//! - [`client`]   transport 抽象 + LocalTransport (mpsc 同进程, 房主自己用)
//! - 后续 phase 加: server, room, discovery, ai_seat

pub mod client;
pub mod protocol;
pub mod room;
