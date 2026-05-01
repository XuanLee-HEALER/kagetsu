//! 游戏状态机.
//!
//! MVP+ 版本: 配牌 → 摸切 → 鸣牌(碰/吃/杠) → 立直 → 自摸/荣和 → 流局 → 下一局.
//! 简化: 振听不强制(能和就和), 多家荣和按头跳, AI 不主动鸣牌(只荣和).
//! 详见 docs/spec/game-flow.md

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::config::GameConfig;
use crate::decompose::decompose;
use crate::hand::Hand;
use crate::meld::{Meld, MeldKind, Seat};
use crate::score::{PaymentDistribution, ScoreResult, distribute, evaluate};
use crate::tile::{Tile, TileIndex, count_by_kind};
use crate::wall::Wall;
use crate::yaku::WinContext;

/// 局内动作事件, 给 UI 渲染最近动作日志使用.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameEvent {
    Draw { who: Seat, tile: Tile },
    Discard { who: Seat, tile: Tile },
    Pon { who: Seat, tile: Tile },
    Chi { who: Seat, tile: Tile },
    Minkan { who: Seat, tile: Tile },
    Ankan { who: Seat, kind: TileIndex },
    Shouminkan { who: Seat, kind: TileIndex },
    Riichi { who: Seat, tile: Tile },
    Tsumo { who: Seat },
    Ron { who: Seat, from: Seat },
}

const MAX_EVENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// 配牌中.
    Deal,
    /// 等当前家摸牌.
    Draw,
    /// 当前家已摸,等切牌(玩家由 UI 选择, AI 自动决定).
    AwaitDiscard,
    /// 切牌后,等他家(非自家)是否荣和.
    AwaitCalls,
    /// 一局结算,展示结果.
    RoundEnd,
    /// 整场终局.
    GameEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundWind {
    East,
    South,
    West,
    North,
}

