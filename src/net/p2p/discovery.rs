//! 房间发现 — 两路:
//! 1. **LAN mDNS**: 同 WiFi 用 libp2p mDNS Behaviour 自动发现 peer, 然后通过
//!    identify 协议拿房间 metadata (agent_version 携带 host_nick=...;players=...).
//! 2. **公网 gossipsub**: 订阅 LOBBY_TOPIC, 房主每 5 秒 publish LobbyAnnouncement,
//!    大厅累积 + 30 秒过期淘汰.
//!
//! 两路结果合并到同一个 RoomEntry 列表, UI 不区分.
//!
//! [`RoomBrowser`] 由 UI 屏 own. 内部 spawn 一个 swarm task 跑两路发现.
//! UI 屏每帧 [`Self::poll`] 拉空内部事件并更新房间列表.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use libp2p::{Multiaddr, PeerId, gossipsub, identify, mdns, swarm::SwarmEvent};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::behaviour::{
    AGENT_PREFIX, LOBBY_TOPIC, LobbyAnnouncement, P2pBehaviourEvent, RELAYS_TOPIC,
    RelayAnnouncement,
};
use super::mode::RoomMode;
use super::region::Region;
use super::swarm::{build_swarm, new_keypair};

/// gossipsub announcement 超过这个时间没刷新视为下线 (从 rooms() / relays() 中过滤).
const LOBBY_ENTRY_TTL_MS: i64 = 30_000;
const RELAY_ENTRY_TTL_MS: i64 = 30_000;

/// 发现到的一个房间. 给 UI 显示用.
#[derive(Debug, Clone)]
pub struct RoomEntry {
    pub peer_id: PeerId,
    pub addrs: Vec<Multiaddr>,
    pub host_nick: String,
    pub players: u8,
    /// `lobby` / `in_game` 等. 字段名 `state` 兼容旧 UI 调用.
    pub state: String,
    pub room_id: String,
    /// 房间地理区域 (M3.E). LAN/mDNS 路径下发现的房间默认 Unknown.
    pub region: Region,
    /// 房间信任模式 (M4.B). LAN/mDNS 路径默认 Standard.
    pub mode: RoomMode,
}

impl RoomEntry {
    pub fn addr(&self) -> String {
        self.primary_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "?".into())
    }

    pub fn primary_addr(&self) -> Option<&Multiaddr> {
        self.addrs
            .iter()
            .find(|a| addr_is_quic(a))
            .or_else(|| self.addrs.first())
    }

    /// 拼一个完整 dial multiaddr (含 /p2p/<peer-id>).
    pub fn dial_multiaddr(&self) -> Option<Multiaddr> {
        let base = self.primary_addr()?.clone();
        Some(base.with(libp2p::multiaddr::Protocol::P2p(self.peer_id)))
    }
}

fn addr_is_quic(addr: &Multiaddr) -> bool {
    addr.iter()
        .any(|p| matches!(p, libp2p::multiaddr::Protocol::QuicV1))
}

#[derive(Default)]
struct BrowserState {
    addrs: HashMap<PeerId, Vec<Multiaddr>>,
    metadata: HashMap<PeerId, RoomMetadata>,
    /// M3.D: 收到的 Tier 2 玩家 relay 池.
    relays: HashMap<PeerId, RelayPoolEntry>,
}

#[derive(Debug, Clone)]
struct RelayPoolEntry {
    multiaddrs: Vec<Multiaddr>,
    last_seen_unix_ms: i64,
}

#[derive(Debug, Clone, Default)]
struct RoomMetadata {
    host_nick: String,
    players: u8,
    state: String,
    room_id: String,
    /// gossipsub announcement 路径填; mDNS 路径填 0 (永不过期).
    last_seen_unix_ms: i64,
    /// 房间地理区域 (M3.E). mDNS 路径目前不携带 region, 默认 Unknown.
    region: Region,
    /// 房间信任模式 (M4.B). mDNS 路径目前不携带 mode, 默认 Standard.
    mode: RoomMode,
}

