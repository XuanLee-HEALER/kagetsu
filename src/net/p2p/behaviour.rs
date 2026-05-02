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

use crate::net::protocol::{ClientMsg, ServerMsg};

/// 协议名: client → server.
pub const PROTOCOL_C2S: StreamProtocol = StreamProtocol::new("/tui-majo/c2s/1");
/// 协议名: server → client.
pub const PROTOCOL_S2C: StreamProtocol = StreamProtocol::new("/tui-majo/s2c/1");
/// identify 协议 agent_version 前缀, 后跟房间 metadata k=v;k=v 字符串.
pub const AGENT_PREFIX: &str = "tui-majo/";
/// gossipsub topic: 在线房间广播.
pub const LOBBY_TOPIC: &str = "tui-majo/lobby/v1";

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

        // gossipsub 配置: 默认 + 签名 (用 swarm 同一个 keypair 签, 让 sender peer-id
        // 跟连接 peer-id 一致) + 短 heartbeat 让 mesh 快速建立.
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(std::time::Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
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
        }
    }
}
