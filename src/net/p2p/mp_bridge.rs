//! Mp bridge — 把 [`crate::net::mp::actor::MpPlayerActor`] 跟 P2P 网络层解耦.
//!
//! ## 设计
//!
//! [`crate::net::mp::actor::MpPlayerActor`] 内部用 cmd/event channel 工作:
//! - 收 [`MpRoomCmd::PeerMsg`] (来自其他玩家的 [`MentalPokerMsg`])
//! - 发 [`MpEvent::OutboundMsg`] (要发出去的消息, 含 to 字段决定广播 / 单播)
//!
//! mp_bridge 把这两端接到 P2P 传输层:
//! - [`MpEvent::OutboundMsg`] (to=None) → broadcast (libp2p gossipsub publish)
//! - [`MpEvent::OutboundMsg`] (to=Some(idx)) → unicast (libp2p request-response)
//! - 入站 P2P 消息 (gossipsub message + rr request) → [`MpRoomCmd::PeerMsg`]
//!
//! 协议层正确性可独立测试 (不需起 4 个真实 swarm), bridge 用抽象
//! [`MpTransport`] trait 表示"出口". 测试用 [`MockTransport`] 用 mpsc 模拟,
//! 生产用 swarm-bound 实现 (M5.B.8.4 加).

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::mental_poker::wire::MentalPokerMsg;
use crate::net::mp::cmd::{MpEvent, MpRoomCmd};

/// mp_bridge 出口 — 决定 OutboundMsg 怎么走到对端.
pub trait MpTransport: Send + 'static {
    /// 广播 (gossipsub publish 或测试 mock 的全连接 fan-out).
    fn broadcast(&mut self, msg: MentalPokerMsg) -> Result<(), MpBridgeError>;
    /// 单播给 own_index = idx 的对方 (rr.send_request 或 mock channel).
    fn unicast(&mut self, target_idx: usize, msg: MentalPokerMsg) -> Result<(), MpBridgeError>;
}

#[derive(Debug, thiserror::Error)]
pub enum MpBridgeError {
    #[error("transport closed")]
    Closed,
    #[error("target index {0} 越界 (n_players={1})")]
    InvalidTarget(usize, usize),
    #[error("publish 失败: {0}")]
    Publish(String),
}

/// Bridge handle: drop 时自动 abort spawn 的 task.
pub struct MpBridgeHandle {
    task: Option<JoinHandle<()>>,
}

impl Drop for MpBridgeHandle {
    fn drop(&mut self) {
        if let Some(h) = self.task.take() {
            h.abort();
        }
    }
}

/// Spawn mp_bridge: 接 actor.event_rx, 路由 OutboundMsg → transport,
/// 接 P2P 入站消息 → MpRoomCmd::PeerMsg → actor_cmd_tx.
///
/// `inbound_rx` 由 caller 控制 — 把从 swarm 收到的 [`MentalPokerMsg`] 塞进去.
/// 测试可用 [`MockTransport`] 互相把 broadcast 直接发到对端的 inbound_tx.
pub fn spawn_mp_bridge<T: MpTransport>(
    mut transport: T,
    mut event_rx: UnboundedReceiver<MpEvent>,
    actor_cmd_tx: UnboundedSender<MpRoomCmd>,
    mut inbound_rx: UnboundedReceiver<MpInboundMsg>,
) -> MpBridgeHandle {
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                ev = event_rx.recv() => {
                    let Some(ev) = ev else { break; };
                    if let MpEvent::OutboundMsg { to, msg } = ev {
                        let res = match to {
                            None => transport.broadcast(msg),
                            Some(idx) => transport.unicast(idx, msg),
                        };
                        if let Err(e) = res {
                            tracing::warn!("mp_bridge transport send failed: {e}");
                        }
                    }
                }
                inbound = inbound_rx.recv() => {
                    let Some(MpInboundMsg { from, msg }) = inbound else { break; };
                    if actor_cmd_tx.send(MpRoomCmd::PeerMsg { from, msg }).is_err() {
                        break;
                    }
                }
            }
        }
        tracing::debug!("mp_bridge task exited");
    });
    MpBridgeHandle { task: Some(task) }
}

/// 入站消息容器. swarm task 收 gossipsub message / rr request 后构造.
#[derive(Debug)]
pub struct MpInboundMsg {
    /// 发送方 own_index (从 PeerId 反查 cfg.all_peer_ids 得来). None 表示未知.
    pub from: Option<usize>,
    pub msg: MentalPokerMsg,
}

/// `MpInbound` 是给 caller 把 inbound 消息推给 bridge 的句柄. clone 即可
/// 跨 task 用.
#[derive(Clone)]
pub struct MpInbound {
    tx: UnboundedSender<MpInboundMsg>,
}

impl MpInbound {
    pub fn deliver(&self, from: Option<usize>, msg: MentalPokerMsg) -> Result<(), MpBridgeError> {
        self.tx
            .send(MpInboundMsg { from, msg })
            .map_err(|_| MpBridgeError::Closed)
    }
}