/// Browser swarm task → UI 的事件.
#[derive(Debug, Clone)]
pub enum BrowserEvent {
    PeerFound {
        peer: PeerId,
        addr: Multiaddr,
    },
    PeerLost {
        peer: PeerId,
    },
    Identified {
        peer: PeerId,
        agent_version: String,
    },
    /// gossipsub 收到的房间 announcement.
    LobbyAnnouncement(LobbyAnnouncement),
    /// gossipsub 收到的 Tier 2 玩家 relay 池 announcement (M3.D).
    RelayAnnouncement(RelayAnnouncement),
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("swarm 启动失败: {0}")]
    Swarm(String),
}

/// 房间浏览器. UI own 一个, advance() 时 [`Self::poll`].
pub struct RoomBrowser {
    state: BrowserState,
    rx: Option<mpsc::UnboundedReceiver<BrowserEvent>>,
    _shutdown: Option<oneshot::Sender<()>>,
}

impl RoomBrowser {
    /// 启动浏览器. 在给定 tokio runtime 上 spawn swarm task.
    ///
    /// `bootstrap_relays` 是公网 lobby mesh 入口 — 大厅必须主动 dial 至少一个
    /// bootstrap, 才能跟其它客户端的 host swarm 建立 gossipsub mesh, 收到 publish
    /// 的房间 announcement. 不传时只走 LAN mDNS 路径 (跟 v1 行为一致).
    ///
    /// 注: build_swarm 内部 libp2p-quic (quinn) 初始化要 tokio runtime context,
    /// 由于本函数被 UI 同步线程调用, 必须先 runtime.enter() 进 context.
    pub fn start(
        runtime: &tokio::runtime::Handle,
        bootstrap_relays: Vec<Multiaddr>,
    ) -> Result<Self, BrowserError> {
        let _guard = runtime.enter();
        let kp = new_keypair();
        let mut swarm =
            build_swarm(kp, "browser".into()).map_err(|e| BrowserError::Swarm(e.to_string()))?;

        // listen 一个 QUIC 端口让 mDNS service 能注册地址.
        // 这是必要的: libp2p mDNS query 需要本地有 listen 才会发 mDNS announcement.
        if let Err(e) = swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap()) {
            tracing::warn!("browser listen QUIC 失败: {e}");
        }

