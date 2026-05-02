//! 局域网房间 · lobby 阶段场景测试.

mod common;

use std::time::Duration;

use tui_majo::engine::rules::{GameRules, LengthRule};
use tui_majo::net::protocol::{ClientMsg, RoomLifecycle, ServerMsg};

use common::TestRoomBuilder;

/// 房主 join 后默认 ready, host_id 指向自己.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_alone_is_ready() {
    let room = TestRoomBuilder::new().humans(1).build_lobby().await;
    let host_view = room.clients[0].latest_room.as_ref().unwrap();
    assert_eq!(host_view.players.len(), 1);
    assert!(host_view.players[0].is_host);
    assert!(host_view.players[0].ready, "host 默认 ready");
    assert_eq!(host_view.host_id, room.clients[0].player_id);
    assert_eq!(host_view.state, RoomLifecycle::Lobby);
}

/// 4 人加入 lobby, host 自动 ready 其他人未 ready.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_humans_lobby() {
    let mut room = TestRoomBuilder::new().humans(4).build_lobby().await;
    let _ = room
        .wait_for_all(
            |c| {
                c.latest_room
                    .as_ref()
                    .map(|r| r.players.len() == 4)
                    .unwrap_or(false)
            },
            Duration::from_secs(1),
        )
        .await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert_eq!(r.players.len(), 4);
    assert!(r.players[0].is_host);
    assert!(r.players[0].ready);
    assert!(!r.players[1].ready);
    assert!(!r.players[2].ready);
    assert!(!r.players[3].ready);
}

/// 玩家 ready toggle 后房主应能看到更新.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn player_ready_toggles_broadcast() {
    let mut room = TestRoomBuilder::new().humans(2).build_lobby().await;
    room.client(1).send(ClientMsg::Ready { ready: true });
    room.drain_all().await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert!(r.players[1].ready, "Alice ready=true 后应该 ready");

    room.client(1).send(ClientMsg::Ready { ready: false });
    room.drain_all().await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert!(!r.players[1].ready, "Alice ready=false 后应该 unready");
}

/// 非房主 StartGame 应被忽略.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_host_start_game_ignored() {
    let mut room = TestRoomBuilder::new().humans(2).build_lobby().await;
    room.client(1).send(ClientMsg::Ready { ready: true });
    // Alice (非 host) 试图开始游戏
    room.client(1).send(ClientMsg::StartGame);
    room.drain_all().await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert_eq!(
        r.state,
        RoomLifecycle::Lobby,
        "非 host StartGame 应忽略, 留在 lobby"
    );
}

/// 房主 UpdateRules 应生效, 非房主忽略.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_can_update_config_others_cannot() {
    let mut room = TestRoomBuilder::new().humans(2).build_lobby().await;
    let new_cfg = GameRules {
        length: LengthRule::Tonpuusen,
        ..GameRules::default()
    };

    // 非 host 改 → 忽略
    room.client(1).send(ClientMsg::UpdateRules(new_cfg.clone()));
    room.drain_all().await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert!(
        matches!(r.config.length, LengthRule::Hanchan),
        "非 host 改 config 应忽略"
    );

    // host 改 → 生效
    room.host().send(ClientMsg::UpdateRules(new_cfg));
    room.drain_all().await;
    let r = room.clients[0].latest_room.as_ref().unwrap();
    assert!(
        matches!(r.config.length, LengthRule::Tonpuusen),
        "host 改 config 应生效"
    );
}

/// 第 5 个 humans 加入应被 RoomFull 拒绝 (用 join_with_token 直接试).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fifth_player_rejected_room_full() {
    use tui_majo::net::room::JoinError;
    use tui_majo::net::room::RoomCmd;

    let room = TestRoomBuilder::new().humans(4).build_lobby().await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
    room.handle
        .tx
        .send(RoomCmd::Join {
            nickname: "Eve".into(),
            reconnect_token: None,
            sender: tx,
            ack: ack_tx,
        })
        .unwrap();
    let result = ack_rx.await.unwrap();
    assert!(matches!(result, Err(JoinError::RoomFull)));
}

/// 房主 Leave 应让所有非房主收到 Error("房主已离开...").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_leave_dissolves_room() {
    let mut room = TestRoomBuilder::new().humans(3).build_lobby().await;
    room.host().leave();
    room.drain_all().await;
    let alice_saw_dissolve = room.clients[1]
        .has_msg(|m| matches!(m, ServerMsg::Error { message } if message.contains("房主")));
    let bob_saw_dissolve = room.clients[2]
        .has_msg(|m| matches!(m, ServerMsg::Error { message } if message.contains("房主")));
    assert!(alice_saw_dissolve, "Alice 应该收到房主已离开的 Error");
    assert!(bob_saw_dissolve, "Bob 应该收到房主已离开的 Error");
}

/// start_game 流程: 所有 ready 后 host StartGame, 应推 GameStateView 给所有人.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_game_pushes_state_view() {
    let room = TestRoomBuilder::new().humans(2).start_game().await;
    // 两个 client 都应该有 latest_view
    assert!(room.clients[0].latest_view.is_some());
    assert!(room.clients[1].latest_view.is_some());
    // 都应处于 InGame
    let host_room = room.clients[0].latest_room.as_ref().unwrap();
    assert_eq!(host_room.state, RoomLifecycle::InGame);
}
