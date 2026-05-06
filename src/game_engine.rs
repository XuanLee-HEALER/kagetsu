//! GameEngine — UI / net / dev 层与 pure-functional engine 的桥梁.
//!
//! 持 RoundState + MatchState, 暴露简洁字段 + 高层 do_* 方法. 所有 mutator
//! 内部走 [`round_apply`] / [`match_apply`] / [`legal_ops`], 不引用 legacy_state.
//!
//! UI / net::room / dev::recorder 共用同一个 wrapper.
//!
//! Wrapper 而非纯 RoundState 暴露的理由:
//! - 简化 driver: phase() / players() / turn() 这种字段访问不必每次手写
//!   `match self.round { ... }`.
//! - 部分 legacy API 与 engine API 语义不 1:1 (如 do_riichi 一步, engine
//!   是 RiichiDeclare + Discard 两步), wrapper 内部翻译.
//! - last_result / 录像 / events buffer 等持续状态由 wrapper 缓存, 与
//!   engine 转移函数解耦.
//! - 录像 (recorded_actions) 在 wrapper 自动 push 真正送给 round_apply 的
//!   AtomicOp 序列, replay 直接顺序 round_apply 即可.

use crate::engine::domain::meld::Seat;
use crate::engine::domain::tile::{Tile, TileIndex};
use crate::engine::event::GameEvent;
use crate::engine::match_state::{MatchState, match_apply};
use crate::engine::op::{AtomicOp, OpError};
use crate::engine::phase::Phase;
use crate::engine::player::PlayerState;
use crate::engine::round_state::{
    RoundResult, RoundState, RoundWind, init_round, legal_ops, round_apply, summarize_round,
};
use crate::engine::rules::GameRules;
use crate::engine::score::ScoreResult;
use std::collections::VecDeque;

pub(crate) const MAX_EVENTS: usize = 32;

/// 包 RoundState + MatchState 给 driver 用. 行为接近 legacy GameState 但内部走 engine.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameEngine {
    pub round: RoundState,
    pub mat: MatchState,
    /// RoundEnd 阶段的结果 — 缓存以便 paint 直接读, 不需要每次 match.
    /// next_round 时清掉.
    pub last_result: Option<RoundResult>,
    pub round_seed: u64,
    /// 最近事件 (UI 用), 滚动 buffer.
    pub events: VecDeque<GameEvent>,
    /// 录像缓冲. None = 不录; Some(vec) = 录, 每次 round_apply 调用时把
    /// 真正送进去的 AtomicOp push 进来. dev::recorder 起 / 结束局时 take/swap.
    #[cfg(feature = "dev-tools")]
    #[serde(skip)]
    pub recorded_actions: Option<Vec<AtomicOp>>,
}

impl GameEngine {
    /// 整庄初始化: MatchState::new + 等 start_round 注入 round.
    pub fn new(rules: GameRules) -> Self {
        let mat = MatchState::new(rules);
        // 占位 round (Deal phase 等价): 用 init_round 0 作初始, 但记 round_seed=0
        // 让外部 advance() 进入 Phase::Deal 分支后会立即 start_round 覆盖.
        // 简化做法: 直接用 init_round 0 但标记 last_result 防止 phase 误读.
        // 实际上 legacy GameState::new 进 Phase::Deal, 我们也必须支持 "尚未开局" 状态.
        // 为此提供一个 phase() 实现: 若 last_result.is_none() 且 round 处 AwaitDraw 且 turn==dealer
        // 还不够区分. 改为 round 里塞一个特殊状态: 我们让 new 时 round = init_round(&mat, 0).
        // 然后 phase() 在 caller 显式 start_round 前不会被读 (legacy 也是 advance() 立刻调 start_round).
        let round = init_round(&mat, 0);
        Self {
            round,
            mat,
            last_result: None,
            round_seed: 0,
            events: VecDeque::new(),
            #[cfg(feature = "dev-tools")]
            recorded_actions: None,
        }
    }

