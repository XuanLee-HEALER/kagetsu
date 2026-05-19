//! 天凤鸣牌 m 字段 (16-bit) 解码.
//!
//! Port from mjx-project/mjx (C++): include/mjx/internal/open.cpp
//! 参考: <https://m77.hatenablog.com/entry/2017/05/21/214529>

use majo_core::engine::domain::meld::{Meld, MeldKind, Seat};
use majo_core::engine::domain::tile::Tile;

use super::tenhou_pai::tenhou_id_to_tile;

const MASK_FROM: u16 = 0b0000_0000_0000_0011;
const MASK_IS_CHI: u16 = 0b0000_0000_0000_0100;
const MASK_IS_PON: u16 = 0b0000_0000_0000_1000;
const MASK_IS_KAN_ADDED: u16 = 0b0000_0000_0001_0000;

/// chi 中三张牌的 aka_offset 位掩码.
const MASK_CHI_OFFSET: [u16; 3] = [
    0b0000_0000_0001_1000, // bit 3-4
    0b0000_0000_0110_0000, // bit 5-6
    0b0000_0001_1000_0000, // bit 7-8
];
const MASK_PON_UNUSED_OFFSET: u16 = 0b0000_0000_0110_0000; // bit 5-6

/// 解码后的鸣牌 (含 from 玩家相对位置).
#[derive(Debug, Clone)]
pub struct DecodedMeld {
    pub meld: Meld,
    /// 鸣牌方相对位置. 0=self (暗杠), 1=下家, 2=对家, 3=上家 (chi).
    pub from_relative: u8,
}

impl DecodedMeld {
    /// 解析 m 字段, `who` 是发起鸣牌的玩家.
    /// 优先级 (与 mjx-project 一致): chi > pon > kakan > kan.
    /// 注意 kakan 标记是 bit 4, 不要求 pon flag 同时 set.
    pub fn decode(m: u16, _who: Seat) -> Result<Self, String> {
        if m & MASK_IS_CHI != 0 {
            decode_chi(m)
        } else if m & MASK_IS_PON != 0 {
            decode_pon(m)
        } else if m & MASK_IS_KAN_ADDED != 0 {
            decode_kakan(m)
        } else {
            decode_kan(m)
        }
    }
}

fn from_relative(m: u16) -> u8 {
    (m & MASK_FROM) as u8
}

/// chi: 上家切的牌 + 自家 2 张组成顺子.
/// `min_type_base21 = (m >> 10) / 3`, 然后跨花色解码.
fn decode_chi(m: u16) -> Result<DecodedMeld, String> {
    let base21 = (m >> 10) / 3;
    // base21 ∈ [0, 21): 0-6 = 1m-7m, 7-13 = 1p-7p, 14-20 = 1s-7s
    let min_kind_u8 = ((base21 / 7) * 9 + base21 % 7) as u8;
    let _stolen_idx = ((m >> 10) % 3) as usize;

    let mut tiles = [Tile {
        id: 0,
        kind: majo_core::engine::domain::tile::TileIndex(0),
        red: false,
    }; 3];
    for (i, slot) in tiles.iter_mut().enumerate() {
        let aka_offset = (m & MASK_CHI_OFFSET[i]) >> (2 * i + 3);
        let tile_id = (min_kind_u8 as u16 + i as u16) * 4 + aka_offset;
        *slot = tenhou_id_to_tile(tile_id)?;
    }

    // 实际 from = 总是上家 (chi 只能从上家鸣)
    Ok(DecodedMeld {
        meld: Meld {
            kind: MeldKind::Chi { tiles },
            from: None, // 在外层根据 who 推算上家
        },
        from_relative: 3, // 上家
    })
}

/// pon: 3 张同 kind, 4 张中有 1 张未用.
fn decode_pon(m: u16) -> Result<DecodedMeld, String> {
    let pon_kind = ((m >> 9) / 3) as u8;
    let _stolen_idx = (m >> 9) % 3;
    let unused_offset = (m & MASK_PON_UNUSED_OFFSET) >> 5;

    // 4 张中跳过 unused_offset 那张
    let mut tiles = [Tile {
        id: 0,
        kind: majo_core::engine::domain::tile::TileIndex(0),
        red: false,
    }; 3];
    let mut copy_idx = 0u16;
    for slot in tiles.iter_mut() {
        if copy_idx == unused_offset {
            copy_idx += 1;
        }
        *slot = tenhou_id_to_tile((pon_kind as u16) * 4 + copy_idx)?;
        copy_idx += 1;
    }

    Ok(DecodedMeld {
        meld: Meld {
            kind: MeldKind::Pon { tiles },
            from: None,
        },
        from_relative: from_relative(m),
    })
}

