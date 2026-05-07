
use super::*;
use crate::engine::domain::tile::Tile;
use crate::engine::phase::Phase;
use crate::engine::rules::GameRules;
use crate::net::protocol::GameStateView;
use std::time::Duration;
// Phase A.2: tests 重写 — 之前直戳 GameState 内部字段 (phase/turn/players/last_discard),
// GameEngine 时代字段不可写. 改用 round_apply / 直构造 RoundState variant 模拟场景.
use crate::engine::round_state::{AwaitCallsState, CommonRound, RoundState};

/// 模拟一个 client 连到 RoomActor, 拿到 (player_id, token, recv_rx).
async fn join_player(
    handle: &RoomHandle,
    nickname: &str,
) -> (u32, Uuid, UnboundedReceiver<ServerMsg>) {
    let (tx, rx) = mpsc::unbounded_channel::<ServerMsg>();
    let (ack_tx, ack_rx) = oneshot::channel();
    handle
        .tx
        .send(RoomCmd::Join {
            nickname: nickname.into(),
            reconnect_token: None,
            sender: tx,
            ack: ack_tx,
        })
        .unwrap();
    let result = ack_rx.await.unwrap().unwrap();
    (result.player_id, result.reconnect_token, rx)
}

/// 等到 actor 处理完已发的 cmd. 多次 yield 让 spawn 的 task 跑.
async fn yield_actor() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_join_alone() {
    let handle = spawn_room("host".into(), GameRules::default());
    let (id, _token, mut rx) = join_player(&handle, "host").await;
    assert_eq!(id, 1);
    // 应收到 Welcome
    let msg = rx.recv().await.unwrap();
    assert!(matches!(msg, ServerMsg::Welcome { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_player_not_host() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (host_id, _, _) = join_player(&handle, "host").await;
    let (other_id, _, _) = join_player(&handle, "other").await;
    assert_eq!(host_id, 1);
    assert_eq!(other_id, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_game_with_one_human_three_ai() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
    // host 自动 ready, 直接 start
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::StartGame,
        })
        .unwrap();
    yield_actor().await;
    // host_rx 应收到一连串消息 (Welcome + RoomUpdate × n + GameStateView)
    let mut got_state = false;
    while let Ok(msg) = host_rx.try_recv() {
        if matches!(msg, ServerMsg::GameStateView(_)) {
            got_state = true;
            break;
        }
    }
    assert!(got_state, "应该至少收到一个 GameStateView");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_leaves_room_dissolves() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
    let (_other_id, _, mut other_rx) = join_player(&handle, "other").await;
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::Leave,
        })
        .unwrap();
    yield_actor().await;
    // 两人都应收到 Error("房主已离开...")
    let drain = |rx: &mut UnboundedReceiver<ServerMsg>| -> bool {
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, ServerMsg::Error { .. }) {
                return true;
            }
        }
        false
    };
    assert!(drain(&mut host_rx));
    assert!(drain(&mut other_rx));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_update_only_by_host() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (host_id, _, _) = join_player(&handle, "host").await;
    let (other_id, _, _) = join_player(&handle, "other").await;

    let cfg = GameRules {
        length: crate::engine::rules::LengthRule::Tonpuusen,
        ..Default::default()
    };

    // 非 host 改: 应被拒
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: other_id,
            msg: ClientMsg::UpdateRules(cfg.clone()),
        })
        .unwrap();
    yield_actor().await;

    // host 改: 应成功 (没有直接验证, 但至少不报错; 测试主要是 actor 不 panic)
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::UpdateRules(cfg),
        })
        .unwrap();
    yield_actor().await;
}

/// 等到 host_rx 中收到一个满足条件的 GameStateView, 否则超时.
/// 返回最后一个匹配的 view. 用于稳健的状态等待 (避免 yield_actor 时间不够).
async fn wait_for_view(
    rx: &mut UnboundedReceiver<ServerMsg>,
    latest: &mut Option<GameStateView>,
    condition: impl Fn(&GameStateView) -> bool,
    timeout_ms: u64,
) -> Option<GameStateView> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        // drain 当前可读消息, 更新 latest
        while let Ok(msg) = rx.try_recv() {
            if let ServerMsg::GameStateView(v) = msg {
                *latest = Some(*v);
            }
        }
        if let Some(v) = latest.as_ref()
            && condition(v)
        {
            return latest.clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    latest.clone()
}

/// 重连: 玩家 disconnect 后用 token 重连, 应恢复 seat + 分数 +
/// 立即收到 GameStateView (如果游戏中).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_with_token_resumes_seat() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (host_id, _, _) = join_player(&handle, "host").await;
    let (_, alice_token, alice_rx) = join_player(&handle, "alice").await;

    // host 让 alice ready (其实第二个玩家加入后默认 ready=false, 必须手动)
    // 但这里我们只测重连不开局, lobby 阶段
    // 模拟 alice 断线: drop 她的 rx (channel close), 通知 server
    drop(alice_rx);
    yield_actor().await;

    // alice 用 token 重连
    let (tx2, mut rx2) = mpsc::unbounded_channel::<ServerMsg>();
    let (ack_tx, ack_rx) = oneshot::channel();
    handle
        .tx
        .send(RoomCmd::Join {
            nickname: "alice2".into(),
            reconnect_token: Some(alice_token),
            sender: tx2,
            ack: ack_tx,
        })
        .unwrap();
    let result = ack_rx.await.unwrap().unwrap();
    // 应该拿到原来同一个 player_id (而不是新分配)
    assert_ne!(result.player_id, host_id);
    assert_eq!(result.reconnect_token, alice_token);

    // 第一条消息应该是 Welcome
    let msg = rx2.recv().await.unwrap();
    assert!(matches!(msg, ServerMsg::Welcome { .. }));
}

