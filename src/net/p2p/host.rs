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
    Multiaddr, PeerId, Swarm, autonat, gossipsub, identify, multiaddr::Protocol, request_response,
    swarm::SwarmEvent,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::mental_poker::wire::MentalPokerMsg;
use crate::net::p2p::behaviour::{
    Ack, LOBBY_TOPIC, LobbyAnnouncement, MP_TOPIC_PREFIX, P2pBehaviour, P2pBehaviourEvent,
    RELAYS_TOPIC, RelayAnnouncement,
};
use crate::net::p2p::mp_swarm::{SwarmCommand, dispatch_swarm_command};
use crate::net::p2p::swarm::{build_swarm, new_keypair};
use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::room::{RoomCmd, RoomHandle};

/// 房主端定期 publish 到 gossipsub LOBBY_TOPIC 的元数据.
#[derive(Debug, Clone)]
pub struct LobbyMeta {
    pub host_nick: String,
    pub room_id: String,
    /// 房间地理区域 (M3.E). 大厅根据用户偏好 region 过滤展示.
    pub region: crate::net::p2p::Region,
    /// 房间信任模式 (M4.B). 决定开局走 Standard 还是 ZeroTrust.
    pub mode: crate::net::p2p::RoomMode,
}

/// LobbyAnnouncement 中随房间状态变化的字段. RoomActor 每次 broadcast_room_update
/// 时通过 watch channel 推送, host_swarm_task 在 publish_lobby 时拉最新值.
#[derive(Debug, Clone)]
pub struct LobbyDynState {
    /// 当前房间已 join 的玩家数 (含 AI? — 只算真人, AI 槽位不计入大厅显示).
    pub players: u8,
    /// 房间生命周期字符串: "lobby" / "in_game" / "game_end".
    pub lifecycle: String,
}

