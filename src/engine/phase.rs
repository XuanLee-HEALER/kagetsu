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