    /// 把 round_apply emit 的 events 累积到本地 buffer (滚动, 最多 MAX_EVENTS 条).
    fn push_events(&mut self, evs: Vec<GameEvent>) {
        for e in evs {
            if self.events.len() >= MAX_EVENTS {
                self.events.pop_front();
            }
            self.events.push_back(e);
        }
    }

    /// 录像 push: 录制中则 append 一条 AtomicOp. dev-tools feature off 时 noop.
    #[inline]
    fn record(&mut self, _op: AtomicOp) {
        #[cfg(feature = "dev-tools")]
        if let Some(buf) = self.recorded_actions.as_mut() {
            buf.push(_op);
        }
    }

    /// round_apply + 累积 events + 录像 push 一站式包装. 内部 mutator 都走这个.
    fn apply(&mut self, op: AtomicOp) -> Result<(), OpError> {
        let (next, evs) = round_apply(&self.round, op.clone())?;
        self.round = next;
        self.push_events(evs);
        self.record(op);
        if let Some(r) = self.round.result() {
            self.last_result = Some(r.clone());
        }
        Ok(())
    }

    // ──────────────────────────────────────────────────────────
    // 字段 accessor
    // ──────────────────────────────────────────────────────────

    pub fn phase(&self) -> Phase {
        // engine 6 phase → legacy 4 phase mapping.
        // RoundEnd 路径: 既覆盖 RoundOutcome 也覆盖整庄结束.
        if self.mat.ended && self.last_result.is_some() {
            return Phase::GameEnd;
        }
        match &self.round {
            RoundState::AwaitDraw(_) => Phase::Draw,
            RoundState::AwaitDiscard(_) => Phase::AwaitDiscard,
            RoundState::AwaitRiichiDiscard(_) => Phase::AwaitDiscard,
            RoundState::AwaitRinshanDraw(_) => Phase::Draw,
            RoundState::AwaitCalls(_) => Phase::AwaitCalls,
            RoundState::RoundEnd(_) => Phase::RoundEnd,
        }
    }

    pub fn players(&self) -> &[PlayerState; 4] {
        &self.round.common().players
    }

    pub fn turn(&self) -> Seat {
        self.round.turn().unwrap_or(self.mat.dealer)
    }

    pub fn dealer(&self) -> Seat {
        self.mat.dealer
    }

    pub fn rules(&self) -> &GameRules {
        &self.mat.rules
    }

    pub fn round_wind(&self) -> RoundWind {
        self.mat.round_wind
    }

    pub fn kyoku(&self) -> u8 {
        self.mat.kyoku
    }

    pub fn honba(&self) -> u8 {
        self.mat.honba
    }

    pub fn riichi_sticks(&self) -> u32 {
        self.mat.riichi_sticks_pool
    }

    pub fn last_discard(&self) -> Option<(Seat, Tile)> {
        self.round.last_discard()
    }

    pub fn seat_wind_of(&self, s: Seat) -> TileIndex {
        self.round.common().seat_wind_of(s)
    }

    /// 牌山剩余 (live wall, 不含死墙).
    pub fn wall_remaining(&self) -> usize {
        self.round.common().wall.remaining()
    }

    /// 当前已翻 dora 指示牌列表.
    pub fn dora_indicators(&self) -> Vec<Tile> {
        self.round.common().wall.dora_indicators()
    }

    /// 直接访问 wall (paint 用). 返 Option 仅为兼容老 GameState 风格 (老字段是
    /// Option<Wall> — Phase::Deal 时 None). engine 任何阶段都有 wall, 总返 Some.
    pub fn wall(&self) -> Option<&crate::engine::wall::Wall> {
        Some(&self.round.common().wall)
    }

    // ──────────────────────────────────────────────────────────
    // 局 / 庄推进 mutators
    // ──────────────────────────────────────────────────────────

    /// 起新一局. 替代 legacy start_round.
    pub fn start_round(&mut self, seed: u64) {
        self.round_seed = seed;
        self.round = init_round(&self.mat, seed);
        self.last_result = None;
    }