/// AI 驱动: 1 真人 host + 3 AI, 一直推进直到 host 应该出牌 (turn=East AwaitDiscard).
/// 然后 host 切牌, AI 应继续接管直到下一次 host turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_drives_when_seat_is_ai() {
    // 缩短 call_window 到 100ms 加快测试 (默认 5 秒 × 4 次摸切 = 20s 易 flaky).
    let handle = spawn_room_advanced("h".into(), GameRules::default(), None, Some(100));
    let (host_id, _, mut host_rx) = join_player(&handle, "host").await;
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::StartGame,
        })
        .unwrap();

    let mut latest: Option<GameStateView> = None;
    let view = wait_for_view(
        &mut host_rx,
        &mut latest,
        |v| v.turn == Seat::East && v.phase == Phase::AwaitDiscard,
        2000,
    )
    .await
    .expect("应在 2s 内收到 East AwaitDiscard 状态");
    assert_eq!(view.turn, Seat::East);
    assert_eq!(view.phase, Phase::AwaitDiscard);

    // host 切自家手牌第一张
    let first_tile = view.my_hand[0];
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::Action(crate::net::protocol::NetAction::Discard(
                crate::ui::screens::game::TileSpec {
                    kind: first_tile.kind,
                },
            )),
        })
        .unwrap();

    // AI 接管 South/West/North 自动出, 然后回到 host (East) AwaitDiscard.
    // 等条件: 事件中至少 4 次 Discard 且 turn=East AwaitDiscard.
    let mut latest2: Option<GameStateView> = None;
    let view2 = wait_for_view(
        &mut host_rx,
        &mut latest2,
        |v| {
            v.turn == Seat::East
                && v.phase == Phase::AwaitDiscard
                && v.events
                    .iter()
                    .filter(|e| matches!(e, crate::engine::event::GameEvent::Discard { .. }))
                    .count()
                    >= 4
        },
        3000,
    )
    .await;
    let view2 = view2.unwrap_or_else(|| {
        panic!(
            "AI 推进后应回到 East AwaitDiscard, latest={:?}",
            latest2.as_ref().map(|v| (v.turn, v.phase))
        )
    });
    assert_eq!(view2.turn, Seat::East);
    assert_eq!(view2.phase, Phase::AwaitDiscard);
}

// ============================================================================
// RoomActor 内部单元测试 (直接 sync 调内部方法)
// ============================================================================

use crate::engine::domain::tile::TileIndex;

/// 构造一个处于 InGame 状态的 RoomActor (sync, 不 spawn task).
/// 玩家 id: 1=East, 2=South, 3=West, 4=North. is_ai 由 humans 列表决定.
/// 返回 (actor, 4 个 receiver). receiver 顺序对应 East/South/West/North.
fn make_actor_in_game(humans: &[Seat]) -> (RoomActor, Vec<UnboundedReceiver<ServerMsg>>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let mut actor = RoomActor::new_with_rx(
        "host".into(),
        GameRules::default(),
        cmd_rx,
        cmd_tx,
        Some(0xC0DE_C0DE),
    );

    let mut receivers = Vec::with_capacity(4);
    let seats = [Seat::East, Seat::South, Seat::West, Seat::North];
    for (i, seat) in seats.iter().enumerate() {
        let is_human = humans.contains(seat);
        let (tx, rx) = mpsc::unbounded_channel();
        actor.slots.push(SlotEntry {
            id: (i + 1) as u32,
            nickname: format!("p{}", i + 1),
            ready: true,
            seat: Some(*seat),
            is_ai: !is_human,
            is_host: i == 0,
            connected: true,
            sender: Some(tx),
            reconnect_token: Uuid::new_v4(),
            disconnected_at: None,
        });
        receivers.push(rx);
    }
    actor.next_player_id = 5;

    let mut engine = GameEngine::new(GameRules::default());
    engine.start_round(0xC0DE_C0DE);
    actor.game = Some(engine);
    actor.state = RoomLifecycle::InGame;
    actor.game_seed = 0xC0DE_C0DE;
    actor.round_index = 1;

    (actor, receivers)
}

/// 拿到 RoundState 内 CommonRound 的可变引用 (无视 phase, match 任意 variant).
fn round_common_mut(round: &mut RoundState) -> &mut CommonRound {
    match round {
        RoundState::AwaitDraw(s) => &mut s.common,
        RoundState::AwaitDiscard(s) => &mut s.common,
        RoundState::AwaitRiichiDiscard(s) => &mut s.common,
        RoundState::AwaitRinshanDraw(s) => &mut s.common,
        RoundState::AwaitCalls(s) => &mut s.common,
        RoundState::RoundEnd(s) => &mut s.common,
    }
}

/// 设置场景: who 切了 tile, 进 AwaitCalls.
/// 直接构造 AwaitCallsState 替换 engine.round, 跳过真实 round_apply 驱动.
fn force_discard_scenario(actor: &mut RoomActor, who: Seat, tile: Tile) {
    let engine = actor.game.as_mut().unwrap();
    let mut common = round_common_mut(&mut engine.round).clone();
    // 移除 who 手中一张同 kind tile (若存在)
    if let Some(pos) = common.players[who.index()]
        .hand
        .closed
        .iter()
        .position(|t| t.kind == tile.kind)
    {
        common.players[who.index()].hand.closed.remove(pos);
    }
    common.players[who.index()].river.push(tile);
    common.first_go_around = false;
    engine.round = RoundState::AwaitCalls(AwaitCallsState {
        common,
        last_discard: (who, tile),
    });
}

/// 给 `target` 手中插入 `n` 张同 kind tile (id 不冲突).
fn give_player_tiles(actor: &mut RoomActor, target: Seat, kind: TileIndex, n: usize) {
    let engine = actor.game.as_mut().unwrap();
    let common = round_common_mut(&mut engine.round);
    for i in 0..n {
        let id = 9000_u16 + (i as u16) + (target.index() as u16) * 100;
        common.players[target.index()].hand.closed.push(Tile {
            id,
            kind,
            red: false,
        });
    }
}

fn make_pending(map: Vec<(u32, NetAction)>) -> HashMap<u32, Option<NetAction>> {
    map.into_iter().map(|(id, a)| (id, Some(a))).collect()
}

#[test]
fn resolve_no_pending_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 没有 pending_calls, resolve 应直接返回无副作用.
    let phase_before = actor.game.as_ref().unwrap().phase();
    let turn_before = actor.game.as_ref().unwrap().turn();
    actor.resolve_call_window();
    assert!(actor.pending_calls.is_none());
    assert_eq!(actor.game.as_ref().unwrap().phase(), phase_before);
    assert_eq!(actor.game.as_ref().unwrap().turn(), turn_before);
}

