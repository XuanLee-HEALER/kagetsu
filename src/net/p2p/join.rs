//! 加入者端 — dial host multiaddr, 跑 Join 流程, 构造 NetSession.

use std::time::Duration;

use futures_util::StreamExt;
use libp2p::{Multiaddr, PeerId, Swarm, multiaddr::Protocol, request_response, swarm::SwarmEvent};
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::net::p2p::behaviour::{Ack, P2pBehaviour, P2pBehaviourEvent};
use crate::net::p2p::swarm::{build_swarm, new_keypair};
use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::session::NetSession;

#[derive(Debug, Error)]
pub enum JoinRemoteError {
    #[error("multiaddr 解析失败: {0}")]
    InvalidAddr(String),
    #[error("dial 失败: {0}")]
    Dial(String),
    #[error("协议错误: {0}")]
    Protocol(String),
    #[error("被拒绝: {0}")]
    Refused(String),
    #[error("超时")]
    Timeout,
}

/// 加入远程房间. `addr` 含 `/p2p/<peer-id>` 后缀.
pub async fn join_remote(
    addr: &Multiaddr,
    nickname: String,
) -> Result<NetSession, JoinRemoteError> {
    let host_peer_id = extract_peer_id(addr)
        .ok_or_else(|| JoinRemoteError::InvalidAddr("multiaddr 缺少 /p2p/<peer-id>".into()))?;

    let kp = new_keypair();
    let mut swarm = build_swarm(kp, "joiner".into())
        .map_err(|e| JoinRemoteError::Dial(format!("swarm build: {e}")))?;

    // 加入者也要 listen 一个本地端口 (M2 dcutr 需要; M1 主要让 connection 双向稳定).
    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .map_err(|e| JoinRemoteError::Dial(format!("listen: {e}")))?;

    swarm
        .dial(addr.clone())
        .map_err(|e| JoinRemoteError::Dial(format!("dial: {e}")))?;

    // 等连接建立 (10s timeout).
    let connect_result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == host_peer_id => {
                    return Ok::<(), JoinRemoteError>(());
                }
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    return Err(JoinRemoteError::Dial(error.to_string()));
                }
                _ => continue,
            }
        }
    })
    .await;

    match connect_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(JoinRemoteError::Timeout),
    }

    // 发 Join.
    swarm.behaviour_mut().rr_c2s.send_request(
        &host_peer_id,
        ClientMsg::Join {
            nickname: nickname.clone(),
            reconnect_token: None,
        },
    );

    // 等 Welcome (或 Error). 这一步内部消费 swarm event.
    let welcome = wait_welcome(&mut swarm, host_peer_id).await?;
    let (player_id, token) = match &welcome {
        ServerMsg::Welcome {
            player_id,
            reconnect_token,
            ..
        } => (*player_id, *reconnect_token),
        _ => return Err(JoinRemoteError::Protocol("未收到 Welcome".into())),
    };

    // 构造 NetSession 双向 channel.
    let (out_tx, out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();

    // 把已收到的 Welcome 推进 in_rx 让 UI 消费.
    let _ = in_tx.send(welcome);

    tokio::spawn(client_swarm_task(swarm, host_peer_id, out_rx, in_tx));

    Ok(NetSession::from_channels(player_id, token, out_tx, in_rx))
}

fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| match p {
        Protocol::P2p(pid) => Some(pid),
        _ => None,
    })
}

async fn wait_welcome(
    swarm: &mut Swarm<P2pBehaviour>,
    host_peer_id: PeerId,
) -> Result<ServerMsg, JoinRemoteError> {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = swarm.select_next_some().await;
            if let SwarmEvent::Behaviour(P2pBehaviourEvent::RrS2c(rr_event)) = event
                && let request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request {
                        request, channel, ..
                    },
                    ..
                } = rr_event
                && peer == host_peer_id
            {
                let _ = swarm.behaviour_mut().rr_s2c.send_response(channel, Ack);
                return Ok::<ServerMsg, JoinRemoteError>(request);
            }
        }
    })
    .await;

    match result {
        Ok(Ok(msg @ ServerMsg::Welcome { .. })) => Ok(msg),
        Ok(Ok(ServerMsg::Error { message })) => Err(JoinRemoteError::Refused(message)),
        Ok(Ok(other)) => Err(JoinRemoteError::Protocol(format!(
            "首条非 Welcome/Error: {:?}",
            std::mem::discriminant(&other)
        ))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(JoinRemoteError::Timeout),
    }
}

async fn client_swarm_task(
    mut swarm: Swarm<P2pBehaviour>,
    host_peer_id: PeerId,
    mut out_rx: mpsc::UnboundedReceiver<ClientMsg>,
    in_tx: mpsc::UnboundedSender<ServerMsg>,
) {
    // hard-coded reconnect_token persistence 简化为运行时 (M1 不持久化磁盘).
    let mut _last_token: Option<Uuid> = None;

    loop {
        tokio::select! {
            biased;
            msg = out_rx.recv() => {
                let Some(msg) = msg else {
                    tracing::debug!("client swarm task: out_rx closed");
                    break;
                };
                swarm.behaviour_mut().rr_c2s.send_request(&host_peer_id, msg);
            }
            event = swarm.select_next_some() => {
                handle_event(&mut swarm, event, host_peer_id, &in_tx, &mut _last_token).await;
                if in_tx.is_closed() {
                    break;
                }
            }
        }
    }
}

async fn handle_event(
    swarm: &mut Swarm<P2pBehaviour>,
    event: SwarmEvent<P2pBehaviourEvent>,
    host_peer_id: PeerId,
    in_tx: &mpsc::UnboundedSender<ServerMsg>,
    last_token: &mut Option<Uuid>,
) {
    match event {
        SwarmEvent::Behaviour(P2pBehaviourEvent::RrS2c(rr_event)) => {
            if let request_response::Event::Message {
                peer,
                message: request_response::Message::Request {
                    request, channel, ..
                },
                ..
            } = rr_event
                && peer == host_peer_id
            {
                let _ = swarm.behaviour_mut().rr_s2c.send_response(channel, Ack);
                if let ServerMsg::Welcome {
                    reconnect_token, ..
                } = &request
                {
                    *last_token = Some(*reconnect_token);
                }
                let _ = in_tx.send(request);
            }
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::RrC2s(request_response::Event::Message {
            message: request_response::Message::Request { channel, .. },
            ..
        })) => {
            // 加入者收到 c2s 入站 request 不应发生, 但收到了立即 ack.
            let _ = swarm.behaviour_mut().rr_c2s.send_response(channel, Ack);
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } if peer_id == host_peer_id => {
            tracing::warn!("host 连接关闭");
            // in_tx drop 时 NetSession 会发现 (try_recv 返回 None).
            // 显式不发任何 ServerMsg, 由 UI 自己 timeout.
        }
        _ => {}
    }
}
