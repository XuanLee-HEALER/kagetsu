//! 完整对局流: 一局 / 多局 / 整庄结束.

mod common;

use std::time::Duration;

use majo_core::engine::event::GameEvent;
use majo_core::engine::phase::Phase;
use majo_core::engine::rules::{GameRules, LengthRule};
use majo_core::net::protocol::{RoomLifecycle, ServerMsg};

use common::TestRoomBuilder;

/// host 始终摸切, 让 AI 推进. 最多跑 max_steps 次 await_my_turn 等迭代.
/// 返回 true = 跑到 RoundEnd, false = 步数耗尽.
async fn host_loops_tsumogiri_until_roundend(
    room: &mut common::TestRoom,
    max_steps: usize,
) -> bool {
    for _ in 0..max_steps {
        // 等到自己回合 OR RoundEnd OR 已收到 GameEnd msg
        // (注意 GameEnd 时 server 不再推 GameStateView, 必须看 history)
        let v_opt = room
            .host()
            .await_view(
                |v| {
                    (v.turn == v.my_seat && v.phase == Phase::AwaitDiscard)
                        || v.phase == Phase::RoundEnd
                        || v.phase == Phase::GameEnd
                },
                Duration::from_secs(2),
            )
            .await;
        // 直接看 history 是否已经收到 GameEnd
        if room.host().has_msg(|m| matches!(m, ServerMsg::GameEnd(_))) {
            return true;
        }
        let v = v_opt.expect("应推进出新状态");
        if matches!(v.phase, Phase::RoundEnd | Phase::GameEnd) {
            return true;
        }
        room.host().tsumogiri();
        room.drain_all().await;
    }
    false
}

/// 1 真人 + 3 AI, host 一直摸切, 跑完一局.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_round_completes() {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(0xCAFE_BABE)
        .start_game()
        .await;

    let reached = host_loops_tsumogiri_until_roundend(&mut room, 80).await;
    assert!(reached, "应在 80 步内到达 RoundEnd / GameEnd");

    // 至少有十几个 Discard 事件
    let view = room.host().latest_view.clone().unwrap();
    let discard_count = view
        .events
        .iter()
        .filter(|e| matches!(e, GameEvent::Discard { .. }))
        .count();
    // events 缓存 ~32 条, 但 Discard 至少占多数
    assert!(
        discard_count >= 8,
        "应有 ≥8 个 Discard, 实际 {discard_count}"
    );

    // host 应收到 RoundResult 或 GameEnd
    let saw_result = room
        .host()
        .has_msg(|m| matches!(m, ServerMsg::RoundResult(_) | ServerMsg::GameEnd(_)));
    assert!(saw_result, "host 应收到 RoundResult 或 GameEnd");
}

/// 局结束后按 N (NextRound) 应推进 kyoku 或进入下一局.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_round_advances_kyoku() {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(0x1111_2222)
        .start_game()
        .await;

    let initial_kyoku = room.host().latest_view.as_ref().unwrap().kyoku;

    // 跑到 RoundEnd
    let reached = host_loops_tsumogiri_until_roundend(&mut room, 80).await;
    assert!(reached);

    let phase = room.host().latest_view.as_ref().unwrap().phase;
    if phase == Phase::GameEnd {
        // 第一局就完结整庄 (不太可能但跳过)
        return;
    }

    // 按 N 进下一局
    room.host().next_round();
    // 等 phase 变成 Draw 或 AwaitDiscard, 且 kyoku 改变
    let new_view = room
        .host()
        .await_view(
            |v| v.phase != Phase::RoundEnd && (v.kyoku != initial_kyoku || v.honba > 0),
            Duration::from_secs(2),
        )
        .await
        .expect("下一局应启动");

    // kyoku 增加 (亲家不连庄) 或 honba 增加 (亲家连庄/流局连庄)
    assert!(
        new_view.kyoku != initial_kyoku || new_view.honba > 0,
        "应进入下一局或加本场"
    );
}

/// 东风战: 跑 4 局直到 GameEnd.
/// (实际上几局可能流局亲连庄, kyoku 不变. 我们只验证 GameEnd 必然到达.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tonpuusen_eventually_ends() {
    let cfg = GameRules {
        length: LengthRule::Tonpuusen,
        ..GameRules::default()
    };
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .config(cfg)
        .seed(0xFEED_FACE)
        .start_game()
        .await;

    // 最多 12 局尝试到达 GameEnd (东风战通常 4 局, 预留流局连庄余量).
    let mut saw_game_end = false;
    for _round_idx in 0..12 {
        let reached = host_loops_tsumogiri_until_roundend(&mut room, 200).await;
        assert!(reached, "round 应能跑到 RoundEnd / GameEnd");
        let lifecycle = room
            .host()
            .latest_room
            .as_ref()
            .map(|r| r.state)
            .unwrap_or(RoomLifecycle::Lobby);
        if lifecycle == RoomLifecycle::GameEnd
            || room.host().has_msg(|m| matches!(m, ServerMsg::GameEnd(_)))
        {
            saw_game_end = true;
            break;
        }
        room.host().next_round();
        let _ = room
            .host()
            .await_view(|v| v.phase != Phase::RoundEnd, Duration::from_secs(2))
            .await;
        room.drain_all().await;
    }
    assert!(saw_game_end, "东风战应在 12 局内到达 GameEnd");

    // 应收到 ServerMsg::GameEnd
    let host = &room.clients[0];
    assert!(
        host.has_msg(|m| matches!(m, ServerMsg::GameEnd(_))),
        "host 应收到 GameEnd"
    );
    // RoomLifecycle::GameEnd
    assert_eq!(
        host.latest_room.as_ref().unwrap().state,
        RoomLifecycle::GameEnd
    );
}

/// 同一 seed 跑两次, 事件序列应完全一致 (决定性).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_seed_produces_same_events() {
    let seed = 0xDEAD_BEEF;
    let events_run1 = run_seeded_round(seed).await;
    let events_run2 = run_seeded_round(seed).await;
    assert_eq!(
        events_run1, events_run2,
        "同 seed 两次跑应产生完全一致的事件序列"
    );
    assert!(!events_run1.is_empty(), "应有事件");
}

async fn run_seeded_round(seed: u64) -> Vec<GameEvent> {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(seed)
        .start_game()
        .await;
    let _ = host_loops_tsumogiri_until_roundend(&mut room, 80).await;
    room.host().latest_view.as_ref().unwrap().events.clone()
}
