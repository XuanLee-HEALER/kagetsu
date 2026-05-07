//! 和牌型 (和了形 / Agari-kei) 分解.
//!
//! 日麻三种合法和牌结构:
//! - **标准型** (4 面子 + 1 雀头, 14 张): 最常见. 4 个 [`Mentsu`] + 1 对子.
//! - **七对子** (七対子 / Chiitoitsu): 7 组不同的对子.
//! - **国士无双** (国士無双 / Kokushi-musou): 13 种幺九牌各一 + 任一为雀头.
//!
//! [`decompose`] 函数枚举所有可能的拆解 (一手牌可能有多解, 例: 平和能拆成
//! 不同顺子组合), 调用方按 yaku 评估选最优.
//!
//! 算法详见 `docs/spec/winning-shapes.md`.

use crate::engine::domain::meld::Meld;
use crate::engine::domain::tile::TileIndex;

/// 面子 (面子 / Mentsu) — 标准型和牌的基本组成单位.
///
/// 4 个面子 + 1 雀头构成 14 张和牌. 鸣牌副露也算面子, 但本 enum 仅描述
/// 暗手分解出的面子 (副露见 [`Meld`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mentsu {
    /// 顺子 (順子 / Shuntsu) — 同花色连续 3 张. 参数 = 起始牌 (例: 3m4m5m → `Shuntsu(3m)`).
    Shuntsu(TileIndex),
    /// 刻子 (刻子 / Koutsu) — 同 kind 3 张.
    /// 参数: `(kind, concealed)`. 拆解阶段统一标 `concealed=true`, 调用方在
    /// 荣和 + 双碰待 时修正为明刻.
    Koutsu(TileIndex, bool),
    /// 杠子 (槓子 / Kantsu) — 同 kind 4 张.
    /// 参数: `(kind, concealed)`. 暗杠 = true, 明杠 / 加杠 = false.
    Kantsu(TileIndex, bool),
}

/// 听牌型 (待ち / Wait / Machi) — 和牌时填入 `winning_tile` 那张的等待结构.
///
/// 5 种待型, 影响符 (Fu) 计算 + 平和 (Pinfu) 役判定 (仅 Ryanmen 算平和).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitKind {
    /// 单骑 (単騎 / Tanki) — 等雀头. +2 符.
    Tanki,
    /// 嵌张 (嵌張 / Kanchan) — 顺子中间缺一张, 例: 3m_5m 等 4m. +2 符.
    Kanchan,
    /// 边张 (辺張 / Penchan) — 顺子边端缺一张, 例: 1m2m 等 3m, 8m9m 等 7m. +2 符.
    Penchan,
    /// 两面 (両面 / Ryanmen) — 顺子两端可和, 例: 4m5m 等 3m/6m. 0 符 (平和必要条件).
    Ryanmen,
    /// 双碰 (双碰 / Shanpon) — 两对中任一对升级成刻子, 例: 4m4m 5p5p 等 4m 或 5p.
    Shanpon,
}

/// 和牌拆解结果 — [`decompose`] 输出.
#[derive(Debug, Clone)]
pub enum Decomposition {
    /// 标准型: 4 面子 + 1 雀头. 含具体的待型 + 和牌张.
    Standard {
        /// 雀头 (頭 / Atama / 对子).
        pair: TileIndex,
        /// 4 个面子.
        mentsu: Vec<Mentsu>,
        /// 和牌张 kind (用于鉴定该张所在面子是否明刻).
        winning_tile: TileIndex,
        /// 听牌型.
        wait: WaitKind,
    },
    /// 七对子型. 7 组不同对子 + 待对子的最后 1 张 = 14 张.
    Chiitoitsu {
        /// 7 个雀头 (含和了对).
        pairs: [TileIndex; 7],
        /// 和牌张 (与某 pair 的另 1 张配对).
        winning_tile: TileIndex,
    },
    /// 国士无双型 (国士無双). 13 种幺九各 1 张 + 任 1 张作雀头.
    Kokushi {
        /// 和牌张.
        winning_tile: TileIndex,
        /// 是否 13 面待 (即手牌已是 13 种幺九各 1 张, 任和 1 张).
        /// `true` 时升级为双倍役满 (若 `rules.double_yakuman` 开).
        thirteen_wait: bool,
    },
}

