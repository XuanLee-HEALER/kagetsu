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

use tui_majo::domain::decompose::{Decomposition, decompose};
use tui_majo::domain::meld::{Meld, MeldKind, Seat};
use tui_majo::domain::tile::{Tile, TileIndex, count_by_kind};
use tui_majo::domain::yaku::WinContext;
use tui_majo::engine::rules::GameRules;
use tui_majo::engine::state::RoundWind;

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
    /// 立直后还在一发窗口期 (尚未自家再切 / 任何人鸣牌).
    ippatsu_active: [bool; 4],
    /// 自家 Kan 后到下次 dahai 之间, 自摸视为岭上开花.
    rinshan_pending: [bool; 4],
    /// 最近一次 Kakan 的 actor (chankan 检测: 紧接着的 Hora 是抢杠).
    last_kakan_actor: Option<Seat>,
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
            ippatsu_active: [false; 4],
            rinshan_pending: [false; 4],
            last_kakan_actor: None,
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
                // 摸非 Kan 后的牌 → chankan 窗口结束 (Kakan 后未被抢直接进 Tsumo)
                self.last_kakan_actor = None;
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
                // 一发: 立直方再次切牌后失效
                self.ippatsu_active[idx] = false;
                // 切牌后 rinshan 窗口关闭
                self.rinshan_pending[idx] = false;
                self.last_kakan_actor = None;
            }
            KyokuEvent::Pon {
                who,
                from,
                target,
                consumed,
            } => {
                // 鸣牌使所有立直方一发失效
                self.ippatsu_active = [false; 4];
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
                self.ippatsu_active = [false; 4];
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
                self.ippatsu_active = [false; 4];
                self.rinshan_pending[who.index()] = true;
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
                self.ippatsu_active = [false; 4];
                self.rinshan_pending[who.index()] = true;
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
                // Kakan 不清 ippatsu: 若被抢杠 (chankan), kakan 不成立, ippatsu 仍有效;
                // 否则后续 dahai 会自然清.
                self.rinshan_pending[who.index()] = true;
                self.last_kakan_actor = Some(*who);
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
                self.ippatsu_active[who.index()] = true;
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
    pub config: &'a GameRules,
}

impl<'a> ReplayDriver<'a> {
    pub fn new(log: &'a KyokuLog, config: &'a GameRules) -> Self {
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

fn verify_hora(state: &ReplayState, log: &KyokuLog, config: &GameRules) -> Vec<ReplayDiff> {
    let mut diffs = Vec::new();
    let (
        winner,
        from,
        winning_tile,
        expected_fu,
        expected_han,
        expected_yakuman,
        _expected_points,
        ura_markers,
    ) = match &log.result {
        Some(KyokuResult::Hora {
            winner,
            from,
            winning_tile,
            fu,
            han,
            yakuman,
            points,
            uradora_markers,
            ..
        }) => (
            *winner,
            *from,
            *winning_tile,
            *fu,
            *han,
            *yakuman,
            *points,
            uradora_markers.clone(),
        ),
        _ => return diffs,
    };

    let widx = winner.index();
    let is_tsumo = winner == from;
    let mut full_hand = state.hands[widx].clone();
    // 自摸时 winning_tile 已经在 hand (上次 Tsumo 加进去后未切),
    // 荣和时不在 hand (是他家切的牌, 在 last_discard).
    if !is_tsumo {
        full_hand.push(winning_tile);
    }
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
    let mut best: Option<tui_majo::engine::score::ScoreResult> = None;
    for decomp in &decomps {
        let ctx = build_ctx(
            decomp,
            state,
            log,
            config,
            winner,
            from,
            winning_tile,
            &ura_markers,
        );
        if let Some(res) = tui_majo::engine::score::evaluate(&ctx, &state.melds[widx])
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
        let expected_yakus = match &log.result {
            Some(KyokuResult::Hora { yakus, .. }) => yakus
                .iter()
                .map(|(n, h)| format!("{n}+{h}"))
                .collect::<Vec<_>>()
                .join(","),
            _ => String::new(),
        };
        let actual_yakus = actual
            .yaku
            .iter()
            .map(|(y, h)| format!("{y:?}+{h}"))
            .collect::<Vec<_>>()
            .join(",");
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!(
                "han 不一致: actual={} expected={} | actual_yakus=[{}] expected_yakus=[{}]",
                actual.han, expected_han, actual_yakus, expected_yakus
            ),
        });
    }

