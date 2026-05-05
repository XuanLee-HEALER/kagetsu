//! 玩家超时默认动作.
//!
//! 与 AI 决策 ([`crate::ai::dummy`]) 区别: **不自动和**
//! (超时不替玩家判断是否要和牌). 仅在 AwaitDiscard 阶段切刚摸到的那张,
//! 其他阶段 Pass.

use crate::engine::domain::action::Action;
use crate::engine::phase::Phase;
use crate::engine::state::GameState;

/// 玩家单步思考超时时执行的默认动作.
pub fn default_action_on_timeout(state: &GameState) -> Action {
    match state.phase {
        Phase::AwaitDiscard => {
            let me = state.turn;
            if let Some(t) = state.players[me.index()].last_drawn {
                return Action::Discard(t);
            }
            if let Some(&t) = state.players[me.index()].hand.closed.last() {
                return Action::Discard(t);
            }
            Action::Pass
        }
        _ => Action::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::GameRules;

    #[test]
    fn timeout_default_discards_last_drawn() {
        let mut g = GameState::new(GameRules::default());
        g.start_round(42);
        let drawn = g.do_draw().unwrap();
        let action = default_action_on_timeout(&g);
        match action {
            Action::Discard(t) => assert_eq!(t.id, drawn.id, "应切刚摸到的那张"),
            other => panic!("期望 Discard, 得到 {:?}", other),
        }
    }

    #[test]
    fn timeout_default_pass_outside_discard_phase() {
        let g = GameState::new(GameRules::default());
        // Phase::Deal
        assert!(matches!(default_action_on_timeout(&g), Action::Pass));
    }
}
