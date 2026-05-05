//! 番符计算 (役判定 + 番符 + 等级 + 支付分配) 与终局排名.
//!
//! # 日麻计分流程
//!
//! 1. 牌型分解 ([`crate::engine::domain::decompose::decompose`]) → 4 面子 + 雀头
//! 2. 役判定 ([`detect_yaku`]) → `Vec<(Yaku, han)>`
//! 3. 符 (符 / Fu) 计算 ([`calculate_fu`]) → 雀头 / 待牌 / 面子等加分
//! 4. 番符 → 基本点 (Basic Points / 基本点)
//! 5. 基本点 + 庄家 / 自摸 / 本场 → 各家支付 ([`distribute`])
//!
//! # 番 (Han) 与 符 (Fu)
//!
//! - **番** (翻 / 飜 / Han): 役本身的等级, 越多番点数越高
//! - **符** (符 / Fu): 牌型细节加分 (雀头 / 待牌 / 面子结构), 单位 10
//!
//! 基本点 = `fu × 2^(han + 2)`, 上限 2000. 满贯及以上按 [`ScoreLevel`] 封顶.

use crate::engine::domain::decompose::{Decomposition, Mentsu, WaitKind};
use crate::engine::domain::meld::{Meld, MeldKind, Seat};
use crate::engine::domain::yaku::{WinContext, Yaku, detect_yaku};
use crate::engine::player::PlayerState;
use crate::engine::rules::GameRules;

/// 和了等级 (得点ランク / 得点等级).
///
/// 当番符达到一定阈值时, 基本点封顶为固定值, 不再按 `fu × 2^(han+2)` 算.
///
/// | Level | 番数阈值 | 基本点 | 子家荣和 (非庄) |
/// |-------|----------|-------|----------------|
/// | Normal | 1-4 番 | fu×2^(han+2), ≤2000 | 实际计算 |
/// | Mangan (満貫) | 5 番 / 4番40符 / 3番70符 | 2000 | 8000 |
/// | Haneman (跳満) | 6-7 番 | 3000 | 12000 |
/// | Baiman (倍満) | 8-10 番 | 4000 | 16000 |
/// | Sanbaiman (三倍満) | 11-12 番 | 6000 | 24000 |
/// | KazoeYakuman (数え役満) | 13+ 番 (累计) | 8000 | 32000 |
/// | Yakuman(n) (役満) | 役满 n 倍 | 8000 × n | 32000 × n |
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScoreLevel {
    /// 普通和了 (低于满贯).
    Normal,
    /// 满贯 (満貫 / Mangan). 5 番 / 4 番 40 符 / 3 番 70 符.
    Mangan,
    /// 跳满 (跳満 / Haneman). 6-7 番.
    Haneman,
    /// 倍满 (倍満 / Baiman). 8-10 番.
    Baiman,
    /// 三倍满 (三倍満 / Sanbaiman). 11-12 番.
    Sanbaiman,
    /// 累计役满 (数え役満 / Kazoe Yakuman). 13+ 番累计.
    /// 仅 `rules.kazoe_yakuman = true` 时启用.
    KazoeYakuman,
    /// 役满 (役満 / Yakuman). 参数 = 倍数 (1 = 单倍, 2 = 双倍).
    /// 双倍役满见 [`crate::engine::rules::GameRules::double_yakuman`].
    Yakuman(u8),
}

/// 评分完整结果 — [`evaluate`] 返回值, 写入 [`crate::engine::round_state::RoundResult::Win::score`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScoreResult {
    /// 总番数 (含 dora / 赤宝牌 / 里宝牌).
    pub han: u32,
    /// 总符数 (向上取整到 10, 最低 30 — 七对子固定 25, 国士 30 占位).
    pub fu: u32,
    /// 役列表: 每条 `(役, 该役番数)`. 含真役 + 宝牌 (Dora / AkaDora / UraDora).
    pub yaku: Vec<(Yaku, u32)>,
    /// 基本点 (基本点 / Basic Points). distribute 据此乘 4/6/2/1 算各家支付.
    pub base_points: u32,
    /// 和了等级 (满贯 / 跳满 / 役满 / 等).
    pub level: ScoreLevel,
}