const YAOCHUU_KINDS: [u8; 13] = [0, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33];

/// 返回所有可能的和牌拆解.
///
/// `closed`: 包含和牌张的暗手计数(34 维); 副露牌不在内.
/// `melds`: 已副露的鸣牌.
/// `winning_tile`: 此次和牌的牌种.
pub fn decompose(closed: &[u8; 34], melds: &[Meld], winning_tile: TileIndex) -> Vec<Decomposition> {
    let mut results = Vec::new();

    if closed[winning_tile.0 as usize] == 0 {
        return results;
    }

    // 七对子和国士仅在门清(不含副露,包括暗杠)时考虑.
    if melds.is_empty() {
        if let Some(d) = try_chiitoitsu(closed, winning_tile) {
            results.push(d);
        }
        if let Some(d) = try_kokushi(closed, winning_tile) {
            results.push(d);
        }
    }

    // 标准型: 把和牌张抽出,基于 13 张做拆解,把 winning 加在不同位置形成不同 wait.
    let total: u32 = closed.iter().map(|&c| c as u32).sum();
    let needed_mentsu = match 4usize.checked_sub(melds.len()) {
        Some(n) => n,
        None => return results,
    };
    let expected = (needed_mentsu * 3 + 2) as u32;
    if total != expected {
        return results;
    }

    let mut without = *closed;
    without[winning_tile.0 as usize] -= 1;

    // (a) winning 完成雀头(单骑).
    if without[winning_tile.0 as usize] >= 1 {
        let mut h = without;
        h[winning_tile.0 as usize] -= 1;
        let mut sols = Vec::new();
        let mut buf = Vec::new();
        enumerate_mentsu(&mut h, needed_mentsu, &mut buf, &mut sols);
        for mentsu in sols {
            results.push(Decomposition::Standard {
                pair: winning_tile,
                mentsu,
                winning_tile,
                wait: WaitKind::Tanki,
            });
        }
    }

    // (b) winning 完成某个面子. 枚举雀头.
    for pair_kind in 0..34u8 {
        if without[pair_kind as usize] < 2 {
            continue;
        }
        let mut h = without;
        h[pair_kind as usize] -= 2;

        // (b.1) winning 完成刻子(双碰): h[winning] >= 2.
        if h[winning_tile.0 as usize] >= 2 {
            h[winning_tile.0 as usize] -= 2;
            let mut buf = vec![Mentsu::Koutsu(winning_tile, true)];
            let mut sols = Vec::new();
            enumerate_mentsu(&mut h, needed_mentsu - 1, &mut buf, &mut sols);
            for mentsu in sols {
                results.push(Decomposition::Standard {
                    pair: TileIndex(pair_kind),
                    mentsu,
                    winning_tile,
                    wait: WaitKind::Shanpon,
                });
            }
            h[winning_tile.0 as usize] += 2;
        }

        // (b.2) winning 完成顺子. 仅数牌.
        if winning_tile.is_suupai() {
            let w = winning_tile.0 as usize;
            let suit_base = (w / 9) * 9;
            let r = w - suit_base; // 0..=8
            for offset in 0..=2 {
                if r < offset {
                    continue;
                }
                let start = w - offset;
                if start < suit_base || start + 2 >= suit_base + 9 {
                    continue;
                }
                let three = [start, start + 1, start + 2];
                let others: Vec<usize> = three.iter().filter(|&&x| x != w).copied().collect();
                if others.len() != 2 {
                    continue;
                }
                if h[others[0]] >= 1 && h[others[1]] >= 1 {
                    h[others[0]] -= 1;
                    h[others[1]] -= 1;
                    let mut buf = vec![Mentsu::Shuntsu(TileIndex(start as u8))];
                    let mut sols = Vec::new();
                    enumerate_mentsu(&mut h, needed_mentsu - 1, &mut buf, &mut sols);
                    let wait = shuntsu_wait(start, w);
                    for mentsu in sols {
                        results.push(Decomposition::Standard {
                            pair: TileIndex(pair_kind),
                            mentsu,
                            winning_tile,
                            wait,
                        });
                    }
                    h[others[0]] += 1;
                    h[others[1]] += 1;
                }
            }
        }
    }

    results
}

