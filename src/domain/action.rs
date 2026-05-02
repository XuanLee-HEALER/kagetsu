//! 玩家可执行的动作 (Action).
//!
//! 由 UI 或 AI 产生, 由 [`crate::game::GameState`] 消费.

use crate::domain::meld::Seat;
use crate::domain::tile::Tile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 普通切牌.
    Discard(Tile),
    /// 立直宣言+切牌.
    Riichi(Tile),
    /// 碰. tiles 为自手将与他家弃牌组成刻子的两张.
    Pon { tiles: [Tile; 2] },
    /// 吃. tiles 为自手将与下家弃牌组成顺子的两张.
    Chi { tiles: [Tile; 2] },
    /// 大明杠.
    Minkan,
    /// 暗杠(自摸第四张).
    Ankan(Tile),
    /// 加杠(已碰刻子加上自摸第四张).
    Shouminkan(Tile),
    /// 自摸和.
    Tsumo,
    /// 荣和(对 by 家弃牌).
    Ron(Seat),
    /// 九种九牌流局宣言.
    KyuushuKyuuhai,
    /// 跳过(对鸣牌/和牌机会放弃).
    Pass,
}
