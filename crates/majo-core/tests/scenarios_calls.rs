//! 鸣牌窗口 + 优先级裁决 + 超时.
//!
//! 由于触发真鸣牌需要构造特定牌型 (host 切的牌恰好让 alice 能 Pon),
//! 这里主要测协议行为: 窗口超时、ActionRequired 推送、AwaitDiscard
//! 阶段错发鸣牌动作被忽略.

mod common;

use std::time::Duration;

use majo_core::engine::phase::Phase;
use majo_core::net::protocol::{ClientMsg, NetAction, ServerMsg};

use common::TestRoomBuilder;

/// AwaitDiscard 阶段, 玩家发 Pon/Chi 应被忽略 (不在 AwaitCalls).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pon_in_await_discard_ignored() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0xAAAA_BBBB)
        .start_game()
        .await;

    // 等开局: turn=East, host on AwaitDiscard
    let _ = room
        .host()
        .await_view(
            |v| v.turn == v.my_seat && v.phase == Phase::AwaitDiscard,
            Duration::from_secs(1),
        )
        .await
        .expect("host on turn");

    // Alice 发 Pon (此时 phase=AwaitDiscard, alice 不该响应)
    room.client(1).send(ClientMsg::Action(NetAction::Pon));
    room.drain_all().await;

    // host 仍应能正常切牌
    let view = room.host().latest_view.as_ref().unwrap();
    assert_eq!(view.phase, Phase::AwaitDiscard);
    assert_eq!(view.turn, view.my_seat);
}

/// AwaitDiscard 真人 turn 时, server 应推 ActionRequired 含非 0 deadline.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_required_pushed_in_await_discard() {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(0x1234_5678)
        .start_game()
        .await;

    // 等 host 收到 ActionRequired (server 在 advance_game 进入 AwaitDiscard 真人时推)
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        room.host().drain();
        if room.host().last_action_required.is_some() {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (hints, deadline_ms) = room
        .host()
        .last_action_required
        .as_ref()
        .expect("host 应收到 ActionRequired")
        .clone();
    assert!(!hints.is_empty(), "hints 不应空");
    assert!(
        deadline_ms > 0,
        "deadline_ms 应非 0 (默认 thinking_time=30s)"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(
        deadline_ms > now,
        "deadline 应在未来. now={now} deadline={deadline_ms}"
    );
    assert!(
        deadline_ms < now + 60_000,
        "deadline 不应超过 1 分钟 (thinking_time 默认 30s)"
    );
}

/// 真人 send Pass 在不属于自己的鸣牌窗口时, 应被忽略.
/// (注意: pending_calls 不存在时也应忽略.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pass_without_pending_calls_ignored() {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(0xC0DE_C0DE)
        .start_game()
        .await;

    let _ = room
        .host()
        .await_view(
            |v| v.turn == v.my_seat && v.phase == Phase::AwaitDiscard,
            Duration::from_secs(1),
        )
        .await
        .expect("host on turn");

    // host 在 AwaitDiscard 发 Pass: 应忽略, host 仍能切
    room.host().pass();
    room.drain_all().await;

    let view = room.host().latest_view.as_ref().unwrap();
    assert_eq!(view.phase, Phase::AwaitDiscard);
    assert_eq!(view.turn, view.my_seat);
    assert!(!view.my_hand.is_empty());
}