fn shuntsu_wait(start: usize, winning: usize) -> WaitKind {
    let start_rank = (start % 9) + 1;
    let winning_rank = (winning % 9) + 1;
    if winning_rank == start_rank + 1 {
        WaitKind::Kanchan
    } else if (start_rank == 1 && winning_rank == 3) || (start_rank == 7 && winning_rank == 7) {
        // 12_ 等 3 / _89 等 7 都是边张.
        WaitKind::Penchan
    } else {
        WaitKind::Ryanmen
    }
}

fn enumerate_mentsu(
    hand: &mut [u8; 34],
    need: usize,
    chosen: &mut Vec<Mentsu>,
    out: &mut Vec<Vec<Mentsu>>,
) {
    if need == 0 {
        if hand.iter().all(|&c| c == 0) {
            out.push(chosen.clone());
        }
        return;
    }
    let i = match hand.iter().position(|&c| c > 0) {
        Some(i) => i,
        None => return,
    };

    // 顺子优先.
    if i < 27 && i % 9 <= 6 && hand[i + 1] >= 1 && hand[i + 2] >= 1 {
        hand[i] -= 1;
        hand[i + 1] -= 1;
        hand[i + 2] -= 1;
        chosen.push(Mentsu::Shuntsu(TileIndex(i as u8)));
        enumerate_mentsu(hand, need - 1, chosen, out);
        chosen.pop();
        hand[i] += 1;
        hand[i + 1] += 1;
        hand[i + 2] += 1;
    }

    if hand[i] >= 3 {
        hand[i] -= 3;
        chosen.push(Mentsu::Koutsu(TileIndex(i as u8), true));
        enumerate_mentsu(hand, need - 1, chosen, out);
        chosen.pop();
        hand[i] += 3;
    }
}

fn try_chiitoitsu(closed: &[u8; 34], winning: TileIndex) -> Option<Decomposition> {
    let total: u32 = closed.iter().map(|&c| c as u32).sum();
    if total != 14 {
        return None;
    }
    let mut pairs = Vec::with_capacity(7);
    for (k, &cnt) in closed.iter().enumerate() {
        match cnt {
            0 => continue,
            2 => pairs.push(TileIndex(k as u8)),
            _ => return None,
        }
    }
    if pairs.len() != 7 || !pairs.contains(&winning) {
        return None;
    }
    let mut arr = [TileIndex(0); 7];
    for (i, &t) in pairs.iter().enumerate() {
        arr[i] = t;
    }
    Some(Decomposition::Chiitoitsu {
        pairs: arr,
        winning_tile: winning,
    })
}

fn try_kokushi(closed: &[u8; 34], winning: TileIndex) -> Option<Decomposition> {
    let total: u32 = closed.iter().map(|&c| c as u32).sum();
    if total != 14 {
        return None;
    }
    if !YAOCHUU_KINDS.contains(&winning.0) {
        return None;
    }
    let mut covered = 0;
    let mut pair_kind = None;
    for k in 0..34u8 {
        let cnt = closed[k as usize];
        let is_yao = YAOCHUU_KINDS.contains(&k);
        if !is_yao {
            if cnt > 0 {
                return None;
            }
            continue;
        }
        match cnt {
            0 => return None, // 13 种必须各 ≥ 1
            1 => covered += 1,
            2 => {
                if pair_kind.is_some() {
                    return None;
                }
                pair_kind = Some(TileIndex(k));
                covered += 1;
            }
            _ => return None,
        }
    }
    if covered != 13 {
        return None;
    }
    let pair = pair_kind?;
    let thirteen_wait = winning == pair;
    Some(Decomposition::Kokushi {
        winning_tile: winning,
        thirteen_wait,
    })
}

