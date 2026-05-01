//! ReplayDriver: 用一个轻量内存状态机吃 [`KyokuLog`] 的事件流, 对比期望.
//!
//! ## 为何不直接用 `GameState`?
//!
//! `GameState::do_draw` 从 `Wall` 取下一张, 但 mjai 牌谱不直接给 wall 顺序
//! (只有 Tsumo events 透露摸到了什么). 重建完整 wall 顺序复杂度高 (要跟踪
//! 普通摸 vs rinshan 摸). 这里用一个 `ReplayState` 自己管手牌 / 副露 / 河,
//! 仅在 Hora 时构造 `WinContext` 调 `score::evaluate` 对比.
//!
//! ## 当前覆盖 (P5)
//!
//! - 事件流回放 (Tsumo / Dahai / Pon / Chi / Kan / Reach / Dora)
//! - Ryukyoku deltas 对比
//! - Hora 时构造 WinContext 调 evaluate 算 fu/han/yakuman → 对比 mjai 期望
//!
//! ## 已知简化
//!
//! - 一发 / 海底 / 河底 / 岭上开花 / 抢杠 / 双倍立直 / 天地人和 等"特殊状态"
//!   暂未追踪 (会引起 yaku 缺漏); 后续 phase 在调 fixture 时逐个补.

use std::collections::HashMap;

use tui_majo::config::GameConfig;
use tui_majo::decompose::{Decomposition, decompose};
use tui_majo::game::RoundWind;
use tui_majo::meld::{Meld, MeldKind, Seat};
use tui_majo::tile::{Tile, TileIndex, count_by_kind};
use tui_majo::yaku::WinContext;

use super::replay_log::{KyokuEvent, KyokuLog, KyokuResult};

#[derive(Debug, Clone)]
pub enum ReplayDiff {
    /// 事件序列中第 idx 个事件应用失败 (e.g. 手中找不到该牌).
    EventFailed { idx: usize, reason: String },
    /// 期望结算结果与实际不一致.
    ResultMismatch { reason: String },
}

#[derive(Debug)]
struct ReplayState {
    hands: [Vec<Tile>; 4],
    melds: [Vec<Meld>; 4],
    rivers: [Vec<Tile>; 4],
    riichi: [bool; 4],
    last_drawn: [Option<Tile>; 4],
    last_discard: Option<(Seat, Tile)>,
    dora_indicators: Vec<Tile>,
    scores: [i32; 4],
    riichi_sticks: u8,
    honba: u8,
    dealer: Seat,
    round_wind: RoundWind,
}

impl ReplayState {
    fn new(log: &KyokuLog) -> Self {
        Self {
            hands: log.initial_hands.clone(),
            melds: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            rivers: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            riichi: [false; 4],
            last_drawn: [None; 4],
            last_discard: None,
            dora_indicators: vec![log.initial_dora_marker],
            scores: log.initial_scores,
            riichi_sticks: log.riichi_sticks,
            honba: log.honba,
            dealer: log.dealer,
            round_wind: log.round_wind,
        }
    }

