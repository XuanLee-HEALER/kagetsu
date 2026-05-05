//! 不变量 fuzz: proptest 跑大量随机 seed, 验证麻将服务器不破坏任何核心规则.
//!
//! ## 检查的不变量
//!
//! ### 全局不变量 (任意时刻)
//! 1. **分数守恒**: `sum(player.score) + riichi_sticks * 1000 == 100_000`
//! 2. **手牌张数恒定**: 每家 closed + meld_tiles + (last_drawn ? 1 : 0) ∈ {13, 14}
//! 3. **4 家 seat 唯一**: players[].seat 各不相同, 且覆盖 East/South/West/North
//! 4. **my_seat 一致**: GameStateView.my_seat 应在 players 中找到对应 PlayerView
//! 5. **wall_remaining 范围**: ∈ [0, 70] (开局 70 张可摸)
//! 6. **dora_indicators**: ≥ 1 (开局至少 1 张), ≤ 5 (4 杠极限)
//! 7. **副露 tile 数正确**: chi/pon = 3 张, kan (各种) = 4 张
//! 8. **kyoku/honba 范围**: kyoku ∈ [1, 4] (单一场风内), honba ≥ 0
//!
//! ### 多局不变量
//! 9. **跨局分数守恒**: 跑 2-3 局, 每局结束分数守恒仍成立
//! 10. **kyoku 单调推进**: 下一局 kyoku 不降 (除非进入新场风重置 1)
//!
//! ### 异常不变量
//! 11. **server 永不 panic**: 任何随机操作序列下不崩溃
//!
//! ## 配置
//!
//! 默认 16 cases (PROPTEST_CASES env 可调). 单 case 跑 2 局, debug ~3s / release ~2.5s.
//! `just fuzz` (release + 1000 cases ≈ 40 min) 严格 fuzz.

mod common;

use std::time::Duration;

use proptest::prelude::*;
use tokio::runtime::Builder;

use tui_majo::engine::domain::meld::{MeldKind, Seat};
use tui_majo::engine::phase::Phase;
use tui_majo::net::protocol::{GameStateView, RoomLifecycle, ServerMsg};

use common::{TestRoom, TestRoomBuilder};

// ============================================================================
// 不变量检查
// ============================================================================