impl Default for LobbyDynState {
    fn default() -> Self {
        Self {
            players: 0,
            lifecycle: "lobby".into(),
        }
    }
}

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
/// `bootstrap_relays` 是 [`crate::net::p2p::bootstrap`] 提供的 Tier 1 relay 列表.
/// 启动时主动 dial + listen on `<relay>/p2p-circuit`, 让 NAT 后房主获得 reservation,
/// 加入者通过 relay 连过来. 公网房主也连 bootstrap 让 AutoNAT 探测自己 Public.
///
/// 返回 [`HostHandle`], drop 时关闭 swarm + 撤销 mDNS 广告 + 释放 reservation.
pub async fn spawn_p2p_listener(
    handle: RoomHandle,
    room_metadata: String,
    bootstrap_relays: Vec<Multiaddr>,
    lobby_meta: LobbyMeta,
) -> Result<HostHandle, ListenerError> {
    let kp = new_keypair();
    let local_peer_id = PeerId::from(&kp.public());

    let mut swarm =
        build_swarm(kp, room_metadata).map_err(|e| ListenerError::Swarm(e.to_string()))?;

    // 订阅 lobby topic, 让自己也参与 mesh 传播 (帮助小型网络下消息扩散).
    let topic = gossipsub::IdentTopic::new(LOBBY_TOPIC);
    if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
        tracing::warn!("订阅 lobby topic 失败: {e}");
    }
    // 订阅 relay 贡献池 topic (M3.D). 自己 Public 时 publish, 平时也参与 mesh forward.
    let relays_topic = gossipsub::IdentTopic::new(RELAYS_TOPIC);
    if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&relays_topic) {
        tracing::warn!("订阅 relays topic 失败: {e}");
    }

    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .map_err(|e| ListenerError::Listen(format!("QUIC: {e}")))?;
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .map_err(|e| ListenerError::Listen(format!("TCP: {e}")))?;

    // 解析 bootstrap relay 的 peer-id, 存为 pending listen on circuit 表.
    // 不能立即 swarm.listen_on(<relay>/p2p-circuit) — libp2p 0.56 在 connection
    // 还没建立 + identify 还没交换前调 listen on circuit 会立刻 ListenerClosed.
    // 必须等 identify::Received from bootstrap peer 后再 listen.
    // 见 https://github.com/libp2p/rust-libp2p/tree/master/examples/relay-server
    let mut pending_circuit_listens: HashMap<PeerId, Multiaddr> = HashMap::new();
    for relay_addr in &bootstrap_relays {
        let Some(pid) = relay_addr.iter().find_map(|p| match p {
            Protocol::P2p(id) => Some(id),
            _ => None,
        }) else {
            tracing::warn!("bootstrap multiaddr 缺 /p2p/<peer-id>: {relay_addr}");
            continue;
        };
        pending_circuit_listens.insert(pid, relay_addr.clone());
    }

    // 主动 dial 每个 bootstrap relay 让 identify 协议交换启动; 失败仅 warn 不致命.
    for relay_addr in &bootstrap_relays {
        if let Err(e) = swarm.dial(relay_addr.clone()) {
            tracing::warn!("dial bootstrap relay {} 失败: {e}", relay_addr);
        }
    }

    tracing::info!(
        "host swarm started: bootstrap_relays={}, pending circuit listens={} (会在 identify 后触发)",
        bootstrap_relays.len(),
        pending_circuit_listens.len()
    );

    // 等到第一批 listen 地址 (尽量拿到 QUIC, 否则 fallback 任意).
    let mut listen_addrs: Vec<Multiaddr> = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                let is_quic = address.iter().any(|p| matches!(p, Protocol::QuicV1));
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
    // M5.D.0: mp 命令 + 入站 channel — UI ZeroTrustGameState 通过 mp_command_tx
    // 发 SwarmCommand, 通过 mp_inbound_rx 接 (PeerId, MentalPokerMsg) 入站消息.
    let (mp_command_tx, mp_command_rx) = mpsc::unbounded_channel::<SwarmCommand>();
    let (mp_inbound_tx, mp_inbound_rx) = mpsc::unbounded_channel::<(PeerId, MentalPokerMsg)>();
    // LobbyAnnouncement 动态字段 (玩家数 / lifecycle): RoomActor 每次
    // broadcast_room_update 时 send_replace, host swarm publish_lobby 时 borrow.
    let (lobby_dyn_tx, lobby_dyn_rx) = watch::channel(LobbyDynState::default());

    // M5.D.2: 通知 RoomActor 房主自己的 libp2p PeerId, ZeroTrust 模式开局时填
    // MpStart.all_peer_ids 用. host slot 已 join 时立即关联 (handle_cmd 内处理),
    // 否则等 host 调 spawn_local_session 加入后再关联.
    let _ = handle.tx.send(RoomCmd::SetLocalPeerId {
        peer_id_bytes: local_peer_id.to_bytes(),
    });
    // LobbyDynState 反向通道注入: RoomActor 持 sender, 状态变更时 push 一次.
    let _ = handle.tx.send(RoomCmd::SetLobbyWatch { tx: lobby_dyn_tx });

    tokio::spawn(host_swarm_task(
        swarm,
        handle,
        event_tx,
        shutdown_rx,
        pending_circuit_listens,
        lobby_meta,
        local_peer_id,
        mp_command_rx,
        mp_inbound_tx,
        lobby_dyn_rx,
    ));

    Ok(HostHandle {
        dial_addr,
        event_rx,
        _shutdown: Some(shutdown_tx),
        mp_command_tx,
        mp_inbound_rx: Some(mp_inbound_rx),
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
    /// M5.D.0: ZeroTrust 模式 mp 命令出口 — UI 通过此 channel 发
    /// [`SwarmCommand`] (Broadcast / Unicast) 让 swarm task 调
    /// `gossipsub.publish` / `rr_mp.send_request`. clone 给 SwarmTransport.
    pub mp_command_tx: mpsc::UnboundedSender<SwarmCommand>,
    /// M5.D.0: ZeroTrust 模式 mp 入站消息 — swarm task 收 RrMp Request 或
    /// Gossipsub Message (mp topic) 后推这里. UI 拿走后做 PeerId → own_index
    /// 反查并 [`MpInbound::deliver`]. take 后变 None (一次性).
    pub mp_inbound_rx: Option<mpsc::UnboundedReceiver<(PeerId, MentalPokerMsg)>>,
}

/// 房主 swarm 向 UI 推送的事件.
#[derive(Debug)]
pub enum HostEvent {
    PeerJoined {
        peer_id: PeerId,
        player_id: u32,
    },
    PeerLeft {
        peer_id: PeerId,
    },
    /// AutoNAT 探测结果变化. UI 用它显示 "你是公网可达 / NAT 后 / 探测中".
    NatStatusChanged {
        reachability: NatReachability,
    },
    /// DCUtR 升级直连成功 / 失败 (relay 中转 → 直接). UI 显示连接质量提示.
    DcutrResult {
        peer_id: PeerId,
        upgraded: bool,
    },
    /// 新的 listen 地址 (本地 LAN, 公网, 或 relay 中转 /p2p-circuit/...).
    /// UI 累积所有 addr 并按优先级选最优作 dial_addr 给加入者.
    NewListenAddr {
        addr: Multiaddr,
    },
}

/// 简化的 NAT 可达性状态 (从 libp2p::autonat::NatStatus 投影).
#[derive(Debug, Clone)]
pub enum NatReachability {
    /// 公网可达, 含 AutoNAT 探测确认的外部 multiaddr.
    Public(Multiaddr),
    /// NAT 后, 必须通过 relay 才能被加入者连上.
    Private,
    /// 还没探测出结果 (未连 AutoNAT server / 数据不足).
    Unknown,
}

// ============================================================================
// swarm task
// ============================================================================

/// 一个已 join 的 peer 在 swarm task 内的状态.
struct PeerSlot {
    player_id: u32,
}

#[allow(clippy::too_many_arguments)]
async fn host_swarm_task(
    mut swarm: Swarm<P2pBehaviour>,
    room_handle: RoomHandle,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut pending_circuit_listens: HashMap<PeerId, Multiaddr>,
    lobby_meta: LobbyMeta,
    local_peer_id: PeerId,
    mut mp_command_rx: mpsc::UnboundedReceiver<SwarmCommand>,
    mp_inbound_tx: mpsc::UnboundedSender<(PeerId, MentalPokerMsg)>,
    lobby_dyn_rx: watch::Receiver<LobbyDynState>,
) {
    let mut peers: HashMap<PeerId, PeerSlot> = HashMap::new();
    let (outbox_tx, mut outbox_rx) = mpsc::unbounded_channel::<(PeerId, ServerMsg)>();
    // 累积全部 listen addr 用于 publish (含 LAN/公网/circuit).
    let mut my_listen_addrs: Vec<Multiaddr> = Vec::new();
    // M3.D: 自己 AutoNAT 探测到 Public 后启用 relay 池 publish.
    // 内容是 AutoNAT 确认的公网 multiaddr (不含 /p2p-circuit/, 不含 LAN/loopback).
    let mut public_addrs: Vec<Multiaddr> = Vec::new();
    // M5.D.0: 已订阅的 mp gossipsub topic 集 — Broadcast 命令时按需 lazy 订阅.
    let mut mp_subscribed_topics: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // 每 5 秒 publish lobby + relays announcement.
    let mut publish_interval = tokio::time::interval(Duration::from_secs(5));
    publish_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let topic = gossipsub::IdentTopic::new(LOBBY_TOPIC);
    let relays_topic = gossipsub::IdentTopic::new(RELAYS_TOPIC);

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
            Some(cmd) = mp_command_rx.recv() => {
                handle_mp_command(&mut swarm, cmd, &mut mp_subscribed_topics);
            }
            _ = publish_interval.tick() => {
                let dyn_state = lobby_dyn_rx.borrow().clone();
                publish_lobby(&mut swarm, &topic, &lobby_meta, &dyn_state, local_peer_id, &my_listen_addrs);
                if !public_addrs.is_empty() {
                    publish_relays(&mut swarm, &relays_topic, local_peer_id, &public_addrs);
                }
            }
            event = swarm.select_next_some() => {
                if let SwarmEvent::NewListenAddr { ref address, .. } = event {
                    let full = if address.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                        address.clone()
                    } else {
                        address.clone().with(Protocol::P2p(local_peer_id))
                    };
                    if !my_listen_addrs.contains(&full) {
                        my_listen_addrs.push(full);
                    }
                }
                // M3.D: AutoNAT 探测结果 → public_addrs (relay 贡献池源)
                if let SwarmEvent::Behaviour(P2pBehaviourEvent::Autonat(
                    autonat::Event::StatusChanged { ref new, .. },
                )) = event
                {
                    match new {
                        autonat::NatStatus::Public(addr) => {
                            let full = if addr.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                                addr.clone()
                            } else {
                                addr.clone().with(Protocol::P2p(local_peer_id))
                            };
                            if !public_addrs.contains(&full) {
                                tracing::info!(
                                    "AutoNAT Public confirmed, adding to relay pool: {full}"
                                );
                                public_addrs.push(full);
                            }
                        }
                        autonat::NatStatus::Private | autonat::NatStatus::Unknown => {
                            if !public_addrs.is_empty() {
                                tracing::info!(
                                    "AutoNAT 不再 Public, 撤销 relay 池 publish ({} addrs)",
                                    public_addrs.len()
                                );
                                public_addrs.clear();
                            }
                        }
                    }
                }
                handle_swarm_event(
                    &mut swarm,
                    event,
                    &mut peers,
                    &room_handle,
                    &outbox_tx,
                    &event_tx,
                    &mut pending_circuit_listens,
                    &mp_inbound_tx,
                ).await;
            }
        }
    }
}

