//! 庄 (Match) 层 — 跨局累积 state + 转移函数 match_apply.
//!
//! Match 是一庄完整比赛 (东风 / 半庄 / 一庄). 一庄含多局 (Round), 每局结束生成
//! `RoundOutcome` 喂回来推进 MatchState. 见 docs/design/abstract-model.md §Layer 1.

use crate::engine::domain::meld::Seat;
use crate::engine::rules::{GameRules, LengthRule};
use crate::engine::score::PaymentDistribution;
use crate::legacy_state::{RoundWind, RyuukyokuKind};
use serde::{Deserialize, Serialize};

/// 跨局累积的庄状态.
///
/// 局间 canonical 数据源. 局开始时由 init_round 注入 RoundState 的 CommonRound,
/// 局结束时 summarize_round 抽 RoundOutcome 喂回 match_apply 更新本 struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchState {
    /// 4 家分数, 索引 = Seat::index().
    pub scores: [i32; 4],
    /// 当前庄家.
    pub dealer: Seat,
    /// 场风 (东 / 南 / 西 / 北).
    pub round_wind: RoundWind,
    /// 局序号 (1..=4 in each round_wind).
    pub kyoku: u8,
    /// 本场数. 庄家和 / 流局连庄 +1, 子家和 0.
    pub honba: u8,
    /// 桌面累积立直棒池 (× 1000 点). 和家整池领走, 流局保留.
    pub riichi_sticks_pool: u32,
    /// 整庄规则参数 (开庄冻结, 整庄不变).
    pub rules: GameRules,
    /// 是否整庄结束.
    pub ended: bool,
}

impl MatchState {
    /// 整庄初始 state.
    pub fn new(rules: GameRules) -> Self {
        let starting = rules.starting_score;
        Self {
            scores: [starting; 4],
            dealer: Seat::East,
            round_wind: RoundWind::East,
            kyoku: 1,
            honba: 0,
            riichi_sticks_pool: 0,
            rules,
            ended: false,
        }
    }
}

/// 一局的产出, 喂给 match_apply 推进 MatchState.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundOutcome {
    /// 和牌局.
    Win {
        winner: Seat,
        is_tsumo: bool,
        /// 放铳家 (荣和才 Some, 自摸 None).
        loser: Option<Seat>,
        /// 由 score::distribute 计算的支付列表 (含立直棒转移给和家的 self-payment).
        payments: Vec<PaymentDistribution>,
    },
    /// 流局.
    Ryuukyoku {
        kind: RyuukyokuKind,
        /// 庄家是否听牌 (决定连庄 vs 进局).
        dealer_tenpai: bool,
        // 注: 听牌罚符 (1-3 听者间 ±1000~3000) MVP 未实现, 留 TODO.
        // tenpai_payments: Vec<PaymentDistribution>,
    },
}

/// 庄层转移函数: 用 RoundOutcome 推进 MatchState.
///
/// 应用顺序:
/// 1. 把 payments 应用到 scores (含立直棒).
/// 2. 立直棒池清算 (Win → 0, Ryuukyoku → 不变).
/// 3. 决定 dealer / honba / kyoku / round_wind 推进.
/// 4. 检测整庄是否结束.
pub fn match_apply(state: &MatchState, outcome: RoundOutcome) -> MatchState {
    let mut s = state.clone();
    match outcome {
        RoundOutcome::Win {
            winner,
            payments,
            loser: _,
            is_tsumo: _,
        } => {
            apply_payments(&mut s.scores, &payments);
            s.riichi_sticks_pool = 0; // 和家通过 payments 已领走立直棒
            if winner == s.dealer {
                // 庄家和: 连庄 + 本场 +1
                s.honba += 1;
            } else {
                // 子家和: honba 清零, 进局
                s.honba = 0;
                advance_kyoku(&mut s);
            }
        }
        RoundOutcome::Ryuukyoku {
            kind: _,
            dealer_tenpai,
        } => {
            // 流局: 本场 +1; 庄家听牌连庄, 不听牌进局. 立直棒池保留.
            s.honba += 1;
            if !dealer_tenpai {
                advance_kyoku(&mut s);
            }
        }
    }
    s.ended = check_match_ended(&s);
    s
}

