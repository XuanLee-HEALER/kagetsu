//! Swarm-bound MpTransport (M5.C.0).
//!
//! [`crate::net::p2p::mp_bridge::MpTransport`] 的真实 P2P 实现:
//! - broadcast → gossipsub.publish(mp_topic, encoded_msg)
//! - unicast → rr_mp.send_request(target_peer, msg)
//!
//! ## 跨 task 设计
//!
//! 一个 [`libp2p::Swarm`] 不能跨 task 共享 (持 `&mut self` 的内部状态).
//! 因此 [`SwarmTransport`] 不直接持 swarm, 而是通过 mpsc channel 给
//! `host_swarm_task` (或 `join_remote` swarm task) 发 [`SwarmCommand`],
//! swarm task 在自己 select! 分支里 dispatch.
//!
//! 反向 (swarm → bridge) 由 swarm task 在 [`libp2p::request_response::Event::Message`]
//! 跟 [`libp2p::gossipsub::Event::Message`] 里把消息解 cbor 后调
//! [`crate::net::p2p::mp_bridge::MpInbound::deliver`].

use libp2p::PeerId;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::mental_poker::wire::MentalPokerMsg;
use crate::net::p2p::mp_bridge::{MpBridgeError, MpTransport};

/// SwarmTransport → swarm task 的命令.
#[derive(Debug)]
pub enum SwarmCommand {
    /// 显式订阅 gossipsub topic (不发数据). 让 mesh 在第一次 publish 之前就成型,
    /// 避免 broadcast 路径上的 `subscribe + publish` race (publish 被 mesh 未稳
    /// 拒绝).
    Subscribe { topic: String },
    /// 广播 (gossipsub publish). 自动 subscribe topic 若未订阅.
    Broadcast {
        /// 完整 mp gossipsub topic (调 [`crate::net::p2p::behaviour::mp_topic_for_room`]
        /// 算).
        topic: String,
        msg: MentalPokerMsg,
    },
    /// 单播 (rr_mp.send_request).
    Unicast { target: PeerId, msg: MentalPokerMsg },
}

/// MpTransport 实现, 把 broadcast / unicast 转 SwarmCommand 走 mpsc.
///
/// `peer_map[idx]` = own_index = idx 的玩家 PeerId. 用于 unicast 反查.
pub struct SwarmTransport {
    cmd_tx: UnboundedSender<SwarmCommand>,
    topic: String,
    peer_map: Vec<PeerId>,
}

impl SwarmTransport {
    /// `peer_map` 长度必须等于 n_players, 顺序按 own_index 0..N.
    pub fn new(
        cmd_tx: UnboundedSender<SwarmCommand>,
        topic: String,
        peer_map: Vec<PeerId>,
    ) -> Self {
        Self {
            cmd_tx,
            topic,
            peer_map,
        }
    }
}

impl MpTransport for SwarmTransport {
    fn broadcast(&mut self, msg: MentalPokerMsg) -> Result<(), MpBridgeError> {
        self.cmd_tx
            .send(SwarmCommand::Broadcast {
                topic: self.topic.clone(),
                msg,
            })
            .map_err(|_| MpBridgeError::Closed)
    }

    fn unicast(&mut self, target_idx: usize, msg: MentalPokerMsg) -> Result<(), MpBridgeError> {
        let n = self.peer_map.len();
        if target_idx >= n {
            return Err(MpBridgeError::InvalidTarget(target_idx, n));
        }
        let target = self.peer_map[target_idx];
        self.cmd_tx
            .send(SwarmCommand::Unicast { target, msg })
            .map_err(|_| MpBridgeError::Closed)
    }
}

/// 创建 (transport, command_rx) 一对. transport 给 mp_bridge 用,
/// command_rx 给 swarm task select! 分支用.
pub fn new_swarm_transport(
    topic: String,
    peer_map: Vec<PeerId>,
) -> (SwarmTransport, UnboundedReceiver<SwarmCommand>) {
    let (tx, rx) = unbounded_channel();
    (SwarmTransport::new(tx, topic, peer_map), rx)
}