    /// 摸牌. 返摸到的牌; None = 山摸尽 (engine 已自动转 RoundEnd).
    pub fn do_draw(&mut self) -> Option<Tile> {
        if self.apply(AtomicOp::Draw).is_err() {
            return None;
        }
        if self.last_result.is_some() {
            return None;
        }
        let turn = self.round.turn()?;
        self.round.common().players[turn.index()].last_drawn
    }

    /// 鸣牌后岭上摸 (engine 自动). do_pon/chi/minkan/ankan/shouminkan 后调.
    fn auto_rinshan_if_needed(&mut self) -> Result<(), OpError> {
        if matches!(self.round, RoundState::AwaitRinshanDraw(_)) {
            self.apply(AtomicOp::RinshanDraw)?;
        }
        Ok(())
    }

    /// 切牌.
    pub fn do_discard(&mut self, tile: Tile) -> Result<(), OpError> {
        self.apply(AtomicOp::Discard { tile })
    }

    /// 立直宣告 + 切牌 (driver 一步, engine 两步).
    pub fn do_riichi(&mut self, tile: Tile) -> Result<(), OpError> {
        self.apply(AtomicOp::RiichiDeclare)?;
        self.apply(AtomicOp::Discard { tile })?;
        Ok(())
    }

    /// 暗杠 + 自动岭上摸.
    pub fn do_ankan(&mut self, kind: TileIndex) -> Result<(), OpError> {
        self.apply(AtomicOp::Ankan { kind })?;
        self.auto_rinshan_if_needed()?;
        Ok(())
    }

    /// 加杠 + 自动岭上摸.
    pub fn do_shouminkan(&mut self, kind: TileIndex) -> Result<(), OpError> {
        self.apply(AtomicOp::Shouminkan { kind })?;
        self.auto_rinshan_if_needed()?;
        Ok(())
    }

    /// 碰.
    pub fn do_pon(&mut self, who: Seat, two: [Tile; 2]) -> Result<(), OpError> {
        self.apply(AtomicOp::Pon {
            who,
            hand_tile_ids: [two[0].id, two[1].id],
        })
    }

    /// 吃.
    pub fn do_chi(&mut self, who: Seat, two: [Tile; 2]) -> Result<(), OpError> {
        self.apply(AtomicOp::Chi {
            who,
            hand_tile_ids: [two[0].id, two[1].id],
        })
    }

    /// 明杠 + 自动岭上摸.
    pub fn do_minkan(&mut self, who: Seat, three: [Tile; 3]) -> Result<(), OpError> {
        self.apply(AtomicOp::Minkan {
            who,
            hand_tile_ids: [three[0].id, three[1].id, three[2].id],
        })?;
        self.auto_rinshan_if_needed()?;
        Ok(())
    }

    // ──────────────────────────────────────────────────────────
    // 查询: 自摸 / 荣和 / 合法选项
    // ──────────────────────────────────────────────────────────

    pub fn can_tsumo(&self) -> bool {
        legal_ops(&self.round).can_tsumo
    }

    /// 获取自摸 score (不应用). engine 没有 pure score query, 用 clone + 模拟 round_apply.
    pub fn try_tsumo(&self) -> Option<ScoreResult> {
        let (next, _) = round_apply(&self.round, AtomicOp::Tsumo).ok()?;
        match next.result()? {
            RoundResult::Win { score, .. } => Some(score.clone()),
            _ => None,
        }
    }

    /// 自摸宣告. score 参数兼容 legacy API; 实际从 round_apply 的 RoundEnd 取.
    pub fn declare_tsumo(&mut self, _score: ScoreResult) {
        self.apply(AtomicOp::Tsumo)
            .expect("declare_tsumo: round_apply Tsumo should succeed (caller must try_tsumo first)");
    }

