//! MpRoomCmd / MpEvent — MpPlayerActor 边界消息 (M5.B.3).
//!
//! actor 通过 mpsc 接 cmd, 通过 mpsc 推 event 给 UI / 上层桥. 跟 LAN/Standard
//! [`crate::net::room::RoomCmd`] 区别:
//! - cmd 不区分"哪个 player_id 发的", 因为 MpPlayerActor 持自己 sk, 自己就是
//!   action source. 远端玩家的消息走 PeerMsg variant.
//! - event 不投影 GameStateView (Standard 隐藏他家手牌的概念), 因为 ZeroTrust
//!   下他家手牌密文对所有人都是公开的, 各自 GameState 镜像直接渲染.

use crate::domain::action::Action;
use crate::mental_poker::wire::MentalPokerMsg;

use super::phase::MpPhase;

/// 上层 (UI / P2P 桥) 发给 MpPlayerActor 的命令.
#[derive(Debug)]
pub enum MpRoomCmd {
    /// 收到 P2P peer 来的 mental poker 消息 (来自 P2P swarm task / mpsc 模拟桥).
    PeerMsg {
        /// 发送方 peer_id (在 actor 的 members 列表内的 index, 或 None 如果未知).
        from: Option<usize>,
        msg: MentalPokerMsg,
    },
    /// 本玩家 UI 触发的游戏动作 (Discard / Pon / Tsumo / Ron 等). Standard 模式
    /// 走 RoomActor 验证, ZeroTrust 模式 actor 自己 validate + broadcast.
    LocalAction(Action),
    /// 主动断线 / 退房间.
    Disconnect,
    /// 收到本地 sub-actor (e.g. shuffle session timeout) 触发的 tick.
    Tick,
}

/// MpPlayerActor 推给上层的事件.
#[derive(Debug, Clone)]
pub enum MpEvent {
    /// phase transition (UI 更新进度).
    PhaseChanged { phase: MpPhase },
    /// 协议 1 进度更新 (X 玩家完成 shuffle round).
    ShuffleProgress {
        /// 已完成轮数 / 总轮数.
        completed: u32,
        total: u32,
    },
    /// 准备发出 P2P 消息 (上层桥 / mpsc test 拿去发).
    OutboundMsg {
        /// 收件人: None = broadcast, Some(idx) = 单播给 members[idx].
        to: Option<usize>,
        msg: MentalPokerMsg,
    },
    /// 协议错误 (作弊检测 / proof 无效 / 远端不响应).
    /// 包含可疑玩家 index, 以便 UI / 上层选择踢人 / abort.
    ProtocolError {
        offender: Option<usize>,
        reason: String,
    },
    /// 一局结束 (流局 / 和牌).
    GameOver { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::meld::Seat;
    use crate::domain::tile::{Tile, TileIndex};

    #[test]
    fn local_action_carries_action_variant() {
        let cmd = MpRoomCmd::LocalAction(Action::Pass);
        match cmd {
            MpRoomCmd::LocalAction(Action::Pass) => {}
            _ => panic!("variant"),
        }
        let cmd = MpRoomCmd::LocalAction(Action::Ron(Seat::West));
        match cmd {
            MpRoomCmd::LocalAction(Action::Ron(s)) => assert_eq!(s, Seat::West),
            _ => panic!(),
        }
    }

    #[test]
    fn peer_msg_from_optional() {
        let cmd = MpRoomCmd::PeerMsg {
            from: Some(2),
            msg: MentalPokerMsg::KeyShare {
                peer_id: vec![],
                pk: vec![],
                proof: vec![],
            },
        };
        match cmd {
            MpRoomCmd::PeerMsg { from: Some(2), .. } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn event_outbound_broadcast_vs_unicast() {
        let bcast = MpEvent::OutboundMsg {
            to: None,
            msg: MentalPokerMsg::Discard {
                player: 0,
                deck_index: 5,
                plaintext: vec![],
            },
        };
        let dm = MpEvent::OutboundMsg {
            to: Some(2),
            msg: MentalPokerMsg::ConcealedKanReveal {
                plaintexts: [vec![], vec![], vec![], vec![]],
            },
        };
        match bcast {
            MpEvent::OutboundMsg { to: None, .. } => {}
            _ => panic!(),
        }
        match dm {
            MpEvent::OutboundMsg { to: Some(2), .. } => {}
            _ => panic!(),
        }
    }

    #[test]
    fn phase_changed_event() {
        let e = MpEvent::PhaseChanged {
            phase: MpPhase::Shuffling,
        };
        match e {
            MpEvent::PhaseChanged { phase } => assert_eq!(phase, MpPhase::Shuffling),
            _ => panic!(),
        }
    }

    /// 使用 Tile / TileIndex 让 enum 编译时 import 有效 (regression: 删 import 不应破坏).
    #[test]
    fn discard_with_tile() {
        let t = Tile {
            kind: TileIndex(5),
            red: false,
            id: 99,
        };
        let cmd = MpRoomCmd::LocalAction(Action::Discard(t));
        match cmd {
            MpRoomCmd::LocalAction(Action::Discard(tile)) => {
                assert_eq!(tile.id, 99);
            }
            _ => panic!(),
        }
    }
}