/// kakan: pon 升级为加杠. 4 张 = pon 的 3 张 + 加杠的 1 张 (= unused_offset).
fn decode_kakan(m: u16) -> Result<DecodedMeld, String> {
    let kind = ((m >> 9) / 3) as u8;
    let mut tiles = [Tile {
        id: 0,
        kind: majo_core::engine::domain::tile::TileIndex(0),
        red: false,
    }; 4];
    for (i, slot) in tiles.iter_mut().enumerate() {
        *slot = tenhou_id_to_tile((kind as u16) * 4 + i as u16)?;
    }

    Ok(DecodedMeld {
        meld: Meld {
            kind: MeldKind::Shouminkan { tiles },
            from: None,
        },
        from_relative: from_relative(m),
    })
}

/// kan: 大明杠 (from > 0) 或 暗杠 (from = 0).
/// 编码: bits >> 8 = 完整 tile id (含 4 张中哪张被偷的偏移).
fn decode_kan(m: u16) -> Result<DecodedMeld, String> {
    let kind = ((m >> 8) / 4) as u8;
    let mut tiles = [Tile {
        id: 0,
        kind: majo_core::engine::domain::tile::TileIndex(0),
        red: false,
    }; 4];
    for (i, slot) in tiles.iter_mut().enumerate() {
        *slot = tenhou_id_to_tile((kind as u16) * 4 + i as u16)?;
    }

    let from_rel = from_relative(m);
    let kind_meld = if from_rel == 0 {
        MeldKind::Ankan { tiles }
    } else {
        MeldKind::Minkan { tiles }
    };

    Ok(DecodedMeld {
        meld: Meld {
            kind: kind_meld,
            from: None,
        },
        from_relative: from_rel,
    })
}