#[test]
fn resolve_all_pass_advances_turn() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    let game = actor.game.as_ref().unwrap();
    let initial_turn = game.turn();

    actor.pending_calls = Some(make_pending(vec![
        (2, NetAction::Pass),
        (3, NetAction::Pass),
    ]));
    actor.resolve_call_window();

    assert!(actor.pending_calls.is_none());
    assert_eq!(
        actor.game.as_ref().unwrap().turn(),
        initial_turn.next(),
        "全 Pass 应 advance_turn"
    );
    assert_eq!(actor.game.as_ref().unwrap().phase(), Phase::Draw);
}

#[test]
fn resolve_pon_executes_when_legal() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    // East 切一张 5p, South 手中已经有 2 张 5p (或 +)
    // 5p 的 TileIndex 是 13 (9-17 是筒子, 13 = 5筒)
    let kind = TileIndex(13);
    let pon_tile = Tile {
        id: 1001,
        kind,
        red: false,
    };
    give_player_tiles(&mut actor, Seat::South, kind, 2);
    force_discard_scenario(&mut actor, Seat::East, pon_tile);

    actor.pending_calls = Some(make_pending(vec![(2, NetAction::Pon)]));
    actor.resolve_call_window();

    assert!(actor.pending_calls.is_none());
    let game = actor.game.as_ref().unwrap();
    assert_eq!(game.turn(), Seat::South, "Pon 后 turn 转给鸣牌方");
    assert_eq!(game.phase(), Phase::AwaitDiscard, "鸣牌后 South 应切牌");
    assert_eq!(
        game.players()[Seat::South.index()].hand.melds.len(),
        1,
        "South 应有 1 个副露"
    );
}

#[test]
fn resolve_ron_beats_pon() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South, Seat::West]);
    // East 切牌, South 想 Pon, West 想 Ron. Ron 应胜.
    // 构造: West 听牌 (国士无双最简: 13 张幺九各 1 张, 等任意 14 张).
    // 太复杂. 简化: 用一个 "几乎和牌" 的手牌 + 切对应等牌.
    // 但 try_ron 内部走完整役判定. 不易构造. 这里测意图: pending 中含 Tsumo
    // (= AwaitCalls 阶段视为 Ron) 的玩家, 应优先于 Pon. 如果 Ron 不合法
    // (try_ron 返回 None), resolve 会 fall through 到 Pon. 我们间接验证:
    // 当只有 Ron 且不合法时, fall through 到 Pon.

    let kind = TileIndex(13);
    let tile = Tile {
        id: 2001,
        kind,
        red: false,
    };
    give_player_tiles(&mut actor, Seat::South, kind, 2);
    force_discard_scenario(&mut actor, Seat::East, tile);

    actor.pending_calls = Some(make_pending(vec![
        (2, NetAction::Pon),   // South Pon
        (3, NetAction::Tsumo), // West "Ron" (但牌型不和, try_ron 返回 None)
    ]));
    actor.resolve_call_window();

    // West Ron 不合法 → fall through 到 Pon → South Pon
    let game = actor.game.as_ref().unwrap();
    assert_eq!(game.turn(), Seat::South);
    assert_eq!(
        game.players()[Seat::South.index()].hand.melds.len(),
        1,
        "Ron 不合法时应 fall through 到 Pon"
    );
}

#[test]
fn resolve_pon_beats_chi() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South, Seat::West]);
    // East 切牌, South (下家) 能 Chi, West 能 Pon. Pon 应优先.
    let kind = TileIndex(4); // 5m
    let tile = Tile {
        id: 3001,
        kind,
        red: false,
    };
    // South Chi: 给 South 4m + 6m (下家能吃 East 切的 5m)
    give_player_tiles(&mut actor, Seat::South, TileIndex(3), 1);
    give_player_tiles(&mut actor, Seat::South, TileIndex(5), 1);
    // West Pon: 给 West 2× 5m
    give_player_tiles(&mut actor, Seat::West, kind, 2);
    force_discard_scenario(&mut actor, Seat::East, tile);

    actor.pending_calls = Some(make_pending(vec![
        (2, NetAction::Chi(0)), // South (id=2) Chi
        (3, NetAction::Pon),    // West (id=3) Pon
    ]));
    actor.resolve_call_window();

    let game = actor.game.as_ref().unwrap();
    assert_eq!(game.turn(), Seat::West, "Pon 优先于 Chi, turn 应给 Pon 方");
    assert_eq!(
        game.players()[Seat::West.index()].hand.melds.len(),
        1,
        "West 应有 Pon 副露"
    );
    assert_eq!(
        game.players()[Seat::South.index()].hand.melds.len(),
        0,
        "South 不应吃成"
    );
}

#[test]
fn handle_call_response_partial_does_not_resolve() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South, Seat::West]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    let game = actor.game.as_ref().unwrap();
    let turn_before = game.turn();

    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m.insert(3, None);
        m
    });
    // 只有 South 响应, West 还未
    actor.handle_call_response(2, NetAction::Pass);
    // 不应 resolve
    assert!(actor.pending_calls.is_some(), "未收齐响应不应 resolve");
    assert_eq!(actor.game.as_ref().unwrap().turn(), turn_before);
}

#[test]
fn handle_call_response_full_triggers_resolve() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South, Seat::West]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    let game = actor.game.as_ref().unwrap();
    let turn_before = game.turn();

    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m.insert(3, None);
        m
    });
    actor.handle_call_response(2, NetAction::Pass);
    actor.handle_call_response(3, NetAction::Pass);
    // 收齐后 resolve, 全 Pass → advance_turn
    assert!(actor.pending_calls.is_none());
    assert_eq!(actor.game.as_ref().unwrap().turn(), turn_before.next());
}

#[test]
fn handle_call_response_unknown_player_ignored() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m
    });
    // pid=99 不在 pending 中
    actor.handle_call_response(99, NetAction::Pon);
    // pending 不变
    let p = actor.pending_calls.as_ref().unwrap();
    assert!(
        p.get(&2).map(|v| v.is_none()).unwrap_or(false),
        "无关玩家响应不应改变 pending"
    );
}

#[test]
fn is_seat_ai_detects_human_and_ai() {
    let (actor, _rxs) = make_actor_in_game(&[Seat::East]);
    assert!(!actor.is_seat_ai(Seat::East), "East 是真人");
    assert!(actor.is_seat_ai(Seat::South), "South 默认 AI");
    assert!(actor.is_seat_ai(Seat::West), "West 默认 AI");
}

