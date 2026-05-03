//! 多人对战模块.
//!
//! - [`protocol`] 网络协议消息定义 (client ↔ server).
//! - [`session`]  NetSession — 房主/远程 client 统一视角.
//! - [`room`]     RoomActor — 持权威 GameState + 处理玩家命令 (标准模式).
//! - [`mp`]       零信任模式 per-player actor (MpPlayerActor) — M5.B 起.
//! - [`p2p`]      libp2p 实现: mDNS 发现 + QUIC/TCP listen/dial + identify
//!   房间 metadata + request-response 消息收发.

pub mod mp;
pub mod p2p;
pub mod protocol;
pub mod room;
pub mod session;
