//! NetSession — 统一房主/远程 client 视角.
//!
//! UI 屏 ([`OnlineRoomState`], [`OnlineGameState`]) 不直接依赖
//! [`RoomHandle`] (那是房主特权) 也不直接依赖 WS, 而是 own 一个 [`NetSession`]:
//! - 房主进程用 [`local_session`] 构造 (内部 bridge task 把 `ClientMsg` 转
//!   成 `RoomCmd::PlayerMsg` 发给 RoomActor).
//! - 远程加入者用 [`crate::net::server::join_remote`] (Phase 5) 构造,
//!   内部 bridge task 把 ClientMsg 通过 ws send/接 ServerMsg.
//!
//! [`OnlineRoomState`]: crate::ui::screens::online_room::OnlineRoomState
//! [`OnlineGameState`]: crate::ui::screens::online_game::OnlineGameState
//! [`RoomHandle`]: crate::net::room::RoomHandle

use libp2p::PeerId;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::mental_poker::wire::MentalPokerMsg;
use crate::net::p2p::mp_swarm::SwarmCommand;
use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::room::{JoinError, JoinResult, RoomCmd, RoomHandle};

/// 简化抽象: UI 屏 own 一个 NetSession, send ClientMsg / try_recv ServerMsg.
///
/// ZeroTrust 模式还会带可选的 mp 边带 (M5.D.0): swarm task 暴露的
/// [`SwarmCommand`] 出口跟 P2P 入站消息流, ZeroTrustGameState 用它跑
/// mental poker 协议. Standard 模式两字段都 None.
pub struct NetSession {
    pub player_id: u32,
    pub token: Uuid,
    out_tx: UnboundedSender<ClientMsg>,
    in_rx: UnboundedReceiver<ServerMsg>,
    /// ZeroTrust mp 出口 (None = Standard 模式或未集成 P2P).
    pub mp_command_tx: Option<UnboundedSender<SwarmCommand>>,
    /// ZeroTrust mp 入站 (None = Standard 模式或已 take). take 后变 None.
    pub mp_inbound_rx: Option<UnboundedReceiver<(PeerId, MentalPokerMsg)>>,
}

impl NetSession {
    /// 直接用现成 channel 构造 (测试用, server bridge 内部用). Standard 模式 —
    /// 不带 mp 边带.
    pub fn from_channels(
        player_id: u32,
        token: Uuid,
        out_tx: UnboundedSender<ClientMsg>,
        in_rx: UnboundedReceiver<ServerMsg>,
    ) -> Self {
        Self {
            player_id,
            token,
            out_tx,
            in_rx,
            mp_command_tx: None,
            mp_inbound_rx: None,
        }
    }

    /// 注入 mp 边带 (远程 join_remote 内部调). caller 之后可 take mp_inbound_rx.
    pub fn with_mp_handles(
        mut self,
        mp_command_tx: UnboundedSender<SwarmCommand>,
        mp_inbound_rx: UnboundedReceiver<(PeerId, MentalPokerMsg)>,
    ) -> Self {
        self.mp_command_tx = Some(mp_command_tx);
        self.mp_inbound_rx = Some(mp_inbound_rx);
        self
    }

    pub fn send(&self, msg: ClientMsg) {
        let _ = self.out_tx.send(msg);
    }

