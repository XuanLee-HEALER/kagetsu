//! libp2p 网络层.
//!
//! ## 架构
//!
//! ```text
//!   房主进程              加入者进程
//!   ──────                ──────
//!   Swarm                 Swarm
//!     ├─ identify           ├─ identify
//!     ├─ mdns (LAN)         ├─ mdns (LAN)
//!     ├─ autonat            ├─ autonat
//!     ├─ relay-server       ├─ relay-client
//!     ├─ dcutr              ├─ dcutr
//!     ├─ rr_c2s (in)        ├─ rr_c2s (out)
//!     └─ rr_s2c (out)       └─ rr_s2c (in)
//! ```
//!
//! - QUIC + TCP 双 transport, 优先 QUIC
//! - mDNS 发现局域网 peer (替代 mdns-sd)
//! - identify 协议 agent_version 字段携带房间 metadata
//! - request-response 双 Behaviour, 协议 `/tui-majo/c2s/1` 和 `/tui-majo/s2c/1`
//!   每条消息一个 substream, response 是空 ack
//! - 房主同时启用 relay-server (Tier 3 桌内 peer relay), 加入者启 relay-client + dcutr
//!
//! ## 子模块
//!
//! - [`behaviour`] P2pBehaviour 复合 Behaviour 定义
//! - [`swarm`]     build_swarm() 构建 Swarm + transport
//! - [`host`]      房主端: spawn_p2p_listener, RoomCmd 桥接
//! - [`join`]      加入者端: join_remote, NetSession 桥接
//! - [`discovery`] 房间发现: RoomEntry / RoomBrowser / RoomAdvertiser

pub mod behaviour;
pub mod bootstrap;
pub mod discovery;
pub mod host;
pub mod join;
pub mod swarm;

pub use discovery::{RoomBrowser, RoomEntry};
pub use host::spawn_p2p_listener;
pub use join::join_remote;