/// 单笔点数转移. `from` 付给 `to` `amount` 点.
///
/// 特殊: `from == to == winner` 表示立直棒池清算给和家 (self-payment).
/// [`crate::engine::match_state::match_apply`] 处理时仅给 `to` 加分, 不从 `from` 扣.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PaymentDistribution {
    /// 付款方 (放铳家 / 子家自摸时的庄家 / etc.).
    pub from: Seat,
    /// 收款方 (通常 = 和家).
    pub to: Seat,
    /// 转移点数 (正数). 已含本场 +300 / 立直棒 +1000.
    pub amount: i32,
}

/// 符计算. 输入: 已确定的拆解 + 上下文 + 副露列表.
pub fn calculate_fu(d: &Decomposition, ctx: &WinContext, melds: &[Meld]) -> u32 {
    match d {
        Decomposition::Chiitoitsu { .. } => 25,
        Decomposition::Kokushi { .. } => 30, // 国士役满, 符没意义但给个值
        Decomposition::Standard {
            mentsu,
            pair,
            wait,
            winning_tile,
        } => {
            // 检测 pinfu (会绑定特殊符规则).
            // 任何鸣牌 (含 ankan, kantsu 破坏 pinfu) 都不允许.
            let is_pinfu = ctx.menzen
                && melds.is_empty()
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

            // 向上取整到 10, 然后兜底 30.
            // 非 pinfu/chiitoitsu 的标准型 fu 最小 30 (天凤规则).
            // 副露 ron + 0 加成 = 20 → 必须圆到 30.
            (fu.div_ceil(10) * 10).max(30)
        }
    }
}