    pub fn try_recv(&mut self) -> Option<ServerMsg> {
        match self.in_rx.try_recv() {
            Ok(m) => Some(m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn is_disconnected(&self) -> bool {
        self.out_tx.is_closed()
    }
}

/// 房主自己 join 本地 RoomActor: 调 [`spawn_local_session`], 它会发 Join cmd, 等
/// ack, 拿到 player_id/token 后构造 NetSession.
///
/// 内部还会 spawn 一个 task 把 UI 发的 ClientMsg → RoomCmd::PlayerMsg 转发给
/// RoomActor.
///
/// [`spawn_local_session`]: spawn_local_session
pub async fn spawn_local_session(
    handle: RoomHandle,
    nickname: String,
) -> Result<NetSession, JoinError> {
    let (s2c_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
    let (ack_tx, ack_rx) = oneshot::channel::<Result<JoinResult, JoinError>>();

    handle
        .tx
        .send(RoomCmd::Join {
            nickname,
            reconnect_token: None,
            sender: s2c_tx,
            ack: ack_tx,
        })
        .map_err(|_| JoinError::AlreadyInGame)?;

    let join = ack_rx.await.map_err(|_| JoinError::AlreadyInGame)??;

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let pid = join.player_id;
    let bridge_handle = handle.clone();
    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if bridge_handle
                .tx
                .send(RoomCmd::PlayerMsg {
                    player_id: pid,
                    msg,
                })
                .is_err()
            {
                break;
            }
        }
    });

    Ok(NetSession {
        player_id: join.player_id,
        token: join.reconnect_token,
        out_tx,
        in_rx,
        mp_command_tx: None,
        mp_inbound_rx: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::GameRules;
    use crate::net::protocol::{ClientMsg, RoomLifecycle, ServerMsg};
    use crate::net::room::spawn_room;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_session_round_trip() {
        let handle = spawn_room("Host".into(), GameRules::default());
        let mut sess = spawn_local_session(handle.clone(), "Host".into())
            .await
            .expect("join");
        assert_eq!(sess.player_id, 1);

        // 应该收到 Welcome
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut got_welcome = false;
        while let Some(msg) = sess.try_recv() {
            if let ServerMsg::Welcome { player_id, .. } = msg {
                got_welcome = true;
                assert_eq!(player_id, 1);
            }
        }
        assert!(got_welcome, "expected Welcome");

        // 发 Ready 后应收到 RoomUpdate
        sess.send(ClientMsg::Ready { ready: true });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut got_room_update = false;
        while let Some(msg) = sess.try_recv() {
            if let ServerMsg::RoomUpdate(view) = msg {
                got_room_update = true;
                assert_eq!(view.state, RoomLifecycle::Lobby);
                assert!(view.players[0].ready);
            }
        }
        assert!(got_room_update, "expected RoomUpdate after ready");
    }

    /// from_channels 直接 own channel, send / try_recv 简单回路.
    #[test]
    fn from_channels_send_and_try_recv() {
        use crate::engine::rules::GameRules;
        use crate::net::protocol::{RoomLifecycle, RoomView};

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
        let mut sess = NetSession::from_channels(7, Uuid::new_v4(), out_tx, in_rx);

        // try_recv 空 → None
        assert!(sess.try_recv().is_none());

        // 模拟 server 发 Welcome
        let token = Uuid::new_v4();
        let room = Box::new(RoomView {
            room_id: "r1".into(),
            host_id: 7,
            config: GameRules::default(),
            players: vec![],
            state: RoomLifecycle::Lobby,
            mode: crate::net::p2p::RoomMode::Standard,
        });
        in_tx
            .send(ServerMsg::Welcome {
                player_id: 7,
                reconnect_token: token,
                room,
            })
            .unwrap();
        match sess.try_recv() {
            Some(ServerMsg::Welcome {
                player_id,
                reconnect_token,
                ..
            }) => {
                assert_eq!(player_id, 7);
                assert_eq!(reconnect_token, token);
            }
            other => panic!("expected Welcome, got {other:?}"),
        }
        // 再 try_recv 应空
        assert!(sess.try_recv().is_none());

        // sess.send 推到 out_rx
        sess.send(ClientMsg::Ready { ready: true });
        match out_rx.try_recv() {
            Ok(ClientMsg::Ready { ready }) => assert!(ready),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    /// 远端 channel close 后 is_disconnected 应反映.
    #[test]
    fn is_disconnected_after_receiver_dropped() {
        let (out_tx, out_rx) = mpsc::unbounded_channel::<ClientMsg>();
        let (_in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
        let sess = NetSession::from_channels(1, Uuid::new_v4(), out_tx, in_rx);
        assert!(!sess.is_disconnected());
        drop(out_rx); // 远端 close
        assert!(sess.is_disconnected());
    }

    /// player_id / token 字段保留.
    #[test]
    fn player_id_and_token_are_preserved() {
        let token = Uuid::new_v4();
        let (out_tx, _) = mpsc::unbounded_channel::<ClientMsg>();
        let (_, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
        let sess = NetSession::from_channels(99, token, out_tx, in_rx);
        assert_eq!(sess.player_id, 99);
        assert_eq!(sess.token, token);
    }
}