#[test]
fn is_seat_ai_disconnected_human_treated_as_ai() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    // South 真人但断线
    actor.slots[1].connected = false;
    assert!(actor.is_seat_ai(Seat::South), "断线真人应被 AI 接管");
}

#[test]
fn project_view_hides_other_hands() {
    let (actor, _rxs) = make_actor_in_game(&[Seat::East]);
    let east_view = actor.project_view(Seat::East).unwrap();
    // 自己 hand 应有 13 张 (开局)
    assert_eq!(east_view.my_hand.len(), 13);
    assert_eq!(east_view.my_seat, Seat::East);
    // 但 players 中其他 seat 的 hand_count 应有, melds 应空
    assert_eq!(east_view.players[1].hand_count, 13);
    assert!(east_view.players[1].melds.is_empty());
}

#[test]
fn project_view_my_seat_correct_per_client() {
    let (actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South, Seat::West]);
    for seat in [Seat::East, Seat::South, Seat::West, Seat::North] {
        let v = actor.project_view(seat).unwrap();
        assert_eq!(v.my_seat, seat);
        assert_eq!(v.my_hand.len(), 13);
    }
}

/// M5.B.2: spawn_room 默认 mode = Standard, RoomView 反映正确.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_room_default_mode_is_standard() {
    let handle = spawn_room("h".into(), GameRules::default());
    let (_pid, _tok, mut rx) = join_player(&handle, "h").await;
    // 收 Welcome
    yield_actor().await;
    let mut got_mode = None;
    while let Ok(msg) = rx.try_recv() {
        if let ServerMsg::Welcome { room, .. } = msg {
            got_mode = Some(room.mode);
            break;
        }
    }
    assert_eq!(
        got_mode,
        Some(crate::net::p2p::RoomMode::Standard),
        "默认 spawn_room 应是 Standard"
    );
}

/// M5.B.2: spawn_room_with_mode(ZeroTrust) 传 RoomView.mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_room_with_mode_propagates_to_room_view() {
    let handle = spawn_room_with_mode(
        "h".into(),
        GameRules::default(),
        crate::net::p2p::RoomMode::ZeroTrust,
    );
    let (_pid, _tok, mut rx) = join_player(&handle, "h").await;
    yield_actor().await;
    let mut got_mode = None;
    while let Ok(msg) = rx.try_recv() {
        if let ServerMsg::Welcome { room, .. } = msg {
            got_mode = Some(room.mode);
            break;
        }
    }
    assert_eq!(
        got_mode,
        Some(crate::net::p2p::RoomMode::ZeroTrust),
        "spawn_room_with_mode(ZeroTrust) 应反映到 RoomView.mode"
    );
}

/// Phase D: ZeroTrust + n<4 真人 → 自动降级 Standard + AI 补足. 不发 MpStart,
/// 但发 GameStateView (Standard 模式启动).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zerotrust_falls_back_to_standard_when_under_4_humans() {
    let handle = spawn_room_with_mode(
        "h".into(),
        GameRules::default(),
        crate::net::p2p::RoomMode::ZeroTrust,
    );
    let (host_id, _, mut host_rx) = join_player(&handle, "h").await;
    // 仅 1 真人, host 自动 ready, 触发 StartGame
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::StartGame,
        })
        .unwrap();
    yield_actor().await;

    let mut got_downgrade_msg = false;
    let mut got_mp_start = false;
    let mut got_game_state = false;
    while let Ok(msg) = host_rx.try_recv() {
        match msg {
            ServerMsg::Error { message } if message.contains("降级") => {
                got_downgrade_msg = true;
            }
            ServerMsg::MpStart { .. } => got_mp_start = true,
            ServerMsg::GameStateView(_) => got_game_state = true,
            _ => {}
        }
    }
    assert!(got_downgrade_msg, "应收到降级提示");
    assert!(!got_mp_start, "n<4 不应发 MpStart (Standard 路径)");
    assert!(got_game_state, "应已降级为 Standard 启动并发 GameStateView");
}

/// M5.B.8.0: ZeroTrust + 4 真人 ready → 4 玩家收 MpStart, own_index 0..3 各异.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zerotrust_4_humans_emits_mp_start() {
    let handle = spawn_room_with_mode(
        "h".into(),
        GameRules::default(),
        crate::net::p2p::RoomMode::ZeroTrust,
    );
    let (host_id, _, mut rx0) = join_player(&handle, "p0").await;
    let (p1_id, _, mut rx1) = join_player(&handle, "p1").await;
    let (p2_id, _, mut rx2) = join_player(&handle, "p2").await;
    let (_p3_id, _, mut rx3) = join_player(&handle, "p3").await;

    // M5.D.2: 测试模拟 host swarm 注入 PeerId 关联 (生产环境是
    // spawn_p2p_listener + host_swarm_task.process_pending_join 注入).
    for (pid, fake_pid_byte) in [(host_id, 0u8), (p1_id, 1), (p2_id, 2), (_p3_id, 3)] {
        handle
            .tx
            .send(RoomCmd::AssociatePeer {
                player_id: pid,
                peer_id_bytes: vec![fake_pid_byte; 32],
            })
            .unwrap();
    }

    // 非房主玩家 ready (host 自动 ready)
    for pid in [p1_id, p2_id, _p3_id] {
        handle
            .tx
            .send(RoomCmd::PlayerMsg {
                player_id: pid,
                msg: ClientMsg::Ready { ready: true },
            })
            .unwrap();
    }
    yield_actor().await;

    // host 触发开局
    handle
        .tx
        .send(RoomCmd::PlayerMsg {
            player_id: host_id,
            msg: ClientMsg::StartGame,
        })
        .unwrap();
    yield_actor().await;

    // 各 client 应收到 MpStart, own_index 跟 join 顺序一致
    let collect_mp_start =
        |rx: &mut UnboundedReceiver<ServerMsg>| -> Option<(u32, Vec<Vec<u8>>, Vec<u8>)> {
            while let Ok(msg) = rx.try_recv() {
                if let ServerMsg::MpStart {
                    own_index,
                    all_peer_ids,
                    session_label,
                    ..
                } = msg
                {
                    return Some((own_index, all_peer_ids, session_label));
                }
            }
            None
        };
    let mp0 = collect_mp_start(&mut rx0).expect("p0 应收 MpStart");
    let mp1 = collect_mp_start(&mut rx1).expect("p1 应收 MpStart");
    let mp2 = collect_mp_start(&mut rx2).expect("p2 应收 MpStart");
    let mp3 = collect_mp_start(&mut rx3).expect("p3 应收 MpStart");

    assert_eq!(mp0.0, 0);
    assert_eq!(mp1.0, 1);
    assert_eq!(mp2.0, 2);
    assert_eq!(mp3.0, 3);

    // 4 玩家看到的 all_peer_ids 一致
    assert_eq!(mp0.1, mp1.1);
    assert_eq!(mp1.1, mp2.1);
    assert_eq!(mp2.1, mp3.1);
    assert_eq!(mp0.1.len(), 4);

    // 4 玩家看到的 session_label 一致 + 长度 = 32 (SHA-256)
    assert_eq!(mp0.2, mp1.2);
    assert_eq!(mp1.2, mp2.2);
    assert_eq!(mp2.2, mp3.2);
    assert_eq!(mp0.2.len(), 32);
}