/// 检查所有 per-frame 不变量 (任何时刻应满足).
fn check_view_invariants(view: &GameStateView, seed: u64) -> Result<(), TestCaseError> {
    // 1. 分数守恒
    let total: i32 = view.players.iter().map(|p| p.score).sum();
    let sticks_value = view.riichi_sticks as i32 * 1000;
    prop_assert_eq!(
        total + sticks_value,
        100_000,
        "[seed={:#x}] 分数守恒破坏: total={} sticks*1000={}\nplayers={:?}",
        seed,
        total,
        sticks_value,
        view.players
            .iter()
            .map(|p| (p.seat, p.score))
            .collect::<Vec<_>>(),
    );

    // 2. 自家手牌张数: closed + meld_tiles + (last_drawn ? 1 : 0) ∈ {13, 14}
    let me_view = view
        .players
        .iter()
        .find(|p| p.seat == view.my_seat)
        .ok_or_else(|| TestCaseError::fail(format!("[seed={seed:#x}] my_seat 不在 players 中")))?;
    let my_meld_tiles: usize = me_view.melds.iter().map(meld_tile_count).sum();
    let my_total = view.my_hand.len() + my_meld_tiles + view.my_last_drawn.map(|_| 1).unwrap_or(0);
    // 注意: my_hand 已经含 last_drawn (server 端 my_hand = closed.clone()).
    // 实际等式: closed + melds = 13 (摸前) 或 14 (摸后).
    // my_hand.len() 应该 = closed.len(), 不重复计 last_drawn.
    let my_total_no_drawn = view.my_hand.len() + my_meld_tiles;
    prop_assert!(
        (13..=14).contains(&my_total_no_drawn),
        "[seed={:#x}] my_hand({}) + melds({}) = {} 应 ∈ {{13,14}}",
        seed,
        view.my_hand.len(),
        my_meld_tiles,
        my_total_no_drawn,
    );
    // 摸到的牌应在 my_hand 内 (server 实现是 closed.clone() 含摸牌)
    if let Some(d) = view.my_last_drawn {
        prop_assert!(
            view.my_hand.iter().any(|t| t.id == d.id),
            "[seed={:#x}] my_last_drawn id={} 应在 my_hand 中",
            seed,
            d.id,
        );
    }
    let _ = my_total; // 避免 warning

    // 3. 4 家 seat 唯一且覆盖 East/South/West/North
    let mut seats: Vec<Seat> = view.players.iter().map(|p| p.seat).collect();
    seats.sort_by_key(|s| s.index());
    prop_assert_eq!(
        seats,
        vec![Seat::East, Seat::South, Seat::West, Seat::North],
        "[seed={:#x}] 4 家 seat 应唯一覆盖东南西北",
        seed,
    );

    // 4. my_seat 在 players 中能找到 (上面已检查)

    // 5. wall_remaining ∈ [0, 70]
    prop_assert!(
        view.wall_remaining <= 70,
        "[seed={:#x}] wall_remaining={} 不应超 70",
        seed,
        view.wall_remaining,
    );

    // 6. dora_indicators ∈ [1, 5]
    if matches!(
        view.phase,
        Phase::AwaitDiscard | Phase::AwaitCalls | Phase::Draw
    ) {
        prop_assert!(
            !view.dora_indicators.is_empty(),
            "[seed={:#x}] 局中 dora_indicators 不应空",
            seed,
        );
    }
    prop_assert!(
        view.dora_indicators.len() <= 5,
        "[seed={:#x}] dora_indicators={} 不应超 5",
        seed,
        view.dora_indicators.len(),
    );

    // 7. 副露 tile 数正确
    for p in &view.players {
        for meld in &p.melds {
            let n = meld_tile_count(meld);
            let expected = match &meld.kind {
                MeldKind::Chi { .. } | MeldKind::Pon { .. } => 3,
                MeldKind::Minkan { .. } | MeldKind::Shouminkan { .. } | MeldKind::Ankan { .. } => 4,
            };
            prop_assert_eq!(
                n,
                expected,
                "[seed={:#x}] {:?} 的 {:?} 应有 {} 张, 实际 {}",
                seed,
                p.seat,
                meld.kind,
                expected,
                n,
            );
        }
    }

    // 8. kyoku ∈ [1, 4], honba ≥ 0
    prop_assert!(
        (1..=4).contains(&view.kyoku),
        "[seed={:#x}] kyoku={} 应 ∈ [1,4]",
        seed,
        view.kyoku,
    );
    // honba 是 u8 总 ≥ 0; 上限不固定 (流局连庄可累加)

    // 他家手牌张数: hand_count + melds*tiles ∈ {13, 14}
    for p in &view.players {
        if p.seat == view.my_seat {
            continue;
        }
        let other_meld_tiles: usize = p.melds.iter().map(meld_tile_count).sum();
        let total_count = p.hand_count + other_meld_tiles;
        prop_assert!(
            (13..=14).contains(&total_count),
            "[seed={:#x}] {:?} hand({}) + melds({}) = {} 应 ∈ {{13,14}}",
            seed,
            p.seat,
            p.hand_count,
            other_meld_tiles,
            total_count,
        );
    }

    Ok(())
}

fn meld_tile_count(m: &tui_majo::engine::domain::meld::Meld) -> usize {
    match &m.kind {
        MeldKind::Chi { tiles } | MeldKind::Pon { tiles } => tiles.len(),
        MeldKind::Minkan { tiles } | MeldKind::Shouminkan { tiles } | MeldKind::Ankan { tiles } => {
            tiles.len()
        }
    }
}

// ============================================================================
// 跑场景 + 检查
// ============================================================================

