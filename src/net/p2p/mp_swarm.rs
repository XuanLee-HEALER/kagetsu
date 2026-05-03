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
    /// 广播 (gossipsub publish).
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

#[cfg(test)]
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
        let (mut t, mut rx) = new_swarm_transport("tui-majo/mp/test/v1".into(), peer_map);
        let msg = MentalPokerMsg::KeyShare {
            peer_id: vec![1, 2, 3],
            pk: vec![],
            proof: vec![],
        };
        t.broadcast(msg.clone()).unwrap();
        let got = rx.try_recv().expect("got cmd");
        match got {
            SwarmCommand::Broadcast { topic, msg: _ } => {
                assert_eq!(topic, "tui-majo/mp/test/v1");
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