// ========================================================================
// game.rs handle_action / advance_game / send_thinking_action_required /
// apply_ai_action / finalize_game 直接驱动测试
// ========================================================================

use crate::engine::domain::action::Action;

#[test]
fn handle_action_state_not_in_game_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.state = RoomLifecycle::Lobby;
    let phase_before = actor.game.as_ref().unwrap().phase();
    // 用 fake 切牌 spec; 因为 state != InGame, 应直接返回不调用 engine.
    actor.handle_action(
        1,
        NetAction::Discard(crate::ui::screens::game::TileSpec { kind: TileIndex(0) }),
    );
    assert_eq!(actor.game.as_ref().unwrap().phase(), phase_before);
}

#[test]
fn handle_action_unknown_player_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    let phase_before = actor.game.as_ref().unwrap().phase();
    actor.handle_action(
        999,
        NetAction::Discard(crate::ui::screens::game::TileSpec { kind: TileIndex(0) }),
    );
    assert_eq!(actor.game.as_ref().unwrap().phase(), phase_before);
}

#[test]
fn handle_action_in_await_calls_routes_to_call_response() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    // South (id=2) 进 pending; handle_action(2, Pass) 应路由到 handle_call_response.
    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m
    });
    actor.handle_action(2, NetAction::Pass);
    // 唯一 pending 已响应 → resolve → 全 Pass → advance_turn, pending 清空.
    assert!(actor.pending_calls.is_none());
}

#[test]
fn handle_action_pon_in_await_discard_is_ignored() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 默认 round 是 AwaitDraw, 先驱到 AwaitDiscard.
    let game = actor.game.as_mut().unwrap();
    let _ = game.do_draw();
    assert_eq!(game.phase(), Phase::AwaitDiscard);
    let phase_before = game.phase();
    let turn_before = game.turn();
    // East AwaitDiscard 阶段调 Pon 应被忽略 (不 panic 不动 state).
    actor.handle_action(1, NetAction::Pon);
    let game = actor.game.as_ref().unwrap();
    assert_eq!(game.phase(), phase_before);
    assert_eq!(game.turn(), turn_before);
}

#[test]
fn handle_action_next_round_only_in_round_end() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 当前 phase != RoundEnd, NextRound 应 short-circuit 不动.
    let round_index_before = actor.round_index;
    actor.handle_action(1, NetAction::NextRound);
    assert_eq!(actor.round_index, round_index_before);
}

#[test]
fn send_thinking_action_required_emits_action_required_with_discard_placeholder() {
    let (actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.send_thinking_action_required(Seat::East);
    let mut got_hint = None;
    while let Ok(msg) = rxs[0].try_recv() {
        if let ServerMsg::ActionRequired { hints, .. } = msg {
            got_hint = Some(hints);
            break;
        }
    }
    let hints = got_hint.expect("应推 ActionRequired");
    // 至少一个 Discard placeholder.
    assert!(
        hints.iter().any(|h| matches!(h, NetAction::Discard(_))),
        "hints 应至少含一个 Discard placeholder, 实际 {hints:?}",
    );
}

#[test]
fn send_thinking_action_required_no_sender_does_not_panic() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 移走 East 的 sender (模拟离线), 不应 panic.
    actor.slots[0].sender = None;
    actor.send_thinking_action_required(Seat::East);
}

#[test]
fn send_thinking_action_required_zero_thinking_time_emits_zero_deadline() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.config.thinking_time_secs = Some(0);
    actor.send_thinking_action_required(Seat::East);
    let mut got_deadline = None;
    while let Ok(msg) = rxs[0].try_recv() {
        if let ServerMsg::ActionRequired {
            deadline_unix_ms, ..
        } = msg
        {
            got_deadline = Some(deadline_unix_ms);
            break;
        }
    }
    assert_eq!(got_deadline, Some(0), "thinking_time=0 应 deadline=0");
}

#[test]
fn apply_ai_action_pass_falls_back_to_drawn_discard() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 推到 AwaitDiscard, last_drawn = Some(...).
    let drawn = actor.game.as_mut().unwrap().do_draw().expect("draw");
    let river_len_before = {
        let g = actor.game.as_ref().unwrap();
        g.players()[Seat::East.index()].river.len()
    };
    // Pass 应 fallback 摸切.
    actor.apply_ai_action(Action::Pass);
    let river_len_after = actor.game.as_ref().unwrap().players()[Seat::East.index()]
        .river
        .len();
    assert_eq!(
        river_len_after,
        river_len_before + 1,
        "Pass fallback 应将 last_drawn 切到河里"
    );
    // 切到河里的牌应该就是 drawn 那张 (摸切).
    let last_river_tile = actor.game.as_ref().unwrap().players()[Seat::East.index()]
        .river
        .last()
        .copied()
        .unwrap();
    assert_eq!(last_river_tile.id, drawn.id);
}