    pub fn can_ron(&self, who: Seat) -> bool {
        let ops = legal_ops(&self.round);
        ops.calls[who.index()].ron
    }

    pub fn try_ron(&self, who: Seat) -> Option<ScoreResult> {
        let (next, _) = round_apply(&self.round, AtomicOp::Ron { who }).ok()?;
        match next.result()? {
            RoundResult::Win { score, .. } => Some(score.clone()),
            _ => None,
        }
    }

    pub fn declare_ron(&mut self, who: Seat, _score: ScoreResult) {
        self.apply(AtomicOp::Ron { who })
            .expect("declare_ron: round_apply Ron should succeed (caller must try_ron first)");
    }

    /// 推进 turn. legacy 在 AwaitCalls 阶段无人响应时调; 在 engine 里这就是 Pass.
    /// 其它 phase 调 advance_turn 是 noop (engine 内 do_discard 等已自动转 phase).
    pub fn advance_turn(&mut self) {
        if matches!(self.round, RoundState::AwaitCalls(_)) {
            self.apply(AtomicOp::Pass)
                .expect("advance_turn: AwaitCalls Pass 永远合法");
        }
    }

    /// 推进到下一局. legacy 内部含 GameEnd 判定 + dealer/honba/kyoku 推进 +
    /// phase 转 Phase::Deal. engine 用 match_apply 完成前两件.
    pub fn next_round(&mut self) {
        let outcome = summarize_round(&self.round)
            .expect("next_round: 必须先到 RoundEnd 才能推进");
        self.mat = match_apply(&self.mat, outcome);
        // last_result 不清, 留给 phase() 判 GameEnd. start_round 时清.
    }

    // ──────────────────────────────────────────────────────────
    // legal options — 转换 engine LegalOps 到 legacy CallOptions / SelfOptions
    // ──────────────────────────────────────────────────────────

    pub fn legal_calls(&self, who: Seat) -> CallOptions {
        let ops = legal_ops(&self.round);
        let pc = &ops.calls[who.index()];
        CallOptions {
            pon: pc.pon,
            chi: pc.chi.clone(),
            minkan: pc.minkan,
            ron: pc.ron,
        }
    }

    pub fn legal_self_options(&self) -> SelfOptions {
        let ops = legal_ops(&self.round);
        SelfOptions {
            tsumo: ops.can_tsumo,
            riichi_discards: ops.riichi_discards,
            ankan: ops.ankan,
            shouminkan: ops.shouminkan,
        }
    }
}

// ──────────────────────────────────────────────────────────
// CallOptions / SelfOptions — 与 legacy 同名同结构, 但定义在本模块
// (UI 不再 import legacy_state, 这俩是它的转换出参).
// ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    pub pon: Option<[Tile; 2]>,
    pub chi: Vec<[Tile; 2]>,
    pub minkan: Option<[Tile; 3]>,
    pub ron: bool,
}

impl CallOptions {
    pub fn any(&self) -> bool {
        self.pon.is_some() || !self.chi.is_empty() || self.minkan.is_some() || self.ron
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelfOptions {
    pub tsumo: bool,
    pub riichi_discards: Vec<Tile>,
    pub ankan: Vec<TileIndex>,
    pub shouminkan: Vec<TileIndex>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::meld::{Meld, MeldKind};
    use crate::engine::round_state::RoundState;
    use crate::engine::rules::LengthRule;

    #[test]
    fn new_engine_initialized_phase_draw() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(0xc0ffee);
        // start_round 后 round 是 AwaitDraw, phase 应为 Phase::Draw.
        assert_eq!(e.phase(), Phase::Draw);
        assert_eq!(e.turn(), Seat::East);
        assert_eq!(e.dealer(), Seat::East);
    }

    #[test]
    fn do_draw_advances_to_await_discard() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(1234);
        let drawn = e.do_draw();
        assert!(drawn.is_some(), "do_draw 应返摸到的牌");
        assert_eq!(e.phase(), Phase::AwaitDiscard);
    }

