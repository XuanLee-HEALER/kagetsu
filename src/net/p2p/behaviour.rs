//! P2pBehaviour — 复合 NetworkBehaviour, 集中所有协议.
//!
//! 子 Behaviour 列表:
//! - identify: 节点身份交换 + 房间 metadata (通过 agent_version)
//! - mdns: LAN 发现
//! - autonat (client): 探测自己是否公网可达
//! - relay-server: 接受被中转流量 (房主公网时启用)
//! - relay-client: 通过 relay 连出去 (NAT 后玩家)
//! - dcutr: relay 升级为直连
//! - rr_c2s: client → server 单向消息 (ClientMsg 作为 request, Ack 作为 response)
//! - rr_s2c: server → client 单向消息 (ServerMsg 作为 request, Ack 作为 response)

use libp2p::{
    PeerId, StreamProtocol, autonat, dcutr, identify, mdns, relay,
    request_response::{self, ProtocolSupport, cbor},
    swarm::NetworkBehaviour,
};

use crate::net::protocol::{ClientMsg, ServerMsg};

/// 协议名: client → server.
pub const PROTOCOL_C2S: StreamProtocol = StreamProtocol::new("/tui-majo/c2s/1");
/// 协议名: server → client.
pub const PROTOCOL_S2C: StreamProtocol = StreamProtocol::new("/tui-majo/s2c/1");
/// identify 协议 agent_version 前缀, 后跟房间 metadata k=v;k=v 字符串.
pub const AGENT_PREFIX: &str = "tui-majo/";

#[derive(NetworkBehaviour)]
pub struct P2pBehaviour {
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub autonat: autonat::Behaviour,
    pub relay_server: relay::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub rr_c2s: cbor::Behaviour<ClientMsg, Ack>,
    pub rr_s2c: cbor::Behaviour<ServerMsg, Ack>,
}

/// 空响应 ack. 应用层不依赖 ack 内容, 仅用于 libp2p 内部请求闭环.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ack;

impl P2pBehaviour {
    pub fn new(
        local_peer_id: PeerId,
        local_pubkey: libp2p::identity::PublicKey,
        relay_client: relay::client::Behaviour,
        agent_metadata: String,
    ) -> Self {
        let agent_version = format!("{AGENT_PREFIX}{agent_metadata}");
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
