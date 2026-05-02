//! 房主端 — listen + 把入站 ClientMsg 桥接到 RoomActor + 跑 mDNS 广告.
//!
//! 架构:
//! - 一个 swarm task 跑 listen + event loop
//! - 每个 peer 加入时, swarm task 创建一个 mpsc<ServerMsg> 给 RoomActor 用
//!   (forwarding task 把这些 ServerMsg 通过全局 outbox 转回 swarm task)
//! - swarm task 在 select! 上同时 poll swarm event 和 outbox, 后者通过
//!   `rr_s2c.send_request(peer, msg)` 发回去

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use libp2p::{
    Multiaddr, PeerId, Swarm,
    multiaddr::Protocol,
    request_response,
    swarm::SwarmEvent,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::net::p2p::behaviour::{Ack, P2pBehaviour, P2pBehaviourEvent};
use crate::net::p2p::swarm::{build_swarm, new_keypair};
use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::room::{RoomCmd, RoomHandle};

#[derive(Debug, Error)]
pub enum ListenerError {
    #[error("swarm 构建失败: {0}")]
    Swarm(String),
    #[error("listen 失败: {0}")]
    Listen(String),
}

/// 启动 P2P listener.
///
/// `room_metadata` 写入 identify agent_version (见 [`crate::net::p2p::discovery::encode_metadata`]).
///
/// 返回 [`HostHandle`], drop 时关闭 swarm + 撤销 mDNS 广告.
pub async fn spawn_p2p_listener(
    handle: RoomHandle,
    room_metadata: String,
) -> Result<HostHandle, ListenerError> {
    let kp = new_keypair();
    let local_peer_id = PeerId::from(&kp.public());

    let mut swarm =
        build_swarm(kp, room_metadata).map_err(|e| ListenerError::Swarm(e.to_string()))?;

    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .map_err(|e| ListenerError::Listen(format!("QUIC: {e}")))?;
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .map_err(|e| ListenerError::Listen(format!("TCP: {e}")))?;

    // 等到第一批 listen 地址 (尽量拿到 QUIC, 否则 fallback 任意).
    let mut listen_addrs: Vec<Multiaddr> = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                let is_quic =
                    address.iter().any(|p| matches!(p, Protocol::QuicV1));
                listen_addrs.push(address);
                if is_quic {
                    return;
                }
            }
        }
    })
    .await;

    let dial_addr = listen_addrs
        .iter()
        .find(|a| a.iter().any(|p| matches!(p, Protocol::QuicV1)))
        .or_else(|| listen_addrs.first())
        .cloned()
        .map(|a| a.with(Protocol::P2p(local_peer_id)));

    let (event_tx, event_rx) = mpsc::unbounded_channel::<HostEvent>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(host_swarm_task(swarm, handle, event_tx, shutdown_rx));

    Ok(HostHandle {
        dial_addr,
        event_rx,
        _shutdown: Some(shutdown_tx),
    })
}

/// 房主 listener 句柄. UI own 它来获取 dial_addr (展示给加入者) 和事件流.
/// drop = 关闭 swarm task + 撤销 mDNS.
pub struct HostHandle {
    /// 一个完整的 dial multiaddr (含 /p2p/<peer-id>), 给加入者复制粘贴用.
    pub dial_addr: Option<Multiaddr>,
    /// swarm task → UI 的事件流.
    pub event_rx: mpsc::UnboundedReceiver<HostEvent>,
    /// drop 时通过此 sender close, 通知 swarm task 退出.
    _shutdown: Option<oneshot::Sender<()>>,
}

/// 房主 swarm 向 UI 推送的事件.
#[derive(Debug)]
pub enum HostEvent {
    PeerJoined { peer_id: PeerId, player_id: u32 },
    PeerLeft { peer_id: PeerId },
}

// ============================================================================
// swarm task
// ============================================================================

/// 一个已 join 的 peer 在 swarm task 内的状态.
struct PeerSlot {
    player_id: u32,
}

