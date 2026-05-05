//! GameEvent — 局内动作事件, 给 UI 渲染最近动作日志使用.

use serde::{Deserialize, Serialize};

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::{Tile, TileIndex};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEvent {
    Draw { who: Seat, tile: Tile },
    Discard { who: Seat, tile: Tile },
    Pon { who: Seat, tile: Tile },
    Chi { who: Seat, tile: Tile },
    Minkan { who: Seat, tile: Tile },
    Ankan { who: Seat, kind: TileIndex },
    Shouminkan { who: Seat, kind: TileIndex },
    Riichi { who: Seat, tile: Tile },
    Tsumo { who: Seat },
    Ron { who: Seat, from: Seat },
}

/// 单局事件 ring buffer 容量上限.
pub(crate) const MAX_EVENTS: usize = 32;

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
    fn max_events_is_positive() {
        const _: () = assert!(MAX_EVENTS > 0);
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