/// 跑 host 摸切直到 RoundEnd / GameEnd, 在每个 view 上检查不变量.
/// 返回 (是否到终态, view 检查次数).
async fn run_round_with_invariants(
    room: &mut TestRoom,
    seed: u64,
    max_steps: usize,
) -> Result<(bool, usize), TestCaseError> {
    let mut checks = 0usize;
    for _ in 0..max_steps {
        let v = room
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
        if room.host().has_msg(|m| matches!(m, ServerMsg::GameEnd(_))) {
            // 检查最后 view
            if let Some(view) = room.host().latest_view.as_ref() {
                check_view_invariants(view, seed)?;
                checks += 1;
            }
            return Ok((true, checks));
        }
        let Some(v) = v else {
            return Ok((false, checks));
        };
        check_view_invariants(&v, seed)?;
        checks += 1;
        if matches!(v.phase, Phase::RoundEnd | Phase::GameEnd) {
            return Ok((true, checks));
        }
        room.host().tsumogiri();
        room.drain_all().await;
    }
    Ok((false, checks))
}

/// 主测试: 任意 seed 跑 2 局, 每帧检查全部不变量.
async fn run_scenario(seed: u64) -> Result<(), TestCaseError> {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(seed)
        .start_game()
        .await;

    // 第 1 局
    let kyoku_before = room.host().latest_view.as_ref().map(|v| v.kyoku);
    let (reached1, checks1) = run_round_with_invariants(&mut room, seed, 200).await?;
    prop_assert!(
        reached1,
        "[seed={:#x}] 第 1 局应在 200 步内到达 RoundEnd. checks={}",
        seed,
        checks1,
    );
    prop_assert!(checks1 > 0, "[seed={:#x}] 应至少检查过一个 view", seed);

    // 整庄结束就停
    let lifecycle = room
        .host()
        .latest_room
        .as_ref()
        .map(|r| r.state)
        .unwrap_or(RoomLifecycle::Lobby);
    if lifecycle == RoomLifecycle::GameEnd {
        return Ok(());
    }

    // 进入第 2 局
    room.host().next_round();
    let _ = room
        .host()
        .await_view(|v| v.phase != Phase::RoundEnd, Duration::from_secs(2))
        .await;
    room.drain_all().await;

    // 检查 kyoku 单调推进或 honba 加 (流局连庄)
    if let (Some(before), Some(after)) = (
        kyoku_before,
        room.host().latest_view.as_ref().map(|v| v.kyoku),
    ) {
        // 注意: 跨场风时 kyoku 重置 1 (Hanchan 东4 → 南1). 这里只测东风战 default,
        // 一直在 East 场风, kyoku 必单增或不变 (流局连庄).
        prop_assert!(
            after >= before || after == 1,
            "[seed={:#x}] kyoku 不应倒退. before={} after={}",
            seed,
            before,
            after,
        );
    }

    // 第 2 局也跑完 (或 GameEnd)
    let (reached2, _) = run_round_with_invariants(&mut room, seed, 200).await?;
    prop_assert!(reached2, "[seed={:#x}] 第 2 局应能终止", seed);

    Ok(())
}

// ============================================================================
// runtime
// ============================================================================

fn run_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    let rt = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(future)
}

// ============================================================================
// proptest
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(16),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// 主属性: 任意 seed 跑 2 局, 每帧不变量都成立.
    #[test]
    fn random_session_preserves_invariants(seed in any::<u64>()) {
        run_async(run_scenario(seed))?;
    }
}

/// 经典 seed 回归 (固定 seed, 不走随机).
#[test]
fn regression_specific_seeds() {
    let seeds = [
        0x0_u64,
        0x1,
        0xDEAD_BEEF,
        0xCAFE_BABE,
        0xFFFF_FFFF_FFFF_FFFF,
        0x1234_5678_DEAD_BEEF, // testkit DEFAULT_SEED
    ];
    for seed in seeds {
        let r = run_async(run_scenario(seed));
        assert!(r.is_ok(), "seed {seed:#x} failed: {r:?}");
    }
}