/// M5.D.0 mp 命令 dispatch — 调 mp_swarm::dispatch_swarm_command 共用实现.
fn handle_mp_command(
    swarm: &mut Swarm<P2pBehaviour>,
    cmd: SwarmCommand,
    subscribed_topics: &mut std::collections::HashSet<String>,
) {
    dispatch_swarm_command(swarm, cmd, subscribed_topics);
}

/// 把当前房间状态序列化为 JSON 后通过 gossipsub publish 到 LOBBY_TOPIC.
/// 失败 (没 mesh peer / 序列化错) 仅 warn 不致命.
fn publish_lobby(
    swarm: &mut Swarm<P2pBehaviour>,
    topic: &gossipsub::IdentTopic,
    meta: &LobbyMeta,
    dyn_state: &LobbyDynState,
    local_peer_id: PeerId,
    listen_addrs: &[Multiaddr],
) {
    if listen_addrs.is_empty() {
        return;
    }
    let announcement = LobbyAnnouncement {
        schema_version: 1,
        host_peer_id: local_peer_id.to_string(),
        host_nick: meta.host_nick.clone(),
        players: dyn_state.players,
        lifecycle: dyn_state.lifecycle.clone(),
        room_id: meta.room_id.clone(),
        multiaddrs: listen_addrs.iter().map(|a| a.to_string()).collect(),
        timestamp_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        region: meta.region,
        mode: meta.mode,
    };
    let payload = match serde_json::to_vec(&announcement) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("序列化 lobby announcement 失败: {e}");
            return;
        }
    };
    match swarm
        .behaviour_mut()
        .gossipsub
        .publish(topic.clone(), payload)
    {
        Ok(_) => tracing::debug!("published lobby announcement, addrs={}", listen_addrs.len()),
        Err(e) => {
            // 启动早期 mesh 还没建立, NoPeers 错误正常; 其它也只 debug 不阻塞.
            tracing::debug!("publish lobby pending: {e}");
        }
    }
}

