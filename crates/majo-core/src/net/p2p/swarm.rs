//! Swarm 构建 — transport stack + behaviour 组装.

use std::time::Duration;

use libp2p::{
    PeerId, Swarm, SwarmBuilder,
    identity::{self, Keypair},
};

use super::behaviour::P2pBehaviour;

/// 构建一个 Swarm. `agent_metadata` 写入 identify 的 agent_version,
/// 房主用它携带房间信息 (host_nick / players / lifecycle / room_id).
pub fn build_swarm(
    keypair: Keypair,
    agent_metadata: String,
) -> Result<Swarm<P2pBehaviour>, Box<dyn std::error::Error + Send + Sync>> {
    let local_pubkey = keypair.public();
    let local_peer_id = PeerId::from(&local_pubkey);
    let keypair_for_gossipsub = keypair.clone();

    let swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_quic()
        .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|_, relay_client| {
            P2pBehaviour::new(
                local_peer_id,
                local_pubkey.clone(),
                keypair_for_gossipsub,
                relay_client,
                agent_metadata,
            )
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    Ok(swarm)
}

/// 生成新的 ed25519 密钥对.
pub fn new_keypair() -> Keypair {
    identity::Keypair::generate_ed25519()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use libp2p::swarm::SwarmEvent;
    use std::time::Duration;

    /// 起一个 swarm, listen QUIC 0 端口, 在 2s 内应该收到 NewListenAddr.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_swarm_listen_quic() {
        let kp = new_keypair();
        let metadata = "host_nick=test;players=1;lifecycle=lobby;room_id=t".into();
        let mut swarm = build_swarm(kp, metadata).expect("build swarm");

        swarm
            .listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap())
            .expect("listen quic");

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                    return address;
                }
            }
        })
        .await;
        assert!(result.is_ok(), "QUIC listen 应在 2s 内回复 NewListenAddr");
    }

    /// 同上但 TCP.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_swarm_listen_tcp() {
        let kp = new_keypair();
        let metadata = "host_nick=test;players=1;lifecycle=lobby;room_id=t".into();
        let mut swarm = build_swarm(kp, metadata).expect("build swarm");

        swarm
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .expect("listen tcp");

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                    return address;
                }
            }
        })
        .await;
        assert!(result.is_ok(), "TCP listen 应在 2s 内回复 NewListenAddr");
    }
}