pub fn can_win(closed: &[u8; 34], melds: &[Meld], winning_tile: TileIndex) -> bool {
    !decompose(closed, melds, winning_tile).is_empty()
}

/// 听牌检测: 返回所有能令手牌和牌的牌种.
pub fn tenpai_tiles(closed: &[u8; 34], melds: &[Meld]) -> Vec<TileIndex> {
    let mut waits = Vec::new();
    for k in 0..34u8 {
        if closed[k as usize] >= 4 {
            continue;
        }
        let mut h = *closed;
        h[k as usize] += 1;
        if can_win(&h, melds, TileIndex(k)) {
            waits.push(TileIndex(k));
        }
    }
    waits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::tile::TileIndex;

    fn h(spec: &[(u8, u8)]) -> [u8; 34] {
        let mut a = [0u8; 34];
        for &(k, c) in spec {
            a[k as usize] = c;
        }
        a
    }

    #[test]
    fn standard_basic_pinfu_shape() {
        // 1m2m3m 4m5m6m 7m8m9m 1p1p (11 张) + 等 2p3p... 让我们造完整 14 张:
        // 123m 234m 345m 456m 11p (实际 = 1m+2*2m+2*3m+2*4m+5m+6m + 11p = 错; 重新)
        // 简单造一个: 234p 234p 234p 234p 11s (4 个 234p 顺子 + 1 雀头)
        let hand = h(&[(10, 4), (11, 4), (12, 4), (18, 2)]); // 2p×4, 3p×4, 4p×4, 1s×2
        // 对应顺子 234p×4 + 11s 雀头, winning=4p (即第 4 个 4p)
        let r = decompose(&hand, &[], TileIndex(12));
        assert!(!r.is_empty(), "234p×4 + 11s 应能拆成和牌型");
    }

    #[test]
    fn chiitoitsu_basic() {
        // 7 组对子: 1m1m 3m3m 5m5m 7m7m 1p1p 中中 西西
        let hand = h(&[(0, 2), (2, 2), (4, 2), (6, 2), (9, 2), (33, 2), (29, 2)]);
        let r = decompose(&hand, &[], TileIndex(0));
        assert!(
            r.iter()
                .any(|d| matches!(d, Decomposition::Chiitoitsu { .. }))
        );
    }

    #[test]
    fn kokushi_basic() {
        // 13 种幺九各 1 + 1m 雀头
        let mut hand = [0u8; 34];
        for &k in &YAOCHUU_KINDS {
            hand[k as usize] = 1;
        }
        hand[0] = 2; // 1m 雀头
        let r = decompose(&hand, &[], TileIndex(0));
        assert!(r.iter().any(|d| matches!(d, Decomposition::Kokushi { .. })));
    }

    #[test]
    fn kokushi_thirteen_wait() {
        // 13 种各 1 + 等任何幺九
        let mut hand = [0u8; 34];
        for &k in &YAOCHUU_KINDS {
            hand[k as usize] = 1;
        }
        hand[8] += 1; // 假设和的是 9m
        let r = decompose(&hand, &[], TileIndex(8));
        let kokushi = r
            .iter()
            .find_map(|d| match d {
                Decomposition::Kokushi { thirteen_wait, .. } => Some(*thirteen_wait),
                _ => None,
            })
            .expect("应有国士拆解");
        assert!(kokushi);
    }

    #[test]
    fn ryanmen_wait() {
        // 234m 234p 234s 78m + 33z 等 6m or 9m. 测试和 9m → ryanmen.
        let mut hand = [0u8; 34];
        for &k in &[1u8, 2, 3, 10, 11, 12, 19, 20, 21] {
            hand[k as usize] += 1;
        }
        hand[6] += 1; // 7m
        hand[7] += 1; // 8m
        hand[31] += 2; // 白 雀头
        hand[8] += 1; // winning 9m
        let r = decompose(&hand, &[], TileIndex(8));
        let waits: Vec<_> = r
            .iter()
            .filter_map(|d| match d {
                Decomposition::Standard { wait, .. } => Some(*wait),
                _ => None,
            })
            .collect();
        assert!(
            waits.contains(&WaitKind::Ryanmen),
            "应识别为 ryanmen, got {:?}",
            waits
        );
    }

    #[test]
    fn penchan_wait() {
        // 12m + 234p + 234s + 444m 刻 + 白白 + winning 3m → 12m 等 3m = penchan
        let mut hand = [0u8; 34];
        hand[0] = 1; // 1m
        hand[1] = 1; // 2m
        hand[3] = 3; // 4m 刻
        hand[10] = 1; // 2p
        hand[11] = 1; // 3p
        hand[12] = 1; // 4p
        hand[19] = 1; // 2s
        hand[20] = 1; // 3s
        hand[21] = 1; // 4s
        hand[31] = 2; // 白×2
        hand[2] = 1; // winning 3m
        let r = decompose(&hand, &[], TileIndex(2));
        let waits: Vec<_> = r
            .iter()
            .filter_map(|d| match d {
                Decomposition::Standard { wait, .. } => Some(*wait),
                _ => None,
            })
            .collect();
        assert!(
            waits.contains(&WaitKind::Penchan),
            "应识别为 penchan, got {:?}",
            waits
        );
    }

    #[test]
    fn shanpon_wait() {
        // 444m 555p 666s + 9m9m + 1p1p, winning=9m → 9m 凑成刻 = shanpon
        let mut hand = [0u8; 34];
        hand[3] = 3; // 4m 刻
        hand[13] = 3; // 5p 刻
        hand[23] = 3; // 6s 刻
        hand[8] = 3; // 9m×3 (含 winning)
        hand[9] = 2; // 1p×2
        let r = decompose(&hand, &[], TileIndex(8));
        let waits: Vec<_> = r
            .iter()
            .filter_map(|d| match d {
                Decomposition::Standard { wait, .. } => Some(*wait),
                _ => None,
            })
            .collect();
        assert!(
            waits.contains(&WaitKind::Shanpon),
            "应识别为 shanpon, got {:?}",
            waits
        );
    }

    #[test]
    fn tanki_wait() {
        // 4 顺子 + 单骑等雀头: 123m 456m 789m 123p + 5p5p (winning=5p 完成雀头)
        let mut hand = [0u8; 34];
        for &k in &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
            hand[k as usize] += 1;
        }
        hand[13] = 2; // 5p×2 (含 winning)
        let r = decompose(&hand, &[], TileIndex(13));
        let waits: Vec<_> = r
            .iter()
            .filter_map(|d| match d {
                Decomposition::Standard { wait, .. } => Some(*wait),
                _ => None,
            })
            .collect();
        assert!(
            waits.contains(&WaitKind::Tanki),
            "应识别为 tanki, got {:?}",
            waits
        );
    }

    #[test]
    fn no_win_returns_empty() {
        // 杂乱 14 张
        let mut hand = [0u8; 34];
        for k in 0..14u8 {
            hand[k as usize] += 1;
        }
        let r = decompose(&hand, &[], TileIndex(0));
        assert!(r.is_empty(), "杂乱手牌不应有拆解");
    }

    #[test]
    fn tenpai_detection() {
        // 13 张听牌: 234m 234p 234s 234m 1m → 听 1m 单骑(实际等 1m 凑 11m 雀头)
        // 简化: 13 张 = 4 顺子 + 1 张, 听该种再来一张作雀头.
        let mut hand = [0u8; 34];
        for &k in &[0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
            hand[k as usize] += 1;
        }
        hand[13] = 1; // 5p
        let waits = tenpai_tiles(&hand, &[]);
        assert!(waits.contains(&TileIndex(13)), "应听 5p, got {:?}", waits);
    }

    // ===== 副露 + 听牌测试 (tenpai_tiles / can_win 在 melds 非空时的契约) =====
    //
    // 听牌型不变量: closed.len() + melds.len() * 3 == 13. 杠虽 4 张但听牌型容量
    // 仍占 1 面子 (杠后岭上摸不增 closed 容量). 下面 helper 构造副露时, 用占位
    // tile id (实际算法不读 id, 只读 kind), 仅保证 Meld 结构合法.

    use crate::engine::domain::meld::{Meld, MeldKind, Seat};
    use crate::engine::domain::tile::Tile;

    fn tile(kind: u8, id: u16) -> Tile {
        Tile {
            kind: TileIndex(kind),
            red: false,
            id,
        }
    }

    /// 构造吃副露 (3 张连续同花色, start = 起始 kind).
    fn chi(start: u8, base_id: u16) -> Meld {
        Meld {
            kind: MeldKind::Chi {
                tiles: [
                    tile(start, base_id),
                    tile(start + 1, base_id + 1),
                    tile(start + 2, base_id + 2),
                ],
            },
            from: Some(Seat::East),
        }
    }

    /// 构造碰副露 (3 张同 kind).
    fn pon(kind: u8, base_id: u16) -> Meld {
        Meld {
            kind: MeldKind::Pon {
                tiles: [
                    tile(kind, base_id),
                    tile(kind, base_id + 1),
                    tile(kind, base_id + 2),
                ],
            },
            from: Some(Seat::South),
        }
    }

    /// 构造明杠 (4 张同 kind).
    fn minkan(kind: u8, base_id: u16) -> Meld {
        Meld {
            kind: MeldKind::Minkan {
                tiles: [
                    tile(kind, base_id),
                    tile(kind, base_id + 1),
                    tile(kind, base_id + 2),
                    tile(kind, base_id + 3),
                ],
            },
            from: Some(Seat::West),
        }
    }

    /// 构造暗杠 (4 张同 kind, from = None).
    fn ankan(kind: u8, base_id: u16) -> Meld {
        Meld {
            kind: MeldKind::Ankan {
                tiles: [
                    tile(kind, base_id),
                    tile(kind, base_id + 1),
                    tile(kind, base_id + 2),
                    tile(kind, base_id + 3),
                ],
            },
            from: None,
        }
    }

    #[test]
    fn tenpai_with_chi_meld_ryanmen() {
        // 副露: 吃 123m + 闭手 10 张 = 78m + 234p + 678p + 11s 雀头, 听 6m(5)/9m(8) 双面.
        let closed = h(&[
            (6, 1),
            (7, 1), // 78m
            (10, 1),
            (11, 1),
            (12, 1), // 234p
            (14, 1),
            (15, 1),
            (16, 1), // 678p
            (18, 2), // 11s 雀头
        ]);
        let melds = vec![chi(0, 100)];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(5)) && waits.contains(&TileIndex(8)),
            "1 吃副露 + 78m 双面应听 6m(5)/9m(8), got {:?}",
            waits
        );
    }

    #[test]
    fn tenpai_with_pon_meld_tanki() {
        // 副露 1 + 闭手 3 面子 (9 张) + 单骑 (1 张) = 10 张. 听单 = 雀头.
        // 副露: 碰 5p. 闭手: 123m + 456m + 789m + 单 5s.
        let closed = h(&[
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (5, 1),
            (6, 1),
            (7, 1),
            (8, 1),
            (22, 1),
        ]);
        let melds = vec![pon(13, 200)];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(22)),
            "1 碰副露 + 单 5s 应听 5s, got {:?}",
            waits
        );
    }

    #[test]
    fn tenpai_with_minkan_shanpon() {
        // 副露 1 + 闭手 2 面子 + 2 对 = 10 张. 对碰: 听其中一对升刻.
        // 副露 明杠 9s. 闭手 123m + 456p + 11s + 22s, 听 1s(18)/2s(19).
        let closed = h(&[
            (0, 1),
            (1, 1),
            (2, 1),
            (12, 1),
            (13, 1),
            (14, 1),
            (18, 2),
            (19, 2),
        ]);
        let melds = vec![minkan(26, 300)];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(18)) && waits.contains(&TileIndex(19)),
            "1 明杠 + 11s+22s 对碰应听 1s/2s, got {:?}",
            waits
        );
    }

    #[test]
    fn tenpai_with_ankan_ryanmen() {
        // 副露 1 (暗杠) + 闭手 2 顺子 + 雀头 + 1 双面搭子 = 10 张.
        // 副露 暗杠 1m. 闭手 234p + 567p + 78s + 99m, 听 6s(23)/9s(26).
        let closed = h(&[
            (10, 1),
            (11, 1),
            (12, 1),
            (13, 1),
            (14, 1),
            (15, 1),
            (24, 1),
            (25, 1),
            (8, 2),
        ]);
        let melds = vec![ankan(0, 400)];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(23)) && waits.contains(&TileIndex(26)),
            "1 暗杠 + 78s 双面应听 6s(23)/9s(26), got {:?}",
            waits
        );
    }

    #[test]
    fn tenpai_with_three_melds_tanki() {
        // 3 副露 → 闭手 13-9=4 张 = 1 面子 + 单骑.
        // 副露: 碰 333p + 碰 555s + 暗杠 7777s. 闭手 4 张: 234m + 单 5m, 听 5m 单骑.
        let closed = h(&[
            (1, 1),
            (2, 1),
            (3, 1), // 234m
            (4, 1), // 单 5m
        ]);
        let melds = vec![
            pon(11, 500),   // 碰 3p (kind=11)
            pon(22, 510),   // 碰 5s (kind=22)
            ankan(24, 520), // 暗杠 7s (kind=24)
        ];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(4)),
            "3 副露 + 234m+5m 应听 5m, got {:?}",
            waits
        );
    }

    #[test]
    fn tenpai_with_four_melds_tanki() {
        // 4 副露 → 闭手 13-12=1 张 = 单骑等雀头. 极端情况.
        // 4 副露随便选: 吃123m + 碰 555p + 明杠 9999s + 暗杠 西西西西 (kind=29). 闭手 = 单 1p, 听 1p 单骑.
        let closed = h(&[(9, 1)]); // 单 1p
        let melds = vec![
            chi(0, 600),     // 吃 123m
            pon(13, 610),    // 碰 5p
            minkan(26, 620), // 明杠 9s
            ankan(29, 630),  // 暗杠 西
        ];
        let waits = tenpai_tiles(&closed, &melds);
        assert!(
            waits.contains(&TileIndex(9)),
            "4 副露 + 单 1p 应听 1p, got {:?}",
            waits
        );
        assert_eq!(waits.len(), 1, "4 副露单骑只听这 1 张, got {:?}", waits);
    }

    #[test]
    fn can_win_with_meld_completes_hand() {
        // can_win 接受闭手数组 (含 winning). 14 张型 = 闭手 11 张 + 副露 1 (3 张).
        // 副露 吃 123m. 闭手 11 张: 234p + 567p + 234s + 99m, winning=9m.
        let closed = h(&[
            (10, 1),
            (11, 1),
            (12, 1), // 234p
            (13, 1),
            (14, 1),
            (15, 1), // 567p
            (19, 1),
            (20, 1),
            (21, 1), // 234s
            (8, 2),  // 99m 雀头 (含 winning)
        ]);
        let melds = vec![chi(0, 700)];
        assert!(
            can_win(&closed, &melds, TileIndex(8)),
            "1 吃 + 11 张闭手 (含 99m 雀头) 应能和 (winning=9m)"
        );
    }
}
