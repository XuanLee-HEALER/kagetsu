//! 玩家超时默认动作.
//!
//! 与 AI 决策 ([`crate::ai::dummy`]) 区别: **不自动和**
//! (超时不替玩家判断是否要和牌). 仅在 AwaitDiscard 阶段切刚摸到的那张,
//! 其他阶段 Pass.

use crate::engine::domain::action::Action;
use crate::engine::round_state::RoundState;

/// 玩家单步思考超时时执行的默认动作.
pub fn default_action_on_timeout(state: &RoundState) -> Action {
    match state {
        RoundState::AwaitDiscard(s) => {
            let me = s.turn;
            if let Some(t) = s.last_drawn {
                return Action::Discard(t);
            }
            if let Some(&t) = s.common.players[me.index()].hand.closed.last() {
                return Action::Discard(t);
            }
            Action::Pass
        }
        RoundState::AwaitRiichiDiscard(s) => Action::Discard(s.last_drawn),
        _ => Action::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::match_state::MatchState;
    use crate::engine::op::AtomicOp;
    use crate::engine::round_state::{init_round, round_apply};
    use crate::engine::rules::GameRules;

    #[test]
    fn timeout_default_discards_last_drawn() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 42);
        let (r, _) = round_apply(&r, AtomicOp::Draw).unwrap();
        let drawn = r
            .last_drawn()
            .expect("Draw 后 AwaitDiscard 必有 last_drawn");
        let action = default_action_on_timeout(&r);
        match action {
            Action::Discard(t) => assert_eq!(t.id, drawn.id, "应切刚摸到的那张"),
            other => panic!("期望 Discard, 得到 {:?}", other),
        }
    }

    #[test]
    fn timeout_default_pass_outside_discard_phase() {
        let m = MatchState::new(GameRules::default());
        let r = init_round(&m, 0);
        // AwaitDraw, 应 Pass.
        assert!(matches!(default_action_on_timeout(&r), Action::Pass));
    }
}