/// 庄家右移; 若回到 East 则推场风, 半庄南 4 完进 GameEnd, 东风战东 4 完进 GameEnd.
fn advance_kyoku(s: &mut MatchState) {
    s.dealer = s.dealer.next();
    if s.dealer == Seat::East {
        // 一圈结束, 推场风
        s.round_wind = match s.round_wind {
            RoundWind::East => {
                if matches!(s.rules.length, LengthRule::Tonpuusen) {
                    // 东风战: 东 4 完即结束
                    s.ended = true;
                    return;
                }
                RoundWind::South
            }
            RoundWind::South => {
                // 半庄: 南 4 完结束
                s.ended = true;
                return;
            }
            // 西 / 北 风目前不会进入 (无 LengthRule 支持), 保留兜底
            _ => RoundWind::East,
        };
        s.kyoku = 1;
    } else {
        s.kyoku += 1;
    }
}

/// 检测整庄是否结束. advance_kyoku 内部已显式 set ended, 此处不动 (保留作扩展点).
pub fn check_match_ended(s: &MatchState) -> bool {
    s.ended
}

/// 把 payments 列表应用到 scores. self-payment (from == to) 仅加 to (用于立直棒转移).
fn apply_payments(scores: &mut [i32; 4], payments: &[PaymentDistribution]) {
    for p in payments {
        if p.from != p.to {
            scores[p.from.index()] -= p.amount;
        }
        scores[p.to.index()] += p.amount;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payment(from: Seat, to: Seat, amount: i32) -> PaymentDistribution {
        PaymentDistribution { from, to, amount }
    }

    #[test]
    fn init_match_default() {
        let m = MatchState::new(GameRules::default());
        assert_eq!(m.scores, [25000; 4]);
        assert_eq!(m.dealer, Seat::East);
        assert_eq!(m.round_wind, RoundWind::East);
        assert_eq!(m.kyoku, 1);
        assert_eq!(m.honba, 0);
        assert_eq!(m.riichi_sticks_pool, 0);
        assert!(!m.ended);
    }

    #[test]
    fn dealer_win_keeps_dealer_increments_honba() {
        let m = MatchState::new(GameRules::default());
        let outcome = RoundOutcome::Win {
            winner: Seat::East,
            is_tsumo: true,
            loser: None,
            payments: vec![
                payment(Seat::South, Seat::East, 4000),
                payment(Seat::West, Seat::East, 4000),
                payment(Seat::North, Seat::East, 4000),
            ],
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.scores, [37000, 21000, 21000, 21000]);
        assert_eq!(m.dealer, Seat::East, "庄家和: 连庄");
        assert_eq!(m.honba, 1);
        assert_eq!(m.kyoku, 1);
        assert_eq!(m.riichi_sticks_pool, 0);
    }

    #[test]
    fn child_win_advances_dealer_resets_honba() {
        let mut m = MatchState::new(GameRules::default());
        m.honba = 3;
        let outcome = RoundOutcome::Win {
            winner: Seat::South,
            is_tsumo: false,
            loser: Some(Seat::East),
            payments: vec![payment(Seat::East, Seat::South, 8000)],
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.scores, [17000, 33000, 25000, 25000]);
        assert_eq!(m.dealer, Seat::South, "子家和: 进局");
        assert_eq!(m.honba, 0);
        assert_eq!(m.kyoku, 2);
    }

    #[test]
    fn ryuukyoku_dealer_tenpai_keeps_dealer() {
        let m = MatchState::new(GameRules::default());
        let outcome = RoundOutcome::Ryuukyoku {
            kind: RyuukyokuKind::Howaipai,
            dealer_tenpai: true,
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.dealer, Seat::East, "庄家听: 连庄");
        assert_eq!(m.honba, 1);
        assert_eq!(m.kyoku, 1);
    }

    #[test]
    fn ryuukyoku_dealer_no_tenpai_advances() {
        let m = MatchState::new(GameRules::default());
        let outcome = RoundOutcome::Ryuukyoku {
            kind: RyuukyokuKind::Howaipai,
            dealer_tenpai: false,
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.dealer, Seat::South, "庄家不听: 进局");
        assert_eq!(m.honba, 1, "流局本场 +1");
        assert_eq!(m.kyoku, 2);
    }

    #[test]
    fn riichi_sticks_pool_reset_on_win() {
        let mut m = MatchState::new(GameRules::default());
        m.riichi_sticks_pool = 2;
        let outcome = RoundOutcome::Win {
            winner: Seat::East,
            is_tsumo: true,
            loser: None,
            // payments 里包含立直棒 self-payment (engine 调 score::distribute 时附加)
            payments: vec![
                payment(Seat::South, Seat::East, 4000),
                payment(Seat::West, Seat::East, 4000),
                payment(Seat::North, Seat::East, 4000),
                payment(Seat::East, Seat::East, 2000), // riichi_sticks × 1000
            ],
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.scores[0], 25000 + 12000 + 2000); // 12000 from payments + 2000 riichi
        assert_eq!(m.riichi_sticks_pool, 0, "和家领走立直棒后池清零");
    }

    #[test]
    fn riichi_sticks_pool_carries_through_ryuukyoku() {
        let mut m = MatchState::new(GameRules::default());
        m.riichi_sticks_pool = 1;
        let outcome = RoundOutcome::Ryuukyoku {
            kind: RyuukyokuKind::Howaipai,
            dealer_tenpai: false,
        };
        let m = match_apply(&m, outcome);
        assert_eq!(m.riichi_sticks_pool, 1, "流局立直棒池保留");
    }

    #[test]
    fn tonpuusen_ends_after_east_4() {
        let mut m = MatchState::new(GameRules::default());
        m.rules.length = LengthRule::Tonpuusen;
        m.dealer = Seat::North;
        m.kyoku = 4;
        let outcome = RoundOutcome::Win {
            winner: Seat::South,
            is_tsumo: false,
            loser: Some(Seat::North),
            payments: vec![payment(Seat::North, Seat::South, 8000)],
        };
        let m = match_apply(&m, outcome);
        assert!(m.ended, "东风战东 4 子家和后整庄结束");
    }

    #[test]
    fn hanchan_advances_to_south_after_east_4() {
        let mut m = MatchState::new(GameRules::default());
        m.rules.length = LengthRule::Hanchan;
        m.dealer = Seat::North;
        m.kyoku = 4;
        let outcome = RoundOutcome::Win {
            winner: Seat::South,
            is_tsumo: false,
            loser: Some(Seat::North),
            payments: vec![payment(Seat::North, Seat::South, 8000)],
        };
        let m = match_apply(&m, outcome);
        assert!(!m.ended, "半庄东 4 后未结束");
        assert_eq!(m.round_wind, RoundWind::South);
        assert_eq!(m.kyoku, 1);
        assert_eq!(m.dealer, Seat::East);
    }

    #[test]
    fn hanchan_ends_after_south_4() {
        let mut m = MatchState::new(GameRules::default());
        m.rules.length = LengthRule::Hanchan;
        m.round_wind = RoundWind::South;
        m.dealer = Seat::North;
        m.kyoku = 4;
        let outcome = RoundOutcome::Win {
            winner: Seat::South,
            is_tsumo: false,
            loser: Some(Seat::North),
            payments: vec![payment(Seat::North, Seat::South, 8000)],
        };
        let m = match_apply(&m, outcome);
        assert!(m.ended, "半庄南 4 子家和后结束");
    }
}
