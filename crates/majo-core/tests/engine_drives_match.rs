//! Stage 6 验证测试: 用 round_apply / match_apply / legal_ops 驱动整局/整庄.
//!
//! 不依赖 UI / driver / legacy GameState. 决策策略采用最简 dummy:
//! - AwaitDraw → Draw
//! - AwaitDiscard → Tsumo (能和就和), 否则切 last_drawn (摸切), 没 last_drawn 切第一张
//! - AwaitRiichiDiscard → 切 last_drawn
//! - AwaitRinshanDraw → RinshanDraw
//! - AwaitCalls → 谁能荣和就 Ron, 否则 Pass (不主动鸣牌, 简化测试)
//! - RoundEnd → 调用方处理 (summarize_round + match_apply 进下一局)
//!
//! 目的: 证明 engine 公开 API 自洽 + 完整 + 能跑完一局/多局, 不卡死.

use majo_core::engine::match_state::{MatchState, match_apply};
use majo_core::engine::op::{AtomicOp, OpError};
use majo_core::engine::round_state::{
    RoundState, init_round, legal_ops, round_apply, summarize_round,
};
use majo_core::engine::rules::{GameRules, LengthRule};

/// 推进单步: 根据当前 RoundState 决定下一 op, 调 round_apply.
fn step(state: &RoundState) -> Result<RoundState, OpError> {
    let op = decide(state);
    let (next, _events) = round_apply(state, op)?;
    Ok(next)
}

/// 最简决策器.
fn decide(state: &RoundState) -> AtomicOp {
    match state {
        RoundState::AwaitDraw(_) => AtomicOp::Draw,
        RoundState::AwaitDiscard(s) => {
            let ops = legal_ops(state);
            if ops.can_tsumo {
                return AtomicOp::Tsumo;
            }
            // 摸切.
            if let Some(t) = s.last_drawn() {
                return AtomicOp::Discard { tile: t };
            }
            // 鸣牌后无 last_drawn, 切第一张.
            let p = &s.common.players[s.turn.index()];
            let t = p
                .hand
                .closed
                .first()
                .copied()
                .expect("AwaitDiscard with empty hand");
            AtomicOp::Discard { tile: t }
        }
        RoundState::AwaitRiichiDiscard(s) => {
            // 立直方必须切 last_drawn (摸切).
            AtomicOp::Discard { tile: s.last_drawn }
        }
        RoundState::AwaitRinshanDraw(_) => AtomicOp::RinshanDraw,
        RoundState::AwaitCalls(_) => {
            let ops = legal_ops(state);
            // 任何家能荣和就 Ron.
            for who in majo_core::engine::domain::meld::Seat::ALL {
                if ops.calls[who.index()].ron {
                    return AtomicOp::Ron { who };
                }
            }
            AtomicOp::Pass
        }
        RoundState::RoundEnd(_) => {
            panic!("decide called on RoundEnd, caller should handle this");
        }
    }
}

/// 驱动一局直到 RoundEnd. max_steps 兜底防死循环.
fn drive_round(initial: RoundState, max_steps: usize) -> RoundState {
    let mut s = initial;
    for i in 0..max_steps {
        if s.is_ended() {
            return s;
        }
        s = step(&s).unwrap_or_else(|e| panic!("step #{} failed: {:?}", i, e));
    }
    panic!("round did not end within {} steps", max_steps);
}

#[test]
fn engine_drives_one_round_to_end() {
    let rules = GameRules {
        length: LengthRule::Tonpuusen,
        ..GameRules::default()
    };
    let m = MatchState::new(rules);
    let r = init_round(&m, 0xdead_beef);
    let end = drive_round(r, 1000);
    assert!(end.is_ended(), "round should end");
    let outcome = summarize_round(&end).expect("RoundEnd should summarize");
    // 至少不 panic, outcome 必合法.
    match outcome {
        majo_core::engine::match_state::RoundOutcome::Win { payments, .. } => {
            assert!(!payments.is_empty(), "Win 必有 payments");
        }
        majo_core::engine::match_state::RoundOutcome::Ryuukyoku { .. } => {}
    }
}

#[test]
fn engine_drives_full_tonpuusen_match() {
    let rules = GameRules {
        length: LengthRule::Tonpuusen,
        ..GameRules::default()
    };
    let mut m = MatchState::new(rules);
    let mut round_seed = 0u64;
    let mut rounds_played = 0usize;

    while !m.ended {
        let r = init_round(&m, round_seed.wrapping_add(0x9E37_79B9_7F4A_7C15));
        let end = drive_round(r, 1000);
        let outcome = summarize_round(&end).expect("RoundEnd 必能 summarize");
        m = match_apply(&m, outcome);
        round_seed = round_seed.wrapping_add(1);
        rounds_played += 1;
        assert!(
            rounds_played < 30,
            "tonpuusen 不可能跑超过 30 局, 死循环了?"
        );
    }

    assert!(m.ended, "match 应已结束");
    assert!(rounds_played >= 4, "tonpuusen 至少 4 局 (无连庄时)");
    let total_score: i32 = m.scores.iter().sum();
    assert_eq!(
        total_score, 100_000,
        "4 家分数总和应守恒 = 4 × 25000 (不考虑供托)"
    );
}

#[test]
fn legal_ops_consistent_with_phase() {
    // 进 AwaitDiscard 后 legal_ops 至少不 panic, riichi_discards/ankan/shouminkan
    // 是 Vec, can_tsumo 是 bool, calls 是 [Default;4].
    let m = MatchState::new(GameRules::default());
    let r = init_round(&m, 42);
    // r 是 AwaitDraw, legal_ops 在该阶段返默认空.
    let ops0 = legal_ops(&r);
    assert!(!ops0.can_tsumo);
    assert!(ops0.riichi_discards.is_empty());

    // 推一步 Draw → AwaitDiscard.
    let (r1, _) = round_apply(&r, AtomicOp::Draw).expect("Draw should succeed");
    assert!(matches!(r1, RoundState::AwaitDiscard(_)));
    let _ops1 = legal_ops(&r1); // 不 panic.
}

#[test]
fn round_apply_rejects_illegal_op_for_phase() {
    let m = MatchState::new(GameRules::default());
    let r = init_round(&m, 1);
    // AwaitDraw 阶段塞 Discard 应当返 IllegalForPhase.
    let dummy_tile = majo_core::engine::domain::tile::Tile {
        kind: majo_core::engine::domain::tile::TileIndex(0),
        red: false,
        id: 0,
    };
    let err = round_apply(&r, AtomicOp::Discard { tile: dummy_tile }).unwrap_err();
    assert!(matches!(err, OpError::IllegalForPhase { .. }));
}

#[test]
fn round_apply_rejects_op_on_ended_round() {
    let rules = GameRules::default();
    let m = MatchState::new(rules);
    let r = init_round(&m, 7777);
    let ended = drive_round(r, 1000);
    assert!(ended.is_ended());
    let err = round_apply(&ended, AtomicOp::Pass).unwrap_err();
    assert!(matches!(err, OpError::AlreadyEnded));
}
