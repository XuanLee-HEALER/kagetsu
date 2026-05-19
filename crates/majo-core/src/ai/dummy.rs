//! 占位 AI 决策.
//!
//! 策略 (足够推动游戏运行):
//! - 摸牌后: 能自摸就和, 否则切刚摸的那张 (摸切).
//! - 他家弃牌: 能荣和就和, 否则跳过 (不鸣牌).
//!
//! 输入是 engine 公开的 RoundState (不依赖 legacy GameState).

use crate::engine::domain::action::Action;
use crate::engine::domain::meld::Seat;
use crate::engine::round_state::{RoundState, legal_ops};

/// 决定 AI 在 AwaitDiscard 阶段的动作.
pub fn ai_choose_discard(state: &RoundState) -> Action {
    let RoundState::AwaitDiscard(s) = state else {
        return Action::Pass;
    };
    // 优先自摸.
    if legal_ops(state).can_tsumo {
        return Action::Tsumo;
    }
    let me = s.turn;
    // 切刚摸到的那张 (摸什么切什么).
    if let Some(t) = s.last_drawn() {
        return Action::Discard(t);
    }
    // 鸣牌后无 last_drawn, 兜底切第一张.
    if let Some(&t) = s.common.players[me.index()].hand.closed.first() {
        return Action::Discard(t);
    }
    Action::Pass
}

/// 决定 AI 是否对最近弃牌响应 (仅荣和; 不鸣牌).
pub fn ai_react_to_discard(state: &RoundState, who: Seat) -> Action {
    if !matches!(state, RoundState::AwaitCalls(_)) {
        return Action::Pass;
    }
    if legal_ops(state).calls[who.index()].ron {
        return Action::Ron(who);
    }
    Action::Pass
}
