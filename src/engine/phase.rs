//! Phase — 一局内的状态机阶段.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// 配牌中.
    Deal,
    /// 等当前家摸牌.
    Draw,
    /// 当前家已摸,等切牌(玩家由 UI 选择, AI 自动决定).
    AwaitDiscard,
    /// 切牌后,等他家(非自家)是否荣和.
    AwaitCalls,
    /// 一局结算,展示结果.
    RoundEnd,
    /// 整场终局.
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
