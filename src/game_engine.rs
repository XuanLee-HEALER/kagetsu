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
}