// ============================================================================
// Swarm-side handlers — host.rs / join.rs / 集成测试 共用
// ============================================================================

/// 把 [`SwarmCommand`] 派发到真 [`libp2p::Swarm<P2pBehaviour>`] 出口:
/// - Broadcast → lazy subscribe topic + json encode + gossipsub.publish
/// - Unicast → rr_mp.send_request
///
/// `subscribed_topics` 是 caller 维护的 HashSet, 防止重复 subscribe.
/// gossipsub publish 失败时入 retry queue, 后续 heartbeat 调 retry_pending_broadcasts
/// 重试. (mesh 形成前 publish 会返 NoPeersSubscribedToTopic / InsufficientPeers,
/// actor 一锤子买卖, 不会重发 → KeyShare 丢失. 网络层保证可靠投递.)
#[derive(Default)]
pub struct PendingBroadcasts {
    queue: Vec<(String, MentalPokerMsg)>,
}

impl PendingBroadcasts {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

pub fn dispatch_swarm_command(
    swarm: &mut libp2p::Swarm<crate::net::p2p::behaviour::P2pBehaviour>,
    cmd: SwarmCommand,
    subscribed_topics: &mut std::collections::HashSet<String>,
) {
    dispatch_swarm_command_with_pending(
        swarm,
        cmd,
        subscribed_topics,
        &mut PendingBroadcasts::new(),
    )
}

/// 跟 dispatch_swarm_command 一样, 但失败的 Broadcast 入 pending 队列.
pub fn dispatch_swarm_command_with_pending(
    swarm: &mut libp2p::Swarm<crate::net::p2p::behaviour::P2pBehaviour>,
    cmd: SwarmCommand,
    subscribed_topics: &mut std::collections::HashSet<String>,
    pending: &mut PendingBroadcasts,
) {
    use libp2p::gossipsub;
    match cmd {
        SwarmCommand::Subscribe { topic } => {
            if subscribed_topics.insert(topic.clone()) {
                let ident = gossipsub::IdentTopic::new(&topic);
                if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&ident) {
                    tracing::warn!("mp_topic={topic} 订阅失败: {e}");
                }
            }
        }
        SwarmCommand::Broadcast { topic, msg } => {
            if subscribed_topics.insert(topic.clone()) {
                let ident = gossipsub::IdentTopic::new(&topic);
                if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&ident) {
                    tracing::warn!("mp_topic={topic} 订阅失败: {e}");
                }
            }
            if !try_publish(swarm, &topic, &msg) {
                // mesh 未稳, 入队等 retry.
                pending.queue.push((topic, msg));
            }
        }
        SwarmCommand::Unicast { target, msg } => {
            swarm.behaviour_mut().rr_mp.send_request(&target, msg);
        }
    }
}

/// 重试 pending 队列里所有 broadcast. 由 swarm task 在 heartbeat tick 调.
/// 成功的从队列移除, 失败的留下下次再试.
pub fn retry_pending_broadcasts(
    swarm: &mut libp2p::Swarm<crate::net::p2p::behaviour::P2pBehaviour>,
    pending: &mut PendingBroadcasts,
) {
    if pending.queue.is_empty() {
        return;
    }
    let mut still_pending = Vec::new();
    for (topic, msg) in std::mem::take(&mut pending.queue) {
        if !try_publish(swarm, &topic, &msg) {
            still_pending.push((topic, msg));
        }
    }
    pending.queue = still_pending;
}

