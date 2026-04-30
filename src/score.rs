//! 番符计算与点数分配.
//!
//! 详见 docs/spec/scoring.md

use crate::decompose::{Decomposition, Mentsu, WaitKind};
use crate::meld::{Meld, MeldKind, Seat};
use crate::yaku::{WinContext, Yaku, detect_yaku};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreLevel {
    Normal,
    Mangan,
    Haneman,
    Baiman,
    Sanbaiman,
    KazoeYakuman,
    /// n 倍役满.
    Yakuman(u8),
}

#[derive(Debug, Clone)]
pub struct ScoreResult {
    pub han: u32,
    pub fu: u32,
    pub yaku: Vec<(Yaku, u32)>,
    pub base_points: u32,
    pub level: ScoreLevel,
}

#[derive(Debug, Clone)]
pub struct PaymentDistribution {
    pub from: Seat,
    pub to: Seat,
    pub amount: i32,
}

/// 符计算. 输入: 已确定的拆解 + 上下文 + 副露列表.
pub fn calculate_fu(d: &Decomposition, ctx: &WinContext, melds: &[Meld]) -> u32 {
    match d {
        Decomposition::Chiitoitsu { .. } => 25,
        Decomposition::Kokushi { .. } => 30, // 国士役满, 符没意义但给个值
        Decomposition::Standard { mentsu, pair, wait, winning_tile } => {
            // 检测 pinfu (会绑定特殊符规则)
            let is_pinfu = ctx.menzen
                && mentsu.iter().all(|m| matches!(m, Mentsu::Shuntsu(_)))
                && *wait == WaitKind::Ryanmen
                && !pair.is_dragon()
                && *pair != ctx.round_wind
                && *pair != ctx.seat_wind;

            if is_pinfu && ctx.is_tsumo {
                return 20;
            }
            if is_pinfu && !ctx.is_tsumo {
                return 30;
            }

            let mut fu: u32 = 20;

            // 自摸 +2 (除平和自摸).
            if ctx.is_tsumo {
                fu += 2;
            }

            // 门前清荣和 +10.
            if !ctx.is_tsumo && ctx.menzen {
                fu += 10;
            }

            // 雀头.
            if pair.is_dragon() {
                fu += 2;
            }
            if *pair == ctx.round_wind && *pair == ctx.seat_wind {
                fu += 4; // 连风
            } else if *pair == ctx.round_wind || *pair == ctx.seat_wind {
                fu += 2;
            }

            // 待牌符.
            match wait {
                WaitKind::Tanki | WaitKind::Kanchan | WaitKind::Penchan => fu += 2,
                _ => {}
            }

            // 面子符 (暗手 mentsu).
            for m in mentsu {
                fu += mentsu_fu(m, ctx, *wait, *winning_tile);
            }

            // 副露面子符.
            for meld in melds {
                fu += meld_fu(meld);
            }

            // 向上取整到 10. 但若没有任何加成(只有副露+无符), 至少给 30 (副露荣和无符的最小).
            
            // 完全开门(全部副露,雀头无符,基础 20+无加成) 时按 30 兜底
            // 实际 fu 起步至少 20 + 自摸 2 / 门清 10 = 22 / 30, 取整后通常 30+.
            // 此处遵守"向上取整 10",不再额外兜底.
            fu.div_ceil(10) * 10
        }
    }
}

fn mentsu_fu(m: &Mentsu, ctx: &WinContext, wait: WaitKind, winning: crate::tile::TileIndex) -> u32 {
    match m {
        Mentsu::Shuntsu(_) => 0,
        Mentsu::Koutsu(t, true) => {
            // 拆解阶段都标 concealed=true. 荣和+双碰时和牌张所在刻子按明刻算.
            let is_open = !ctx.is_tsumo && wait == WaitKind::Shanpon && *t == winning;
            let yaochuu = t.is_yaochuu();
            match (is_open, yaochuu) {
                (false, true) => 8,
                (false, false) => 4,
                (true, true) => 4,
                (true, false) => 2,
            }
        }
        Mentsu::Koutsu(t, false) => {
            let yaochuu = t.is_yaochuu();
            if yaochuu { 4 } else { 2 }
        }
        Mentsu::Kantsu(t, true) => {
            // 暗杠
            let yaochuu = t.is_yaochuu();
            if yaochuu { 32 } else { 16 }
        }
        Mentsu::Kantsu(t, false) => {
            // 明杠
            let yaochuu = t.is_yaochuu();
            if yaochuu { 16 } else { 8 }
        }
    }
}

fn meld_fu(m: &Meld) -> u32 {
    match &m.kind {
        MeldKind::Chi { .. } => 0,
        MeldKind::Pon { tiles } => {
            let yao = tiles[0].kind.is_yaochuu();
            if yao { 4 } else { 2 }
        }
        MeldKind::Minkan { tiles } | MeldKind::Shouminkan { tiles } => {
            let yao = tiles[0].kind.is_yaochuu();
            if yao { 16 } else { 8 }
        }
        MeldKind::Ankan { tiles } => {
            let yao = tiles[0].kind.is_yaochuu();
            if yao { 32 } else { 16 }
        }
    }
}

/// 基本点 = fu × 2^(han+2),上限 2000;含满贯及以上的封顶.
pub fn base_points(han: u32, fu: u32) -> u32 {
    if han >= 13 {
        return 8000;
    }
    if han >= 11 {
        return 6000;
    }
    if han >= 8 {
        return 4000;
    }
    if han >= 6 {
        return 3000;
    }
    if han == 5 || (han == 4 && fu >= 40) || (han == 3 && fu >= 70) {
        return 2000;
    }
    let raw = fu * (1u32 << (han + 2));
    raw.min(2000)
}

