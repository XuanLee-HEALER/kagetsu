//! 玩家与 AI 决策.
//!
//! AI 占位策略 (足够推动游戏运行):
//! - 摸牌后: 能自摸就和, 否则切刚摸的那张.
//! - 他家弃牌: 能荣和就和, 否则跳过.
//!
//! 后续可替换为更复杂的策略(牌效率/防守/听牌选择等).

use crate::action::Action;
use crate::game::GameState;
use crate::meld::Seat;

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