/// 尝试 publish 一次. 返 true = 成功.
fn try_publish(
    swarm: &mut libp2p::Swarm<crate::net::p2p::behaviour::P2pBehaviour>,
    topic: &str,
    msg: &MentalPokerMsg,
) -> bool {
    use libp2p::gossipsub;
    let payload = match serde_json::to_vec(msg) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("mp Broadcast json encode 失败: {e}");
            return true; // 编码失败放弃, 别留队列里反复试
        }
    };
    let ident = gossipsub::IdentTopic::new(topic);
    match swarm.behaviour_mut().gossipsub.publish(ident, payload) {
        Ok(_) => true,
        Err(e) => {
            tracing::debug!("mp publish to {topic} pending: {e}");
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

    fn fake_peer_id(seed: u8) -> PeerId {
        // 派生一个 deterministic fake PeerId 给单测用 (不需要真 keypair)
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        let kp = libp2p::identity::Keypair::ed25519_from_bytes(bytes).expect("keypair");
        PeerId::from(&kp.public())
    }

    #[test]
    fn broadcast_sends_swarm_command() {
        let peer_map = vec![
            fake_peer_id(0),
            fake_peer_id(1),
            fake_peer_id(2),
            fake_peer_id(3),
        ];
        let (mut t, mut rx) = new_swarm_transport("kagetsu/mp/test/v1".into(), peer_map);
        let msg = MentalPokerMsg::KeyShare {
            peer_id: vec![1, 2, 3],
            pk: vec![],
            proof: vec![],
        };
        t.broadcast(msg.clone()).unwrap();
        let got = rx.try_recv().expect("got cmd");
        match got {
            SwarmCommand::Broadcast { topic, msg: _ } => {
                assert_eq!(topic, "kagetsu/mp/test/v1");
            }
            _ => panic!("expected Broadcast"),
        }
    }

    #[test]
    fn unicast_sends_swarm_command_with_target() {
        let p0 = fake_peer_id(10);
        let p1 = fake_peer_id(20);
        let p2 = fake_peer_id(30);
        let p3 = fake_peer_id(40);
        let peer_map = vec![p0, p1, p2, p3];
        let (mut t, mut rx) = new_swarm_transport("topic".into(), peer_map);
        let msg = MentalPokerMsg::DrawShareRequest {
            request_id: uuid::Uuid::nil(),
            ct: vec![],
            deck_index: 5,
        };
        t.unicast(2, msg).unwrap();
        let got = rx.try_recv().expect("got cmd");
        match got {
            SwarmCommand::Unicast { target, .. } => {
                assert_eq!(target, p2);
            }
            _ => panic!("expected Unicast"),
        }
    }

    #[test]
    fn unicast_invalid_target_returns_err() {
        let peer_map = vec![
            fake_peer_id(0),
            fake_peer_id(1),
            fake_peer_id(2),
            fake_peer_id(3),
        ];
        let (mut t, _rx) = new_swarm_transport("topic".into(), peer_map);
        let msg = MentalPokerMsg::DrawShareRequest {
            request_id: uuid::Uuid::nil(),
            ct: vec![],
            deck_index: 0,
        };
        let res = t.unicast(99, msg);
        assert!(matches!(res, Err(MpBridgeError::InvalidTarget(99, 4))));
    }

    /// **M5.D.3 真实 libp2p 4-swarm 集成 e2e**: 起 4 个真 libp2p swarm
    /// (TCP localhost) 互相 dial 形成 mesh, 4 个 MpPlayerActor + bridge +
    /// SwarmTransport 用 *真实* libp2p gossipsub + rr_mp 跑通 keygen + shuffle.
    ///
    /// 跟前面的 in-memory dispatcher 测试区别: 这次走完整 libp2p 数据路径
    /// (TCP transport + noise 加密 + yamux 复用 + gossipsub mesh + rr_mp protocol),
    /// 验证生产链路在真实 OS socket 下也 work.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_real_swarms_keygen_and_shuffle() {
        use crate::net::mp::MpPhase;
        use crate::net::mp::actor::{MpConfig, spawn_mp_player};
        use crate::net::mp::cmd::MpEvent;
        use crate::net::p2p::behaviour::{
            MP_TOPIC_PREFIX, P2pBehaviour, P2pBehaviourEvent, mp_topic_for_room,
        };
        use crate::net::p2p::mp_bridge::{MpInboundMsg, new_inbound_channel, spawn_mp_bridge};
        use crate::net::p2p::swarm::{build_swarm, new_keypair};
        use futures_util::StreamExt;
        use libp2p::request_response;
        use libp2p::swarm::SwarmEvent;
        use libp2p::{Multiaddr, multiaddr::Protocol};
        use std::collections::{HashMap, HashSet};
        use std::time::Duration;
        use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

        const N: usize = 4;
        let topic = mp_topic_for_room("real-swarm-test");

        // === Phase 1: build 4 swarm + listen TCP 0 + 收集 multiaddr ===
        let mut swarms = Vec::with_capacity(N);
        let mut peer_ids: Vec<libp2p::PeerId> = Vec::with_capacity(N);
        let mut listen_addrs: Vec<Multiaddr> = Vec::with_capacity(N);

        for i in 0..N {
            let kp = new_keypair();
            let pid = libp2p::PeerId::from(&kp.public());
            peer_ids.push(pid);
            let mut sw = build_swarm(kp, format!("test-{i}")).expect("build_swarm");
            sw.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .expect("listen");

            // 等第一个 NewListenAddr (TCP 通常 < 1s)
            let addr = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let SwarmEvent::NewListenAddr { address, .. } = sw.select_next_some().await {
                        return address.with(Protocol::P2p(pid));
                    }
                }
            })
            .await
            .expect("listen addr timeout");
            listen_addrs.push(addr);
            swarms.push(sw);
        }

        // === Phase 2: 单向 dial (只 i < j, 6 个 connection 而非 12) ===
        // 双向 dial 会触发 simultaneous open, libp2p TCP + noise 在同时 dial 时
        // handshake 偶尔会失败 (input error). 单向 dial 让 swarm[j] accept,
        // libp2p 自动 share connection 给 swarm[i] 反向使用.
        for i in 0..N {
            for j in (i + 1)..N {
                if let Err(e) = swarms[i].dial(listen_addrs[j].clone()) {
                    tracing::warn!("swarm[{i}] dial swarm[{j}] 失败: {e}");
                }
            }
        }

        // === Phase 3: spawn mp_swarm_task per swarm ===
        // 每个 swarm 持自己的 mp_command_rx + mp_inbound_tx, 跟 host_swarm_task
        // 简化版 (无 RoomActor / outbox / publish_lobby — 只跑 mp).
        let mut cmd_txs = Vec::with_capacity(N);
        let mut inbound_rxs: Vec<UnboundedReceiver<(libp2p::PeerId, MentalPokerMsg)>> =
            Vec::with_capacity(N);
        let mut shutdown_txs = Vec::with_capacity(N);

        for sw in swarms.into_iter() {
            let (cmd_tx, mut cmd_rx) = unbounded_channel::<SwarmCommand>();
            let (in_tx, in_rx) = unbounded_channel::<(libp2p::PeerId, MentalPokerMsg)>();
            let (sd_tx, mut sd_rx) = tokio::sync::oneshot::channel::<()>();
            cmd_txs.push(cmd_tx);
            inbound_rxs.push(in_rx);
            shutdown_txs.push(sd_tx);

            let mut sw = sw;
            tokio::spawn(async move {
                let mut subscribed: HashSet<String> = HashSet::new();
                let mut pending = super::PendingBroadcasts::new();
                let mut retry_tick = tokio::time::interval(Duration::from_millis(500));
                retry_tick.tick().await; // 跳过第一个 immediate tick
                loop {
                    tokio::select! {
                        biased;
                        _ = &mut sd_rx => break,
                        Some(cmd) = cmd_rx.recv() => {
                            super::dispatch_swarm_command_with_pending(&mut sw, cmd, &mut subscribed, &mut pending);
                        }
                        _ = retry_tick.tick() => {
                            super::retry_pending_broadcasts(&mut sw, &mut pending);
                        }
                        event = sw.select_next_some() => {
                            match event {
                                SwarmEvent::Behaviour(P2pBehaviourEvent::RrMp(
                                    request_response::Event::Message {
                                        peer,
                                        message: request_response::Message::Request {
                                            request, channel, ..
                                        },
                                        ..
                                    }
                                )) => {
                                    let _ = sw.behaviour_mut().rr_mp.send_response(channel, super::super::behaviour::Ack);
                                    let _ = in_tx.send((peer, request));
                                }
                                SwarmEvent::Behaviour(P2pBehaviourEvent::Gossipsub(
                                    libp2p::gossipsub::Event::Message {
                                        propagation_source,
                                        message,
                                        ..
                                    }
                                )) if message.topic.as_str().starts_with(MP_TOPIC_PREFIX) => {
                                    if let Ok(m) = serde_json::from_slice::<MentalPokerMsg>(&message.data) {
                                        let _ = in_tx.send((propagation_source, m));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                let _: &mut libp2p::Swarm<P2pBehaviour> = &mut sw;
            });
        }

        // === Phase 4: 显式 subscribe + 等 mesh 形成 ===
        // 各 swarm 先 explicit subscribe topic (不发数据), 然后 sleep 等 gossipsub
        // heartbeat (1s) × 几次让 mesh 在 4 节点间形成. 不能依赖 actor 第一次
        // broadcast 的 lazy subscribe — 那时收方还没 subscribe → 消息丢.
        for cmd_tx in &cmd_txs {
            let _ = cmd_tx.send(SwarmCommand::Subscribe {
                topic: topic.clone(),
            });
        }
        // gossipsub heartbeat 1s + mesh 选 peer 几个 heartbeat. 4 节点拓扑用 mesh_n=3
        // 配置 (见 behaviour.rs), 8s 应足够. 给 15s 留余量.
        tokio::time::sleep(Duration::from_secs(15)).await;

        // === Phase 5: spawn 4 个 MpPlayerActor + bridge + forward task ===
        let session_label = b"real-swarm-session".to_vec();
        let cfg_template = MpConfig {
            own_index: 0,
            all_peer_ids: peer_ids.iter().map(|p| p.to_bytes()).collect(),
            session_label: session_label.clone(),
            deck_size: 8,
            cnc_k_rounds: 4,
        };

        let mut handles = Vec::with_capacity(N);
        let mut event_rxs = Vec::with_capacity(N);
        let mut bridges = Vec::with_capacity(N);
        let mut forward_handles = Vec::with_capacity(N);

        for i in 0..N {
            let cfg = MpConfig {
                own_index: i,
                ..cfg_template.clone()
            };
            let mut player = spawn_mp_player(cfg, Some((i + 12345) as u64));
            let cmd_tx_actor = player.cmd_tx.clone();
            let event_rx = player.take_event_rx().unwrap();

            let (bridge_event_tx, bridge_event_rx) = unbounded_channel::<MpEvent>();
            let (test_event_tx, test_event_rx) = unbounded_channel::<MpEvent>();
            tokio::spawn(async move {
                let mut rx = event_rx;
                while let Some(ev) = rx.recv().await {
                    let _ = bridge_event_tx.send(ev.clone());
                    let _ = test_event_tx.send(ev);
                }
            });
            event_rxs.push(test_event_rx);

            // SwarmTransport 用 swarm[i] 的 mp_command_tx
            let transport =
                SwarmTransport::new(cmd_txs[i].clone(), topic.clone(), peer_ids.clone());

            // forward task: NetSession.mp_inbound_rx → MpInbound (反查 PeerId → idx)
            let (mp_inbound, mp_inbound_rx) = new_inbound_channel();
            let peer_to_idx: HashMap<libp2p::PeerId, usize> =
                peer_ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();
            let mut swarm_inbound_rx =
                std::mem::replace(&mut inbound_rxs[i], unbounded_channel().1);
            let forward = tokio::spawn(async move {
                while let Some((peer, msg)) = swarm_inbound_rx.recv().await {
                    let idx = peer_to_idx.get(&peer).copied();
                    let _ = mp_inbound.deliver(idx, msg);
                }
            });
            forward_handles.push(forward);

            let bridge = spawn_mp_bridge(transport, bridge_event_rx, cmd_tx_actor, mp_inbound_rx);
            handles.push(player);
            bridges.push(bridge);
        }

        // === Phase 6: 跑直到 all in Playing 或超时 ===
        let mut phases = [MpPhase::KeyExchange; N];
        let timeout = tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                for (i, rx) in event_rxs.iter_mut().enumerate() {
                    while let Ok(ev) = rx.try_recv() {
                        if let MpEvent::PhaseChanged { phase } = ev {
                            phases[i] = phase;
                        }
                        if let MpEvent::ProtocolError { offender, reason } = ev {
                            panic!("actor {i} ProtocolError offender={offender:?}: {reason}");
                        }
                    }
                }
                if phases.iter().all(|p| *p == MpPhase::Playing) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        let _ = MpInboundMsg {
            from: None,
            msg: MentalPokerMsg::KeyShare {
                peer_id: vec![],
                pk: vec![],
                proof: vec![],
            },
        }; // import sanity

        // === Phase 7: cleanup ===
        for sd in shutdown_txs {
            let _ = sd.send(());
        }
        drop(bridges);
        drop(handles);
        for f in forward_handles {
            f.abort();
        }

        assert!(
            timeout.is_ok() && timeout.unwrap(),
            "4 个真 libp2p swarm 应在 60s 内完成 keygen+shuffle, 实际 phases={phases:?}"
        );
    }

    /// **M5.C.1 in-memory swarm dispatcher e2e**: SwarmTransport + SwarmCommand
    /// 通过 mpsc dispatcher 模拟 libp2p swarm 行为 (gossipsub publish + rr send_request),
    /// 4 actor 跑通 keygen + 联合洗牌. 验证 SwarmTransport ↔ swarm wiring 链条可用,
    /// 不依赖真 libp2p swarm.
    ///
    /// dispatcher 充当 "假 swarm task" 角色:
    /// - 接 SwarmCommand::Broadcast{topic, msg} → 给所有 (除发送者) MpInbound deliver(msg)
    /// - 接 SwarmCommand::Unicast{target, msg} → 给 target 对应的 MpInbound deliver(msg)
    ///
    /// 跟 mp_bridge::MockTransport 区别: 这次走 SwarmTransport 路径 (生产链路),
    /// 验证 SwarmCommand 协议字段 (topic / target PeerId) 转发正确.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_actors_via_swarm_transport_dispatcher() {
        use crate::net::mp::MpPhase;
        use crate::net::mp::actor::{MpConfig, spawn_mp_player};
        use crate::net::mp::cmd::MpEvent;
        use crate::net::p2p::behaviour::mp_topic_for_room;
        use crate::net::p2p::mp_bridge::{
            MpInbound, MpInboundMsg, new_inbound_channel, spawn_mp_bridge,
        };
        use std::collections::HashMap;
        use std::time::Duration;
        use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

        const N: usize = 4;
        let topic = mp_topic_for_room("dispatcher-test");

        // 派生 4 个 fake PeerId — 每个对应一个 own_index.
        let peer_ids: Vec<PeerId> = (0..N as u8).map(fake_peer_id).collect();
        let peer_to_idx: HashMap<PeerId, usize> =
            peer_ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();

        // 各玩家的 MpInbound channel
        let mut inbound_pairs: Vec<(MpInbound, UnboundedReceiver<MpInboundMsg>)> =
            (0..N).map(|_| new_inbound_channel()).collect();
        let inbounds: Vec<MpInbound> = inbound_pairs.iter().map(|(i, _)| i.clone()).collect();

        // 各玩家创建 SwarmTransport + 拿 cmd_rx 给 dispatcher 用
        let mut transports = Vec::with_capacity(N);
        let mut cmd_rxs: Vec<UnboundedReceiver<SwarmCommand>> = Vec::with_capacity(N);
        for _ in 0..N {
            let (t, rx) = new_swarm_transport(topic.clone(), peer_ids.clone());
            transports.push(t);
            cmd_rxs.push(rx);
        }

        // spawn 4 actor
        let mut handles = Vec::with_capacity(N);
        let mut event_rxs = Vec::with_capacity(N);
        let mut bridges = Vec::with_capacity(N);
        let cfg_template = MpConfig {
            own_index: 0,
            all_peer_ids: peer_ids.iter().map(|p| p.to_bytes()).collect(),
            session_label: b"swarm-dispatcher-test".to_vec(),
            deck_size: 16,
            cnc_k_rounds: 8,
        };
        for i in 0..N {
            let cfg = MpConfig {
                own_index: i,
                all_peer_ids: cfg_template.all_peer_ids.clone(),
                session_label: cfg_template.session_label.clone(),
                deck_size: cfg_template.deck_size,
                cnc_k_rounds: cfg_template.cnc_k_rounds,
            };
            let mut player = spawn_mp_player(cfg, Some((i + 5000) as u64));
            let cmd_tx = player.cmd_tx.clone();
            let event_rx = player.take_event_rx().unwrap();

            // fan-out event_rx: bridge_rx + test_rx
            let (bridge_event_tx, bridge_event_rx) = unbounded_channel::<MpEvent>();
            let (test_event_tx, test_event_rx) = unbounded_channel::<MpEvent>();
            tokio::spawn(async move {
                let mut event_rx = event_rx;
                while let Some(ev) = event_rx.recv().await {
                    let _ = bridge_event_tx.send(ev.clone());
                    let _ = test_event_tx.send(ev);
                }
            });
            event_rxs.push(test_event_rx);

            let transport = transports.remove(0);
            let inbound_rx = inbound_pairs.remove(0).1;
            let bridge = spawn_mp_bridge(transport, bridge_event_rx, cmd_tx, inbound_rx);

            handles.push(player);
            bridges.push(bridge);
        }

        // dispatcher: 起 4 个 task, 各接一方 cmd_rx, 路由 SwarmCommand → MpInbound
        for i in 0..N {
            let mut cmd_rx = cmd_rxs.remove(0);
            let inbounds_clone = inbounds.clone();
            let peer_to_idx_clone = peer_to_idx.clone();
            let my_peer = peer_ids[i];
            tokio::spawn(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        SwarmCommand::Subscribe { topic: _ } => {
                            // in-memory dispatcher 不需要 subscribe (无 mesh).
                        }
                        SwarmCommand::Broadcast { topic: _, msg } => {
                            // libp2p gossipsub 默认不回环 (sender 不收自己的消息)
                            for (idx, inbound) in inbounds_clone.iter().enumerate() {
                                if idx == i {
                                    continue;
                                }
                                let _ = inbound.deliver(Some(i), msg.clone());
                            }
                            let _ = my_peer; // sanity: 自己 PeerId 跟 own_index i 一致
                        }
                        SwarmCommand::Unicast { target, msg } => {
                            if let Some(&target_idx) = peer_to_idx_clone.get(&target) {
                                let _ = inbounds_clone[target_idx].deliver(Some(i), msg);
                            }
                        }
                    }
                }
            });
        }

        // 跑到 all in Playing
        let mut phases = [MpPhase::KeyExchange; N];
        let timeout = tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                for (i, rx) in event_rxs.iter_mut().enumerate() {
                    while let Ok(ev) = rx.try_recv() {
                        if let MpEvent::PhaseChanged { phase } = ev {
                            phases[i] = phase;
                        }
                    }
                }
                if phases.iter().all(|p| *p == MpPhase::Playing) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert!(
            timeout.is_ok() && timeout.unwrap(),
            "4 actor 通过 SwarmTransport + dispatcher 应全 transition 到 Playing, 实际 {phases:?}"
        );

        drop(bridges);
        drop(handles);
    }

    #[test]
    fn closed_channel_returns_err() {
        let peer_map = vec![
            fake_peer_id(0),
            fake_peer_id(1),
            fake_peer_id(2),
            fake_peer_id(3),
        ];
        let (mut t, rx) = new_swarm_transport("t".into(), peer_map);
        drop(rx); // close
        let msg = MentalPokerMsg::KeyShare {
            peer_id: vec![],
            pk: vec![],
            proof: vec![],
        };
        let res = t.broadcast(msg);
        assert!(matches!(res, Err(MpBridgeError::Closed)));
    }
}