/// M3.D: 公网 host 周期 publish 自己作 relay 的可 dial 公网 multiaddr.
///
/// 调用前提: AutoNAT 探测确认 Public, public_addrs 非空. relay-server
/// behaviour 已在 P2pBehaviour 启用, 入站 reservation 自动接受
/// (受 relay::Config::default() 限制 — 默认 128 reservations / 16 circuits).
fn publish_relays(
    swarm: &mut Swarm<P2pBehaviour>,
    topic: &gossipsub::IdentTopic,
    local_peer_id: PeerId,
    public_addrs: &[Multiaddr],
) {
    if public_addrs.is_empty() {
        return;
    }
    let announcement = RelayAnnouncement {
        schema_version: 1,
        peer_id: local_peer_id.to_string(),
        multiaddrs: public_addrs.iter().map(|a| a.to_string()).collect(),
        timestamp_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    };
    let payload = match serde_json::to_vec(&announcement) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("序列化 relay announcement 失败: {e}");
            return;
        }
    };
    match swarm
        .behaviour_mut()
        .gossipsub
        .publish(topic.clone(), payload)
    {
        Ok(_) => tracing::debug!(
            "published relay announcement (Tier 2), addrs={}",
            public_addrs.len()
        ),
        Err(e) => tracing::debug!("publish relay pending: {e}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_swarm_event(
    swarm: &mut Swarm<P2pBehaviour>,
    event: SwarmEvent<P2pBehaviourEvent>,
    peers: &mut HashMap<PeerId, PeerSlot>,
    room_handle: &RoomHandle,
    outbox_tx: &mpsc::UnboundedSender<(PeerId, ServerMsg)>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    pending_circuit_listens: &mut HashMap<PeerId, Multiaddr>,
    mp_inbound_tx: &mpsc::UnboundedSender<(PeerId, MentalPokerMsg)>,
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
        SwarmEvent::Behaviour(P2pBehaviourEvent::RrMp(request_response::Event::Message {
            peer,
            message:
                request_response::Message::Request {
                    request, channel, ..
                },
            ..
        })) => {
            // M5.D.0: ZeroTrust 模式 mp unicast 入站 (e.g. DrawShareRequest /
            // DrawShareResponse / ConcealedKanReveal). 立即 Ack 避免对方阻塞.
            let _ = swarm.behaviour_mut().rr_mp.send_response(channel, Ack);
            let _ = mp_inbound_tx.send((peer, request));
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            propagation_source,
            message,
            ..
        })) if message.topic.as_str().starts_with(MP_TOPIC_PREFIX) => {
            // M5.D.0: ZeroTrust 模式 mp broadcast 入站 (KeyShare / Shuffle /
            // Discard / Call / Win 等). 解 cbor 后推 mp_inbound_tx.
            match serde_json::from_slice::<MentalPokerMsg>(&message.data) {
                Ok(msg) => {
                    let _ = mp_inbound_tx.send((propagation_source, msg));
                }
                Err(e) => {
                    tracing::warn!(
                        "mp gossipsub message from {propagation_source} decode 失败: {e}"
                    );
                }
            }
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            if let Some(slot) = peers.remove(&peer_id) {
                let _ = room_handle.tx.send(RoomCmd::Disconnect {
                    player_id: slot.player_id,
                });
                let _ = event_tx.send(HostEvent::PeerLeft { peer_id });
            }
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Identify(identify::Event::Received {
            peer_id,
            info,
            ..
        })) => {
            // bootstrap relay 完成 identify 后, 此时 connection 稳定, 协议谈判完毕,
            // 才能 listen on /p2p-circuit 触发 reservation 请求.
            if let Some(relay_addr) = pending_circuit_listens.remove(&peer_id) {
                let supports_hop = info
                    .protocols
                    .iter()
                    .any(|p| p.as_ref().contains("/libp2p/circuit/relay/0.2.0/hop"));
                if !supports_hop {
                    tracing::warn!(
                        "bootstrap peer {peer_id} 不支持 relay hop 协议, 跳过 reservation"
                    );
                    return;
                }
                let circuit = relay_addr.clone().with(Protocol::P2pCircuit);
                match swarm.listen_on(circuit.clone()) {
                    Ok(_) => {
                        tracing::info!("listening on circuit via relay {peer_id} (addr={circuit})")
                    }
                    Err(e) => tracing::warn!("listen on circuit {circuit} 失败: {e}"),
                }
            }
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::RelayClient(relay_event)) => {
            tracing::info!("relay-client event: {relay_event:?}");
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Autonat(autonat::Event::StatusChanged {
            new,
            ..
        })) => {
            let reachability = match new {
                autonat::NatStatus::Public(addr) => NatReachability::Public(addr),
                autonat::NatStatus::Private => NatReachability::Private,
                autonat::NatStatus::Unknown => NatReachability::Unknown,
            };
            tracing::info!("autonat status changed: {reachability:?}");
            let _ = event_tx.send(HostEvent::NatStatusChanged { reachability });
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Dcutr(dcutr_event)) => {
            // 0.56 dcutr::Event 是 struct 含 remote_peer_id + result.
            let upgraded = dcutr_event.result.is_ok();
            tracing::info!(
                "dcutr peer={} upgraded={upgraded} ({:?})",
                dcutr_event.remote_peer_id,
                dcutr_event.result
            );
            let _ = event_tx.send(HostEvent::DcutrResult {
                peer_id: dcutr_event.remote_peer_id,
                upgraded,
            });
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            // 含 /p2p-circuit/ 时表示 reservation 已确认, 加入者可通过 relay 连过来.
            // 不含 circuit 的也推 (LAN / 公网直连地址).
            // 用 with(P2p(local_peer_id)) 让 multiaddr 完整可 dial.
            let local_peer_id = *swarm.local_peer_id();
            let full = if address
                .iter()
                .any(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
            {
                address
            } else {
                address.with(libp2p::multiaddr::Protocol::P2p(local_peer_id))
            };
            tracing::info!("new listen addr: {full}");
            let _ = event_tx.send(HostEvent::NewListenAddr { addr: full });
        }
        SwarmEvent::Behaviour(P2pBehaviourEvent::Mdns(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::Identify(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::Autonat(_))
        | SwarmEvent::Behaviour(P2pBehaviourEvent::RelayServer(_)) => {
            // 其它细节事件不消费, M2 后期可加 logging.
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
        message: request_response::Message::Request {
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

    // M5.D.2: 关联加入者 player_id ↔ libp2p PeerId, ZeroTrust 模式 start 时
    // 拼 MpStart.all_peer_ids 用.
    let _ = room_handle.tx.send(RoomCmd::AssociatePeer {
        player_id: join.player_id,
        peer_id_bytes: peer.to_bytes(),
    });

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
        let host = spawn_p2p_listener(
            handle.clone(),
            "host_nick=Host;players=1;lifecycle=lobby;room_id=t".into(),
            vec![], // 单元测试不连 bootstrap relay
            LobbyMeta {
                host_nick: "Host".into(),
                room_id: "t".into(),
                region: crate::net::p2p::Region::Unknown,
                mode: crate::net::p2p::RoomMode::Standard,
            },
        )
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
        assert!(max_ready >= 3, "host 应看到 ≥3 ready, 实际 {max_ready}");
    }
}