    fn apply(&mut self, ev: &KyokuEvent) -> Result<(), String> {
        match ev {
            KyokuEvent::Tsumo { who, tile } => {
                self.hands[who.index()].push(*tile);
                self.last_drawn[who.index()] = Some(*tile);
            }
            KyokuEvent::Dahai {
                who,
                tile,
                tsumogiri: _,
            } => {
                let idx = who.index();
                let pos = self.hands[idx]
                    .iter()
                    .position(|t| t.kind == tile.kind && t.red == tile.red)
                    .ok_or_else(|| format!("Dahai: 手中找不到 {:?} ({:?})", tile.kind, tile.red))?;
                self.hands[idx].remove(pos);
                self.rivers[idx].push(*tile);
                self.last_drawn[idx] = None;
                self.last_discard = Some((*who, *tile));
            }
            KyokuEvent::Pon {
                who,
                from,
                target,
                consumed,
            } => {
                let idx = who.index();
                for c in consumed {
                    let pos = self.hands[idx]
                        .iter()
                        .position(|t| t.kind == c.kind && t.red == c.red)
                        .ok_or_else(|| format!("Pon consumed 找不到 {:?}", c.kind))?;
                    self.hands[idx].remove(pos);
                }
                // pop discard from from's river
                if let Some(last) = self.rivers[from.index()].pop() {
                    debug_assert_eq!(last.kind, target.kind);
                }
                self.melds[idx].push(Meld {
                    kind: MeldKind::Pon {
                        tiles: [consumed[0], consumed[1], *target],
                    },
                    from: Some(*from),
                });
                self.last_discard = None;
            }
            KyokuEvent::Chi {
                who,
                from,
                target,
                consumed,
            } => {
                let idx = who.index();
                for c in consumed {
                    let pos = self.hands[idx]
                        .iter()
                        .position(|t| t.kind == c.kind && t.red == c.red)
                        .ok_or_else(|| format!("Chi consumed 找不到 {:?}", c.kind))?;
                    self.hands[idx].remove(pos);
                }
                if let Some(last) = self.rivers[from.index()].pop() {
                    debug_assert_eq!(last.kind, target.kind);
                }
                self.melds[idx].push(Meld {
                    kind: MeldKind::Chi {
                        tiles: [consumed[0], consumed[1], *target],
                    },
                    from: Some(*from),
                });
                self.last_discard = None;
            }
            KyokuEvent::Daiminkan {
                who,
                from,
                target,
                consumed,
            } => {
                let idx = who.index();
                for c in consumed {
                    let pos = self.hands[idx]
                        .iter()
                        .position(|t| t.kind == c.kind && t.red == c.red)
                        .ok_or_else(|| format!("Daiminkan consumed 找不到 {:?}", c.kind))?;
                    self.hands[idx].remove(pos);
                }
                if let Some(last) = self.rivers[from.index()].pop() {
                    debug_assert_eq!(last.kind, target.kind);
                }
                self.melds[idx].push(Meld {
                    kind: MeldKind::Minkan {
                        tiles: [consumed[0], consumed[1], consumed[2], *target],
                    },
                    from: Some(*from),
                });
                self.last_discard = None;
            }
            KyokuEvent::Ankan { who, consumed } => {
                let idx = who.index();
                for c in consumed {
                    let pos = self.hands[idx]
                        .iter()
                        .position(|t| t.kind == c.kind && t.red == c.red)
                        .ok_or_else(|| format!("Ankan consumed 找不到 {:?}", c.kind))?;
                    self.hands[idx].remove(pos);
                }
                self.melds[idx].push(Meld {
                    kind: MeldKind::Ankan {
                        tiles: [consumed[0], consumed[1], consumed[2], consumed[3]],
                    },
                    from: None,
                });
            }
            KyokuEvent::Kakan { who, target, .. } => {
                let idx = who.index();
                // 把已碰的刻子升级为加杠
                let pon_idx = self.melds[idx]
                    .iter()
                    .position(
                        |m| matches!(&m.kind, MeldKind::Pon { tiles } if tiles[0].kind == target.kind),
                    )
                    .ok_or_else(|| format!("Kakan 找不到对应碰刻 {:?}", target.kind))?;
                let prev = self.melds[idx].remove(pon_idx);
                let pon_tiles = match prev.kind {
                    MeldKind::Pon { tiles } => tiles,
                    _ => unreachable!(),
                };
                // 从手中移除加杠的第 4 张
                let pos = self.hands[idx]
                    .iter()
                    .position(|t| t.kind == target.kind)
                    .ok_or_else(|| format!("Kakan 手中找不到 {:?}", target.kind))?;
                self.hands[idx].remove(pos);
                self.melds[idx].push(Meld {
                    kind: MeldKind::Shouminkan {
                        tiles: [pon_tiles[0], pon_tiles[1], pon_tiles[2], *target],
                    },
                    from: prev.from,
                });
            }
            KyokuEvent::Reach { who } => {
                self.riichi[who.index()] = true;
                // 实际立直成立在 ReachAccepted, 这里只标志
            }
            KyokuEvent::ReachAccepted { who } => {
                self.scores[who.index()] -= 1000;
                self.riichi_sticks += 1;
            }
            KyokuEvent::Dora { tile } => {
                self.dora_indicators.push(*tile);
            }
        }
        Ok(())
    }
}