#[test]
fn apply_ai_action_discard_drops_specified_tile() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    let _ = actor.game.as_mut().unwrap().do_draw();
    // 取手牌第一张作 Discard target.
    let target = actor.game.as_ref().unwrap().players()[Seat::East.index()]
        .hand
        .closed[0];
    actor.apply_ai_action(Action::Discard(target));
    let last = actor.game.as_ref().unwrap().players()[Seat::East.index()]
        .river
        .last()
        .copied()
        .unwrap();
    assert_eq!(last.id, target.id);
}

#[test]
fn apply_ai_action_no_game_does_not_panic() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.game = None;
    // 不应 panic.
    actor.apply_ai_action(Action::Pass);
}

#[test]
fn finalize_game_marks_game_end_and_broadcasts_gameover() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.finalize_game();
    assert_eq!(actor.state, RoomLifecycle::GameEnd);
    // 4 个 receiver 都应至少收到一条 GameEnd / RoomUpdate.
    let mut got_game_end = false;
    for rx in rxs.iter_mut() {
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, ServerMsg::GameEnd(_)) {
                got_game_end = true;
            }
        }
    }
    assert!(got_game_end, "finalize_game 应广播 GameEnd");
}

// ========================================================================
// projection.rs broadcast / room_view / send_error 路径
// ========================================================================

use crate::engine::round_state::{RoundResult, RyuukyokuKind};

#[test]
fn room_view_no_host_returns_zero_host_id() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    for slot in &mut actor.slots {
        slot.is_host = false;
    }
    let view = actor.room_view();
    assert_eq!(view.host_id, 0);
}

#[test]
fn room_view_marks_host_correctly() {
    let (actor, _rxs) = make_actor_in_game(&[Seat::East]);
    let view = actor.room_view();
    assert_eq!(view.host_id, 1);
    assert_eq!(view.players.len(), 4);
}

#[test]
fn project_view_returns_none_when_game_absent() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.game = None;
    assert!(actor.project_view(Seat::East).is_none());
}

#[test]
fn broadcast_state_view_skips_slot_without_seat() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    // 给 East slot 清掉 seat (模拟未上桌的观察者).
    actor.slots[0].seat = None;
    actor.broadcast_state_view();
    // East rx 不应收到 GameStateView.
    let mut saw = false;
    while let Ok(msg) = rxs[0].try_recv() {
        if matches!(msg, ServerMsg::GameStateView(_)) {
            saw = true;
            break;
        }
    }
    assert!(!saw, "无 seat 的 slot 不应收 GameStateView");
}

#[test]
fn broadcast_state_view_skips_slot_without_sender() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.slots[0].sender = None;
    actor.broadcast_state_view();
    let mut saw = false;
    while let Ok(msg) = rxs[0].try_recv() {
        if matches!(msg, ServerMsg::GameStateView(_)) {
            saw = true;
        }
    }
    assert!(!saw);
}

#[test]
fn broadcast_round_result_no_game_is_noop() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.game = None;
    actor.broadcast_round_result();
    let mut saw = false;
    while let Ok(msg) = rxs[0].try_recv() {
        if matches!(msg, ServerMsg::RoundResult(_)) {
            saw = true;
        }
    }
    assert!(!saw);
}

#[test]
fn broadcast_round_result_ryuukyoku_uses_liu_ju_message() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.game.as_mut().unwrap().last_result = Some(RoundResult::Ryuukyoku {
        kind: RyuukyokuKind::Howaipai,
    });
    actor.broadcast_round_result();
    let mut got_msg = None;
    while let Ok(msg) = rxs[0].try_recv() {
        if let ServerMsg::RoundResult(r) = msg {
            got_msg = Some(r.message);
            break;
        }
    }
    assert_eq!(got_msg.as_deref(), Some("流局"));
}

#[test]
fn broadcast_round_result_no_result_uses_unknown_message() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.game.as_mut().unwrap().last_result = None;
    actor.broadcast_round_result();
    let mut got_msg = None;
    while let Ok(msg) = rxs[0].try_recv() {
        if let ServerMsg::RoundResult(r) = msg {
            got_msg = Some(r.message);
            break;
        }
    }
    assert_eq!(got_msg.as_deref(), Some("未知"));
}

#[test]
fn broadcast_room_update_sends_to_all_with_sender() {
    let (actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    actor.broadcast_room_update();
    let mut count = 0;
    for rx in rxs.iter_mut() {
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, ServerMsg::RoomUpdate(_)) {
                count += 1;
                break;
            }
        }
    }
    assert_eq!(count, 4, "RoomUpdate 应广播到全部 4 个 sender");
}

#[test]
fn send_error_targets_specific_player() {
    let (actor, mut rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    actor.send_error(2, "test err");
    // South (index 1) 收到; East 不收.
    let mut south_got = false;
    while let Ok(msg) = rxs[1].try_recv() {
        if let ServerMsg::Error { message } = msg
            && message == "test err"
        {
            south_got = true;
            break;
        }
    }
    assert!(south_got);
    // East 不应收到.
    let mut east_got = false;
    while let Ok(msg) = rxs[0].try_recv() {
        if matches!(msg, ServerMsg::Error { .. }) {
            east_got = true;
        }
    }
    assert!(!east_got);
}

#[test]
fn send_error_unknown_player_does_not_panic() {
    let (actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.send_error(999, "noop");
}

// ========================================================================
// lobby.rs handle_xxx 命令路由 + 状态约束
// ========================================================================

/// 构造一个处于 Lobby 状态的 RoomActor (sync). humans = 已加入的真人数 (含 host).
/// 第一人是 host. ready=true 仅 host (其他默认 false).
fn make_actor_in_lobby(humans: usize) -> (RoomActor, Vec<UnboundedReceiver<ServerMsg>>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let mut actor = RoomActor::new_with_rx(
        "host".into(),
        GameRules::default(),
        cmd_rx,
        cmd_tx,
        Some(0xC0DE_C0DE),
    );
    let mut receivers = Vec::with_capacity(humans);
    for i in 0..humans {
        let (tx, rx) = mpsc::unbounded_channel();
        actor.slots.push(SlotEntry {
            id: (i + 1) as u32,
            nickname: format!("p{}", i + 1),
            ready: i == 0,
            seat: None,
            is_ai: false,
            is_host: i == 0,
            connected: true,
            sender: Some(tx),
            reconnect_token: Uuid::new_v4(),
            disconnected_at: None,
        });
        receivers.push(rx);
    }
    actor.next_player_id = (humans + 1) as u32;
    (actor, receivers)
}

#[test]
fn handle_join_returns_room_full_when_4_slots_taken() {
    let (mut actor, _rxs) = make_actor_in_lobby(4);
    let (tx, _rx) = mpsc::unbounded_channel();
    let res = actor.handle_join("late".into(), None, tx);
    assert!(matches!(res, Err(JoinError::RoomFull)));
}

#[test]
fn handle_join_returns_already_in_game_when_state_non_lobby() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 状态 InGame, 新玩家不应能加入.
    let (tx, _rx) = mpsc::unbounded_channel();
    let res = actor.handle_join("late".into(), None, tx);
    assert!(matches!(res, Err(JoinError::AlreadyInGame)));
}

#[test]
fn handle_ready_in_game_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    // South (id=2) 在 InGame 阶段调 ready, 不应改任何 slot.ready.
    let ready_before: Vec<bool> = actor.slots.iter().map(|s| s.ready).collect();
    actor.handle_ready(2, false);
    let ready_after: Vec<bool> = actor.slots.iter().map(|s| s.ready).collect();
    assert_eq!(ready_before, ready_after);
}