impl RoundWind {
    pub fn tile(self) -> TileIndex {
        match self {
            RoundWind::East => TileIndex::EAST,
            RoundWind::South => TileIndex::SOUTH,
            RoundWind::West => TileIndex::WEST,
            RoundWind::North => TileIndex::NORTH,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RoundWind::East => "东",
            RoundWind::South => "南",
            RoundWind::West => "西",
            RoundWind::North => "北",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub seat: Seat,
    pub hand: Hand,
    pub river: Vec<Tile>,
    pub score: i32,
    pub riichi: bool,
    pub double_riichi: bool,
    pub ippatsu_active: bool,
    pub last_drawn: Option<Tile>,
}

impl PlayerState {
    pub fn new(seat: Seat, score: i32) -> Self {
        Self {
            seat,
            hand: Hand::new(),
            river: Vec::new(),
            score,
            riichi: false,
            double_riichi: false,
            ippatsu_active: false,
            last_drawn: None,
        }
    }

    pub fn reset_round(&mut self) {
        self.hand = Hand::new();
        self.river.clear();
        self.riichi = false;
        self.double_riichi = false;
        self.ippatsu_active = false;
        self.last_drawn = None;
    }

    /// 返回 13 (含暗杠时仍为 13 + 杠的 1 张) 或 14 (摸牌后).
    pub fn closed_count(&self) -> usize {
        self.hand.closed.len()
    }
}

#[derive(Debug, Clone)]
pub enum RoundResult {
    Win {
        winner: Seat,
        is_tsumo: bool,
        loser: Option<Seat>,
        score: ScoreResult,
        payments: Vec<PaymentDistribution>,
    },
    Ryuukyoku {
        kind: RyuukyokuKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RyuukyokuKind {
    Howaipai,
    NoYaku,
}

/// 某家对最近弃牌的合法响应选项.
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    pub pon: Option<[Tile; 2]>,
    /// 多种吃方案; tile1 是顺子三张中除被吃牌之外的两张.
    pub chi: Vec<[Tile; 2]>,
    pub minkan: Option<[Tile; 3]>,
    pub ron: bool,
}

impl CallOptions {
    pub fn any(&self) -> bool {
        self.pon.is_some() || !self.chi.is_empty() || self.minkan.is_some() || self.ron
    }
}

/// 当前家(turn)在 AwaitDiscard 阶段可主动宣言的动作.
#[derive(Debug, Clone, Default)]
pub struct SelfOptions {
    pub tsumo: bool,
    /// 切哪几张可立直成立(去重 by kind).
    pub riichi_discards: Vec<Tile>,
    pub ankan: Vec<TileIndex>,
    pub shouminkan: Vec<TileIndex>,
}

pub struct GameState {
    pub config: GameConfig,
    pub round_wind: RoundWind,
    pub kyoku: u8, // 1..=4
    pub honba: u8,
    pub riichi_sticks: u8,
    pub players: [PlayerState; 4],
    pub wall: Option<Wall>,
    pub dealer: Seat,
    pub turn: Seat,
    pub phase: Phase,
    pub last_discard: Option<(Seat, Tile)>,
    pub last_result: Option<RoundResult>,
    /// 当前局的种子(便于复现).
    pub round_seed: u64,
    /// 第一巡是否仍未被打断(用于天和/地和等).
    pub first_go_around: bool,
    /// 最近动作事件 (UI 用), 最多 MAX_EVENTS 条 (新事件 push_back).
    pub events: VecDeque<GameEvent>,
}

impl GameState {
    pub fn new(config: GameConfig) -> Self {
        let starting = config.starting_score;
        Self {
            config,
            round_wind: RoundWind::East,
            kyoku: 1,
            honba: 0,
            riichi_sticks: 0,
            players: [
                PlayerState::new(Seat::East, starting),
                PlayerState::new(Seat::South, starting),
                PlayerState::new(Seat::West, starting),
                PlayerState::new(Seat::North, starting),
            ],
            wall: None,
            dealer: Seat::East,
            turn: Seat::East,
            phase: Phase::Deal,
            last_discard: None,
            last_result: None,
            round_seed: 0,
            first_go_around: true,
            events: VecDeque::new(),
        }
    }

    fn push_event(&mut self, ev: GameEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(ev);
    }

    pub fn start_round(&mut self, seed: u64) {
        self.round_seed = seed;
        self.last_discard = None;
        self.last_result = None;
        self.events.clear();
        self.first_go_around = true;

        for p in self.players.iter_mut() {
            p.reset_round();
        }

        let mut wall = Wall::shuffled(seed, self.config.aka_dora);
        // 配牌: 每家 13 张.
        for _ in 0..13 {
            for seat in Seat::ALL {
                if let Some(t) = wall.draw() {
                    self.players[seat.index()].hand.closed.push(t);
                }
            }
        }
        self.wall = Some(wall);
        self.turn = self.dealer;
        self.phase = Phase::Draw;

        // 排序手牌方便显示.
        for p in self.players.iter_mut() {
            sort_hand(&mut p.hand.closed);
        }
    }

    /// 测试 / replay 用: 用预设手牌 + 牌山顺序起一局, 跳过随机洗牌.
    ///
    /// `initial_hands[i]` 是 [`Seat::ALL`][i] 的开局 13 张手牌.
    /// `live_wall` 顺序按摸牌顺序 (第一张摸的在 `live_wall[0]`).
    /// `dead_wall` 必须 14 张, 索引约定见 [`Wall::from_components`].
    /// `dora_revealed` ∈ \[1, 5\], 默认 1.
    ///
    /// 调用前应已设置 `dealer / round_wind / kyoku / honba / riichi_sticks /
    /// players[].score`.
    pub fn start_round_with_state(
        &mut self,
        initial_hands: [Vec<Tile>; 4],
        live_wall: Vec<Tile>,
        dead_wall: Vec<Tile>,
        dora_revealed: usize,
    ) {
        self.round_seed = 0;
        self.last_discard = None;
        self.last_result = None;
        self.events.clear();
        self.first_go_around = true;

        for p in self.players.iter_mut() {
            p.reset_round();
        }

        // 注入手牌
        for (i, hand) in initial_hands.into_iter().enumerate() {
            assert_eq!(hand.len(), 13, "seat {i} initial_hands 必须 13 张");
            self.players[i].hand.closed = hand;
        }

        // Wall::from_components 的 live 顺序与摸牌顺序相反 (pop 从尾部),
        // 所以这里把 live_wall reverse 进 Wall.
        let mut live_reversed = live_wall;
        live_reversed.reverse();
        let wall = Wall::from_components(live_reversed, dead_wall, dora_revealed);
        self.wall = Some(wall);

        self.turn = self.dealer;
        self.phase = Phase::Draw;

        for p in self.players.iter_mut() {
            sort_hand(&mut p.hand.closed);
        }
    }

    /// 当前家摸牌,转入 AwaitDiscard.
    /// 返回摸到的牌(如山牌已尽返回 None,触发流局).
    pub fn do_draw(&mut self) -> Option<Tile> {
        debug_assert_eq!(self.phase, Phase::Draw);
        let wall = self.wall.as_mut()?;
        let t = wall.draw()?;
        let seat = self.turn;
        self.players[seat.index()].hand.closed.push(t);
        sort_hand(&mut self.players[seat.index()].hand.closed);
        self.players[seat.index()].last_drawn = Some(t);
        self.phase = Phase::AwaitDiscard;
        self.push_event(GameEvent::Draw { who: seat, tile: t });
        Some(t)
    }

    /// 当前家自摸和牌检测.
    pub fn try_tsumo(&self) -> Option<ScoreResult> {
        let seat = self.turn;
        let p = &self.players[seat.index()];
        let last = p.last_drawn?;
        let counts = count_by_kind(&p.hand.closed);
        let r = decompose(&counts, &p.hand.melds, last.kind);
        if r.is_empty() {
            return None;
        }
        // 选第一个拆解,后续可优化为选最高分.
        let menzen = p.hand.is_menzen();
        let fully = p.hand.is_fully_concealed();
        let ctx = WinContext {
            decomposition: &r[0],
            seat_wind: self.seat_wind_of(seat),
            round_wind: self.round_wind.tile(),
            winning_tile: last.kind,
            is_tsumo: true,
            is_riichi: p.riichi,
            is_double_riichi: p.double_riichi,
            is_ippatsu: p.ippatsu_active,
            is_haitei: self.wall.as_ref().map(|w| w.remaining()).unwrap_or(0) == 0,
            is_houtei: false,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: self.first_go_around && seat == self.dealer,
            is_chiihou: self.first_go_around && seat != self.dealer,
            is_renhou: false,
            menzen,
            fully_concealed: fully,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            config: &self.config,
        };
        evaluate(&ctx, &p.hand.melds)
    }

    /// 检测某家(非当前切牌家)能否对 last_discard 荣和.
    pub fn try_ron(&self, who: Seat) -> Option<ScoreResult> {
        let (from, tile) = self.last_discard?;
        if from == who {
            return None;
        }
        let p = &self.players[who.index()];
        let mut counts = count_by_kind(&p.hand.closed);
        counts[tile.kind.0 as usize] += 1;
        let r = decompose(&counts, &p.hand.melds, tile.kind);
        if r.is_empty() {
            return None;
        }
        // 振听简化: 暂不强制.
        let menzen = p.hand.is_menzen();
        let fully = p.hand.is_fully_concealed();
        let ctx = WinContext {
            decomposition: &r[0],
            seat_wind: self.seat_wind_of(who),
            round_wind: self.round_wind.tile(),
            winning_tile: tile.kind,
            is_tsumo: false,
            is_riichi: p.riichi,
            is_double_riichi: p.double_riichi,
            is_ippatsu: p.ippatsu_active,
            is_haitei: false,
            is_houtei: self.wall.as_ref().map(|w| w.remaining()).unwrap_or(0) == 0,
            is_rinshan: false,
            is_chankan: false,
            is_tenhou: false,
            is_chiihou: false,
            is_renhou: self.first_go_around && who != self.dealer,
            menzen,
            fully_concealed: fully,
            dora_count: 0,
            aka_count: 0,
            ura_dora_count: 0,
            config: &self.config,
        };
        evaluate(&ctx, &p.hand.melds)
    }

    /// 当前家弃一张牌(必须是其手中存在的某张).
    /// 进入 AwaitCalls 等他家荣和.
    pub fn do_discard(&mut self, tile: Tile) -> Result<(), &'static str> {
        if self.phase != Phase::AwaitDiscard {
            return Err("not awaiting discard");
        }
        let seat = self.turn;
        let p = &mut self.players[seat.index()];
        let pos = p
            .hand
            .closed
            .iter()
            .position(|t| t.id == tile.id)
            .ok_or("tile not in hand")?;
        let removed = p.hand.closed.remove(pos);
        p.river.push(removed);
        p.last_drawn = None;
        self.last_discard = Some((seat, removed));
        // 一发判定: 任何鸣牌(包括弃牌后被鸣)使一发失效, MVP 暂不鸣牌, 但弃自家牌后清自己一发.
        p.ippatsu_active = false;
        sort_hand(&mut p.hand.closed);
        self.phase = Phase::AwaitCalls;
        self.push_event(GameEvent::Discard {
            who: seat,
            tile: removed,
        });
        Ok(())
    }

    /// 完成 AwaitCalls 阶段, 推进到下一家摸牌.
    pub fn advance_turn(&mut self) {
        self.turn = self.turn.next();
        // 如果回到了起家, 第一巡结束.
        if self.turn == self.dealer {
            self.first_go_around = false;
        }
        // 山摸尽后, 进入流局判断.
        let remaining = self.wall.as_ref().map(|w| w.remaining()).unwrap_or(0);
        if remaining == 0 {
            self.phase = Phase::RoundEnd;
            self.last_result = Some(RoundResult::Ryuukyoku {
                kind: RyuukyokuKind::Howaipai,
            });
            return;
        }
        self.phase = Phase::Draw;
    }

    /// 宣告某家自摸和牌, 写入结算.
    pub fn declare_tsumo(&mut self, score: ScoreResult) {
        let winner = self.turn;
        let payments = distribute(
            &score,
            winner,
            self.dealer,
            true,
            None,
            self.honba as u32,
            self.riichi_sticks as u32,
        );
        self.apply_payments(&payments);
        self.last_result = Some(RoundResult::Win {
            winner,
            is_tsumo: true,
            loser: None,
            score,
            payments,
        });
        self.riichi_sticks = 0;
        self.phase = Phase::RoundEnd;
        self.push_event(GameEvent::Tsumo { who: winner });
    }

    /// 宣告某家荣和.
    pub fn declare_ron(&mut self, who: Seat, score: ScoreResult) {
        let loser = self.last_discard.map(|(s, _)| s);
        let payments = distribute(
            &score,
            who,
            self.dealer,
            false,
            loser,
            self.honba as u32,
            self.riichi_sticks as u32,
        );
        self.apply_payments(&payments);
        self.last_result = Some(RoundResult::Win {
            winner: who,
            is_tsumo: false,
            loser,
            score,
            payments,
        });
        self.riichi_sticks = 0;
        self.phase = Phase::RoundEnd;
        if let Some(from) = loser {
            self.push_event(GameEvent::Ron { who, from });
        }
    }

    fn apply_payments(&mut self, payments: &[PaymentDistribution]) {
        for p in payments {
            if p.from != p.to {
                self.players[p.from.index()].score -= p.amount;
            }
            self.players[p.to.index()].score += p.amount;
        }
    }

    /// 推进到下一局: 处理连庄/局推进/场风切换/终局判定.
    pub fn next_round(&mut self) {
        let dealer_won = matches!(
            &self.last_result,
            Some(RoundResult::Win { winner, .. }) if *winner == self.dealer
        );
        let is_ryuukyoku = matches!(&self.last_result, Some(RoundResult::Ryuukyoku { .. }));

        if dealer_won {
            self.honba += 1;
        } else if is_ryuukyoku {
            // 流局: 本场总 +1; 亲家听牌则连庄, 不听牌则下庄.
            self.honba += 1;
            let dealer_p = &self.players[self.dealer.index()];
            let counts = count_by_kind(&dealer_p.hand.closed);
            let dealer_tenpai =
                !crate::decompose::tenpai_tiles(&counts, &dealer_p.hand.melds).is_empty();
            if !dealer_tenpai {
                self.advance_kyoku();
            }
        } else {
            self.honba = 0;
            self.advance_kyoku();
        }

        // advance_kyoku 触发整庄结束时, 不要覆盖 phase.
        if self.phase != Phase::GameEnd {
            self.phase = Phase::Deal;
        }
    }

    fn advance_kyoku(&mut self) {
        self.dealer = self.dealer.next();
        if self.dealer == Seat::East {
            // 一圈结束, 推场风.
            self.round_wind = match self.round_wind {
                RoundWind::East => {
                    // 东风战: 东 4 完即结束; 半庄战: 东 4 完进南风.
                    if matches!(self.config.length, crate::config::LengthRule::Tonpuusen) {
                        self.phase = Phase::GameEnd;
                        return;
                    }
                    RoundWind::South
                }
                RoundWind::South => {
                    self.phase = Phase::GameEnd;
                    return;
                }
                _ => RoundWind::East,
            };
            self.kyoku = 1;
        } else {
            self.kyoku += 1;
        }
    }

    pub fn seat_wind_of(&self, s: Seat) -> TileIndex {
        // 自风以亲家相对位置决定: 亲家=东,下家=南,对家=西,上家=北.
        let offset = (s.index() + 4 - self.dealer.index()) % 4;
        match offset {
            0 => TileIndex::EAST,
            1 => TileIndex::SOUTH,
            2 => TileIndex::WEST,
            _ => TileIndex::NORTH,
        }
    }

    /// 当前家是否能自摸和.
    pub fn can_tsumo(&self) -> bool {
        self.try_tsumo().is_some()
    }

    /// 某家(非当前家)能否对最近弃牌荣和.
    pub fn can_ron(&self, who: Seat) -> bool {
        self.try_ron(who).is_some()
    }

    /// 当前家是否听牌(仅用于显示提示).
    pub fn current_player_tenpai_tiles(&self) -> Vec<TileIndex> {
        let p = &self.players[self.turn.index()];
        let counts = count_by_kind(&p.hand.closed);
        // 13 张才听牌; 若是 14 张(刚摸), 需要先尝试切一张
        if p.hand.closed.len() == 13 {
            crate::decompose::tenpai_tiles(&counts, &p.hand.melds)
        } else {
            Vec::new()
        }
    }

    /// 是否听牌,假设切某张.
    pub fn tenpai_after_discard(&self, seat: Seat, discard: Tile) -> bool {
        let p = &self.players[seat.index()];
        let mut counts = count_by_kind(&p.hand.closed);
        if counts[discard.kind.0 as usize] == 0 {
            return false;
        }
        counts[discard.kind.0 as usize] -= 1;
        !crate::decompose::tenpai_tiles(&counts, &p.hand.melds).is_empty()
    }

    /// 列出某家对最近弃牌的合法鸣牌选项(碰/吃/明杠/荣和).
    pub fn legal_calls(&self, who: Seat) -> CallOptions {
        let mut opts = CallOptions::default();
        let Some((from, tile)) = self.last_discard else {
            return opts;
        };
        if from == who {
            return opts;
        }
        let p = &self.players[who.index()];

        // 立直后只能荣和.
        if p.riichi {
            if self.try_ron(who).is_some() {
                opts.ron = true;
            }
            return opts;
        }

        let counts = count_by_kind(&p.hand.closed);
        let kind = tile.kind;
        let kind_idx = kind.0 as usize;

        // 碰: 自手有 2 张同种.
        if counts[kind_idx] >= 2 {
            let mut found: Vec<Tile> = Vec::new();
            for t in &p.hand.closed {
                if t.kind == kind && found.len() < 2 {
                    found.push(*t);
                }
            }
            if found.len() == 2 {
                opts.pon = Some([found[0], found[1]]);
            }
        }

        // 明杠: 自手有 3 张同种.
        if counts[kind_idx] >= 3 {
            let mut found: Vec<Tile> = Vec::new();
            for t in &p.hand.closed {
                if t.kind == kind && found.len() < 3 {
                    found.push(*t);
                }
            }
            if found.len() == 3 {
                opts.minkan = Some([found[0], found[1], found[2]]);
            }
        }

        // 吃: 仅可吃上家弃牌(from.next() == who 表示 who 是 from 的下家).
        if from.next() == who && kind.is_suupai() {
            let r = (kind.0 % 9) as i32;
            let suit_base = (kind.0 / 9) as i32 * 9;
            for (a, b) in [(-2i32, -1i32), (-1, 1), (1, 2)] {
                let na = r + a;
                let nb = r + b;
                if !(0..=8).contains(&na) || !(0..=8).contains(&nb) {
                    continue;
                }
                let ka = (suit_base + na) as usize;
                let kb = (suit_base + nb) as usize;
                if counts[ka] > 0 && counts[kb] > 0 {
                    let ta = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.kind.0 as usize == ka)
                        .copied()
                        .unwrap();
                    let tb = p
                        .hand
                        .closed
                        .iter()
                        .find(|t| t.kind.0 as usize == kb)
                        .copied()
                        .unwrap();
                    opts.chi.push([ta, tb]);
                }
            }
        }

        if self.try_ron(who).is_some() {
            opts.ron = true;
        }
        opts
    }

    /// 当前家在 AwaitDiscard 阶段可宣言的自身动作.
    pub fn legal_self_options(&self) -> SelfOptions {
        let mut opts = SelfOptions::default();
        if self.phase != Phase::AwaitDiscard {
            return opts;
        }
        let seat = self.turn;
        let p = &self.players[seat.index()];
        let counts = count_by_kind(&p.hand.closed);

        if self.try_tsumo().is_some() {
            opts.tsumo = true;
        }

        // 立直: 门清 + 听牌 + 点棒 ≥ 1000 + 牌山 ≥ 4.
        if !p.riichi
            && p.hand.is_menzen()
            && p.score >= 1000
            && self.wall.as_ref().map(|w| w.remaining()).unwrap_or(0) >= 4
        {
            let mut seen_kinds: Vec<u8> = Vec::new();
            for tile in &p.hand.closed {
                if seen_kinds.contains(&tile.kind.0) {
                    continue;
                }
                let mut c = counts;
                c[tile.kind.0 as usize] -= 1;
                if !crate::decompose::tenpai_tiles(&c, &p.hand.melds).is_empty() {
                    opts.riichi_discards.push(*tile);
                    seen_kinds.push(tile.kind.0);
                }
            }
        }

        // 暗杠: 自手有 4 张同种(立直后简化为禁止).
        if !p.riichi {
            for k in 0..34u8 {
                if counts[k as usize] == 4 {
                    opts.ankan.push(TileIndex(k));
                }
            }
        }

        // 加杠: 已碰刻子 + 自手第四张(立直后禁止).
        if !p.riichi {
            for meld in &p.hand.melds {
                if let MeldKind::Pon { tiles } = &meld.kind {
                    let kind = tiles[0].kind;
                    if counts[kind.0 as usize] >= 1 {
                        opts.shouminkan.push(kind);
                    }
                }
            }
        }

        opts
    }

    /// 执行碰. 鸣牌后 turn 转给 who, phase = AwaitDiscard.
    pub fn do_pon(&mut self, who: Seat, two: [Tile; 2]) -> Result<(), &'static str> {
        let (from, tile) = self.last_discard.ok_or("no discard")?;
        if from == who {
            return Err("own discard");
        }
        self.remove_from_hand(who, &[two[0].id, two[1].id])?;
        self.players[who.index()].hand.melds.push(Meld {
            kind: MeldKind::Pon {
                tiles: [two[0], two[1], tile],
            },
            from: Some(from),
        });
        self.consume_discard(from, tile);
        self.break_first_round_and_ippatsu();
        self.last_discard = None;
        self.turn = who;
        self.phase = Phase::AwaitDiscard;
        sort_hand(&mut self.players[who.index()].hand.closed);
        self.push_event(GameEvent::Pon { who, tile });
        Ok(())
    }