fn mentsu_fu(
    m: &Mentsu,
    ctx: &WinContext,
    wait: WaitKind,
    winning: crate::engine::domain::tile::TileIndex,
) -> u32 {
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
    let yaku = detect_yaku(ctx, melds);
    if yaku.is_empty() {
        return None;
    }
    // 必须至少有一个非 dora 役.
    let has_real_yaku = yaku
        .iter()
        .any(|(y, _)| !matches!(y, Yaku::Dora(_) | Yaku::AkaDora(_) | Yaku::UraDora(_)));
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
        (
            8000 * yakuman_count as u32,
            ScoreLevel::Yakuman(yakuman_count),
        )
    } else if han >= 13 && ctx.rules.kazoe_yakuman {
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

/// 终局某家的最终成绩 — 返点 + 赌马 (uma) + 头名奖 (oka), 单位 K (千点).
///
/// 由 [`final_ranking`] 计算返回 (整庄结束时调).
///
/// # 公式
///
/// - `return_diff_k` = (`raw_score` - `target_score`) / 1000
/// - `uma` = 按 1..=4 位从 `rules.uma[0..4]` 取
/// - `oka` = 头名独得 (其余 0): `(target_score - starting_score) × 4 / 1000`
/// - `final_score` = return_diff_k + uma + oka
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ranking {
    /// 该家座位.
    pub seat: Seat,
    /// 顺位 (1..=4). 1 = 第一名.
    pub place: u8,
    /// 整庄结束时的原始持点.
    pub raw_score: i32,
    /// 返点差 = (raw_score - target_score) / 1000, 单位千点.
    /// 例: 34000 raw - 30000 target = +4 K.
    pub return_diff_k: i32,
    /// 赌马 (ウマ / Uma) 加减分, 单位 K. 来自 [`GameRules::uma`].
    pub uma: i32,
    /// 头名奖 (オカ / Oka), 单位 K. 仅 1 位非 0.
    pub oka: i32,
    /// 最终得分 = `return_diff_k + uma + oka`.
    pub final_score: i32,
}

/// 计算终局四家排名 + uma + oka.
///
/// 规则:
/// - 按 raw_score 降序; 同分按起家顺(East > South > West > North).
/// - uma 按位次发放 config.uma[i].
/// - oka 给 1 位: (target_score - starting_score) * 4 / 1000 (K).
/// - 单位统一为 K(千点), 现实社团报点常用单位.
pub fn final_ranking(players: &[PlayerState; 4], config: &GameRules) -> [Ranking; 4] {
    let mut indices = [0usize, 1, 2, 3];
    // 降序排; 同分按 Seat 顺序(index 小的在前).
    indices.sort_by(|&a, &b| {
        players[b]
            .score
            .cmp(&players[a].score)
            .then_with(|| a.cmp(&b))
    });

    let target_k = config.target_score / 1000;
    let oka_top_k = (config.target_score - config.starting_score) * 4 / 1000;

    let mut out = [Ranking {
        seat: Seat::East,
        place: 0,
        raw_score: 0,
        return_diff_k: 0,
        uma: 0,
        oka: 0,
        final_score: 0,
    }; 4];

    for (rank_idx, &player_idx) in indices.iter().enumerate() {
        let p = &players[player_idx];
        let return_diff_k = p.score / 1000 - target_k;
        let uma = config.uma[rank_idx];
        let oka = if rank_idx == 0 { oka_top_k } else { 0 };
        out[rank_idx] = Ranking {
            seat: p.seat,
            place: (rank_idx + 1) as u8,
            raw_score: p.score,
            return_diff_k,
            uma,
            oka,
            final_score: return_diff_k + uma + oka,
        };
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
        let d = distribute(
            &result,
            Seat::East,
            Seat::East,
            false,
            Some(Seat::South),
            0,
            0,
        );
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

    fn ps(seat: Seat, score: i32) -> PlayerState {
        let mut p = PlayerState::new(seat, score);
        p.score = score;
        p
    }

    #[test]
    fn final_ranking_orders_by_score_then_seat() {
        let players = [
            ps(Seat::East, 30000),
            ps(Seat::South, 40000),
            ps(Seat::West, 20000),
            ps(Seat::North, 10000),
        ];
        let cfg = GameRules::default();
        let r = final_ranking(&players, &cfg);
        assert_eq!(r[0].seat, Seat::South);
        assert_eq!(r[1].seat, Seat::East);
        assert_eq!(r[2].seat, Seat::West);
        assert_eq!(r[3].seat, Seat::North);
        assert_eq!(r[0].place, 1);
        assert_eq!(r[3].place, 4);
    }

    #[test]
    fn final_ranking_uma_oka_default() {
        // 默认: uma=[15,5,-5,-15], starting=25000, target=30000.
        // oka_top = (30000-25000)*4/1000 = 20.
        // 终局总点棒守恒: 100000.
        let players = [
            ps(Seat::East, 50000),  // 1位
            ps(Seat::South, 30000), // 2位
            ps(Seat::West, 15000),  // 3位
            ps(Seat::North, 5000),  // 4位
        ];
        let cfg = GameRules::default();
        let r = final_ranking(&players, &cfg);

        // 1 位: (50000-30000)/1000 + 15 + 20 = 20 + 15 + 20 = 55
        assert_eq!(r[0].final_score, 55);
        // 2 位: 0 + 5 + 0 = 5
        assert_eq!(r[1].final_score, 5);
        // 3 位: -15 + (-5) = -20
        assert_eq!(r[2].final_score, -20);
        // 4 位: -25 + (-15) = -40
        assert_eq!(r[3].final_score, -40);
        // 总和守恒(uma 和 oka 都来自玩家间转移): 55 + 5 + (-20) + (-40) = 0.
        let total: i32 = r.iter().map(|x| x.final_score).sum();
        assert_eq!(total, 0);
    }

    #[test]
    fn final_ranking_tie_uses_seat_order() {
        // 两家同分 → 按 East > South > West > North.
        let players = [
            ps(Seat::East, 25000),
            ps(Seat::South, 25000),
            ps(Seat::West, 25000),
            ps(Seat::North, 25000),
        ];
        let cfg = GameRules::default();
        let r = final_ranking(&players, &cfg);
        assert_eq!(r[0].seat, Seat::East);
        assert_eq!(r[1].seat, Seat::South);
        assert_eq!(r[2].seat, Seat::West);
        assert_eq!(r[3].seat, Seat::North);
    }
}
