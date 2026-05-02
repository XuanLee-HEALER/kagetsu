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

use super::behaviour::{AGENT_PREFIX, LOBBY_TOPIC, LobbyAnnouncement, P2pBehaviourEvent};
use super::swarm::{build_swarm, new_keypair};

/// gossipsub announcement 超过这个时间没刷新视为下线 (从 rooms() 中过滤).
const LOBBY_ENTRY_TTL_MS: i64 = 30_000;

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
}

#[derive(Debug, Clone, Default)]
struct RoomMetadata {
    host_nick: String,
    players: u8,
    state: String,
    room_id: String,
    /// gossipsub announcement 路径填; mDNS 路径填 0 (永不过期).
    last_seen_unix_ms: i64,
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
    /// 注: build_swarm 内部 libp2p-quic (quinn) 初始化要 tokio runtime context,
    /// 由于本函数被 UI 同步线程调用, 必须先 runtime.enter() 进 context.
    pub fn start(runtime: &tokio::runtime::Handle) -> Result<Self, BrowserError> {
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
                                if message.topic.as_str() == LOBBY_TOPIC {
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
                })
            })
            .collect();
        out.sort_by(|a, b| a.host_nick.cmp(&b.host_nick));
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

    /// 回归: RoomBrowser::start 必须能从非 runtime context 的同步线程调用.
    /// (UI 屏从 ratatui sync 线程调时 quinn 初始化需要 runtime, 必须 enter)
    #[test]
    fn start_from_sync_thread_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        // 主线程不在 runtime context 里.
        let _br = RoomBrowser::start(rt.handle()).expect("start");
        // drop runtime 让 spawn 的 task 退出.
    }
}