    /// 执行吃.
    pub fn do_chi(&mut self, who: Seat, two: [Tile; 2]) -> Result<(), &'static str> {
        let (from, tile) = self.last_discard.ok_or("no discard")?;
        if from.next() != who {
            return Err("can only chi from upper");
        }
        // 验证组成顺子.
        let mut three = [tile.kind.0, two[0].kind.0, two[1].kind.0];
        three.sort();
        if !TileIndex(three[0]).is_suupai()
            || three[0] / 9 != three[2] / 9
            || three[1] != three[0] + 1
            || three[2] != three[0] + 2
        {
            return Err("not a valid sequence");
        }
        self.remove_from_hand(who, &[two[0].id, two[1].id])?;
        self.players[who.index()].hand.melds.push(Meld {
            kind: MeldKind::Chi {
                tiles: [two[0], two[1], tile],
            },
            from: Some(from),
        });
        self.consume_discard(from, tile);
        self.break_first_round_and_ippatsu();
        self.last_discard = None;
        self.turn = who;
        self.phase = Phase::AwaitDiscard;
        sort_hand(&mut self.players[who.index()].hand.closed);
        self.push_event(GameEvent::Chi { who, tile });
        Ok(())
    }

    /// 执行明杠. 杠后摸岭上 + 翻 dora.
    pub fn do_minkan(&mut self, who: Seat, three: [Tile; 3]) -> Result<(), &'static str> {
        let (from, tile) = self.last_discard.ok_or("no discard")?;
        if from == who {
            return Err("own discard");
        }
        if !three.iter().all(|t| t.kind == tile.kind) {
            return Err("not same kind");
        }
        self.remove_from_hand(who, &[three[0].id, three[1].id, three[2].id])?;
        self.players[who.index()].hand.melds.push(Meld {
            kind: MeldKind::Minkan {
                tiles: [three[0], three[1], three[2], tile],
            },
            from: Some(from),
        });
        self.consume_discard(from, tile);
        self.break_first_round_and_ippatsu();
        self.last_discard = None;
        self.turn = who;
        self.kan_draw_and_reveal(who);
        self.phase = Phase::AwaitDiscard;
        self.push_event(GameEvent::Minkan { who, tile });
        Ok(())
    }

    /// 执行暗杠.
    pub fn do_ankan(&mut self, kind: TileIndex) -> Result<(), &'static str> {
        if self.phase != Phase::AwaitDiscard {
            return Err("not at discard phase");
        }
        let seat = self.turn;
        let four: Vec<Tile> = self.players[seat.index()]
            .hand
            .closed
            .iter()
            .filter(|t| t.kind == kind)
            .copied()
            .collect();
        if four.len() != 4 {
            return Err("need 4 tiles");
        }
        self.players[seat.index()]
            .hand
            .closed
            .retain(|t| t.kind != kind);
        self.players[seat.index()].hand.melds.push(Meld {
            kind: MeldKind::Ankan {
                tiles: [four[0], four[1], four[2], four[3]],
            },
            from: None,
        });
        self.break_first_round_and_ippatsu();
        self.kan_draw_and_reveal(seat);
        self.push_event(GameEvent::Ankan { who: seat, kind });
        Ok(())
    }

    /// 执行加杠(小明杠).
    pub fn do_shouminkan(&mut self, kind: TileIndex) -> Result<(), &'static str> {
        if self.phase != Phase::AwaitDiscard {
            return Err("not at discard phase");
        }
        let seat = self.turn;
        let meld_pos = self.players[seat.index()]
            .hand
            .melds
            .iter()
            .position(|m| matches!(&m.kind, MeldKind::Pon { tiles } if tiles[0].kind == kind))
            .ok_or("no pon for this kind")?;
        let fourth_pos = self.players[seat.index()]
            .hand
            .closed
            .iter()
            .position(|t| t.kind == kind)
            .ok_or("no 4th tile")?;
        let fourth = self.players[seat.index()].hand.closed.remove(fourth_pos);
        let from = self.players[seat.index()].hand.melds[meld_pos].from;
        let tiles = match &self.players[seat.index()].hand.melds[meld_pos].kind {
            MeldKind::Pon { tiles } => *tiles,
            _ => return Err("internal: not pon"),
        };
        self.players[seat.index()].hand.melds[meld_pos] = Meld {
            kind: MeldKind::Shouminkan {
                tiles: [tiles[0], tiles[1], tiles[2], fourth],
            },
            from,
        };
        self.break_first_round_and_ippatsu();
        self.kan_draw_and_reveal(seat);
        self.push_event(GameEvent::Shouminkan { who: seat, kind });
        Ok(())
    }

    /// 执行立直: 切 tile 并设立直标志.
    pub fn do_riichi(&mut self, tile: Tile) -> Result<(), &'static str> {
        let seat = self.turn;
        {
            let p = &self.players[seat.index()];
            if !p.hand.is_menzen() {
                return Err("not menzen");
            }
            if p.riichi {
                return Err("already riichi");
            }
            if p.score < 1000 {
                return Err("not enough score");
            }
            let remaining = self.wall.as_ref().map(|w| w.remaining()).unwrap_or(0);
            if remaining < 4 {
                return Err("wall too few");
            }
            let mut counts = count_by_kind(&p.hand.closed);
            let idx = tile.kind.0 as usize;
            if counts[idx] == 0 {
                return Err("tile not in hand");
            }
            counts[idx] -= 1;
            if crate::decompose::tenpai_tiles(&counts, &p.hand.melds).is_empty() {
                return Err("not tenpai after discard");
            }
        }
        let first_go = self.first_go_around;
        self.do_discard(tile)?;
        let p = &mut self.players[seat.index()];
        p.riichi = true;
        p.double_riichi = first_go;
        p.ippatsu_active = true;
        p.score -= 1000;
        self.riichi_sticks += 1;
        self.push_event(GameEvent::Riichi { who: seat, tile });
        Ok(())
    }

    // ===== 内部 helper =====

    fn remove_from_hand(&mut self, who: Seat, ids: &[u16]) -> Result<(), &'static str> {
        let p = &mut self.players[who.index()];
        let mut to_remove: Vec<u16> = ids.to_vec();
        p.hand.closed.retain(|t| {
            if let Some(pos) = to_remove.iter().position(|id| *id == t.id) {
                to_remove.swap_remove(pos);
                false
            } else {
                true
            }
        });
        if to_remove.is_empty() {
            Ok(())
        } else {
            Err("some tile not in hand")
        }
    }

    fn consume_discard(&mut self, from: Seat, tile: Tile) {
        let p = &mut self.players[from.index()];
        if p.river.last().map(|t| t.id) == Some(tile.id) {
            p.river.pop();
        }
    }

    fn break_first_round_and_ippatsu(&mut self) {
        for pp in self.players.iter_mut() {
            pp.ippatsu_active = false;
        }
        self.first_go_around = false;
    }

    fn kan_draw_and_reveal(&mut self, seat: Seat) {
        if let Some(wall) = self.wall.as_mut() {
            wall.reveal_next_dora();
            if let Some(t) = wall.rinshan_draw() {
                self.players[seat.index()].hand.closed.push(t);
                self.players[seat.index()].last_drawn = Some(t);
                sort_hand(&mut self.players[seat.index()].hand.closed);
            }
        }
    }
}