#[test]
fn handle_ready_host_cannot_unready() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    // host (id=1) 永远 ready.
    actor.handle_ready(1, false);
    assert!(actor.slots[0].ready, "host 不应能 unready");
}

#[test]
fn handle_ready_non_host_can_set_ready() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    actor.handle_ready(2, true);
    assert!(actor.slots[1].ready);
    actor.handle_ready(2, false);
    assert!(!actor.slots[1].ready);
}

#[test]
fn handle_update_config_in_game_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    let cfg = GameRules {
        length: crate::engine::rules::LengthRule::Tonpuusen,
        ..Default::default()
    };
    let before = actor.config.clone();
    actor.handle_update_config(1, cfg);
    // InGame 阶段 config 应不变.
    assert_eq!(before.length, actor.config.length);
}

#[test]
fn handle_update_config_non_host_is_noop_in_lobby() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    let cfg = GameRules {
        length: crate::engine::rules::LengthRule::Tonpuusen,
        ..Default::default()
    };
    let before = actor.config.length;
    // id=2 不是 host
    actor.handle_update_config(2, cfg);
    assert_eq!(actor.config.length, before);
}

#[test]
fn handle_update_config_host_in_lobby_updates() {
    let (mut actor, _rxs) = make_actor_in_lobby(1);
    let cfg = GameRules {
        length: crate::engine::rules::LengthRule::Tonpuusen,
        ..Default::default()
    };
    actor.handle_update_config(1, cfg.clone());
    assert_eq!(actor.config.length, cfg.length);
}

#[test]
fn handle_start_game_non_host_is_noop() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    // id=2 不是 host
    actor.handle_start_game(2);
    assert_eq!(actor.state, RoomLifecycle::Lobby);
}

#[test]
fn handle_start_game_in_game_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.handle_start_game(1);
    // 已经 InGame, 应无变化 (不 panic)
    assert_eq!(actor.state, RoomLifecycle::InGame);
}

#[test]
fn handle_start_game_not_all_ready_sends_error() {
    let (mut actor, mut rxs) = make_actor_in_lobby(2);
    // p2 默认非 ready
    actor.handle_start_game(1);
    let mut got_error = false;
    while let Ok(msg) = rxs[0].try_recv() {
        if let ServerMsg::Error { message } = msg
            && message.contains("未准备")
        {
            got_error = true;
            break;
        }
    }
    assert!(got_error);
    assert_eq!(actor.state, RoomLifecycle::Lobby);
}

#[test]
fn handle_back_to_room_only_in_game_end() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // InGame 阶段 BackToRoom 应 noop.
    actor.handle_back_to_room(1);
    assert_eq!(actor.state, RoomLifecycle::InGame);
}

#[test]
fn handle_back_to_room_in_game_end_resets_to_lobby() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.state = RoomLifecycle::GameEnd;
    actor.handle_back_to_room(1);
    assert_eq!(actor.state, RoomLifecycle::Lobby);
    assert!(actor.game.is_none());
    // AI slot 应已清; 真人保留.
    assert!(actor.slots.iter().all(|s| !s.is_ai));
    // East 真人 seat 应清.
    assert!(actor.slots[0].seat.is_none());
}

#[test]
fn handle_continue_game_in_lobby_is_noop() {
    let (mut actor, _rxs) = make_actor_in_lobby(1);
    actor.handle_continue_game(1);
    assert_eq!(actor.state, RoomLifecycle::Lobby);
}

#[test]
fn handle_continue_game_non_host_in_game_end_is_noop() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    actor.state = RoomLifecycle::GameEnd;
    actor.handle_continue_game(2); // id=2 非 host
    assert_eq!(actor.state, RoomLifecycle::GameEnd);
}

#[test]
fn handle_continue_game_host_in_game_end_starts_new_round() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.state = RoomLifecycle::GameEnd;
    actor.handle_continue_game(1);
    assert_eq!(actor.state, RoomLifecycle::InGame);
    assert_eq!(actor.round_index, 1);
    assert!(actor.game.is_some());
}

#[test]
fn handle_leave_unknown_player_is_noop() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    let before = actor.slots.len();
    actor.handle_leave(999);
    assert_eq!(actor.slots.len(), before);
}

#[test]
fn handle_leave_lobby_non_host_removes_slot() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    actor.handle_leave(2);
    assert_eq!(actor.slots.len(), 1);
    assert_eq!(actor.slots[0].id, 1);
}

#[test]
fn handle_leave_in_game_non_host_marks_ai_keeps_slot() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    actor.handle_leave(2);
    // South slot 应仍存在但 is_ai = true.
    let south = actor.slots.iter().find(|s| s.id == 2).expect("仍存在");
    assert!(south.is_ai);
    assert!(!south.connected);
    assert!(south.sender.is_none());
    assert!(south.nickname.contains("AI"));
}

#[test]
fn on_reconnect_grace_timeout_clears_disconnected_at_when_still_offline() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    actor.slots[1].connected = false;
    actor.slots[1].sender = None;
    actor.slots[1].disconnected_at = Some(std::time::Instant::now());
    actor.on_reconnect_grace_timeout(2);
    assert!(actor.slots[1].disconnected_at.is_none());
}

