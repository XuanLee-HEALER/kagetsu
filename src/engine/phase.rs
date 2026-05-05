//! Legacy 4-phase 表示. 兼容 UI 用.
//!
//! 真正的状态机现在是 [`crate::engine::round_state::RoundState`] 的 6-variant.
//! 此 [`Phase`] 是 4-phase 折叠 (AwaitRiichiDiscard / AwaitRinshanDraw 折进
//! Draw / AwaitDiscard, GameEnd 来自 [`MatchState::ended`]).
//!
//! 网络协议 (`net::protocol`) + UI 仍用此粒度. 新代码应优先用
//! `RoundState` 直接 match.
//!
//! [`MatchState::ended`]: crate::engine::match_state::MatchState::ended

use serde::{Deserialize, Serialize};

/// 局内状态机阶段 (legacy 4-phase 表示, 实际 6 phase 见 [`RoundState`]).
///
/// [`RoundState`]: crate::engine::round_state::RoundState
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// 配牌中 (起手未完成). 当前 `RoundState` 不再单独建模这个阶段, 仅 legacy
    /// `GameState::new` 后的瞬态.
    Deal,
    /// 等摸牌 (含 `AwaitDraw` + `AwaitRinshanDraw`, 后者岭上摸由 driver 自动).
    Draw,
    /// 等切牌 (含 `AwaitDiscard` + `AwaitRiichiDiscard`).
    AwaitDiscard,
    /// 切牌后等他家鸣 / 荣和 / 跳过.
    AwaitCalls,
    /// 局结束 (和了 / 流局), 等 driver 调 `next_round` 推进.
    RoundEnd,
    /// 整庄结束. 当 [`MatchState::ended`] = true 且局已 end 时由 GameEngine wrapper 暴露.
    ///
    /// [`MatchState::ended`]: crate::engine::match_state::MatchState::ended
    GameEnd,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_serde_roundtrip() {
        for p in [
            Phase::Deal,
            Phase::Draw,
            Phase::AwaitDiscard,
            Phase::AwaitCalls,
            Phase::RoundEnd,
            Phase::GameEnd,
        ] {
            let s = serde_json::to_string(&p).unwrap();
            let back: Phase = serde_json::from_str(&s).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn phase_distinct() {
        assert_ne!(Phase::Deal, Phase::Draw);
        assert_ne!(Phase::AwaitDiscard, Phase::AwaitCalls);
        assert_ne!(Phase::RoundEnd, Phase::GameEnd);
    }

    #[test]
    fn phase_copy_semantics() {
        let p = Phase::Draw;
        let q = p; // Copy
        assert_eq!(p, q);
    }
}
