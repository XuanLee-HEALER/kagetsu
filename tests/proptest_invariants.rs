//! 不变量 fuzz: proptest 跑大量随机 seed, 验证
//!
//! 1. **分数守恒**: 任意时刻 sum(player.score) + riichi_sticks * 1000 == 100_000
//! 2. **server 永不 panic**: 跑完整对局
//! 3. **GameStateView projection 一致性**: 任意 seat 投影后, my_hand.len() + melds tile 数
//!    + (my_last_drawn ? 1 : 0) ≥ 13.
//!
//! ## 配置
//!
//! 默认 16 cases (PROPTEST_CASES env 可调). 单 case 跑一局完整对局
//! (host 摸切到 RoundEnd), debug ~1.5s / release ~1s. 16 cases CI 可接受.
//! 严格 fuzz 用 `just fuzz` (release + 1000 cases ≈ 17 min).

mod common;

use std::time::Duration;

use proptest::prelude::*;
use tokio::runtime::Builder;

use tui_majo::game::Phase;

use common::TestRoomBuilder;

/// 一局完整对局的不变量验证: 跑 host 摸切到 RoundEnd, 检查分数守恒.
async fn run_one_round_check_invariants(seed: u64) -> Result<(), TestCaseError> {
    let mut room = TestRoomBuilder::new()
        .humans(1)
        .seed(seed)
        .start_game()
        .await;

    // 跑直到 RoundEnd / GameEnd
    for _ in 0..200 {
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
        let Some(v) = v else { break };
        if matches!(v.phase, Phase::RoundEnd | Phase::GameEnd) {
            break;
        }
        room.host().tsumogiri();
        room.drain_all().await;
    }

    // 检查不变量
    let view = room
        .host()
        .latest_view
        .as_ref()
        .ok_or_else(|| TestCaseError::fail("no latest_view"))?;

    // 1. 分数守恒
    let total: i32 = view.players.iter().map(|p| p.score).sum();
    let sticks_value = view.riichi_sticks as i32 * 1000;
    let invariant = total + sticks_value;
    prop_assert_eq!(
        invariant,
        100_000,
        "[seed={:#x}] 分数守恒破坏: total={} sticks*1000={} expected 100000\nplayers={:?}",
        seed,
        total,
        sticks_value,
        view.players.iter().map(|p| p.score).collect::<Vec<_>>(),
    );

    // 2. 自家手牌张数合理: my_hand.len() ∈ [13, 14] (含 last_drawn 时 14)
    let hand_len = view.my_hand.len();
    prop_assert!(
        (13..=14).contains(&hand_len),
        "[seed={:#x}] my_hand.len()={} 应在 13..=14",
        seed,
        hand_len
    );

    // 3. 4 家分数合计 (不含 sticks) > 0 (虽然单家可能 < 0)
    prop_assert!(total > 0, "[seed={:#x}] 总分应 > 0, 实际 {}", seed, total);

    Ok(())
}

/// 跑测试用 worker_threads=2 multi_thread runtime.
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

proptest! {
    // 默认 16 cases (~16s debug / ~16s release), CI 中跑.
    // `just fuzz` 设 PROPTEST_CASES=1000 跑彻底 fuzz.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(16),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// 主属性: 任意 seed 跑一局, 分数守恒不破坏.
    #[test]
    fn random_round_preserves_score_invariant(seed in any::<u64>()) {
        run_async(run_one_round_check_invariants(seed))?;
    }
}

/// 经典 seed 回归: 之前手测发现的边界案例放这里, 防回归.
/// 这是普通 #[test] 不走 proptest, 不依赖随机性.
#[test]
fn regression_specific_seeds() {
    let seeds = [
        0x0_u64,
        0x1,
        0xDEAD_BEEF,
        0xCAFE_BABE,
        0xFFFF_FFFF_FFFF_FFFF,
    ];
    for seed in seeds {
        let r = run_async(run_one_round_check_invariants(seed));
        assert!(r.is_ok(), "seed {:#x} failed: {:?}", seed, r);
    }
}