    // points 对比 (winner 的 delta).
    // 注意: 天凤 deltas 含立直棒收回 (winner +1000×sticks 而无对应输家 -1000),
    // 所以总和 ≠ 0 是正常. 我们只对比 winner 的 delta 是否 ≈ 期望 hora_points + 本场.
    // 由于本场 (× 300) 算法各家分配不同, 此 phase 只做最简对比: winner delta > 0.
    let expected_winner_delta = match &log.result {
        Some(KyokuResult::Hora { deltas, .. }) => deltas[widx],
        _ => 0,
    };
    if expected_winner_delta <= 0 {
        diffs.push(ReplayDiff::ResultMismatch {
            reason: format!(
                "winner {:?} 的 delta {} 应 > 0",
                winner, expected_winner_delta
            ),
        });
    }

    diffs
}

#[allow(clippy::too_many_arguments)]
fn build_ctx<'a>(
    decomp: &'a Decomposition,
    state: &'a ReplayState,
    _log: &'a KyokuLog,
    config: &'a GameRules,
    winner: Seat,
    from: Seat,
    winning_tile: Tile,
    ura_markers: &[Tile],
) -> WinContext<'a> {
    let widx = winner.index();
    let is_tsumo = winner == from;
    // 门清: 没有副露 (暗杠不算副露 → 仍门清)
    let menzen = state.melds[widx]
        .iter()
        .all(|m| matches!(m.kind, MeldKind::Ankan { .. }));
    let fully_concealed = state.melds[widx].is_empty();

    let seat_wind = seat_wind_tile(winner, state.dealer);
    let round_wind_tile = round_wind_to_tile(state.round_wind);

    // dora 计数: 每张 dora indicator 的 next 是 dora.
    // 自摸时 winning_tile 已在 state.hands, 荣和时不在.
    let winning_in_hand = is_tsumo;
    let dora_count = count_dora_by_indicators(
        state,
        widx,
        winning_tile,
        winning_in_hand,
        &state.dora_indicators,
    );
    let aka_count = count_aka(state, widx, winning_tile, winning_in_hand);
    // 立直时和才看里 dora; mjai 给的 ura_markers 已含全部 (1 + 杠 + 数).
    let ura_dora_count = if state.riichi[widx] {
        count_dora_by_indicators(state, widx, winning_tile, winning_in_hand, ura_markers)
    } else {
        0
    };

    // chankan: 自家荣和 (winner != from), 而 from 刚 Kakan
    let is_chankan = !is_tsumo && state.last_kakan_actor == Some(from);
    // rinshan: 自摸 + 自家有 rinshan 待和 (Kan 后摸的牌)
    let is_rinshan = is_tsumo && state.rinshan_pending[widx];

    WinContext {
        decomposition: decomp,
        seat_wind,
        round_wind: round_wind_tile,
        winning_tile: winning_tile.kind,
        is_tsumo,
        is_riichi: state.riichi[widx],
        is_double_riichi: false,
        is_ippatsu: state.ippatsu_active[widx],
        is_haitei: false,
        is_houtei: false,
        is_rinshan,
        is_chankan,
        is_tenhou: false,
        is_chiihou: false,
        is_renhou: false,
        menzen,
        fully_concealed,
        dora_count,
        aka_count,
        ura_dora_count,
        rules: config,
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

/// 统计 winner 的牌 (含和牌) 中按给定 indicators 的 dora 张数.
/// 同 kind 的 indicator 重复算 (4 个 N 切牌 → next dora 各算 4 次).
///
/// `winning_in_hand`: 自摸=true (winning_tile 已在 hand) / 荣和=false.
fn count_dora_by_indicators(
    state: &ReplayState,
    widx: usize,
    winning_tile: Tile,
    winning_in_hand: bool,
    indicators: &[Tile],
) -> u32 {
    let dora_kinds: Vec<TileIndex> = indicators.iter().map(|t| t.kind.next_dora()).collect();
    let mut all_tiles: Vec<Tile> = state.hands[widx].clone();
    if !winning_in_hand {
        all_tiles.push(winning_tile);
    }
    for m in &state.melds[widx] {
        for t in meld_tiles(m) {
            all_tiles.push(t);
        }
    }
    let mut count = 0u32;
    for tile in &all_tiles {
        for dk in &dora_kinds {
            if tile.kind == *dk {
                count += 1;
            }
        }
    }
    count
}

fn count_aka(state: &ReplayState, widx: usize, winning_tile: Tile, winning_in_hand: bool) -> u32 {
    let mut all_tiles: Vec<Tile> = state.hands[widx].clone();
    if !winning_in_hand {
        all_tiles.push(winning_tile);
    }
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
        let cfg = GameRules::default();
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
        let cfg = GameRules::default();
        let driver = ReplayDriver::new(&replay.kyokus[0], &cfg);
        let diffs = driver.replay();
        assert!(!diffs.is_empty());
        assert!(matches!(diffs[0], ReplayDiff::EventFailed { .. }));
    }
}