// ============================================================================
// Replay 入口
// ============================================================================

pub struct ReplayDriver<'a> {
    pub log: &'a KyokuLog,
    pub config: &'a GameConfig,
}

impl<'a> ReplayDriver<'a> {
    pub fn new(log: &'a KyokuLog, config: &'a GameConfig) -> Self {
        Self { log, config }
    }

    pub fn replay(&self) -> Vec<ReplayDiff> {
        let mut diffs = Vec::new();
        let mut state = ReplayState::new(self.log);
        for (i, ev) in self.log.events.iter().enumerate() {
            if let Err(e) = state.apply(ev) {
                diffs.push(ReplayDiff::EventFailed { idx: i, reason: e });
                // 事件失败后续可能连锁错误, 提早返回避免噪音
                return diffs;
            }
        }
        match &self.log.result {
            Some(KyokuResult::Hora { .. }) => {
                diffs.extend(verify_hora(&state, self.log, self.config));
            }
            Some(KyokuResult::Ryukyoku { deltas, .. }) => {
                // 流局: 我们目前不模拟 noten 罚符, 仅检查 deltas 总和守恒
                let total: i32 = deltas.iter().sum();
                if total != 0 {
                    diffs.push(ReplayDiff::ResultMismatch {
                        reason: format!("流局 deltas 总和 {total}, 应 == 0"),
                    });
                }
            }
            None => diffs.push(ReplayDiff::ResultMismatch {
                reason: "kyoku 没有 result (既不 Hora 也不 Ryukyoku)".into(),
            }),
        }
        diffs
    }
}

// ============================================================================
// Hora 验证: 用 winner 的 hand+melds 调 evaluate
// ============================================================================

fn verify_hora(state: &ReplayState, log: &KyokuLog, config: &GameConfig) -> Vec<ReplayDiff> {
    let mut diffs = Vec::new();
    let (winner, from, winning_tile, expected_fu, expected_han, expected_yakuman, _expected_points) =
        match &log.result {
            Some(KyokuResult::Hora {
                winner,
                from,
                winning_tile,
                fu,
                han,
                yakuman,
                points,
                ..
            }) => (*winner, *from, *winning_tile, *fu, *han, *yakuman, *points),
            _ => return diffs,
        };

    let widx = winner.index();
    let mut full_hand = state.hands[widx].clone();
    // hand + winning_tile 应可分解
    full_hand.push(winning_tile);
    let counts = count_by_kind(&full_hand);
    let decomps = decompose(&counts, &state.melds[widx], winning_tile.kind);
    if decomps.is_empty() {
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!(
                "winner {:?} hand={:?} + win={:?} 无法分解为和牌型",
                winner,
                state.hands[widx]
                    .iter()
                    .map(|t| t.kind.0)
                    .collect::<Vec<_>>(),
                winning_tile.kind.0,
            ),
        });
        return diffs;
    }

    // 取分数最高的分解
    let mut best: Option<tui_majo::score::ScoreResult> = None;
    for decomp in &decomps {
        let ctx = build_ctx(decomp, state, log, config, winner, from, winning_tile);
        if let Some(res) = tui_majo::score::evaluate(&ctx, &state.melds[widx])
            && best
                .as_ref()
                .map(|b| res.han > b.han || (res.han == b.han && res.fu > b.fu))
                .unwrap_or(true)
        {
            best = Some(res);
        }
    }

    let actual = match best {
        Some(s) => s,
        None => {
            diffs.push(ReplayDiff::ResultMismatch {
                reason: "evaluate 没找到任何役 (无役 / 缺役实现)".into(),
            });
            return diffs;
        }
    };

    // 对比 fu / han
    if actual.fu as u8 != expected_fu {
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!("fu 不一致: actual={} expected={}", actual.fu, expected_fu),
        });
    }
    if expected_yakuman > 0 {
        // 役満对比 (我们 score 用 han 表示役満, e.g. 13han = 役満, 26 = 双倍)
        let actual_yakuman = actual.han / 13;
        if actual_yakuman as u8 != expected_yakuman {
            diffs.push(ReplayDiff::ResultMismatch {
                reason: format!(
                    "yakuman 不一致: actual={} expected={}",
                    actual_yakuman, expected_yakuman
                ),
            });
        }
    } else if actual.han as u8 != expected_han {
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!(
                "han 不一致: actual={} expected={}",
                actual.han, expected_han
            ),
        });
    }

    // points 对比 (winner 的 delta)
    let expected_winner_delta = match &log.result {
        Some(KyokuResult::Hora { deltas, .. }) => deltas[widx],
        _ => 0,
    };
    // 我们的 distribute 给出 PaymentDistribution, 但简化: ScoreResult.basic_points
    // 不直接是玩家收入. mjai 的 deltas 已含本场 / 立直棒. 简化对比: 验证 deltas
    // 总和 = 0.
    let total_delta: i32 = match &log.result {
        Some(KyokuResult::Hora { deltas, .. }) => deltas.iter().sum(),
        _ => 0,
    };
    if total_delta != 0 {
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!("Hora deltas 总和 {total_delta} 应 == 0"),
        });
    }
    let _ = expected_winner_delta; // 后续 phase 加严格 winner delta 对比

    diffs
}

