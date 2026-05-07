//! P2pBehaviour — 复合 NetworkBehaviour, 集中所有协议.
//!
//! 子 Behaviour 列表:
//! - identify: 节点身份交换 + 房间 metadata (通过 agent_version)
//! - mdns: LAN 发现
//! - autonat (client): 探测自己是否公网可达
//! - relay-server: 接受被中转流量 (房主公网时启用)
//! - relay-client: 通过 relay 连出去 (NAT 后玩家)
//! - dcutr: relay 升级为直连
//! - gossipsub: 在线大厅广播 — 房主 publish RoomMetadata, 客户端订阅
//! - rr_c2s: client → server 单向消息 (ClientMsg 作为 request, Ack 作为 response)
//! - rr_s2c: server → client 单向消息 (ServerMsg 作为 request, Ack 作为 response)

use libp2p::{
    PeerId, StreamProtocol, autonat, dcutr, gossipsub, identify, mdns, relay,
    request_response::{self, ProtocolSupport, cbor},
    swarm::NetworkBehaviour,
};
use serde::{Deserialize, Serialize};

use crate::mental_poker::wire::MentalPokerMsg;
use crate::net::p2p::mode::RoomMode;
use crate::net::p2p::region::Region;
use crate::net::protocol::{ClientMsg, ServerMsg};

/// 协议名: client → server.
pub const PROTOCOL_C2S: StreamProtocol = StreamProtocol::new("/tui-majo/c2s/1");
/// 协议名: server → client.
pub const PROTOCOL_S2C: StreamProtocol = StreamProtocol::new("/tui-majo/s2c/1");
/// 协议名: ZeroTrust 模式 P2P 对等通信 (M5.B.8). 用于 unicast (DrawShareRequest /
/// Response, ConcealedKanReveal). broadcast 走 gossipsub mp_topic.
pub const PROTOCOL_MP: StreamProtocol = StreamProtocol::new("/tui-majo/mp/1");
/// identify 协议 agent_version 前缀, 后跟房间 metadata k=v;k=v 字符串.
pub const AGENT_PREFIX: &str = "tui-majo/";
/// gossipsub topic: 在线房间广播.
pub const LOBBY_TOPIC: &str = "tui-majo/lobby/v1";
/// gossipsub topic 前缀: ZeroTrust 模式 mental poker broadcast (M5.B.8).
/// 完整 topic = `tui-majo/mp/<room_id>/v1`, 4 玩家在 MpStart 后订阅.
pub const MP_TOPIC_PREFIX: &str = "tui-majo/mp/";
/// gossipsub topic: Tier 2 玩家 relay 贡献池广播.
///
/// 公网可达 (AutoNAT 探测确认 Public) 的房主在 host swarm 周期 publish
/// 自己的可 dial multiaddr, 让 NAT 后玩家把它们当作候选 relay (除 Tier 1
/// claw 之外的备选, 减少单点依赖).
///
/// 大厅 [`crate::net::p2p::discovery::RoomBrowser`] 订阅累积成 relay pool,
/// UI 创建房间时合并进 bootstrap_relays.
pub const RELAYS_TOPIC: &str = "tui-majo/relays/v1";

#[derive(NetworkBehaviour)]
pub struct P2pBehaviour {
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub autonat: autonat::Behaviour,
    pub relay_server: relay::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub gossipsub: gossipsub::Behaviour,
    pub rr_c2s: cbor::Behaviour<ClientMsg, Ack>,
    pub rr_s2c: cbor::Behaviour<ServerMsg, Ack>,
    /// ZeroTrust mental poker P2P unicast (M5.B.8). 用于 DrawShareRequest /
    /// Response, ConcealedKanReveal 等需要点对点的协议消息. broadcast (KeyShare,
    /// Shuffle, Discard 等) 走 gossipsub mp topic.
    pub rr_mp: cbor::Behaviour<MentalPokerMsg, Ack>,
}

/// 空响应 ack. 应用层不依赖 ack 内容, 仅用于 libp2p 内部请求闭环.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ack;

