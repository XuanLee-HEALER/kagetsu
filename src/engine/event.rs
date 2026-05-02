//! GameEvent — 局内动作事件, 给 UI 渲染最近动作日志使用.

use serde::{Deserialize, Serialize};

use crate::domain::meld::Seat;
use crate::domain::tile::{Tile, TileIndex};

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
