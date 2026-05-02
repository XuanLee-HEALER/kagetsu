//! 玩家与 AI 决策.
//!
//! AI 占位策略 (足够推动游戏运行):
//! - 摸牌后: 能自摸就和, 否则切刚摸的那张.
//! - 他家弃牌: 能荣和就和, 否则跳过.
//!
//! 后续可替换为更复杂的策略(牌效率/防守/听牌选择等).

use crate::domain::action::Action;
use crate::engine::phase::Phase;
use crate::engine::state::GameState;
use crate::domain::meld::Seat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    Human,
    DummyAi,
}

/// 决定 AI 在 AwaitDiscard 阶段的动作.
pub fn ai_choose_discard(state: &GameState) -> Action {
    // 优先自摸.
    if state.can_tsumo() {
        return Action::Tsumo;
    }
    // 切刚摸到的那张(摸什么切什么).
    let me = state.turn;
    if let Some(t) = state.players[me.index()].last_drawn {
        return Action::Discard(t);
    }
    // 兜底: 切最后一张.
    if let Some(&t) = state.players[me.index()].hand.closed.last() {
        return Action::Discard(t);
    }
    Action::Pass
}

/// 决定 AI 是否对最近弃牌响应(仅荣和; 不鸣牌).
pub fn ai_react_to_discard(state: &GameState, who: Seat) -> Action {
    if state.can_ron(who) {
        return Action::Ron(who);
    }
    Action::Pass
}

/// 玩家单步思考超时时执行的默认动作.
///
/// 与 [`ai_choose_discard`] 区别: **不自动和**(超时不替玩家判断是否要和牌).
/// - AwaitDiscard: 切刚摸到的那张; 兜底切最后一张
/// - 其他阶段: Pass
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