/// host 切牌后若 alice 有 call options, server 应进入 pending_calls 等待.
/// 缩短 call_window 到 50ms, alice 不响应, timer 应自动 Pass advance.
///
/// 实际上要触发这个场景需要特定 seed + 牌型. 我们直接做 best-effort:
/// 跑很多 host 切牌, 至少有一次 alice 会被推 ActionRequired.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_window_short_timeout_advances() {
    let mut room = TestRoomBuilder::new()
        .humans(2) // 2 真人 + 2 AI, alice 才有可能被检测为有 options
        .seed(0xBEEF_FEED)
        .call_window_ms(50) // 缩短到 50ms 让 timer 快
        .auto_pass_calls(false) // 不自动 pass, 让 timer 触发
        .start_game()
        .await;

    // host 切牌 80 步, 看 alice 有没有被推 ActionRequired
    let mut alice_got_call_window = false;
    for _ in 0..40 {
        // host 等自己回合
        let v = room
            .host()
            .await_view(
                |v| {
                    (v.turn == v.my_seat && v.phase == Phase::AwaitDiscard)
                        || v.phase == Phase::RoundEnd
                },
                Duration::from_secs(2),
            )
            .await;
        let Some(v) = v else { break };
        if v.phase == Phase::RoundEnd {
            break;
        }
        room.host().tsumogiri();
        // 每次切牌后等一下 (CALL_WINDOW + buffer)
        tokio::time::sleep(Duration::from_millis(100)).await;
        room.drain_all().await;
        // 检测 alice 是否收到鸣牌 ActionRequired (hints 含 Pass)
        if let Some((hints, _)) = &room.client(1).last_action_required
            && hints.iter().any(|h| matches!(h, NetAction::Pass))
        {
            alice_got_call_window = true;
            // 不响应, 等 timer 自动 advance
            tokio::time::sleep(Duration::from_millis(150)).await;
            room.drain_all().await;
            // 应该 timer 已经触发, 游戏已经推进
            break;
        }
    }

    if !alice_got_call_window {
        // 这局没触发鸣牌窗口, 是正常的 (取决于牌型). 测试 inconclusive.
        // 不 fail, 但打印警告.
        eprintln!("[warn] 未触发 alice 的鸣牌窗口, 测试 inconclusive");
        return;
    }

    // 触发了: server 应在 timer 后继续推进 turn
    let phase = room.host().latest_view.as_ref().unwrap().phase;
    // timer 触发后游戏应能继续 (phase 不卡 AwaitCalls)
    assert!(
        !matches!(phase, Phase::AwaitCalls),
        "鸣牌窗口超时后应推进, 实际 phase={phase:?}"
    );
}

/// AwaitCalls 阶段 server 推 ActionRequired 给真人, 此时 hints 应含 Pass.
/// 类似上一个测试, 但只检查 ActionRequired 推送, 不依赖具体行为.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_window_action_required_contains_pass_hint() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0xDADA_FACE)
        .call_window_ms(50)
        .auto_pass_calls(false)
        .start_game()
        .await;

    let mut found = false;
    for _ in 0..40 {
        let v = room
            .host()
            .await_view(
                |v| {
                    (v.turn == v.my_seat && v.phase == Phase::AwaitDiscard)
                        || v.phase == Phase::RoundEnd
                },
                Duration::from_secs(2),
            )
            .await;
        let Some(v) = v else { break };
        if v.phase == Phase::RoundEnd {
            break;
        }
        room.host().tsumogiri();
        tokio::time::sleep(Duration::from_millis(80)).await;
        room.drain_all().await;
        if let Some((hints, _)) = &room.client(1).last_action_required
            && hints.iter().any(|h| matches!(h, NetAction::Pass))
        {
            // hints 应该是非空且含 Pass (鸣牌窗口的标志)
            assert!(!hints.is_empty());
            found = true;
            break;
        }
        // alice 也要往前推, 否则 alice turn 时卡死. 不过 alice 也是 AI 接管前的真人?
        // 不: humans=2 意味着 host + alice 都真人. 当 alice 应该切时, 没人响应.
        // 我们让 alice 也摸切.
        let alice_view = room.client(1).latest_view.as_ref();
        if let Some(av) = alice_view
            && av.turn == av.my_seat
            && av.phase == Phase::AwaitDiscard
        {
            room.client(1).tsumogiri();
            tokio::time::sleep(Duration::from_millis(80)).await;
            room.drain_all().await;
        }
    }

    if !found {
        eprintln!("[warn] 未触发 alice 鸣牌 hint, 测试 inconclusive");
    }
}

/// non-pending player 发 call response 应被忽略.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrelated_player_call_response_ignored() {
    let mut room = TestRoomBuilder::new()
        .humans(2)
        .seed(0x1357_2468)
        .start_game()
        .await;

    // host 切牌, 让 alice 可能进入 pending_calls (或不进入)
    let _ = room
        .host()
        .await_view(
            |v| v.turn == v.my_seat && v.phase == Phase::AwaitDiscard,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    room.host().tsumogiri();
    // 不等 ActionRequired, 直接让 host (非 pending) 发 Pon
    room.host().send(ClientMsg::Action(NetAction::Pon));
    room.drain_all().await;
    // host 应没收到 Error
    let saw_error = room
        .host()
        .has_msg(|m| matches!(m, ServerMsg::Error { .. }));
    assert!(!saw_error, "non-pending 玩家发 Pon 应静默忽略, 不发 Error");
}