pub fn ceil_to_100(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    ((n + 99) / 100) * 100
}

/// 综合判定: 调用 detect_yaku, 算 fu, 算 base_points 和 level.
/// 若无役 (除 dora 外没有真役) 返回 None.
pub fn evaluate(ctx: &WinContext, melds: &[Meld]) -> Option<ScoreResult> {
    let yaku = detect_yaku(ctx);
    if yaku.is_empty() {
        return None;
    }
    // 必须至少有一个非 dora 役.
    let has_real_yaku = yaku.iter().any(|(y, _)| {
        !matches!(
            y,
            Yaku::Dora(_) | Yaku::AkaDora(_) | Yaku::UraDora(_)
        )
    });
    if !has_real_yaku {
        return None;
    }

    // 役满判定: 是否包含役满役.
    let yakuman_count: u8 = yaku
        .iter()
        .filter(|(y, _)| y.is_yakuman())
        .map(|(_, han)| (han / 13) as u8)
        .sum();

    let han: u32 = yaku.iter().map(|(_, h)| *h).sum();
    let fu = calculate_fu(ctx.decomposition, ctx, melds);

    let (base, level) = if yakuman_count > 0 {
        (8000 * yakuman_count as u32, ScoreLevel::Yakuman(yakuman_count))
    } else if han >= 13 && ctx.config.kazoe_yakuman {
        (8000, ScoreLevel::KazoeYakuman)
    } else if han >= 11 {
        (6000, ScoreLevel::Sanbaiman)
    } else if han >= 8 {
        (4000, ScoreLevel::Baiman)
    } else if han >= 6 {
        (3000, ScoreLevel::Haneman)
    } else if han >= 5 || (han == 4 && fu >= 40) || (han == 3 && fu >= 70) {
        (2000, ScoreLevel::Mangan)
    } else {
        (base_points(han, fu), ScoreLevel::Normal)
    };

    Some(ScoreResult {
        han,
        fu,
        yaku,
        base_points: base,
        level,
    })
}

/// 根据结果与场况(亲/自摸/放铳家/本场/立直棒)生成支付列表.
pub fn distribute(
    result: &ScoreResult,
    winner: Seat,
    dealer: Seat,
    is_tsumo: bool,
    ronned: Option<Seat>,
    honba: u32,
    riichi_sticks: u32,
) -> Vec<PaymentDistribution> {
    let mut out = Vec::new();
    let is_dealer_win = winner == dealer;
    let base = result.base_points as i32;

    let honba_per = honba as i32 * 100;
    let honba_total = honba as i32 * 300;

    if is_tsumo {
        if is_dealer_win {
            // 亲家自摸: 每家各 2B,向上取整到 100.
            let per = ceil_to_100(2 * base) + honba_per;
            for s in Seat::ALL {
                if s != winner {
                    out.push(PaymentDistribution {
                        from: s,
                        to: winner,
                        amount: per,
                    });
                }
            }
        } else {
            // 子家自摸: 亲付 2B, 子付 B.
            for s in Seat::ALL {
                if s == winner {
                    continue;
                }
                let amount = if s == dealer {
                    ceil_to_100(2 * base) + honba_per
                } else {
                    ceil_to_100(base) + honba_per
                };
                out.push(PaymentDistribution {
                    from: s,
                    to: winner,
                    amount,
                });
            }
        }
    } else if let Some(loser) = ronned {
        let mult = if is_dealer_win { 6 } else { 4 };
        let amount = ceil_to_100(mult * base) + honba_total;
        out.push(PaymentDistribution {
            from: loser,
            to: winner,
            amount,
        });
    }

    // 立直棒一并给和牌方.
    if riichi_sticks > 0 {
        out.push(PaymentDistribution {
            from: winner,
            to: winner,
            amount: riichi_sticks as i32 * 1000,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_points_thresholds() {
        assert_eq!(base_points(5, 30), 2000);
        assert_eq!(base_points(6, 30), 3000);
        assert_eq!(base_points(8, 30), 4000);
        assert_eq!(base_points(11, 30), 6000);
        assert_eq!(base_points(13, 30), 8000);
        assert_eq!(base_points(4, 40), 2000);
        assert_eq!(base_points(3, 70), 2000);
    }

    #[test]
    fn ceil_round() {
        assert_eq!(ceil_to_100(0), 0);
        assert_eq!(ceil_to_100(1), 100);
        assert_eq!(ceil_to_100(101), 200);
        assert_eq!(ceil_to_100(500), 500);
    }

    #[test]
    fn ron_distribution_dealer() {
        let result = ScoreResult {
            han: 5,
            fu: 30,
            yaku: vec![],
            base_points: 2000,
            level: ScoreLevel::Mangan,
        };
        let d = distribute(&result, Seat::East, Seat::East, false, Some(Seat::South), 0, 0);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].amount, 12000); // 亲家荣和 mangan = 12000
    }

    #[test]
    fn tsumo_distribution_non_dealer() {
        let result = ScoreResult {
            han: 5,
            fu: 30,
            yaku: vec![],
            base_points: 2000,
            level: ScoreLevel::Mangan,
        };
        let d = distribute(&result, Seat::South, Seat::East, true, None, 0, 0);
        // 子家自摸: 亲 4000, 子 2000 + 2000 = 8000 (mangan).
        let total: i32 = d.iter().map(|p| p.amount).sum();
        assert_eq!(total, 8000);
        let from_dealer: i32 = d
            .iter()
            .filter(|p| p.from == Seat::East)
            .map(|p| p.amount)
            .sum();
        assert_eq!(from_dealer, 4000);
    }
}