    #[test]
    fn engine_drives_full_tonpuusen() {
        let rules = GameRules {
            length: LengthRule::Tonpuusen,
            ..GameRules::default()
        };
        let mut e = GameEngine::new(rules);
        e.start_round(0xdead);
        let mut steps = 0;
        loop {
            steps += 1;
            assert!(steps < 5000, "整庄不应超过 5000 步");
            match e.phase() {
                Phase::Deal => {
                    e.start_round(steps as u64);
                }
                Phase::Draw => {
                    if e.do_draw().is_none() && e.phase() != Phase::RoundEnd {
                        panic!("do_draw None 但未进 RoundEnd");
                    }
                }
                Phase::AwaitDiscard => {
                    if e.can_tsumo() {
                        let s = e.try_tsumo().unwrap();
                        e.declare_tsumo(s);
                        continue;
                    }
                    let turn = e.turn();
                    let last = e.players()[turn.index()].last_drawn;
                    let t = match last {
                        Some(t) => t,
                        None => {
                            // 鸣牌后无 last_drawn, 切第一张.
                            e.players()[turn.index()].hand.closed[0]
                        }
                    };
                    e.do_discard(t).unwrap();
                }
                Phase::AwaitCalls => {
                    let mut roned = false;
                    for who in Seat::ALL {
                        if e.can_ron(who) {
                            let s = e.try_ron(who).unwrap();
                            e.declare_ron(who, s);
                            roned = true;
                            break;
                        }
                    }
                    if !roned {
                        e.advance_turn();
                    }
                }
                Phase::RoundEnd => {
                    e.next_round();
                    if e.mat.ended {
                        break;
                    }
                    e.start_round(steps as u64);
                }
                Phase::GameEnd => {
                    break;
                }
            }
        }
        let total: i32 = e.mat.scores.iter().sum();
        assert_eq!(total, 100_000, "tonpuusen 整庄分数守恒");
    }

    // ===== accessor 全覆盖 =====

