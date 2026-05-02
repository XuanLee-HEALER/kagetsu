//! 跨屏共享的小 helper. 牌张渲染走 [`crate::ui::paint`] 的 paint_tile_*.

use crate::domain::meld::Seat;

pub fn seat_label(s: Seat) -> &'static str {
    match s {
        Seat::East => "东",
        Seat::South => "南",
        Seat::West => "西",
        Seat::North => "北",
    }
}