/// 创建 (inbound, inbound_rx) 一对. caller 自己保留 inbound 给 swarm task,
/// inbound_rx 传给 [`spawn_mp_bridge`].
pub fn new_inbound_channel() -> (MpInbound, UnboundedReceiver<MpInboundMsg>) {
    let (tx, rx) = unbounded_channel();
    (MpInbound { tx }, rx)
}

// ============================================================================
// MockTransport — 测试用, mpsc 模拟全连接
// ============================================================================

/// 测试用 transport: broadcast/unicast 都通过 [`MpInbound`] 模拟 swarm 出口.
/// `inbounds[i]` = player i 的 inbound. broadcast 跳过 own_index.
pub struct MockTransport {
    own_index: usize,
    inbounds: Vec<MpInbound>,
}

impl MockTransport {
    pub fn new(own_index: usize, inbounds: Vec<MpInbound>) -> Self {
        Self {
            own_index,
            inbounds,
        }
    }
}

impl MpTransport for MockTransport {
    fn broadcast(&mut self, msg: MentalPokerMsg) -> Result<(), MpBridgeError> {
        for (idx, inbound) in self.inbounds.iter().enumerate() {
            if idx == self.own_index {
                continue;
            }
            inbound.deliver(Some(self.own_index), msg.clone())?;
        }
        Ok(())
    }

    fn unicast(&mut self, target_idx: usize, msg: MentalPokerMsg) -> Result<(), MpBridgeError> {
        let n = self.inbounds.len();
        if target_idx >= n {
            return Err(MpBridgeError::InvalidTarget(target_idx, n));
        }
        if target_idx == self.own_index {
            return Ok(());
        }
        self.inbounds[target_idx].deliver(Some(self.own_index), msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::mp::MpPhase;
    use crate::net::mp::actor::{MpConfig, spawn_mp_player};
    use crate::net::mp::cmd::MpEvent;
    use std::time::Duration;

    fn test_cfg(own: usize) -> MpConfig {
        MpConfig {
            own_index: own,
            all_peer_ids: vec![
                b"p0".to_vec(),
                b"p1".to_vec(),
                b"p2".to_vec(),
                b"p3".to_vec(),
            ],
            session_label: b"bridge-test".to_vec(),
            deck_size: 16,
            cnc_k_rounds: 8,
        }
    }

    /// 4 actor 通过 mp_bridge + MockTransport 互连, 跑通 keygen + 联合洗牌.
    /// 验证 bridge 的抽象不破坏协议正确性 — 跟 actor.rs 内 inline mpsc 桥接
    /// 走相同 outcome.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_actors_via_mp_bridge_keygen_and_shuffle() {
        const N: usize = 4;

        // 1. 创建 4 个 inbound channel
        let mut inbound_pairs: Vec<(MpInbound, UnboundedReceiver<MpInboundMsg>)> =
            (0..N).map(|_| new_inbound_channel()).collect();
        let inbounds: Vec<MpInbound> = inbound_pairs.iter().map(|(i, _)| i.clone()).collect();

        // 2. spawn 4 actor + 4 bridge
        let mut handles = Vec::with_capacity(N);
        let mut event_rxs_for_assert = Vec::with_capacity(N);
        let mut bridges = Vec::with_capacity(N);
        for i in 0..N {
            let mut player = spawn_mp_player(test_cfg(i), Some((i + 7000) as u64));
            let cmd_tx = player.cmd_tx.clone();
            let event_rx = player.take_event_rx().unwrap();

            // 用 fan-out: 拿一个 mpsc 把 actor event 拷一份给测试 + 一份给 bridge.
            let (bridge_event_tx, bridge_event_rx) = unbounded_channel::<MpEvent>();
            let (test_event_tx, test_event_rx) = unbounded_channel::<MpEvent>();
            tokio::spawn(async move {
                let mut event_rx = event_rx;
                while let Some(ev) = event_rx.recv().await {
                    let _ = bridge_event_tx.send(ev.clone());
                    let _ = test_event_tx.send(ev);
                }
            });
            event_rxs_for_assert.push(test_event_rx);

            let transport = MockTransport::new(i, inbounds.clone());
            let inbound_rx = inbound_pairs.remove(0).1;
            let bridge = spawn_mp_bridge(transport, bridge_event_rx, cmd_tx, inbound_rx);

            // 保留 player handle 让 actor 不退出 (drop = shutdown)
            handles.push(player);
            bridges.push(bridge);
        }

        // 3. 收集每方 phase, 等到 all in Playing
        let mut phases = [MpPhase::KeyExchange; N];
        let timeout = tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                for (i, rx) in event_rxs_for_assert.iter_mut().enumerate() {
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
            "4 actor 通过 mp_bridge 应全 transition 到 Playing, 实际 {phases:?}"
        );

        // 4. cleanup
        drop(bridges);
        drop(handles);
    }
}