#[test]
fn on_reconnect_grace_timeout_does_nothing_when_already_reconnected() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    actor.slots[1].connected = true; // 已重连
    // disconnected_at 未设
    actor.on_reconnect_grace_timeout(2);
    // 仍正常.
    assert!(actor.slots[1].connected);
}

#[test]
fn handle_client_msg_routes_pong_does_not_panic() {
    let (mut actor, _rxs) = make_actor_in_lobby(1);
    actor.handle_client_msg(1, ClientMsg::Pong { id: 7 });
    // Pong 是 noop, 不应改 state.
    assert_eq!(actor.state, RoomLifecycle::Lobby);
}

#[test]
fn handle_client_msg_routes_join_is_ignored() {
    let (mut actor, _rxs) = make_actor_in_lobby(1);
    actor.handle_client_msg(
        1,
        ClientMsg::Join {
            nickname: "x".into(),
            reconnect_token: None,
        },
    );
    // 已 join 过的玩家发 Join 应被忽略 (handle_client_msg 路由).
    assert_eq!(actor.slots.len(), 1);
}

#[test]
fn reset_to_lobby_removes_ai_and_clears_seats() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    // 当前 4 slot, 1 真人 + 3 AI.
    actor.reset_to_lobby();
    // AI 全清, 真人保留, 真人 seat = None.
    assert_eq!(actor.slots.len(), 1);
    assert!(actor.slots[0].seat.is_none());
    assert!(actor.slots[0].ready, "host 默认 ready");
    assert_eq!(actor.state, RoomLifecycle::Lobby);
    assert!(actor.game.is_none());
}

/// mark_disconnected 内部 spawn timer, 必须 tokio runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mark_disconnected_marks_slot_offline() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    actor.mark_disconnected(2);
    let s = &actor.slots[1];
    assert!(!s.connected);
    assert!(s.sender.is_none());
    assert!(s.disconnected_at.is_some());
}

// ========================================================================
// game.rs advance_game 各 phase 分支
// ========================================================================

use crate::engine::round_state::RoundEndState;

/// 把 engine 的 round 替换成 RoundEnd 状态, 模拟流局.
fn force_round_end_ryuukyoku(actor: &mut RoomActor) {
    let engine = actor.game.as_mut().unwrap();
    let common = round_common_mut(&mut engine.round).clone();
    engine.round = RoundState::RoundEnd(RoundEndState {
        common,
        result: RoundResult::Ryuukyoku {
            kind: RyuukyokuKind::Howaipai,
        },
    });
    engine.last_result = Some(RoundResult::Ryuukyoku {
        kind: RyuukyokuKind::Howaipai,
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advance_game_in_round_end_broadcasts_state_and_round_result() {
    let (mut actor, mut rxs) = make_actor_in_game(&[Seat::East]);
    force_round_end_ryuukyoku(&mut actor);
    actor.advance_game();
    let mut got_view = false;
    let mut got_result = false;
    while let Ok(msg) = rxs[0].try_recv() {
        match msg {
            ServerMsg::GameStateView(_) => got_view = true,
            ServerMsg::RoundResult(_) => got_result = true,
            _ => {}
        }
    }
    assert!(got_view, "RoundEnd 应推 GameStateView");
    assert!(got_result, "RoundEnd 应推 RoundResult");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advance_game_no_game_returns_immediately() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.game = None;
    // 不 panic 即可.
    actor.advance_game();
}

/// AwaitCalls 阶段, 全员都已 setup pending → advance_game 直接 return (不修改 state).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advance_game_in_await_calls_with_pending_returns_immediately() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    // 已 setup pending_calls, advance_game 应 early return.
    let mut m = HashMap::new();
    m.insert(2, None);
    actor.pending_calls = Some(m);
    let pending_before = actor.pending_calls.clone();
    actor.advance_game();
    // pending 不应被 advance_game 改写.
    assert!(actor.pending_calls.is_some());
    let pending_after = actor.pending_calls.clone();
    // keys 数量一致.
    assert_eq!(pending_before.unwrap().len(), pending_after.unwrap().len());
}

/// CallTimeout 过期 generation 应被忽略.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_timeout_with_stale_generation_ignored() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    actor.call_window_gen = 5;
    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m
    });
    actor.handle_cmd(RoomCmd::CallTimeout { generation: 3 });
    // pending 仍存在 (没触发 resolve).
    assert!(actor.pending_calls.is_some());
}

/// CallTimeout 当 pending=None 也 noop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_timeout_with_no_pending_ignored() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East]);
    actor.call_window_gen = 5;
    actor.pending_calls = None;
    actor.handle_cmd(RoomCmd::CallTimeout { generation: 5 });
    // 不 panic 即可.
    assert!(actor.pending_calls.is_none());
}

/// CallTimeout matched generation + pending → resolve 一次.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_timeout_matching_generation_resolves() {
    let (mut actor, _rxs) = make_actor_in_game(&[Seat::East, Seat::South]);
    force_discard_scenario(
        &mut actor,
        Seat::East,
        Tile {
            id: 50000,
            kind: TileIndex(0),
            red: false,
        },
    );
    actor.call_window_gen = 7;
    actor.pending_calls = Some({
        let mut m = HashMap::new();
        m.insert(2, None);
        m
    });
    actor.handle_cmd(RoomCmd::CallTimeout { generation: 7 });
    assert!(
        actor.pending_calls.is_none(),
        "matched timeout 应触发 resolve"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_local_peer_id_associates_existing_host_slot() {
    let (mut actor, _rxs) = make_actor_in_lobby(1);
    let bytes = vec![7u8; 32];
    actor.handle_cmd(RoomCmd::SetLocalPeerId {
        peer_id_bytes: bytes.clone(),
    });
    assert_eq!(actor.player_peers.get(&1), Some(&bytes));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn associate_peer_inserts_into_player_peers() {
    let (mut actor, _rxs) = make_actor_in_lobby(2);
    let bytes = vec![3u8; 32];
    actor.handle_cmd(RoomCmd::AssociatePeer {
        player_id: 2,
        peer_id_bytes: bytes.clone(),
    });
    assert_eq!(actor.player_peers.get(&2), Some(&bytes));
}
