//! tui-majo-relay — 独立 libp2p relay 节点 (Tier 1 bootstrap).
//!
//! 部署到公网机器 (e.g. 阿里云国际版 ECS), 给 NAT 后的 tui-majo 客户端中转流量.
//! relay-server 不持有任何游戏状态, 也看不到加密流量内容 (libp2p Noise 端到端).
//!
//! 用法:
//! ```text
//! tui-majo-relay [PORT] [KEY_FILE] [EXTERNAL_IP]
//! tui-majo-relay                                  # 默认 PORT=4001 KEY=tui-majo-relay.key
//! tui-majo-relay 4001 ./relay.key 47.84.49.170    # 都自定
//! RUST_LOG=debug tui-majo-relay                   # 详细日志
//! ```
//!
//! ## EXTERNAL_IP (重要 — 阿里云 / AWS / GCP 等 NAT 类云需要)
//!
//! 云 ECS 通常 listen 在内网网卡 (172.x / 10.x), 公网 IP 由 NAT 转发. relay
//! 自己 enumerate listen_addrs 拿到的是内网 addr, reservation reply 给 client
//! 的 addrs 列表里没用 (client 报 NoAddressesInReservation). 需 explicit 告知
//! relay 它的公网 IP, 通过 swarm.add_external_address() 加进 external 列表.
//!
//! 如不提供, relay 仍能跑 + 接受 reservation, 但 NAT 后 client 拿不到 dial-back addr.
//!
//! ## PeerId 持久化
//!
//! KEY_FILE 不存在时生成新 ed25519 keypair 写入, 存在时加载. 重启保持同 PeerId,
//! 客户端 bootstrap_relays 配置只填一次即可.
//!
//! ## 部署
//!
//! 启动后输出完整 multiaddr (含 /p2p/<peer-id>), 把它加进
//! `src/net/p2p/bootstrap.rs::DEFAULT_BOOTSTRAP_RELAYS` 或客户端 prefs.toml 的
//! `[network] bootstrap_relays = [...]`.
//!
//! 长跑进程, 用 systemd / pm2 等管理.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use libp2p::{
    Multiaddr, PeerId, SwarmBuilder, identify, identity, multiaddr::Protocol, ping, relay,
    swarm::NetworkBehaviour, swarm::SwarmEvent,
};

/// relay 节点的 NetworkBehaviour: 只跑必要协议.
#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    identify: identify::Behaviour,
    relay: relay::Behaviour,
    ping: ping::Behaviour,
}

const DEFAULT_KEY_FILE: &str = "tui-majo-relay.key";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    init_tracing();

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4001);
    let key_file: PathBuf = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_KEY_FILE));
    let external_ip: Option<String> = std::env::args().nth(3);

    let keypair = load_or_create_keypair(&key_file)?;
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

    // 加公网 external addr (NAT 类云必需, 否则 reservation reply 没 addrs)
    if let Some(ip) = &external_ip {
        for addr_str in &[
            format!("/ip4/{ip}/udp/{port}/quic-v1"),
            format!("/ip4/{ip}/tcp/{port}"),
        ] {
            match addr_str.parse::<Multiaddr>() {
                Ok(addr) => {
                    swarm.add_external_address(addr.clone());
                    println!("[external] add_external_address {addr}");
                    tracing::info!("declared external addr: {addr}");
                }
                Err(e) => tracing::warn!("external addr 解析失败 {addr_str}: {e}"),
            }
        }
    } else {
        tracing::warn!(
            "未提供 EXTERNAL_IP — NAT 类云 (阿里云/AWS) 上 client 会报 NoAddressesInReservation. \
             启动加第三个参数 e.g. tui-majo-relay {port} {} <your-public-ip>",
            key_file.display()
        );
    }

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

/// 加载或创建 ed25519 keypair 持久化文件.
///
/// 文件不存在 → 生成新 keypair, 写入 protobuf 编码 (libp2p 标准格式).
/// 文件存在 → 读取并解码.
///
/// 文件权限不做特殊处理 (Windows 上 chmod 无意义). Unix 部署建议用户自行 `chmod 600`.
fn load_or_create_keypair(path: &Path) -> Result<identity::Keypair> {
    if path.exists() {
        let bytes = std::fs::read(path)
            .with_context(|| format!("读取 keypair 文件失败: {}", path.display()))?;
        let kp = identity::Keypair::from_protobuf_encoding(&bytes)
            .with_context(|| format!("keypair 文件格式错误: {}", path.display()))?;
        tracing::info!("loaded keypair from {}", path.display());
        Ok(kp)
    } else {
        let kp = identity::Keypair::generate_ed25519();
        let bytes = kp
            .to_protobuf_encoding()
            .context("encode keypair to protobuf")?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, bytes)
            .with_context(|| format!("写入 keypair 文件失败: {}", path.display()))?;
        tracing::info!("generated new keypair, saved to {}", path.display());
        eprintln!(
            "[relay] 已生成新 keypair → {}, 重启保持同 PeerId. 备份 / 妥善保管此文件.",
            path.display()
        );
        Ok(kp)
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