        // 订阅 lobby topic 让 gossipsub 路径发现房间.
        let topic = gossipsub::IdentTopic::new(LOBBY_TOPIC);
        if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
            tracing::warn!("browser 订阅 lobby topic 失败: {e}");
        }
        // 订阅 relays topic 累积 Tier 2 玩家 relay 池 (M3.D).
        let relays_topic = gossipsub::IdentTopic::new(RELAYS_TOPIC);
        if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&relays_topic) {
            tracing::warn!("browser 订阅 relays topic 失败: {e}");
        }

        // 主动 dial bootstrap relay 让 gossipsub mesh 通过它们扩散.
        for addr in &bootstrap_relays {
            if let Err(e) = swarm.dial(addr.clone()) {
                tracing::warn!("browser dial bootstrap {addr} 失败: {e}");
            } else {
                tracing::info!("browser dialed bootstrap {addr}");
            }
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel::<BrowserEvent>();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        runtime.spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut shutdown_rx => break,
                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(P2pBehaviourEvent::Mdns(mdns_event)) => {
                                handle_mdns(&mut swarm, mdns_event, &event_tx);
                            }
                            SwarmEvent::Behaviour(P2pBehaviourEvent::Identify(
                                identify::Event::Received { peer_id, info, .. },
                            )) => {
                                let _ = event_tx.send(BrowserEvent::Identified {
                                    peer: peer_id,
                                    agent_version: info.agent_version,
                                });
                            }
                            SwarmEvent::Behaviour(P2pBehaviourEvent::Gossipsub(
                                gossipsub::Event::Message { message, .. },
                            )) => {
                                let topic_str = message.topic.as_str();
                                if topic_str == LOBBY_TOPIC {
                                    match serde_json::from_slice::<LobbyAnnouncement>(&message.data) {
                                        Ok(ann) => {
                                            let _ = event_tx
                                                .send(BrowserEvent::LobbyAnnouncement(ann));
                                        }
                                        Err(e) => {
                                            tracing::debug!(
                                                "lobby announcement 解析失败: {e}"
                                            );
                                        }
                                    }
                                } else if topic_str == RELAYS_TOPIC {
                                    match serde_json::from_slice::<RelayAnnouncement>(&message.data) {
                                        Ok(ann) => {
                                            let _ = event_tx
                                                .send(BrowserEvent::RelayAnnouncement(ann));
                                        }
                                        Err(e) => {
                                            tracing::debug!(
                                                "relay announcement 解析失败: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(Self {
            state: BrowserState::default(),
            rx: Some(event_rx),
            _shutdown: Some(shutdown_tx),
        })
    }

    /// 每帧调用. 拉空 rx 内事件并更新内部状态.
    pub fn poll(&mut self) {
        let Some(rx) = self.rx.as_mut() else {
            return;
        };
        while let Ok(ev) = rx.try_recv() {
            self.state.apply(ev);
        }
    }

    /// 当前可显示房间. gossipsub 路径填了 last_seen_unix_ms 的, 超过 TTL 则过滤掉.
    pub fn rooms(&self) -> Vec<RoomEntry> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut out: Vec<RoomEntry> = self
            .state
            .addrs
            .iter()
            .filter_map(|(peer, addrs)| {
                let md = self.state.metadata.get(peer)?;
                // gossipsub 路径 (last_seen_unix_ms > 0) 过期检查.
                if md.last_seen_unix_ms > 0 && now_ms - md.last_seen_unix_ms > LOBBY_ENTRY_TTL_MS {
                    return None;
                }
                Some(RoomEntry {
                    peer_id: *peer,
                    addrs: addrs.clone(),
                    host_nick: md.host_nick.clone(),
                    players: md.players,
                    state: md.state.clone(),
                    room_id: md.room_id.clone(),
                    region: md.region,
                    mode: md.mode,
                })
            })
            .collect();
        out.sort_by(|a, b| a.host_nick.cmp(&b.host_nick));
        out
    }

    /// M3.D: 当前累积的 Tier 2 玩家 relay 候选 (TTL 过期已过滤).
    ///
    /// 返回的 multiaddr 已含 `/p2p/<peer-id>`. UI 创建房间时合并到
    /// bootstrap_relays 减少对 Tier 1 单点依赖.
    pub fn relays(&self) -> Vec<Multiaddr> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut out: Vec<Multiaddr> = Vec::new();
        for (peer, entry) in &self.state.relays {
            if entry.last_seen_unix_ms > 0 && now_ms - entry.last_seen_unix_ms > RELAY_ENTRY_TTL_MS
            {
                continue;
            }
            for addr in &entry.multiaddrs {
                let full = if addr
                    .iter()
                    .any(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
                {
                    addr.clone()
                } else {
                    addr.clone().with(libp2p::multiaddr::Protocol::P2p(*peer))
                };
                if !out.contains(&full) {
                    out.push(full);
                }
            }
        }
        out
    }
}

fn handle_mdns(
    swarm: &mut libp2p::Swarm<super::behaviour::P2pBehaviour>,
    event: mdns::Event,
    event_tx: &mpsc::UnboundedSender<BrowserEvent>,
) {
    match event {
        mdns::Event::Discovered(list) => {
            for (peer, addr) in list {
                let _ = event_tx.send(BrowserEvent::PeerFound {
                    peer,
                    addr: addr.clone(),
                });
                // 主动 dial 触发 identify exchange. 失败不影响后续重试.
                if let Err(e) = swarm.dial(addr) {
                    tracing::debug!("browser dial 失败 (peer {peer}): {e}");
                }
            }
        }
        mdns::Event::Expired(list) => {
            for (peer, _) in list {
                let _ = event_tx.send(BrowserEvent::PeerLost { peer });
            }
        }
    }
}

impl BrowserState {
    fn apply(&mut self, ev: BrowserEvent) {
        match ev {
            BrowserEvent::PeerFound { peer, addr } => {
                let list = self.addrs.entry(peer).or_default();
                if !list.contains(&addr) {
                    list.push(addr);
                }
            }
            BrowserEvent::PeerLost { peer } => {
                self.addrs.remove(&peer);
                self.metadata.remove(&peer);
            }
            BrowserEvent::Identified {
                peer,
                agent_version,
            } => {
                if let Some(md) = parse_metadata(&agent_version) {
                    self.metadata.insert(peer, md);
                }
            }
            BrowserEvent::RelayAnnouncement(ann) => {
                let Ok(peer) = ann.peer_id.parse::<PeerId>() else {
                    return;
                };
                let parsed: Vec<Multiaddr> = ann
                    .multiaddrs
                    .iter()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if parsed.is_empty() {
                    return;
                }
                self.relays.insert(
                    peer,
                    RelayPoolEntry {
                        multiaddrs: parsed,
                        last_seen_unix_ms: ann.timestamp_unix_ms,
                    },
                );
            }
            BrowserEvent::LobbyAnnouncement(ann) => {
                let Ok(peer) = ann.host_peer_id.parse::<PeerId>() else {
                    return;
                };
                // 把 announcement 含的 multiaddrs 全部加进 addrs 列表.
                let list = self.addrs.entry(peer).or_default();
                for s in &ann.multiaddrs {
                    if let Ok(addr) = s.parse::<Multiaddr>()
                        && !list.contains(&addr)
                    {
                        list.push(addr);
                    }
                }
                self.metadata.insert(
                    peer,
                    RoomMetadata {
                        host_nick: ann.host_nick,
                        players: ann.players,
                        state: ann.lifecycle,
                        room_id: ann.room_id,
                        last_seen_unix_ms: ann.timestamp_unix_ms,
                        region: ann.region,
                        mode: ann.mode,
                    },
                );
            }
        }
    }
}

fn parse_metadata(agent_version: &str) -> Option<RoomMetadata> {
    let rest = agent_version.strip_prefix(AGENT_PREFIX)?;
    let mut md = RoomMetadata::default();
    for kv in rest.split(';') {
        if let Some((k, v)) = kv.split_once('=') {
            match k {
                "host_nick" => md.host_nick = v.to_string(),
                "players" => md.players = v.parse().unwrap_or(0),
                "lifecycle" | "state" => md.state = v.to_string(),
                "room_id" => md.room_id = v.to_string(),
                _ => {}
            }
        }
    }
    Some(md)
}

/// 把房间 metadata 编码成 identify agent_version 字符串 (不含前缀).
pub fn encode_metadata(host_nick: &str, players: u8, lifecycle: &str, room_id: &str) -> String {
    let san = |s: &str| s.replace([';', '='], "_");
    format!(
        "host_nick={};players={};lifecycle={};room_id={}",
        san(host_nick),
        players,
        san(lifecycle),
        san(room_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip() {
        let encoded = encode_metadata("Alice", 2, "lobby", "abcd-1234");
        let agent = format!("{AGENT_PREFIX}{encoded}");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.host_nick, "Alice");
        assert_eq!(md.players, 2);
        assert_eq!(md.state, "lobby");
        assert_eq!(md.room_id, "abcd-1234");
    }

    #[test]
    fn parse_rejects_non_tui_majo_agent() {
        assert!(parse_metadata("ipfs/0.1.0").is_none());
    }

    #[test]
    fn sanitize_handles_special_chars() {
        let m = encode_metadata("A;li=ce", 1, "lobby", "id");
        assert!(!m.contains("A;li=ce"));
        assert!(m.contains("A_li_ce"));
    }

    /// M3.D: BrowserState 累积有效 RelayAnnouncement, 并按 TTL 过滤 stale 条目.
    #[test]
    fn relay_pool_ttl_filtering() {
        let mut state = BrowserState::default();
        let kp = libp2p::identity::Keypair::generate_ed25519();
        let peer = PeerId::from(&kp.public());
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // 新鲜 announcement 应进 pool.
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec!["/ip4/8.8.8.8/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms,
        }));
        assert_eq!(state.relays.len(), 1);

        // 模拟 RoomBrowser.relays() 的 TTL 过滤逻辑.
        let entry = state.relays.get(&peer).unwrap();
        assert!(entry.last_seen_unix_ms > 0);
        assert_eq!(entry.multiaddrs.len(), 1);

        // stale (> 30s 前) announcement 进入 state 但 fresh-cutoff 时被排除.
        let kp_old = libp2p::identity::Keypair::generate_ed25519();
        let peer_old = PeerId::from(&kp_old.public());
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer_old.to_string(),
            multiaddrs: vec!["/ip4/9.9.9.9/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms - 60_000, // 60 秒前
        }));
        assert_eq!(state.relays.len(), 2);
        let stale_entry = state.relays.get(&peer_old).unwrap();
        assert!(now_ms - stale_entry.last_seen_unix_ms > RELAY_ENTRY_TTL_MS);
    }

    /// M3.D: 解析失败的 multiaddr 字符串不应入 pool, 但部分有效保留.
    #[test]
    fn relay_announcement_skips_invalid_addrs() {
        let mut state = BrowserState::default();
        let kp = libp2p::identity::Keypair::generate_ed25519();
        let peer = PeerId::from(&kp.public());
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec![
                "/ip4/8.8.8.8/udp/4001/quic-v1".into(),
                "garbage not multiaddr".into(),
            ],
            timestamp_unix_ms: 1,
        }));
        let entry = state.relays.get(&peer).expect("should exist");
        assert_eq!(entry.multiaddrs.len(), 1, "garbage 应被跳过");
    }

    /// M3.D: 全部 multiaddr 解析失败应不入 pool (避免空条目).
    #[test]
    fn relay_announcement_all_invalid_skipped() {
        let mut state = BrowserState::default();
        let kp = libp2p::identity::Keypair::generate_ed25519();
        let peer = PeerId::from(&kp.public());
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec!["garbage".into(), "also garbage".into()],
            timestamp_unix_ms: 1,
        }));
        assert!(state.relays.is_empty());
    }

    /// 回归: RoomBrowser::start 必须能从非 runtime context 的同步线程调用.
    /// (UI 屏从 ratatui sync 线程调时 quinn 初始化需要 runtime, 必须 enter)
    #[test]
    fn start_from_sync_thread_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        // 主线程不在 runtime context 里.
        let _br = RoomBrowser::start(rt.handle(), vec![]).expect("start");
        // drop runtime 让 spawn 的 task 退出.
    }

    // ============================================================================
    // helpers
    // ============================================================================

    fn make_peer() -> PeerId {
        let kp = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from(&kp.public())
    }

    fn make_entry(addrs: Vec<Multiaddr>) -> RoomEntry {
        RoomEntry {
            peer_id: make_peer(),
            addrs,
            host_nick: "h".into(),
            players: 0,
            state: "lobby".into(),
            room_id: "r".into(),
            region: Region::default(),
            mode: RoomMode::default(),
        }
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    impl RoomBrowser {
        /// 测试用: 直接构造一个带预填 state 的 browser, 跳过 swarm 启动.
        fn with_state_for_test(state: BrowserState) -> Self {
            Self {
                state,
                rx: None,
                _shutdown: None,
            }
        }
    }

    // ============================================================================
    // RoomEntry 字段 / addr 选择
    // ============================================================================

    #[test]
    fn room_entry_primary_addr_prefers_quic() {
        let entry = make_entry(vec![
            "/ip4/127.0.0.1/tcp/4001".parse().unwrap(),
            "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap(),
        ]);
        assert!(addr_is_quic(entry.primary_addr().unwrap()));
    }

    #[test]
    fn room_entry_primary_addr_no_quic_falls_back_to_first() {
        let entry = make_entry(vec!["/ip4/127.0.0.1/tcp/4001".parse().unwrap()]);
        let primary = entry.primary_addr().unwrap();
        assert!(!addr_is_quic(primary));
        assert_eq!(primary.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }

    #[test]
    fn room_entry_primary_addr_empty_returns_none_and_addr_returns_question_mark() {
        let entry = make_entry(vec![]);
        assert!(entry.primary_addr().is_none());
        assert_eq!(entry.addr(), "?");
        assert!(entry.dial_multiaddr().is_none());
    }

    #[test]
    fn room_entry_addr_returns_quic_string() {
        let entry = make_entry(vec!["/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap()]);
        assert!(entry.addr().contains("quic-v1"));
    }

    #[test]
    fn room_entry_dial_multiaddr_appends_p2p() {
        let entry = make_entry(vec!["/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap()]);
        let dial = entry.dial_multiaddr().unwrap();
        assert!(
            dial.iter()
                .any(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_))),
            "dial_multiaddr 应自动 append /p2p/<peer-id>"
        );
    }

    // ============================================================================
    // BrowserState::apply 各 variant
    // ============================================================================

    #[test]
    fn apply_peer_found_dedupes_addrs() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap();
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: addr.clone(),
        });
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: addr.clone(),
        });
        assert_eq!(state.addrs.get(&peer).unwrap().len(), 1);
    }

    #[test]
    fn apply_peer_lost_removes_addr_and_metadata() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap(),
        });
        state.apply(BrowserEvent::Identified {
            peer,
            agent_version: format!("{AGENT_PREFIX}host_nick=Z;players=1;lifecycle=lobby;room_id=t"),
        });
        assert!(state.addrs.contains_key(&peer));
        assert!(state.metadata.contains_key(&peer));

        state.apply(BrowserEvent::PeerLost { peer });
        assert!(!state.addrs.contains_key(&peer));
        assert!(!state.metadata.contains_key(&peer));
    }

    #[test]
    fn apply_identified_with_non_tui_majo_agent_does_not_overwrite() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::Identified {
            peer,
            agent_version: format!("{AGENT_PREFIX}host_nick=A;players=1;lifecycle=lobby;room_id=r"),
        });
        let before = state.metadata.get(&peer).unwrap().host_nick.clone();
        state.apply(BrowserEvent::Identified {
            peer,
            agent_version: "ipfs/0.1.0".into(),
        });
        assert_eq!(state.metadata.get(&peer).unwrap().host_nick, before);
    }

    #[test]
    fn apply_lobby_announcement_writes_metadata_and_addrs() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::LobbyAnnouncement(LobbyAnnouncement {
            schema_version: 1,
            host_peer_id: peer.to_string(),
            host_nick: "Host".into(),
            players: 2,
            lifecycle: "lobby".into(),
            room_id: "abc".into(),
            multiaddrs: vec!["/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms(),
            region: Region::CnEast,
            mode: RoomMode::ZeroTrust,
        }));
        let md = state.metadata.get(&peer).unwrap();
        assert_eq!(md.host_nick, "Host");
        assert_eq!(md.players, 2);
        assert_eq!(md.region, Region::CnEast);
        assert_eq!(md.mode, RoomMode::ZeroTrust);
        assert_eq!(state.addrs.get(&peer).unwrap().len(), 1);
    }

    #[test]
    fn apply_lobby_announcement_with_invalid_peer_id_is_no_op() {
        let mut state = BrowserState::default();
        state.apply(BrowserEvent::LobbyAnnouncement(LobbyAnnouncement {
            schema_version: 1,
            host_peer_id: "garbage".into(),
            host_nick: "X".into(),
            players: 1,
            lifecycle: "lobby".into(),
            room_id: "r".into(),
            multiaddrs: vec![],
            timestamp_unix_ms: 1,
            region: Region::Unknown,
            mode: RoomMode::Standard,
        }));
        assert!(state.metadata.is_empty());
        assert!(state.addrs.is_empty());
    }

    #[test]
    fn apply_lobby_announcement_skips_unparseable_addrs() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::LobbyAnnouncement(LobbyAnnouncement {
            schema_version: 1,
            host_peer_id: peer.to_string(),
            host_nick: "h".into(),
            players: 0,
            lifecycle: "lobby".into(),
            room_id: "r".into(),
            multiaddrs: vec!["garbage".into(), "/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms(),
            region: Region::Unknown,
            mode: RoomMode::Standard,
        }));
        assert_eq!(
            state.addrs.get(&peer).unwrap().len(),
            1,
            "garbage addr 应被跳过"
        );
    }

    #[test]
    fn apply_lobby_announcement_dedupes_existing_addrs() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        let same: Multiaddr = "/ip4/1.2.3.4/udp/4001/quic-v1".parse().unwrap();
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: same.clone(),
        });
        state.apply(BrowserEvent::LobbyAnnouncement(LobbyAnnouncement {
            schema_version: 1,
            host_peer_id: peer.to_string(),
            host_nick: "h".into(),
            players: 0,
            lifecycle: "lobby".into(),
            room_id: "r".into(),
            multiaddrs: vec![same.to_string()],
            timestamp_unix_ms: now_ms(),
            region: Region::Unknown,
            mode: RoomMode::Standard,
        }));
        assert_eq!(state.addrs.get(&peer).unwrap().len(), 1);
    }

    #[test]
    fn apply_relay_announcement_invalid_peer_id_is_no_op() {
        let mut state = BrowserState::default();
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: "garbage".into(),
            multiaddrs: vec!["/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: 1,
        }));
        assert!(state.relays.is_empty());
    }

    // ============================================================================
    // parse_metadata 边界
    // ============================================================================

    #[test]
    fn parse_metadata_lifecycle_alias() {
        let agent = format!("{AGENT_PREFIX}host_nick=Bob;players=1;lifecycle=in_game;room_id=x");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.state, "in_game");
    }

    #[test]
    fn parse_metadata_state_legacy_alias() {
        let agent = format!("{AGENT_PREFIX}host_nick=Bob;players=1;state=in_game;room_id=x");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.state, "in_game");
    }

    #[test]
    fn parse_metadata_unknown_keys_skipped() {
        let agent =
            format!("{AGENT_PREFIX}host_nick=Z;players=1;lifecycle=lobby;room_id=r;extra=ok");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.host_nick, "Z");
    }

    #[test]
    fn parse_metadata_invalid_players_clamps_to_zero() {
        let agent = format!("{AGENT_PREFIX}host_nick=Z;players=abc;lifecycle=lobby;room_id=r");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.players, 0);
    }

    #[test]
    fn parse_metadata_kv_without_eq_is_skipped() {
        let agent =
            format!("{AGENT_PREFIX}host_nick=Z;orphankey;players=1;lifecycle=lobby;room_id=r");
        let md = parse_metadata(&agent).unwrap();
        assert_eq!(md.host_nick, "Z");
    }

    // ============================================================================
    // RoomBrowser::rooms() / relays() / poll()
    // ============================================================================

    #[test]
    fn rooms_filters_peer_without_metadata() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap(),
        });
        let br = RoomBrowser::with_state_for_test(state);
        assert!(br.rooms().is_empty());
    }

    #[test]
    fn rooms_returns_mdns_entry_with_zero_timestamp() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::PeerFound {
            peer,
            addr: "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap(),
        });
        state.apply(BrowserEvent::Identified {
            peer,
            agent_version: format!(
                "{AGENT_PREFIX}host_nick=Bob;players=1;lifecycle=lobby;room_id=r"
            ),
        });
        let br = RoomBrowser::with_state_for_test(state);
        let rooms = br.rooms();
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].host_nick, "Bob");
    }

    #[test]
    fn rooms_filters_expired_gossipsub_entries() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::LobbyAnnouncement(LobbyAnnouncement {
            schema_version: 1,
            host_peer_id: peer.to_string(),
            host_nick: "Stale".into(),
            players: 1,
            lifecycle: "lobby".into(),
            room_id: "r".into(),
            multiaddrs: vec!["/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms() - 60_000, // 60s 前
            region: Region::Unknown,
            mode: RoomMode::Standard,
        }));
        let br = RoomBrowser::with_state_for_test(state);
        assert!(br.rooms().is_empty(), "60s 前的 announcement 应过期");
    }

    #[test]
    fn rooms_sorted_by_host_nick() {
        let mut state = BrowserState::default();
        let peers: Vec<PeerId> = (0..3).map(|_| make_peer()).collect();
        let nicks = ["Charlie", "Alice", "Bob"];
        for (i, peer) in peers.iter().enumerate() {
            state.apply(BrowserEvent::PeerFound {
                peer: *peer,
                addr: "/ip4/127.0.0.1/udp/4001/quic-v1".parse().unwrap(),
            });
            state.apply(BrowserEvent::Identified {
                peer: *peer,
                agent_version: format!(
                    "{AGENT_PREFIX}host_nick={};players=1;lifecycle=lobby;room_id=r{i}",
                    nicks[i],
                ),
            });
        }
        let br = RoomBrowser::with_state_for_test(state);
        let rooms = br.rooms();
        assert_eq!(rooms.len(), 3);
        assert_eq!(rooms[0].host_nick, "Alice");
        assert_eq!(rooms[1].host_nick, "Bob");
        assert_eq!(rooms[2].host_nick, "Charlie");
    }

    #[test]
    fn relays_returns_addr_with_p2p_appended_when_missing() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec!["/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms(),
        }));
        let br = RoomBrowser::with_state_for_test(state);
        let relays = br.relays();
        assert_eq!(relays.len(), 1);
        assert!(
            relays[0]
                .iter()
                .any(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_))),
            "relays() 应自动 append /p2p/ 段"
        );
    }

    #[test]
    fn relays_keeps_addr_with_existing_p2p() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        let addr_with_p2p = format!("/ip4/1.2.3.4/udp/4001/quic-v1/p2p/{peer}");
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec![addr_with_p2p.clone()],
            timestamp_unix_ms: now_ms(),
        }));
        let br = RoomBrowser::with_state_for_test(state);
        let relays = br.relays();
        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].to_string(), addr_with_p2p);
    }

    #[test]
    fn relays_filters_expired_entries() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec!["/ip4/1.2.3.4/udp/4001/quic-v1".into()],
            timestamp_unix_ms: now_ms() - 60_000,
        }));
        let br = RoomBrowser::with_state_for_test(state);
        assert!(br.relays().is_empty());
    }

    #[test]
    fn relays_dedupes_same_full_addr_across_announcements() {
        let mut state = BrowserState::default();
        let peer = make_peer();
        state.apply(BrowserEvent::RelayAnnouncement(RelayAnnouncement {
            schema_version: 1,
            peer_id: peer.to_string(),
            multiaddrs: vec![
                "/ip4/1.2.3.4/udp/4001/quic-v1".into(),
                "/ip4/1.2.3.4/udp/4001/quic-v1".into(),
            ],
            timestamp_unix_ms: now_ms(),
        }));
        let br = RoomBrowser::with_state_for_test(state);
        assert_eq!(br.relays().len(), 1, "同 multiaddr 应去重");
    }

    #[test]
    fn poll_with_no_rx_does_not_panic() {
        let mut br = RoomBrowser::with_state_for_test(BrowserState::default());
        br.poll();
    }
}