fn build_ctx<'a>(
    decomp: &'a Decomposition,
    state: &'a ReplayState,
    _log: &'a KyokuLog,
    config: &'a GameConfig,
    winner: Seat,
    from: Seat,
    winning_tile: Tile,
) -> WinContext<'a> {
    let widx = winner.index();
    let is_tsumo = winner == from;
    let menzen = state.melds[widx]
        .iter()
        .all(|m| matches!(m.kind, MeldKind::Ankan { .. }));
    let fully_concealed = state.melds[widx].is_empty();

    let seat_wind = seat_wind_tile(winner, state.dealer);
    let round_wind_tile = round_wind_to_tile(state.round_wind);

    // dora 计数: 每张 dora indicator 的 next 是 dora, 检查 winner 的牌中有几张
    let dora_count = count_dora(state, widx, winning_tile, false);
    let aka_count = count_aka(state, widx, winning_tile);

    WinContext {
        decomposition: decomp,
        seat_wind,
        round_wind: round_wind_tile,
        winning_tile: winning_tile.kind,
        is_tsumo,
        is_riichi: state.riichi[widx],
        is_double_riichi: false,
        is_ippatsu: false,
        is_haitei: false,
        is_houtei: false,
        is_rinshan: false,
        is_chankan: false,
        is_tenhou: false,
        is_chiihou: false,
        is_renhou: false,
        menzen,
        fully_concealed,
        dora_count,
        aka_count,
        ura_dora_count: 0, // 我们没存 ura_dora_indicators (mjai 给在 hora 内)
        config,
    }
}

fn seat_wind_tile(seat: Seat, dealer: Seat) -> TileIndex {
    let offset = (seat.index() + 4 - dealer.index()) % 4;
    match offset {
        0 => TileIndex::EAST,
        1 => TileIndex::SOUTH,
        2 => TileIndex::WEST,
        _ => TileIndex::NORTH,
    }
}

fn round_wind_to_tile(rw: RoundWind) -> TileIndex {
    match rw {
        RoundWind::East => TileIndex::EAST,
        RoundWind::South => TileIndex::SOUTH,
        RoundWind::West => TileIndex::WEST,
        RoundWind::North => TileIndex::NORTH,
    }
}

/// 统计 winner 的牌 (含和牌) 中 dora 的张数.
fn count_dora(state: &ReplayState, widx: usize, winning_tile: Tile, _ura: bool) -> u32 {
    let dora_kinds: Vec<TileIndex> = state
        .dora_indicators
        .iter()
        .map(|t| t.kind.next_dora())
        .collect();
    let mut all_tiles: Vec<Tile> = state.hands[widx].clone();
    all_tiles.push(winning_tile);
    for m in &state.melds[widx] {
        for t in meld_tiles(m) {
            all_tiles.push(t);
        }
    }
    all_tiles
        .iter()
        .filter(|t| dora_kinds.contains(&t.kind))
        .count() as u32
}

