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

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::net::protocol::{ClientMsg, ServerMsg};
use crate::net::room::{JoinError, JoinResult, RoomCmd, RoomHandle};

/// 简化抽象: UI 屏 own 一个 NetSession, send ClientMsg / try_recv ServerMsg.
pub struct NetSession {
    pub player_id: u32,
    pub token: Uuid,
    out_tx: UnboundedSender<ClientMsg>,
    in_rx: UnboundedReceiver<ServerMsg>,
}

impl NetSession {
    /// 直接用现成 channel 构造 (测试用, server bridge 内部用).
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
        }
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
}
