//! 天凤 tile id (0-135) ↔ [`Tile`] 转换.
//!
//! ## 编码
//!
//! 天凤每张牌都有唯一 id 0-135 (= 4 张 × 34 种 kind):
//! - `id / 4` = TileIndex (0-33)
//! - `id % 4` = 同种花色中的第几张 (0-3)
//!
//! ## TileIndex 对应
//! - 0-8: 1m-9m
//! - 9-17: 1p-9p
//! - 18-26: 1s-9s
//! - 27: 东, 28: 南, 29: 西, 30: 北
//! - 31: 白, 32: 发, 33: 中
//!
//! ## 赤 5
//! 天凤约定: 5m 赤是 id 16, 5p 赤是 id 52, 5s 赤是 id 88
//! (即 5 这种 kind 的 4 张中 id 最小的那张).

use tui_majo::tile::{Tile, TileIndex};

/// 天凤 tile id (0-135) → Tile.
pub fn tenhou_id_to_tile(id: u16) -> Result<Tile, String> {
    if id >= 136 {
        return Err(format!("tenhou id {id} 超出范围 0-135"));
    }
    let kind = TileIndex((id / 4) as u8);
    let red = matches!(id, 16 | 52 | 88);
    Ok(Tile { id, kind, red })
}

/// Tile → 天凤 id. 选 4 张中"任意"一张的 id (id_within_4 ∈ 0..4).
/// `id_within_4 = 0` 是赤 5 (如果 kind 是 5m/5p/5s).
pub fn tile_to_tenhou_id(t: Tile) -> u16 {
    let base = (t.kind.0 as u16) * 4;
    if t.red {
        match t.kind.0 {
            4 => 16,  // 5m
            13 => 52, // 5p
            22 => 88, // 5s
            _ => base,
        }
    } else {
        // 非赤 5 的 5: 选 id_within_4 = 1, 2, or 3
        if matches!(t.kind.0, 4 | 13 | 22) {
            base + 1
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_0_is_1m() {
        let t = tenhou_id_to_tile(0).unwrap();
        assert_eq!(t.kind.0, 0);
        assert!(!t.red);
    }

    #[test]
    fn id_3_is_also_1m() {
        // id 0,1,2,3 都是 1m
        for id in 0..=3 {
            assert_eq!(tenhou_id_to_tile(id).unwrap().kind.0, 0);
        }
        // id 4 起是 2m
        assert_eq!(tenhou_id_to_tile(4).unwrap().kind.0, 1);
    }

    #[test]
    fn aka_5m_id_16() {
        let t = tenhou_id_to_tile(16).unwrap();
        assert_eq!(t.kind.0, 4);
        assert!(t.red);
        // id 17, 18, 19 是普通 5m
        for id in 17..=19 {
            let t = tenhou_id_to_tile(id).unwrap();
            assert_eq!(t.kind.0, 4);
            assert!(!t.red);
        }
    }

    #[test]
    fn aka_5p_id_52() {
        let t = tenhou_id_to_tile(52).unwrap();
        assert_eq!(t.kind.0, 13);
        assert!(t.red);
    }

    #[test]
    fn aka_5s_id_88() {
        let t = tenhou_id_to_tile(88).unwrap();
        assert_eq!(t.kind.0, 22);
        assert!(t.red);
    }

    #[test]
    fn id_108_to_135_are_honors() {
        // 108-111: 东 (kind 27)
        for id in 108..=111 {
            assert_eq!(tenhou_id_to_tile(id).unwrap().kind.0, 27);
        }
        // 132-135: 中 (kind 33)
        for id in 132..=135 {
            assert_eq!(tenhou_id_to_tile(id).unwrap().kind.0, 33);
        }
    }

    #[test]
    fn invalid_id_returns_err() {
        assert!(tenhou_id_to_tile(136).is_err());
        assert!(tenhou_id_to_tile(255).is_err());
    }

    #[test]
    fn round_trip_basic_tiles() {
        // 非 5 的牌: round trip 准确
        for kind in [0u8, 1, 2, 3, 5, 6, 7, 8, 27, 28, 29, 30, 31, 32, 33] {
            let t = Tile {
                id: 0,
                kind: TileIndex(kind),
                red: false,
            };
            let id = tile_to_tenhou_id(t);
            let back = tenhou_id_to_tile(id).unwrap();
            assert_eq!(back.kind.0, kind);
            assert!(!back.red);
        }
    }

    #[test]
    fn round_trip_aka() {
        for kind in [4u8, 13, 22] {
            let t = Tile {
                id: 0,
                kind: TileIndex(kind),
                red: true,
            };
            let id = tile_to_tenhou_id(t);
            let back = tenhou_id_to_tile(id).unwrap();
            assert_eq!(back.kind.0, kind);
            assert!(back.red);
        }
    }
}
