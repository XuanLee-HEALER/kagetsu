//! tui-majo-relay — 独立 libp2p relay 节点 (Tier 1 bootstrap).
//!
//! 部署到公网机器 (e.g. 阿里云国际版 ECS), 给 NAT 后的 tui-majo 客户端中转流量.
//! relay-server 不持有任何游戏状态, 也看不到加密流量内容 (libp2p Noise 端到端).
//!
//! 用法:
//! ```text
//! tui-majo-relay [PORT]            # 默认 4001
//! tui-majo-relay 12345
//! RUST_LOG=debug tui-majo-relay    # 看详细日志
//! ```
//!
//! 启动后输出自己的完整 multiaddr (含 /p2p/<peer-id>), 把它加进
//! `src/net/p2p/bootstrap.rs::BOOTSTRAP_RELAYS` 或客户端 prefs.toml 即可.
//!
//! 长跑进程, 用 systemd / pm2 等管理. 重启会换 PeerId, 客户端列表得跟着更新.
//! TODO: 持久化 keypair 到磁盘文件, 重启后保持同 PeerId.

use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use libp2p::{
    PeerId, SwarmBuilder, identify, identity, multiaddr::Protocol, ping, relay,
    swarm::NetworkBehaviour, swarm::SwarmEvent,
};

/// relay 节点的 NetworkBehaviour: 只跑必要协议.
#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    identify: identify::Behaviour,
    relay: relay::Behaviour,
    ping: ping::Behaviour,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    init_tracing();

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4001);

    let keypair = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(&keypair.public());

    let mut swarm = SwarmBuilder::with_existing_identity(keypair.clone())
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| RelayBehaviour {
            identify: identify::Behaviour::new(
                identify::Config::new("/tui-majo/id/1".into(), key.public())
                    .with_agent_version("tui-majo-relay/1".into()),
            ),
            relay: relay::Behaviour::new(local_peer_id, relay::Config::default()),
            ping: ping::Behaviour::new(ping::Config::default()),
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm.listen_on(format!("/ip4/0.0.0.0/udp/{port}/quic-v1").parse()?)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{port}").parse()?)?;

    println!("=========================================================");
    println!("tui-majo-relay 启动");
    println!("peer-id = {local_peer_id}");
    println!("listen TCP/QUIC = {port}");
    println!("等待 NewListenAddr...");
    println!("=========================================================");

    loop {
        let event = swarm.select_next_some().await;
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                let full = address.clone().with(Protocol::P2p(local_peer_id));
                println!();
                println!("Bootstrap multiaddr (复制这一行给客户端):");
                println!("  {full}");
                tracing::info!("listening on {full}");
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(e)) => {
                tracing::info!("relay event: {e:?}");
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info,
                ..
            })) => {
                tracing::debug!("identified peer={peer_id} agent={}", info.agent_version);
            }
            SwarmEvent::Behaviour(RelayBehaviourEvent::Ping(e)) => {
                tracing::trace!("ping: {e:?}");
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                tracing::info!("peer connected: {peer_id}");
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                tracing::info!("peer disconnected: {peer_id} ({cause:?})");
            }
            _ => {}
        }
    }
}

/// relay daemon 直接打 stderr (没 TUI), 默认 info 级别.
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,libp2p_swarm=warn,libp2p_tcp=warn")
            }),
        )
        .init();
}
