//! 局内动作事件 ([`GameEvent`]).
//!
//! [`crate::engine::round_state::round_apply`] 在每次成功 op 后返回该步 emit 的
//! `Vec<GameEvent>`. UI / 录像 / 网络 protocol 用这个流给玩家展示动作历史.
//!
//! # GameEvent vs AtomicOp
//!
//! - [`crate::engine::op::AtomicOp`] = *做什么决策* (input). caller 喂给 engine.
//! - `GameEvent` = *做了什么事情* (output / 历史). engine 返回给 caller.
//!
//! 对应关系大致 1:1 (除了 RiichiDeclare + Discard 二步合并为一个 Riichi 事件).

use serde::{Deserialize, Serialize};

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::{Tile, TileIndex};

/// 局内一个动作事件 — engine emit 出来给 UI / 录像 消费.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEvent {
    /// 摸牌 (含普通摸 + 岭上摸 — 当前未细分).
    Draw { who: Seat, tile: Tile },
    /// 切牌. 触发鸣牌窗口.
    Discard { who: Seat, tile: Tile },
    /// 碰 (Pon). `tile` = 被鸣的弃牌.
    Pon { who: Seat, tile: Tile },
    /// 吃 (Chi). `tile` = 被鸣的弃牌.
    Chi { who: Seat, tile: Tile },
    /// 大明杠 (Minkan).
    Minkan { who: Seat, tile: Tile },
    /// 暗杠 (Ankan). 用 `kind` 而非 `tile` 因为 4 张全是同 kind, 不必指定具体 id.
    Ankan { who: Seat, kind: TileIndex },
    /// 加杠 (Shouminkan).
    Shouminkan { who: Seat, kind: TileIndex },
    /// 立直 (Riichi) 宣告 + 切牌的合并事件. `tile` = 宣告时切的那张 (UI 横置展示).
    Riichi { who: Seat, tile: Tile },
    /// 自摸和了.
    Tsumo { who: Seat },
    /// 荣和. `from` = 放铳家.
    Ron { who: Seat, from: Seat },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::tile::TileIndex;

    fn t(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    #[test]
    fn event_serde_roundtrip_draw() {
        let e = GameEvent::Draw {
            who: Seat::East,
            tile: t(0, 0),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: GameEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn event_serde_roundtrip_ron() {
        let e = GameEvent::Ron {
            who: Seat::South,
            from: Seat::West,
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: GameEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn event_serde_roundtrip_ankan_with_kind() {
        let e = GameEvent::Ankan {
            who: Seat::North,
            kind: TileIndex(33),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: GameEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn distinct_events_not_equal() {
        let e1 = GameEvent::Tsumo { who: Seat::East };
        let e2 = GameEvent::Tsumo { who: Seat::South };
        assert_ne!(e1, e2);
    }

    #[test]
    fn event_clone_preserves() {
        let e = GameEvent::Riichi {
            who: Seat::East,
            tile: t(4, 0),
        };
        let copy = e.clone();
        assert_eq!(e, copy);
    }
}
