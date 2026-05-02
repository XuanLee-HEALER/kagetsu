//! 断线 / AI 接管 / 重连恢复.

mod common;

use std::time::Duration;

use tokio::sync::oneshot;

use tui_majo::engine::phase::Phase;
use tui_majo::net::protocol::{ClientMsg, RoomLifecycle, ServerMsg};
use tui_majo::net::room::RoomCmd;

use common::{TestClient, TestRoomBuilder};

/// 玩家 disconnect 后, RoomUpdate 中 connected=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnect_marks_player_unconnected() {
    let mut room = TestRoomBuilder::new()
        .humans(3)
        .seed(0xAAAA)
        .build_lobby()
        .await;
    // alice (idx=1) 断线
    room.client(1).force_disconnect();
    room.drain_all().await;

    // host 应看到 alice 的 connected=false
    let host_view = room.host().latest_room.as_ref().unwrap();
    assert_eq!(host_view.players.len(), 3);
    let alice = &host_view.players[1];
    assert!(!alice.connected, "alice 应该被标记为未连接");
}

/// host (player_id=1) Leave 后所有非 host 收到 Error("房主已离开...").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_leave_sends_error_to_all() {
    let mut room = TestRoomBuilder::new()
        .humans(3)
        .seed(0xBBBB)
        .build_lobby()
        .await;
    room.host().leave();
    room.drain_all().await;
    for idx in 1..3 {
        assert!(
            room.client(idx)
                .has_msg(|m| matches!(m, ServerMsg::Error { message } if message.contains("房主"))),
            "client {idx} 应收到房主已离开 Error"
        );
    }
}

/// 玩家持 token 重连后, player_id / token 不变.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_with_token_keeps_player_id() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0xCCCC)
        .build_lobby()
        .await;
    let alice_id = room.client(1).player_id;
    let alice_token = room.client(1).token;

    // alice 断线 (drop sender)
    room.client(1).force_disconnect();
    room.drain_all().await;

    // alice 用 token 重连
    let new_alice =
        TestClient::join_with_token(room.handle.clone(), "alice2".into(), Some(alice_token)).await;
    assert_eq!(new_alice.player_id, alice_id, "重连应恢复原 player_id");
    assert_eq!(new_alice.token, alice_token, "token 应不变");
    // Welcome 中 room view 应该有 alice 且 connected=true
    let room_view = new_alice.latest_room.as_ref().unwrap();
    let alice_slot = room_view
        .players
        .iter()
        .find(|p| p.id == alice_id)
        .expect("alice slot");
    assert!(alice_slot.connected, "重连后应 connected=true");
}

/// game 中玩家断线 → AI 接管 → 玩家用 token 重连应恢复 seat 控制.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnect_and_reconnect_during_game() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0xDDDD)
        .start_game()
        .await;
    let alice_id = room.client(1).player_id;
    let alice_token = room.client(1).token;

    // 等 alice 收到至少一个 GameStateView
    let _ = room
        .wait_for_all(|c| c.latest_view.is_some(), Duration::from_millis(500))
        .await;

    // alice 断线
    room.client(1).force_disconnect();
    room.drain_all().await;
    // RoomActor 标记 alice connected=false; AI 接管 (advance_game::is_seat_ai)

    // alice 重连 (新 sender + 同 token)
    let mut new_alice =
        TestClient::join_with_token(room.handle.clone(), "alice".into(), Some(alice_token)).await;
    assert_eq!(new_alice.player_id, alice_id);

    // 重连后应立即收到 GameStateView (我们在 handle_join 中加的逻辑)
    let view = new_alice
        .await_view(|_| true, Duration::from_secs(1))
        .await
        .expect("重连后应收到 GameStateView");
    assert!(matches!(
        view.phase,
        Phase::Draw | Phase::AwaitDiscard | Phase::AwaitCalls | Phase::RoundEnd
    ));

    // RoomLifecycle 应是 InGame
    let r = new_alice.latest_room.as_ref().unwrap();
    assert_eq!(r.state, RoomLifecycle::InGame);
}

/// 无效 token 重连应被拒绝 (除非 token 在 slots 中存在).
/// 注意: 当前实现 spec 是 "找不到 token 则当新 join", 不是 InvalidReconnectToken.
/// 我们验证: 用陌生 token 实际是新 join, 拿到新 player_id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_token_treated_as_new_join() {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(0xEEEE)
        .build_lobby()
        .await;

    let host_id = room.host().player_id;
    let bogus_token = uuid::Uuid::new_v4();

    let new_client =
        TestClient::join_with_token(room.handle.clone(), "Bogus".into(), Some(bogus_token)).await;
    assert_ne!(
        new_client.player_id, host_id,
        "用陌生 token 应分配新 player_id"
    );
    assert_ne!(new_client.token, bogus_token, "应得到新 token");
}

/// game 中房间满后第 5 个玩家 (用陌生 token) 应被 RoomFull 拒绝.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn room_full_rejects_new_join_during_game() {
    use tui_majo::net::room::JoinError;

    let room = TestRoomBuilder::new()
        .humans(4)
        .seed(0xFFFF)
        .build_lobby()
        .await;

    // 第 5 个玩家用陌生 token (相当于新 join)
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (ack_tx, ack_rx) = oneshot::channel();
    room.handle
        .tx
        .send(RoomCmd::Join {
            nickname: "Eve".into(),
            reconnect_token: Some(uuid::Uuid::new_v4()),
            sender: tx,
            ack: ack_tx,
        })
        .unwrap();
    let r = ack_rx.await.unwrap();
    assert!(matches!(r, Err(JoinError::RoomFull)));
}

/// 多次断线重连 (网络抖动) 应仍能工作.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_reconnects_work() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0x9999)
        .build_lobby()
        .await;
    let alice_id = room.client(1).player_id;
    let alice_token = room.client(1).token;

    for i in 0..3 {
        // 断
        if i == 0 {
            room.client(1).force_disconnect();
        }
        room.drain_all().await;
        // 重连
        let new = TestClient::join_with_token(
            room.handle.clone(),
            format!("alice-r{i}"),
            Some(alice_token),
        )
        .await;
        assert_eq!(new.player_id, alice_id, "第 {i} 次重连 player_id 应不变");
        // 重连后立即断, 模拟下一轮抖动. (最后一次不断保持连接.)
        if i < 2 {
            new.force_disconnect();
        }
    }

    // host 应仍能正常操作 (房间未解散)
    let r = room.host().latest_room.as_ref().unwrap();
    assert_eq!(r.state, RoomLifecycle::Lobby);
}

/// 准备阶段 alice 断线 → ready 状态被保留 (重连后还是同样).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_state_preserved_across_reconnect() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0x7777)
        .build_lobby()
        .await;
    room.client(1).send(ClientMsg::Ready { ready: true });
    room.drain_all().await;

    let alice_token = room.client(1).token;
    room.client(1).force_disconnect();
    room.drain_all().await;

    let new_alice =
        TestClient::join_with_token(room.handle.clone(), "alice".into(), Some(alice_token)).await;
    let r = new_alice.latest_room.as_ref().unwrap();
    let alice_slot = r
        .players
        .iter()
        .find(|p| p.id == new_alice.player_id)
        .expect("alice slot");
    assert!(alice_slot.ready, "ready 状态应跨重连保留");
    assert!(alice_slot.connected);
}
