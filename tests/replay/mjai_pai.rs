//! mjai 牌名字符串 ↔ [`Tile`] 双向映射.
//!
//! ## mjai 命名约定
//!
//! - `1m`..`9m` 万子
//! - `1p`..`9p` 筒子
//! - `1s`..`9s` 索子
//! - `5mr`/`5pr`/`5sr` 赤 5
//! - 字牌: `E`(东) `S`(南) `W`(西) `N`(北) `P`(白/haku) `F`(发) `C`(中)
//!
//! ## 我们的 [`TileIndex`] 编码
//!
//! - 0-8: 1m-9m
//! - 9-17: 1p-9p
//! - 18-26: 1s-9s
//! - 27: 东, 28: 南, 29: 西, 30: 北
//! - 31: 白, 32: 发, 33: 中

use tui_majo::domain::tile::{Tile, TileIndex};

/// 解析 mjai pai 字符串 → Tile (id 自动分配, 不保证全局唯一).
///
/// 用 `id_seed` 当 base id 给生成的 Tile, 调用方负责保证 id 唯一.
pub fn parse_mjai_pai(s: &str, id_seed: u16) -> Result<Tile, String> {
    let kind_red = parse_mjai_kind(s)?;
    Ok(Tile {
        id: id_seed,
        kind: kind_red.0,
        red: kind_red.1,
    })
}

/// 解析 mjai pai → (TileIndex, is_red).
pub fn parse_mjai_kind(s: &str) -> Result<(TileIndex, bool), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("空牌名".into());
    }
    // 字牌: E/S/W/N/P/F/C
    if s.len() == 1 {
        return match s {
            "E" => Ok((TileIndex::EAST, false)),
            "S" => Ok((TileIndex::SOUTH, false)),
            "W" => Ok((TileIndex::WEST, false)),
            "N" => Ok((TileIndex::NORTH, false)),
            "P" => Ok((TileIndex::HAKU, false)),
            "F" => Ok((TileIndex::HATSU, false)),
            "C" => Ok((TileIndex::CHUN, false)),
            _ => Err(format!("未知字牌 '{s}'")),
        };
    }
    // 数牌: NX 或 NXr (X = m/p/s)
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes.len() > 3 {
        return Err(format!("无效 pai 长度 '{s}'"));
    }
    let n_byte = bytes[0];
    if !n_byte.is_ascii_digit() {
        return Err(format!("第 1 字符非数字 '{s}'"));
    }
    let n = n_byte - b'0';
    if !(1..=9).contains(&n) {
        return Err(format!("数字 {n} 超出 1-9 '{s}'"));
    }
    let suit = bytes[1] as char;
    let red = bytes.len() == 3 && bytes[2] == b'r';
    if red && n != 5 {
        return Err(format!("仅 5 可为赤 '{s}'"));
    }
    let base = match suit {
        'm' => 0u8,
        'p' => 9,
        's' => 18,
        _ => return Err(format!("未知花色 '{suit}' in '{s}'")),
    };
    Ok((TileIndex(base + n - 1), red))
}

/// Tile → mjai pai 字符串.
pub fn tile_to_mjai_pai(t: Tile) -> String {
    let n = t.kind.0;
    let suffix = if t.red { "r" } else { "" };
    match n {
        0..=8 => format!("{}m{}", n + 1, suffix),
        9..=17 => format!("{}p{}", n - 9 + 1, suffix),
        18..=26 => format!("{}s{}", n - 18 + 1, suffix),
        27 => "E".into(),
        28 => "S".into(),
        29 => "W".into(),
        30 => "N".into(),
        31 => "P".into(),
        32 => "F".into(),
        33 => "C".into(),
        _ => "??".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_man() {
        let (k, r) = parse_mjai_kind("1m").unwrap();
        assert_eq!(k.0, 0);
        assert!(!r);
        let (k, r) = parse_mjai_kind("9m").unwrap();
        assert_eq!(k.0, 8);
        assert!(!r);
    }

    #[test]
    fn parse_basic_pin_sou() {
        assert_eq!(parse_mjai_kind("1p").unwrap().0.0, 9);
        assert_eq!(parse_mjai_kind("9p").unwrap().0.0, 17);
        assert_eq!(parse_mjai_kind("1s").unwrap().0.0, 18);
        assert_eq!(parse_mjai_kind("9s").unwrap().0.0, 26);
    }

    #[test]
    fn parse_aka_red_5() {
        let (k, r) = parse_mjai_kind("5mr").unwrap();
        assert_eq!(k.0, 4);
        assert!(r);
        assert!(parse_mjai_kind("5pr").unwrap().1);
        assert!(parse_mjai_kind("5sr").unwrap().1);
    }

    #[test]
    fn parse_honors() {
        assert_eq!(parse_mjai_kind("E").unwrap().0.0, 27);
        assert_eq!(parse_mjai_kind("S").unwrap().0.0, 28);
        assert_eq!(parse_mjai_kind("W").unwrap().0.0, 29);
        assert_eq!(parse_mjai_kind("N").unwrap().0.0, 30);
        assert_eq!(parse_mjai_kind("P").unwrap().0.0, 31);
        assert_eq!(parse_mjai_kind("F").unwrap().0.0, 32);
        assert_eq!(parse_mjai_kind("C").unwrap().0.0, 33);
    }

    #[test]
    fn parse_invalid_returns_error() {
        assert!(parse_mjai_kind("").is_err());
        assert!(parse_mjai_kind("0m").is_err());
        assert!(parse_mjai_kind("10m").is_err());
        assert!(parse_mjai_kind("1x").is_err());
        assert!(parse_mjai_kind("4mr").is_err()); // 仅 5 可赤
        assert!(parse_mjai_kind("Z").is_err());
    }

    #[test]
    fn round_trip_all_tiles() {
        // 0-33 各 kind 都能 round-trip
        for k in 0..34u8 {
            let t = Tile {
                id: 0,
                kind: TileIndex(k),
                red: false,
            };
            let s = tile_to_mjai_pai(t);
            let (k2, r2) = parse_mjai_kind(&s).unwrap();
            assert_eq!(k, k2.0, "kind {k} round-trip via '{s}'");
            assert!(!r2);
        }
        // 赤 5
        for kind in [4u8, 13, 22] {
            let t = Tile {
                id: 0,
                kind: TileIndex(kind),
                red: true,
            };
            let s = tile_to_mjai_pai(t);
            assert!(s.ends_with('r'), "赤 5 应以 r 结尾, 实际 '{s}'");
            let (k2, r2) = parse_mjai_kind(&s).unwrap();
            assert_eq!(kind, k2.0);
            assert!(r2);
        }
    }
}