    #[test]
    fn accessors_return_consistent_with_match_state() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(0xc0ffee);
        assert_eq!(e.round_wind(), crate::engine::round_state::RoundWind::East);
        assert_eq!(e.kyoku(), 1);
        assert_eq!(e.honba(), 0);
        assert_eq!(e.riichi_sticks(), 0);
        assert_eq!(e.dealer(), Seat::East);
        assert_eq!(e.rules().length, LengthRule::Hanchan);
        assert!(e.last_discard().is_none());
        assert_eq!(e.seat_wind_of(Seat::East), TileIndex::EAST);
        assert_eq!(e.seat_wind_of(Seat::South), TileIndex::SOUTH);
        assert!(e.wall_remaining() > 0);
        assert!(!e.dora_indicators().is_empty());
        assert!(e.wall().is_some());
    }

    // ===== phase 各路径 =====

    #[test]
    fn phase_maps_riichi_discard_to_await_discard() {
        // RoundState::AwaitRiichiDiscard → Phase::AwaitDiscard.
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        // 黑魔法把 round 替成 AwaitRiichiDiscard.
        let common = e.round.common().clone();
        let last = e.round.last_drawn().unwrap();
        e.round = RoundState::AwaitRiichiDiscard(crate::engine::round_state::AwaitRiichiDiscardState {
            common,
            turn: Seat::East,
            last_drawn: last,
        });
        assert_eq!(e.phase(), Phase::AwaitDiscard);
    }

    #[test]
    fn phase_maps_rinshan_draw_to_draw() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        let common = e.round.common().clone();
        e.round = RoundState::AwaitRinshanDraw(crate::engine::round_state::AwaitRinshanDrawState {
            common,
            turn: Seat::East,
        });
        assert_eq!(e.phase(), Phase::Draw);
    }

    // ===== do_* 各方法直接覆盖 =====

    #[test]
    fn do_riichi_decreases_score_and_increments_pool() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        // 黑魔法替闭手为 14 张听牌型 (含 last_drawn).
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            let hand = vec![
                Tile { kind: TileIndex(1), red: false, id: 100 },
                Tile { kind: TileIndex(2), red: false, id: 101 },
                Tile { kind: TileIndex(3), red: false, id: 102 },
                Tile { kind: TileIndex(10), red: false, id: 103 },
                Tile { kind: TileIndex(11), red: false, id: 104 },
                Tile { kind: TileIndex(12), red: false, id: 105 },
                Tile { kind: TileIndex(19), red: false, id: 106 },
                Tile { kind: TileIndex(20), red: false, id: 107 },
                Tile { kind: TileIndex(21), red: false, id: 108 },
                Tile { kind: TileIndex(22), red: false, id: 109 },
                Tile { kind: TileIndex(23), red: false, id: 110 },
                Tile { kind: TileIndex(24), red: false, id: 111 },
                Tile { kind: TileIndex(8), red: false, id: 112 },
                Tile { kind: TileIndex(8), red: false, id: 113 },
            ];
            s.common.players[Seat::East.index()].hand.closed = hand;
            s.last_drawn = Some(Tile { kind: TileIndex(8), red: false, id: 113 });
            s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
        }
        let drawn = e.round.last_drawn().unwrap();
        e.do_riichi(drawn).expect("立直应成功");
        assert!(e.players()[Seat::East.index()].riichi);
        assert_eq!(e.players()[Seat::East.index()].score, 24000);
        // riichi_sticks_pool 在 round 的 common 内 +1; mat.pool 等 next_round 回写.
        assert_eq!(e.round.common().riichi_sticks_pool, 1);
    }

    #[test]
    fn do_ankan_creates_meld_and_advances_to_rinshan() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        // 替闭手为 4 张 1m + 9 其它 + last_drawn = 1m.
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            let mut hand = vec![
                Tile { kind: TileIndex(0), red: false, id: 200 },
                Tile { kind: TileIndex(0), red: false, id: 201 },
                Tile { kind: TileIndex(0), red: false, id: 202 },
                Tile { kind: TileIndex(0), red: false, id: 203 },
            ];
            for k in 1..=10u8 {
                hand.push(Tile { kind: TileIndex(k), red: false, id: 210 + k as u16 });
            }
            s.common.players[Seat::East.index()].hand.closed = hand;
            s.last_drawn = Some(Tile { kind: TileIndex(0), red: false, id: 203 });
            s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
        }
        e.do_ankan(TileIndex(0)).expect("暗杠应成功");
        // do_ankan 内部走 auto_rinshan_if_needed → RinshanDraw apply → 转 AwaitDiscard.
        assert_eq!(e.phase(), Phase::AwaitDiscard);
        assert_eq!(e.players()[Seat::East.index()].hand.melds.len(), 1);
    }

    #[test]
    fn do_pon_chi_minkan_from_await_calls() {
        // 测试 do_pon / do_chi / do_minkan 三个方法跑同一份 fixture: 切牌后 South 鸣.
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let pon_tile = Tile { kind: TileIndex(13), red: false, id: 300 };
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            s.common.players[Seat::East.index()].hand.closed.push(pon_tile);
            s.last_drawn = Some(pon_tile);
            s.common.players[Seat::East.index()].last_drawn = Some(pon_tile);
            // South 手中插 2 张 5p 备 Pon.
            s.common.players[Seat::South.index()].hand.closed.push(Tile {
                kind: TileIndex(13), red: false, id: 301,
            });
            s.common.players[Seat::South.index()].hand.closed.push(Tile {
                kind: TileIndex(13), red: false, id: 302,
            });
        }
        e.do_discard(pon_tile).expect("切 5p");
        assert_eq!(e.phase(), Phase::AwaitCalls);
        // 测 legal_calls.
        let opts = e.legal_calls(Seat::South);
        assert!(opts.pon.is_some());
        // do_pon.
        e.do_pon(
            Seat::South,
            [
                Tile { kind: TileIndex(13), red: false, id: 301 },
                Tile { kind: TileIndex(13), red: false, id: 302 },
            ],
        )
        .expect("do_pon 应成功");
        assert_eq!(e.turn(), Seat::South);
        assert_eq!(e.phase(), Phase::AwaitDiscard);
    }

    #[test]
    fn do_shouminkan_upgrades_existing_pon() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            s.common.players[Seat::East.index()].hand.melds.push(Meld {
                kind: MeldKind::Pon {
                    tiles: [
                        Tile { kind: TileIndex(13), red: false, id: 400 },
                        Tile { kind: TileIndex(13), red: false, id: 401 },
                        Tile { kind: TileIndex(13), red: false, id: 402 },
                    ],
                },
                from: Some(Seat::West),
            });
            s.common.players[Seat::East.index()].hand.closed.push(Tile {
                kind: TileIndex(13), red: false, id: 403,
            });
            s.last_drawn = Some(Tile { kind: TileIndex(13), red: false, id: 403 });
            s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
        }
        e.do_shouminkan(TileIndex(13)).expect("加杠应成功");
        // do_shouminkan → auto_rinshan → AwaitDiscard.
        assert_eq!(e.phase(), Phase::AwaitDiscard);
    }

    // ===== try_*/declare_*/can_* 系列 =====

    #[test]
    fn try_tsumo_and_can_tsumo_and_declare() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        // 把 East 替成国士单役满型, winning=1m.
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            let mut hand = Vec::new();
            let mut id = 500u16;
            for &k in &[0u8, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33] {
                hand.push(Tile { kind: TileIndex(k), red: false, id });
                id += 1;
            }
            hand.push(Tile { kind: TileIndex(0), red: false, id });
            s.common.players[Seat::East.index()].hand.closed = hand;
            s.last_drawn = Some(Tile { kind: TileIndex(0), red: false, id });
            s.common.players[Seat::East.index()].last_drawn = s.last_drawn;
        }
        assert!(e.can_tsumo());
        let score = e.try_tsumo().expect("应能算 score");
        e.declare_tsumo(score);
        assert!(matches!(e.round, RoundState::RoundEnd(_)));
    }

    #[test]
    fn try_ron_can_ron_and_declare_ron() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let ron_tile = Tile { kind: TileIndex(8), red: false, id: 700 };
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            s.common.players[Seat::East.index()].hand.closed.push(ron_tile);
            s.last_drawn = Some(ron_tile);
            s.common.players[Seat::East.index()].last_drawn = Some(ron_tile);
            // South 闭手 13 张国士型, ron 9m.
            let mut south_hand = Vec::new();
            let mut id = 701u16;
            for &k in &[0u8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33] {
                south_hand.push(Tile { kind: TileIndex(k), red: false, id });
                id += 1;
            }
            south_hand.push(Tile { kind: TileIndex(0), red: false, id }); // 1m 雀头
            s.common.players[Seat::South.index()].hand.closed = south_hand;
        }
        e.do_discard(ron_tile).expect("切 9m");
        assert!(e.can_ron(Seat::South));
        let score = e.try_ron(Seat::South).expect("应能 ron");
        e.declare_ron(Seat::South, score);
        assert!(matches!(e.round, RoundState::RoundEnd(_)));
    }

    // ===== Phase::GameEnd 路径 =====

    #[test]
    fn phase_returns_game_end_when_mat_ended_with_result() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(0);
        // 黑魔法直接设 mat.ended + last_result.
        e.mat.ended = true;
        e.last_result = Some(crate::engine::round_state::RoundResult::Ryuukyoku {
            kind: crate::engine::round_state::RyuukyokuKind::Howaipai,
        });
        assert_eq!(e.phase(), Phase::GameEnd);
    }

    // ===== legal_self_options =====

    #[test]
    fn legal_self_options_returns_structured() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let opts = e.legal_self_options();
        // 应该不 panic, riichi_discards 0..=14.
        assert!(opts.riichi_discards.len() <= 14);
    }

    // ===== 错误返回路径 + do_chi/do_minkan 直调 =====

    #[test]
    fn try_tsumo_returns_none_when_not_winning() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        // East 起手十有八九不能自摸 → try_tsumo 返 None.
        // (即使偶尔能, can_tsumo 也跟着真; 测试只验"接口语义一致").
        if !e.can_tsumo() {
            assert!(e.try_tsumo().is_none());
        }
    }

    #[test]
    fn try_ron_returns_none_when_not_winning() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let t = e.round.last_drawn().unwrap();
        e.do_discard(t).expect("切牌");
        // 此时 phase=AwaitCalls. South 通常不能 ron → try_ron None.
        if !e.can_ron(Seat::South) {
            assert!(e.try_ron(Seat::South).is_none());
        }
    }

    #[test]
    fn do_chi_creates_chi_meld() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let chi_tile = Tile { kind: TileIndex(2), red: false, id: 1000 };
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            s.common.players[Seat::East.index()].hand.closed.push(chi_tile);
            s.last_drawn = Some(chi_tile);
            s.common.players[Seat::East.index()].last_drawn = Some(chi_tile);
            // South 手中加 1m + 2m 备吃 (吃成 1-2-3m).
            s.common.players[Seat::South.index()].hand.closed.push(Tile {
                kind: TileIndex(0), red: false, id: 1001,
            });
            s.common.players[Seat::South.index()].hand.closed.push(Tile {
                kind: TileIndex(1), red: false, id: 1002,
            });
        }
        e.do_discard(chi_tile).expect("切 3m");
        e.do_chi(
            Seat::South,
            [
                Tile { kind: TileIndex(0), red: false, id: 1001 },
                Tile { kind: TileIndex(1), red: false, id: 1002 },
            ],
        )
        .expect("吃应成功");
        assert_eq!(e.turn(), Seat::South);
        assert!(matches!(
            &e.players()[Seat::South.index()].hand.melds[0].kind,
            MeldKind::Chi { .. }
        ));
    }

    #[test]
    fn do_minkan_creates_minkan_meld() {
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        e.do_draw();
        let kan_tile = Tile { kind: TileIndex(24), red: false, id: 1100 };
        if let RoundState::AwaitDiscard(s) = &mut e.round {
            s.common.players[Seat::East.index()].hand.closed.push(kan_tile);
            s.last_drawn = Some(kan_tile);
            s.common.players[Seat::East.index()].last_drawn = Some(kan_tile);
            for id in 1101..=1103 {
                s.common.players[Seat::South.index()].hand.closed.push(Tile {
                    kind: TileIndex(24), red: false, id,
                });
            }
        }
        e.do_discard(kan_tile).expect("切 7s");
        e.do_minkan(
            Seat::South,
            [
                Tile { kind: TileIndex(24), red: false, id: 1101 },
                Tile { kind: TileIndex(24), red: false, id: 1102 },
                Tile { kind: TileIndex(24), red: false, id: 1103 },
            ],
        )
        .expect("明杠应成功");
        assert!(matches!(
            &e.players()[Seat::South.index()].hand.melds[0].kind,
            MeldKind::Minkan { .. }
        ));
    }

    #[test]
    fn do_draw_returns_none_at_round_end() {
        // engine 已转 RoundEnd 时 do_draw 应返 None.
        let mut e = GameEngine::new(GameRules::default());
        e.start_round(42);
        // 黑魔法直接转 RoundEnd.
        let common = e.round.common().clone();
        e.round = RoundState::RoundEnd(crate::engine::round_state::RoundEndState {
            common,
            result: crate::engine::round_state::RoundResult::Ryuukyoku {
                kind: crate::engine::round_state::RyuukyokuKind::Howaipai,
            },
        });
        // round_apply Draw 在 RoundEnd 返 AlreadyEnded → do_draw None.
        assert!(e.do_draw().is_none());
    }
}