async fn host_swarm_task(
    mut swarm: Swarm<P2pBehaviour>,
    room_handle: RoomHandle,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let mut peers: HashMap<PeerId, PeerSlot> = HashMap::new();
    let (outbox_tx, mut outbox_rx) = mpsc::unbounded_channel::<(PeerId, ServerMsg)>();
    // pending: 还没收到 Join 之前的 peer (其入站消息会被立刻处理 Join).
    // 已 Join 的 peer 直接走 RoomCmd::PlayerMsg.

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => {
                tracing::debug!("host swarm task: shutdown signaled");
                break;
            }
            Some((peer_id, msg)) = outbox_rx.recv() => {
                if peers.contains_key(&peer_id) {
                    swarm.behaviour_mut().rr_s2c.send_request(&peer_id, msg);
                }
            }
            event = swarm.select_next_some() => {
                handle_swarm_event(
                    &mut swarm,
                    event,
                    &mut peers,
                    &room_handle,
                    &outbox_tx,
                    &event_tx,
                ).await;
            }
        }
    }
}

async fn handle_swarm_event(
    swarm: &mut Swarm<P2pBehaviour>,
    event: SwarmEvent<P2pBehaviourEvent>,
    peers: &mut HashMap<PeerId, PeerSlot>,
    room_handle: &RoomHandle,
    outbox_tx: &mpsc::UnboundedSender<(PeerId, ServerMsg)>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
) {
    match event {
        SwarmEvent::Behaviour(P2pBehaviourEvent::RrC2s(rr_event)) => {
            handle_rr_c2s(swarm, rr_event, peers, room_handle, outbox_tx, event_tx).await;
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::RrS2c(request_response::Event::Message {
            message: request_response::Message::Request { channel, .. },
            ..
        })) => {
            // 入站 ServerMsg request 不应发生 (server 只发不收), 立即 ack 避免对方阻塞.
            let _ = swarm.behaviour_mut().rr_s2c.send_response(channel, Ack);
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            if let Some(slot) = peers.remove(&peer_id) {
                let _ = room_handle.tx.send(RoomCmd::Disconnect {
                    player_id: slot.player_id,
                });
                let _ = event_tx.send(HostEvent::PeerLeft { peer_id });
            }
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Mdns(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::Identify(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::Autonat(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::RelayServer(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::RelayClient(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::Dcutr(_)) => {
            // M1 阶段不消费这些, M2 加 NAT 处理时再细化.
        }
        _ => {}
    }
}

async fn handle_rr_c2s(
    swarm: &mut Swarm<P2pBehaviour>,
    event: request_response::Event<ClientMsg, Ack>,
    peers: &mut HashMap<PeerId, PeerSlot>,
    room_handle: &RoomHandle,
    outbox_tx: &mpsc::UnboundedSender<(PeerId, ServerMsg)>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
) {
    if let request_response::Event::Message {
        peer,
        message:
            request_response::Message::Request {
                request, channel, ..
            },
        ..
    } = event
    {
        // 立即 ack, 应用层不依赖 ack 内容.
        let _ = swarm.behaviour_mut().rr_c2s.send_response(channel, Ack);

        if let Some(slot) = peers.get(&peer) {
            // 已 Join 的 peer, 转发 PlayerMsg.
            let _ = room_handle.tx.send(RoomCmd::PlayerMsg {
                player_id: slot.player_id,
                msg: request,
            });
        } else {
            // 未 Join 的 peer, 期望第一条是 Join.
            process_pending_join(peer, request, peers, room_handle, outbox_tx, event_tx).await;
        }
    }
}

async fn process_pending_join(
    peer: PeerId,
    msg: ClientMsg,
    peers: &mut HashMap<PeerId, PeerSlot>,
    room_handle: &RoomHandle,
    outbox_tx: &mpsc::UnboundedSender<(PeerId, ServerMsg)>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
) {
    let (nickname, reconnect_token) = match msg {
        ClientMsg::Join {
            nickname,
            reconnect_token,
        } => (nickname, reconnect_token),
        _ => {
            // 未 Join 但发了别的消息, 忽略 (或可主动断连; 简化为忽略).
            tracing::warn!("peer {peer} 首条非 Join, 忽略");
            return;
        }
    };

    // 给 RoomActor 一个 sender, 它把 ServerMsg 推到这个 channel,
    // forwarding task 转推到 outbox.
    let (s2c_tx, mut s2c_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let (ack_tx, ack_rx) = oneshot::channel();

    if room_handle
        .tx
        .send(RoomCmd::Join {
            nickname,
            reconnect_token,
            sender: s2c_tx,
            ack: ack_tx,
        })
        .is_err()
    {
        return;
    }

    let join_result = match ack_rx.await {
        Ok(r) => r,
        Err(_) => return,
    };

    let join = match join_result {
        Ok(j) => j,
        Err(e) => {
            // RoomActor 拒绝, 推一条 Error 消息回去后断连意图.
            let _ = outbox_tx.send((
                peer,
                ServerMsg::Error {
                    message: format!("加入失败: {e}"),
                },
            ));
            return;
        }
    };

    peers.insert(
        peer,
        PeerSlot {
            player_id: join.player_id,
        },
    );

    // 起 forwarding task: s2c_rx → outbox.
    let outbox_tx_clone = outbox_tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = s2c_rx.recv().await {
            if outbox_tx_clone.send((peer, msg)).is_err() {
                break;
            }
        }
    });

    // 立即给 peer 发 Welcome (RoomActor 已在 handle_join 里通过 s2c_tx 发送过了, 这里不再重复).
    // 但 Welcome 是通过 sender 进 forwarding task → outbox_tx, 延迟到下一个 tick 才发出.
    let _ = event_tx.send(HostEvent::PeerJoined {
        peer_id: peer,
        player_id: join.player_id,
    });
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::GameRules;
    use crate::net::p2p::join::join_remote;
    use crate::net::room::spawn_room;
    use std::time::Duration;

    /// e2e: 起 host swarm + 2 个加入者 swarm, 互相 dial + Join + Ready, 验证 host
    /// 看到 ≥2 ready.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn end_to_end_two_clients_join_via_p2p() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "warn".into()),
            )
            .try_init();

        let handle = spawn_room("Host".into(), GameRules::default());
        let host = spawn_p2p_listener(handle.clone(), "host_nick=Host;players=1;lifecycle=lobby;room_id=t".into())
            .await
            .expect("listener");
        let dial_addr = host.dial_addr.clone().expect("dial_addr");

        // 房主自己用 spawn_local_session join (作为 player_id=1).
        let mut s_host = crate::net::session::spawn_local_session(handle.clone(), "Host".into())
            .await
            .expect("host local session");
        // 等 Welcome
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if matches!(s_host.try_recv(), Some(ServerMsg::Welcome { .. })) {
                break;
            }
        }

        // 两个加入者通过 P2P join.
        let s1 = join_remote(&dial_addr, "Alice".into())
            .await
            .expect("s1 join");
        let s2 = join_remote(&dial_addr, "Bob".into())
            .await
            .expect("s2 join");

        assert_ne!(s1.player_id, 0);
        assert_ne!(s2.player_id, 0);
        assert_ne!(s1.player_id, s2.player_id);

        // s1 是后加入者 (player_id != 1), 默认非 ready, 主动 Ready.
        s1.send(ClientMsg::Ready { ready: true });
        s2.send(ClientMsg::Ready { ready: true });

        // 轮询 host 端 RoomUpdate, 等 ≥3 ready (host 自己默认 ready + s1 + s2).
        let mut max_ready = 0usize;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && max_ready < 3 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            while let Some(msg) = s_host.try_recv() {
                if let ServerMsg::RoomUpdate(view) = &msg {
                    let n = view.players.iter().filter(|p| p.ready).count();
                    if n > max_ready {
                        max_ready = n;
                    }
                }
            }
        }
        assert!(
            max_ready >= 3,
            "host 应看到 ≥3 ready, 实际 {max_ready}"
        );
    }
}