fn count_aka(state: &ReplayState, widx: usize, winning_tile: Tile) -> u32 {
    let mut all_tiles: Vec<Tile> = state.hands[widx].clone();
    all_tiles.push(winning_tile);
    for m in &state.melds[widx] {
        for t in meld_tiles(m) {
            all_tiles.push(t);
        }
    }
    all_tiles.iter().filter(|t| t.red).count() as u32
}

fn meld_tiles(m: &Meld) -> Vec<Tile> {
    match &m.kind {
        MeldKind::Chi { tiles } | MeldKind::Pon { tiles } => tiles.to_vec(),
        MeldKind::Minkan { tiles } | MeldKind::Shouminkan { tiles } | MeldKind::Ankan { tiles } => {
            tiles.to_vec()
        }
    }
}

// ============================================================================
// 工具: 收集 mjai → ReplayLog 后的 yaku 名集合 (用于 yaku 名对照映射)
// ============================================================================

#[allow(dead_code)]
pub fn collect_yaku_names(log: &KyokuLog) -> HashMap<String, u8> {
    let mut out = HashMap::new();
    if let Some(KyokuResult::Hora { yakus, .. }) = &log.result {
        for (name, han) in yakus {
            out.insert(name.clone(), *han);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::mjai_parser::parse_mjai_log;
    use crate::replay::replay_log::build_replay_log;

    /// 极简: tsumo 一张 dahai 一张, 流局, deltas 守恒.
    #[test]
    fn replay_simple_ryukyoku() {
        let log = r#"{"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","E","S","W","N"],["1p","2p","3p","4p","5p","6p","7p","8p","9p","E","S","W","N"],["1s","2s","3s","4s","5s","6s","7s","8s","9s","E","S","W","N"],["1m","1p","1s","E","S","W","N","P","F","C","2m","2p","2s"]],"dora_marker":"5p"}
{"type":"tsumo","actor":0,"pai":"5m"}
{"type":"dahai","actor":0,"pai":"5m","tsumogiri":true}
{"type":"ryukyoku","reason":"fanpai","deltas":[1500,-1500,1500,-1500],"tenpais":[true,false,true,false]}
{"type":"end_kyoku"}"#;
        let evs = parse_mjai_log(log).unwrap();
        let replay = build_replay_log(evs).unwrap();
        let cfg = GameConfig::default();
        let driver = ReplayDriver::new(&replay.kyokus[0], &cfg);
        let diffs = driver.replay();
        assert!(diffs.is_empty(), "应无差异, 实际: {diffs:#?}");
    }

    /// dahai 找不到牌 → EventFailed.
    #[test]
    fn replay_dahai_missing_tile_reports_diff() {
        let log = r#"{"type":"start_kyoku","bakaze":"E","kyoku":1,"honba":0,"riichi_sticks":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","E","S","W","N"],["1p","2p","3p","4p","5p","6p","7p","8p","9p","E","S","W","N"],["1s","2s","3s","4s","5s","6s","7s","8s","9s","E","S","W","N"],["1m","1p","1s","E","S","W","N","P","F","C","2m","2p","2s"]],"dora_marker":"5p"}
{"type":"dahai","actor":0,"pai":"7s","tsumogiri":false}
{"type":"ryukyoku","reason":"fanpai","deltas":[0,0,0,0],"tenpais":[false,false,false,false]}
{"type":"end_kyoku"}"#;
        let evs = parse_mjai_log(log).unwrap();
        let replay = build_replay_log(evs).unwrap();
        let cfg = GameConfig::default();
        let driver = ReplayDriver::new(&replay.kyokus[0], &cfg);
        let diffs = driver.replay();
        assert!(!diffs.is_empty());
        assert!(matches!(diffs[0], ReplayDiff::EventFailed { .. }));
    }
}