/// 把相对方位转为绝对 Seat (who 视角).
///
/// - 0 = self
/// - 1 = 下家 (who.next())
/// - 2 = 对家
/// - 3 = 上家 (who.next().next().next() = who.prev())
pub fn relative_to_seat(who: Seat, rel: u8) -> Seat {
    match rel {
        0 => who,
        1 => who.next(),
        2 => who.next().next(),
        3 => who.next().next().next(),
        _ => who,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use majo_core::engine::domain::tile::TileIndex;

    /// real-world m 例子 (从 fetched mjlog 中抽出): 14411 (鸣牌方=3, who=3 这条不在我们手册上,
    /// 但可以验证解码不崩溃).
    #[test]
    fn decode_real_world_m_does_not_panic() {
        // 抓取的 fixture 中 m 值: 44042, 11273, 43083, 47658, 29707
        for m in [14411u16, 44042, 11273, 43083, 47658, 29707, 19979] {
            let result = DecodedMeld::decode(m, Seat::East);
            assert!(result.is_ok(), "m={m} 解码应不报错: {result:?}");
        }
    }

    /// 暗杠: from=0, 例如 4 张 1m → m = 0b....
    /// 简化: 我们手工构造 m. 暗杠编码: bits 2-5 都为 0, from=0, bits[8..16] = tile id.
    /// 4 张 1m 之一, 任意 id 0-3 都行 (我们用 id=0).
    /// m = 0b0000_0000_0000_0000 | (0 << 8) = 0
    /// 但 m=0 也是 from=0, 也无法区分. 实际暗杠 m 至少 (1 << 8) = 256?
    /// 看 mjx: 暗杠 编码 type * 16 + ...
    #[test]
    fn decode_ankan_zero_from_returns_ankan() {
        // 用 type = 31 (白), tile id = 31*4 = 124
        // bits[8..16] = 124, 低 8 位 = 0 (from=0, no chi/pon/kakan flag)
        let m = (124u16) << 8;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        assert_eq!(d.from_relative, 0);
        assert!(matches!(d.meld.kind, MeldKind::Ankan { .. }));
        if let MeldKind::Ankan { tiles } = d.meld.kind {
            assert_eq!(tiles[0].kind.0, 31);
        }
    }

    #[test]
    fn decode_minkan_with_from_returns_minkan() {
        // type = 30 (北), tile id base = 120, 偷 from = 1 (下家)
        let m = (120u16) << 8 | 1;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        assert_eq!(d.from_relative, 1);
        assert!(matches!(d.meld.kind, MeldKind::Minkan { .. }));
    }

    #[test]
    fn decode_pon_basic() {
        // pon: type=15 (7p), stolen=0, unused_offset=3, from=2 (对家)
        // bits[9..] = type*3 + stolen = 15*3+0 = 45 → bits >> 9 = 45 → m bits 9-15 = 45 (= 0b0101101)
        // m = (45 << 9) | (3 << 5) | 0b0000001000 (pon flag) | 2 (from)
        let m = (45u16 << 9) | (3u16 << 5) | MASK_IS_PON | 2;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        assert_eq!(d.from_relative, 2);
        assert!(matches!(d.meld.kind, MeldKind::Pon { .. }));
        if let MeldKind::Pon { tiles } = d.meld.kind {
            assert_eq!(tiles[0].kind.0, 15); // 7p
            assert_eq!(tiles[1].kind.0, 15);
            assert_eq!(tiles[2].kind.0, 15);
        }
    }

    #[test]
    fn decode_kakan_returns_shouminkan() {
        // kakan: type=10 (2p), 仅 kakan flag (mjx 实际编码: kakan 不与 pon 同时 set)
        let m = ((10u16 * 3) << 9) | MASK_IS_KAN_ADDED | 1;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        assert!(matches!(d.meld.kind, MeldKind::Shouminkan { .. }));
        if let MeldKind::Shouminkan { tiles } = d.meld.kind {
            assert_eq!(tiles.len(), 4);
            assert_eq!(tiles[0].kind.0, 10);
        }
    }

    #[test]
    fn decode_chi_returns_chi() {
        // chi: 上家切 5m, 自家 4m+6m. 顺子起点 = 4m (kind 3), called = 5m (idx 1 in run)
        // base21 (= base 0..21, 0-6=m,7-13=p,14-20=s) = 4m's base = 3 (即 base21=3 means 4m start)
        // (m >> 10) = base21 * 3 + stolen = 3*3 + 1 = 10 → m bits 10..16 = 10
        // chi flag = 4
        // m = (10 << 10) | MASK_IS_CHI | 3  (from=3 上家)
        let m = (10u16 << 10) | MASK_IS_CHI | 3;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        assert_eq!(d.from_relative, 3);
        assert!(matches!(d.meld.kind, MeldKind::Chi { .. }));
        if let MeldKind::Chi { tiles } = d.meld.kind {
            assert_eq!(tiles[0].kind.0, 3); // 4m
            assert_eq!(tiles[1].kind.0, 4); // 5m
            assert_eq!(tiles[2].kind.0, 5); // 6m
        }
    }

    #[test]
    fn relative_to_seat_works() {
        assert_eq!(relative_to_seat(Seat::East, 0), Seat::East);
        assert_eq!(relative_to_seat(Seat::East, 1), Seat::South);
        assert_eq!(relative_to_seat(Seat::East, 2), Seat::West);
        assert_eq!(relative_to_seat(Seat::East, 3), Seat::North);
        assert_eq!(relative_to_seat(Seat::South, 1), Seat::West);
        // 上家 (rel=3) of South 是 East
        assert_eq!(relative_to_seat(Seat::South, 3), Seat::East);
    }

    /// 验证 chi base21 跨花色逻辑.
    #[test]
    fn chi_base21_pin() {
        // 1p 起点的吃: base21 = 7 (1p index in 0..21)
        // base21 / 7 = 1, % 7 = 0
        // min_kind = 1*9 + 0 = 9 = 1p ✓
        let base21 = 7u16;
        let kind = ((base21 / 7) * 9 + base21 % 7) as u8;
        assert_eq!(kind, 9); // 1p
    }

    #[test]
    fn chi_base21_sou() {
        // 5s 起点 (= sou 第 5 张 1-9 中第 5 = base21 14+4 = 18)
        let base21 = 18u16;
        let kind = ((base21 / 7) * 9 + base21 % 7) as u8;
        assert_eq!(kind, 22); // 5s = kind 22
    }

    #[test]
    fn aka_5m_decoded_in_pon() {
        // pon 5m, unused_offset=1 (跳过 id=17, 18, 19 中的 17): aka 5m id=16 是第 0 张 → 包含
        // type = 4 (5m), bits >> 9 = type * 3 + stolen = 12 + 0 = 12
        // unused_offset = 1
        let m = (12u16 << 9) | (1u16 << 5) | MASK_IS_PON;
        let d = DecodedMeld::decode(m, Seat::East).unwrap();
        if let MeldKind::Pon { tiles } = d.meld.kind {
            // unused_offset=1 → 跳过 id 17 (即 copy_idx 1), 包含 id 16, 18, 19
            let kinds: Vec<u8> = tiles.iter().map(|t| t.kind.0).collect();
            assert_eq!(kinds, vec![4, 4, 4]);
            // tiles[0] 应是 id=16 = aka 5m
            assert!(tiles[0].red, "第 0 张应是赤 5");
            assert!(!tiles[1].red);
            assert!(!tiles[2].red);
        }
    }

    /// TileIndex 引入只是为了避免 import warning.
    #[test]
    fn _tileindex_use() {
        let _t = TileIndex(0);
    }
}
