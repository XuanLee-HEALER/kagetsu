//! 庄 (Match / 半庄 / 一庄 / 東風戦) 层状态 + 转移函数.
//!
//! 一庄 (Match) 是一次完整比赛 — 4 家从同一起始分数出发, 经过若干局
//! ([`crate::engine::round_state::RoundState`]) 直到长度规则 (
//! [`crate::engine::rules::LengthRule`]) 满足. 比赛结束时 4 家分数即最终
//! 排名依据.
//!
//! # 长度规则
//!
//! - **东风战** (Tonpuusen / 東風戦): 仅东风圈, 东 1 → 东 4 共 4 局
//! - **半庄** (Hanchan / 半荘): 东 1 → 东 4 → 南 1 → 南 4 共 8 局
//! - 一庄 (Ichijou / 一荘): 东南西北全跑, 16 局 — 本 engine 不支持
//!
//! # 庄/局关系
//!
//! 庄 (Match) 是 *外层 fold*, 局 (Round) 是 *内层 fold*:
//!
//! ```text
//! match_state = ROUNDS.fold(match_apply, init_match)
//! round_state = OPS.try_fold(round_apply, init_round)
//! ```
//!
//! [`MatchState`] 在两局间充当 canonical 数据源:
//! 1. 上局结束 → [`crate::engine::round_state::summarize_round`] 抽 [`RoundOutcome`]
//! 2. [`match_apply`] 用 outcome 更新 scores / dealer / honba / kyoku / round_wind
//! 3. 检测是否整庄结束 (`ended = true`)
//! 4. 若没结束, [`crate::engine::round_state::init_round`] 起下局
//!
//! # 引用
//!
//! 设计文档: `docs/design/abstract-model.md` §Layer 1

use crate::engine::domain::meld::Seat;
use crate::engine::round_state::{RoundWind, RyuukyokuKind};
use crate::engine::rules::{GameRules, LengthRule};
use crate::engine::score::PaymentDistribution;
use serde::{Deserialize, Serialize};

/// 跨局累积的庄状态.
///
/// 一庄比赛的 *canonical 数据源*. 局间维护 4 家分数 + 当前庄家 + 局序号 +
/// 本场 + 立直棒池 + 整庄是否结束.
///
/// # 与 RoundState 的关系
///
/// MatchState 数据在局开始时通过 [`crate::engine::round_state::init_round`]
/// *拷贝* 进 [`crate::engine::round_state::CommonRound`]. 局内变化 (摸牌 / 切牌 /
/// 立直 / 等) 都改 RoundState, *不直接改* MatchState. 局结束后 [`match_apply`]
/// 把 [`RoundOutcome`] 投影回 MatchState — 这是 outcome 唯一能改 MatchState 的入口.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchState {
    /// 4 家分数 (持点 / Mochiten). 索引 = `Seat::index()` (East=0, South=1, ...).
    /// 初始 = `rules.starting_score` (默认 25000), 整庄分数和恒定为 100000
    /// (除供托外, 杠点 / 立直棒等都在 4 家间转移).
    pub scores: [i32; 4],
    /// 当前庄家 (亲家 / Oya). 庄家和 / 流局听牌时连庄 (Renchan), 否则下庄.
    pub dealer: Seat,
    /// 场风 (場風 / Bakaze) — 决定字牌役牌身份 + 整庄进度.
    pub round_wind: RoundWind,
    /// 局序号 (局数 / Kyoku). 取值 `1..=4`, 每个 `round_wind` 内独立编号.
    /// 例: 半庄共 8 局 = 东 1/2/3/4 + 南 1/2/3/4.
    pub kyoku: u8,
    /// 本场数 (本場 / Honba). 庄家连和 / 流局每次 +1, 子家和清零, 进局清零.
    /// 影响和点 (每本场 +300 自摸 / +300 荣和).
    pub honba: u8,
    /// 立直棒池 (供托 / Kyoutaku). 累积尚未被领走的 1000 点立直棒.
    /// 局内有人立直 → +1; 和家通过 payments 整池领走; 流局保留到下局.
    pub riichi_sticks_pool: u32,
    /// 整庄规则参数. 开庄时冻结, 整庄内不允许修改.
    pub rules: GameRules,
    /// 是否整庄结束. `true` 时不应再调 [`match_apply`] / `init_round`,
    /// 改用 [`crate::engine::score::final_ranking`] 算最终排名.
    pub ended: bool,
}

impl MatchState {
    /// 整庄初始 state.
    ///
    /// 4 家初始分数 = `rules.starting_score`, 庄家 = East, 起 *东 1 局 0 本场*.
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

/// 一局结束的庄层产出. 由 [`crate::engine::round_state::summarize_round`] 抽出,
/// 喂给 [`match_apply`] 推进 [`MatchState`].
///
/// 与 [`crate::engine::round_state::RoundResult`] 的区别: `RoundResult` 是局
/// *内部* 视角 (含完整 ScoreResult / 役), `RoundOutcome` 是 *庄层视角* (只关心
/// 谁赢 + 怎么算分 + 是否连庄).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundOutcome {
    /// 和了局.
    Win {
        /// 和家.
        winner: Seat,
        /// `true` = 自摸; `false` = 荣和.
        is_tsumo: bool,
        /// 放铳家. 自摸 `None` (4 家分摊 / 庄家平摊), 荣和 `Some(切牌方)`.
        loser: Option<Seat>,
        /// 完整支付列表 (由 [`crate::engine::score::distribute`] 算出).
        /// 含立直棒转移给和家的 *self-payment* (from == to == winner) — 表示
        /// 立直棒池清算给和家.
        payments: Vec<PaymentDistribution>,
    },
    /// 流局.
    Ryuukyoku {
        /// 流局类型 (本 engine 当前仅 `Howaipai` 真触发).
        kind: RyuukyokuKind,
        /// 庄家是否听牌 (Tenpai). 决定本局连庄 vs 进局:
        /// - 听 → `dealer` 不变, `honba += 1` (连庄)
        /// - 不听 → `dealer = dealer.next()`, `honba += 1` (进局)
        ///
        /// 注: 听牌罚符 (听者从不听者收 ±1000~3000) 当前未实现, future work.
        dealer_tenpai: bool,
    },
}

/// 庄层转移函数 — 用 [`RoundOutcome`] 推进 [`MatchState`].
///
/// pure function (不改 input, 返新 state).
///
/// # 应用顺序
///
/// 1. 把 `payments` 应用到 scores (含立直棒 self-payment)
/// 2. 立直棒池清算: `Win` → 0 (和家领走); `Ryuukyoku` → 不变 (留下局)
/// 3. 庄家 / 本场 / 局序号 / 场风 推进:
///    - 庄家和 → 连庄, `honba += 1`
///    - 子家和 → 进局, `honba = 0`, `dealer = dealer.next()`
///    - 流局听 → 连庄, `honba += 1`
///    - 流局不听 → 进局, `honba += 1`, `dealer = dealer.next()`
/// 4. 检测整庄是否结束 (按 `LengthRule`)
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

/// 是否整庄结束. 当前实现仅返 `s.ended` (`advance_kyoku` 内部已显式 set).
///
/// 留作扩展点 — 未来若加规则 (例: 任何家分数 < 0 即终止), 可在此实现.
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