/// 房主定期 publish 到 gossipsub LOBBY_TOPIC 的房间记录.
///
/// 大厅订阅 topic, 收到 message 就转 BrowserEvent (跟 mDNS 路径一致),
/// RoomBrowser 累积 + 过期淘汰.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyAnnouncement {
    pub schema_version: u32,
    /// 房主 PeerId (字符串编码, 跨进程兼容).
    pub host_peer_id: String,
    pub host_nick: String,
    pub players: u8,
    pub lifecycle: String,
    pub room_id: String,
    /// 房主当前所有 dial multiaddr (字符串编码, 含 LAN / 公网 / 中转).
    /// 加入者从中选最优.
    pub multiaddrs: Vec<String>,
    /// unix 毫秒, 用于过期判断 (大厅超过 30 秒没收到新 announcement 视为下线).
    pub timestamp_unix_ms: i64,
    /// 房间地理区域 (M3.E). 老 schema 的 announcement 没此字段时 default = Unknown.
    #[serde(default)]
    pub region: Region,
    /// 房间信任模式 (M4.B). 老 schema 时 default = Standard.
    #[serde(default)]
    pub mode: RoomMode,
}

/// Tier 2 玩家 relay 贡献池公告.
///
/// 公网可达 (AutoNAT NatStatus::Public) 的 host swarm 周期 publish 这个,
/// 大厅累积形成动态 relay 池, 减少对 Tier 1 claw 单点依赖.
///
/// 与 [`LobbyAnnouncement`] 的区别: 后者在房间存在时才 publish, 内容是
/// 房间元数据; 前者只要节点 Public 就 publish, 内容是 relay 服务地址.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayAnnouncement {
    pub schema_version: u32,
    /// relay 节点的 PeerId (字符串编码).
    pub peer_id: String,
    /// 此 relay 的可 dial multiaddrs (公网直连, 不含 /p2p-circuit/ 自循环).
    pub multiaddrs: Vec<String>,
    /// unix 毫秒. 大厅超过 30 秒没刷新视为下线.
    pub timestamp_unix_ms: i64,
}

impl P2pBehaviour {
    pub fn new(
        local_peer_id: PeerId,
        local_pubkey: libp2p::identity::PublicKey,
        keypair: libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
        agent_metadata: String,
    ) -> Self {
        let agent_version = format!("{AGENT_PREFIX}{agent_metadata}");

        // gossipsub 配置: ZeroTrust 房间固定 4 节点, 默认 mesh_n_low=4 / mesh_n=6 在
        // N=4 时永远低于 low watermark (每节点最多 3 mesh peer < 4) → mesh 不稳 →
        // publish 返 InsufficientPeers → 协议卡 KeyExchange. 调到适配 4 节点拓扑.
        // 大厅 (LobbyTopic) 节点数可能更多, 这套参数仍兼容 (mesh_n_high=4 限制 mesh
        // 大小, fanout 走 gossip 兜底, lobby 房间发现仍 OK).
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(std::time::Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .mesh_n_low(2)
            .mesh_n(3)
            .mesh_n_high(4)
            .mesh_outbound_min(1)
            .build()
            .expect("valid gossipsub config");
        let gossipsub_behaviour = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(keypair),
            gossipsub_config,
        )
        .expect("gossipsub init");

        Self {
            identify: identify::Behaviour::new(
                identify::Config::new("/tui-majo/id/1".into(), local_pubkey)
                    .with_agent_version(agent_version),
            ),
            mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)
                .expect("mdns init"),
            autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
            relay_server: relay::Behaviour::new(local_peer_id, relay::Config::default()),
            relay_client,
            dcutr: dcutr::Behaviour::new(local_peer_id),
            gossipsub: gossipsub_behaviour,
            rr_c2s: cbor::Behaviour::new(
                [(PROTOCOL_C2S, ProtocolSupport::Full)],
                request_response::Config::default(),
            ),
            rr_s2c: cbor::Behaviour::new(
                [(PROTOCOL_S2C, ProtocolSupport::Full)],
                request_response::Config::default(),
            ),
            rr_mp: cbor::Behaviour::new(
                [(PROTOCOL_MP, ProtocolSupport::Full)],
                request_response::Config::default(),
            ),
        }
    }
}

/// 计算 ZeroTrust 房间的 gossipsub mp topic 名: `tui-majo/mp/<room_id>/v1`.
pub fn mp_topic_for_room(room_id: &str) -> String {
    format!("{MP_TOPIC_PREFIX}{room_id}/v1")
}