fn sort_hand(tiles: &mut [Tile]) {
    tiles.sort_by_key(|t| (t.kind.0, !t.red));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_round_deals_correctly() {
        let mut g = GameState::new(GameConfig::default());
        g.start_round(42);
        for p in &g.players {
            assert_eq!(p.hand.closed.len(), 13);
        }
        assert_eq!(g.wall.as_ref().unwrap().remaining(), 70);
    }

    #[test]
    fn draw_then_discard() {
        let mut g = GameState::new(GameConfig::default());
        g.start_round(42);
        let drawn = g.do_draw().unwrap();
        assert_eq!(g.players[0].hand.closed.len(), 14);
        g.do_discard(drawn).unwrap();
        assert_eq!(g.players[0].hand.closed.len(), 13);
        assert_eq!(g.players[0].river.len(), 1);
        assert!(matches!(g.phase, Phase::AwaitCalls));
    }

    #[test]
    fn full_round_no_one_wins_eventually_ends() {
        let mut g = GameState::new(GameConfig::default());
        g.start_round(42);
        // 70 张山, 每摸切循环 3 步状态转换, 留充足兜底.
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > 1000 {
                panic!("循环步数超限, 状态机有问题");
            }
            match g.phase {
                Phase::Draw => {
                    if g.do_draw().is_none() {
                        break;
                    }
                }
                Phase::AwaitDiscard => {
                    let last = g.players[g.turn.index()].last_drawn.unwrap();
                    g.do_discard(last).unwrap();
                }
                Phase::AwaitCalls => {
                    g.advance_turn();
                }
                Phase::RoundEnd | Phase::GameEnd => break,
                _ => break,
            }
        }
        assert!(matches!(g.phase, Phase::RoundEnd));
    }
}
