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
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::net::mp::MpPhase;
    use crate::net::mp::actor::{MpConfig, MpPlayerHandle, spawn_mp_player};
    use crate::net::mp::cmd::{MpEvent, MpRoomCmd};
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

    /// 启动 N 个 actor + N 个 bridge 用 MockTransport 互连. 返回:
    /// - handles: actor handle (持有保活)
    /// - cmd_txs: 给 caller 发 MpRoomCmd 用 (TriggerDraw / Discard / etc.)
    /// - test_event_rxs: 测试侧 event 流 (跟 bridge 各拿一份)
    /// - bridges: bridge handle (持有保活)
    #[allow(clippy::type_complexity)]
    fn spawn_n_actors_with_bridges(
        n: usize,
        seed_base: u64,
    ) -> (
        Vec<MpPlayerHandle>,
        Vec<UnboundedSender<MpRoomCmd>>,
        Vec<UnboundedReceiver<MpEvent>>,
        Vec<MpBridgeHandle>,
    ) {
        let mut inbound_pairs: Vec<(MpInbound, UnboundedReceiver<MpInboundMsg>)> =
            (0..n).map(|_| new_inbound_channel()).collect();
        let inbounds: Vec<MpInbound> = inbound_pairs.iter().map(|(i, _)| i.clone()).collect();

        let mut handles = Vec::with_capacity(n);
        let mut cmd_txs = Vec::with_capacity(n);
        let mut event_rxs_for_assert = Vec::with_capacity(n);
        let mut bridges = Vec::with_capacity(n);
        let mut cfg = test_cfg(0);
        cfg.deck_size = 36; // 大一点 deck 让多步流程 deck_index 0..30 都有效
        let mut cfg_template = cfg;
        for i in 0..n {
            cfg_template.own_index = i;
            let cfg_i = MpConfig {
                own_index: i,
                all_peer_ids: cfg_template.all_peer_ids.clone(),
                session_label: cfg_template.session_label.clone(),
                deck_size: cfg_template.deck_size,
                cnc_k_rounds: cfg_template.cnc_k_rounds,
            };
            let mut player = spawn_mp_player(cfg_i, Some(seed_base + i as u64));
            cmd_txs.push(player.cmd_tx.clone());
            let event_rx = player.take_event_rx().unwrap();

            // fan-out: actor event → bridge_event_rx + test_event_rx
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
            let bridge = spawn_mp_bridge(
                transport,
                bridge_event_rx,
                player.cmd_tx.clone(),
                inbound_rx,
            );

            handles.push(player);
            bridges.push(bridge);
        }
        (handles, cmd_txs, event_rxs_for_assert, bridges)
    }

    /// 推动 e2e 直到 cond 返回 true 或 max_steps 用完. drain 各 actor 的 event,
    /// 累积进 events_out 给 cond 判断.
    async fn drive_until<F>(
        rxs: &mut [UnboundedReceiver<MpEvent>],
        max_steps: usize,
        events_out: &mut Vec<(usize, MpEvent)>,
        mut cond: F,
    ) -> bool
    where
        F: FnMut(&[(usize, MpEvent)]) -> bool,
    {
        let n = rxs.len();
        for _step in 0..max_steps {
            let mut any = false;
            for src in 0..n {
                while let Ok(ev) = rxs[src].try_recv() {
                    any = true;
                    if let MpEvent::ProtocolError { offender, reason } = &ev {
                        panic!("actor {src} ProtocolError offender={offender:?}: {reason}");
                    }
                    events_out.push((src, ev));
                }
            }
            if cond(events_out) {
                return true;
            }
            if !any {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        false
    }

    /// 4 actor 通过 mp_bridge + MockTransport 互连, 跑通 keygen + 联合洗牌.
    /// 验证 bridge 的抽象不破坏协议正确性 — 4 actor 通过 mp_bridge + MockTransport
    /// 互连, 跑通 keygen + 联合洗牌.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_actors_via_mp_bridge_keygen_and_shuffle() {
        const N: usize = 4;
        let (handles, _cmd_txs, mut rxs, bridges) = spawn_n_actors_with_bridges(N, 7000);

        let mut events: Vec<(usize, MpEvent)> = Vec::new();
        let ok = drive_until(&mut rxs, 5000, &mut events, |evs| {
            let mut phase = [MpPhase::KeyExchange; N];
            for (src, ev) in evs {
                if let MpEvent::PhaseChanged { phase: p } = ev {
                    phase[*src] = *p;
                }
            }
            phase.iter().all(|p| *p == MpPhase::Playing)
        })
        .await;
        assert!(ok, "4 actor 通过 mp_bridge 应全 transition 到 Playing");

        drop(bridges);
        drop(handles);
    }

    /// **M5 in-memory mock e2e**: 4 actor 通过 mp_bridge + MockTransport 互连,
    /// 跑通完整一手 (协议 0+1+2+3+4+5+7). 验证整个 wiring 链条 work:
    ///   actor → MpEvent::OutboundMsg → bridge → MockTransport
    ///   → 对端 MpInbound → MpRoomCmd::PeerMsg → actor
    /// 跟 actor.rs::protocol_full_hand_e2e 跑相同流程, 但走 bridge 抽象.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn full_hand_e2e_via_mp_bridge() {
        const N: usize = 4;
        let (handles, cmd_txs, mut rxs, bridges) = spawn_n_actors_with_bridges(N, 8000);

        // Step 1: 跑到 all in Playing
        let mut events: Vec<(usize, MpEvent)> = Vec::new();
        let entered_playing = drive_until(&mut rxs, 5000, &mut events, |evs| {
            let mut phase = [MpPhase::KeyExchange; N];
            for (src, ev) in evs {
                if let MpEvent::PhaseChanged { phase: p } = ev {
                    phase[*src] = *p;
                }
            }
            phase.iter().all(|p| *p == MpPhase::Playing)
        })
        .await;
        assert!(entered_playing, "4 actor 应全 transition 到 Playing");

        // Step 2: actor 0 摸 deck[0], deck[1] (准备 Pon 的 2 张)
        events.clear();
        cmd_txs[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 0 })
            .unwrap();
        let drew_0 = drive_until(&mut rxs, 600, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 0 && matches!(e, MpEvent::DrawComplete { deck_index: 0, .. }))
        })
        .await;
        assert!(drew_0, "actor 0 应完成摸 deck[0]");

        events.clear();
        cmd_txs[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 1 })
            .unwrap();
        let drew_1 = drive_until(&mut rxs, 600, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 0 && matches!(e, MpEvent::DrawComplete { deck_index: 1, .. }))
        })
        .await;
        assert!(drew_1, "actor 0 应完成摸 deck[1]");

        // Step 3: actor 1 摸 deck[2] → 弃 deck[2]
        events.clear();
        cmd_txs[1]
            .send(MpRoomCmd::TriggerDraw { deck_index: 2 })
            .unwrap();
        let drew_2 = drive_until(&mut rxs, 600, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 1 && matches!(e, MpEvent::DrawComplete { deck_index: 2, .. }))
        })
        .await;
        assert!(drew_2, "actor 1 应完成摸 deck[2]");

        events.clear();
        cmd_txs[1]
            .send(MpRoomCmd::Discard { deck_index: 2 })
            .unwrap();
        let discard_2_done = drive_until(&mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::DiscardApplied {
                            player: 1,
                            deck_index: 2,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(
            discard_2_done,
            "4 actor 应都收 DiscardApplied(player=1, deck=2)"
        );

        // Step 4: actor 0 Pon [0,1,2] from=1
        events.clear();
        cmd_txs[0]
            .send(MpRoomCmd::Call {
                call_type: crate::mental_poker::wire::WireCallType::Pon,
                deck_indices: vec![0, 1, 2],
                from_player: 1,
                from_position_in_meld: 2,
            })
            .unwrap();
        let pon_done = drive_until(&mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::CallApplied {
                            player: 0,
                            from_player: 1,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(pon_done, "4 actor 应都收 CallApplied(player=0, from=1)");

        // Step 5: actor 0 揭示 deck[15] (dora indicator), 4 actor 收同 tile_id
        events.clear();
        cmd_txs[0]
            .send(MpRoomCmd::TriggerReveal { deck_index: 15 })
            .unwrap();
        let reveal_done = drive_until(&mut rxs, 800, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| matches!(e, MpEvent::RevealComplete { deck_index: 15, .. }))
                .count()
                == N
        })
        .await;
        assert!(reveal_done, "4 actor 应都收 RevealComplete(deck=15)");
        let reveal_tids: Vec<usize> = events
            .iter()
            .filter_map(|(_, e)| match e {
                MpEvent::RevealComplete {
                    deck_index: 15,
                    tile_id,
                } => Some(*tile_id),
                _ => None,
            })
            .collect();
        let dora_tid = reveal_tids[0];
        for tid in &reveal_tids {
            assert_eq!(*tid, dora_tid, "4 actor 看到的 dora tile_id 应一致");
        }

        // Step 6: actor 0 摸 deck[3,4,5]
        for idx in [3u32, 4, 5] {
            events.clear();
            cmd_txs[0]
                .send(MpRoomCmd::TriggerDraw { deck_index: idx })
                .unwrap();
            let ok = drive_until(&mut rxs, 600, &mut events, |evs| {
                evs.iter().any(|(s, e)| {
                    *s == 0
                        && matches!(e, MpEvent::DrawComplete { deck_index: di, .. } if *di == idx)
                })
            })
            .await;
            assert!(ok, "actor 0 应完成摸 deck[{idx}]");
        }

        // Step 7: actor 0 Tsumo, hand=[3,4,5], winning=5
        events.clear();
        cmd_txs[0]
            .send(MpRoomCmd::Tsumo {
                hand_indices: vec![3, 4, 5],
                winning_tile_index: 5,
            })
            .unwrap();
        let win_done = drive_until(&mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::WinValidated {
                            player: 0,
                            is_tsumo: true,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(win_done, "4 actor 应都收 WinValidated(player=0, tsumo)");

        // 各 actor 进 GameOver
        let game_over_count = events
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    MpEvent::PhaseChanged {
                        phase: MpPhase::GameOver
                    }
                )
            })
            .count();
        assert!(
            game_over_count >= N,
            "至少 {N} 个 PhaseChanged(GameOver), 实际 {game_over_count}"
        );

        drop(bridges);
        drop(handles);
    }
}
