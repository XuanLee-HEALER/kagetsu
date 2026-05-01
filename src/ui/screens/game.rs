//! 游戏屏幕(InGame): 摸牌 / 切牌 / 鸣牌 / 立直 / 自摸 / 荣和 / 流局 / 下一局.
//!
//! 玩家固定为东家(亲), 三家 AI.
//! 河按弃牌顺序 6 列分行展示, 副露独立显示.
//!
//! 操作:
//! - ←/→ 或 h/l: 选手牌
//! - Enter / Space: 切选中牌
//! - W: 和牌(自摸 / 荣和)
//! - R: 立直(切当前选中牌)
//! - K: 暗杠 / 加杠 (自动选可执行的)
//! - P: 碰  A: 吃  M: 明杠
//! - C: 跳过(他家弃牌或自家鸣牌机会)
//! - N: 下一局
//!
//! 全局快捷键 (Q 退出 / Esc 回主菜单) 由 [`crate::ui::App`] 统一处理.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::config::GameConfig;
use crate::game::{CallOptions, GameState, Phase, RoundResult, RyuukyokuKind};
use crate::meld::Seat;
use crate::player::{ai_choose_discard, default_action_on_timeout};
use crate::score::final_ranking;
use crate::ui::Transition;
use crate::ui::widgets::{
    render_melds_inline, render_river_lines, seat_label, separator_span, tile_content_span,
    tile_label,
};

const PLAYER_SEAT: Seat = Seat::East;
/// AI 操作的节流时间, 让玩家看清.
const AI_STEP_DELAY_MS: u64 = 350;

pub struct GameScreenState {
    pub game: GameState,
    /// 玩家选中的手牌索引.
    pub selected: usize,
    /// 当玩家有可执行的鸣牌/荣和时缓存.
    pub player_calls: Option<CallOptions>,
    /// 已扫描过 AwaitCalls 阶段(避免重复检查).
    pub calls_resolved: bool,
    /// 庄 seed (整场游戏的根种子).
    pub game_seed: u64,
    /// 局序号, 局 seed = game_seed ^ round_index.
    pub round_index: u64,
    /// 上次 AI 操作的时间, 用于节流.
    pub last_step_at: Instant,
    /// 状态栏临时消息.
    pub message: String,
    /// 当前等待玩家决策的截止时刻 (None = AI 回合或不限时).
    pub decision_deadline: Option<Instant>,
}

impl GameScreenState {
    pub fn new(config: GameConfig, game_seed: u64) -> Self {
        let mut g = GameState::new(config);
        g.start_round(game_seed ^ 1);
        Self {
            game: g,
            selected: 0,
            player_calls: None,
            calls_resolved: false,
            game_seed,
            round_index: 1,
            last_step_at: Instant::now(),
            message: String::from("东 1 局开始. 你是东家(亲)."),
            decision_deadline: None,
        }
    }

    /// 推进自动状态. 返回 Some(Transition) 表示要切屏(整场结束).
    pub fn advance(&mut self) -> Option<Transition> {
        // 1) 超时检查 (玩家在 AwaitDiscard / AwaitCalls 等输入时).
        if let Some(d) = self.decision_deadline
            && Instant::now() >= d
        {
            self.apply_timeout_default();
            return None;
        }

        // 2) 玩家有未决定的鸣牌/和牌选项时, 等输入.
        if self.player_calls.is_some() {
            return None;
        }

        // 3) AI 节流.
        if !self.is_player_turn()
            && self.last_step_at.elapsed().as_millis() < AI_STEP_DELAY_MS as u128
        {
            return None;
        }

        match self.game.phase {
            Phase::Deal => {
                self.round_index += 1;
                let seed = self.game_seed ^ self.round_index;
                self.game.start_round(seed);
                self.selected = 0;
                self.player_calls = None;
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            Phase::Draw => {
                if self.game.do_draw().is_none() {
                    self.game.phase = Phase::RoundEnd;
                    self.game.last_result = Some(RoundResult::Ryuukyoku {
                        kind: RyuukyokuKind::Howaipai,
                    });
                    self.message = String::from("流局.");
                    return None;
                }
                if self.is_player_turn() {
                    self.update_self_message();
                    if let Some(drawn) = self.game.players[PLAYER_SEAT.index()].last_drawn {
                        self.selected = self.game.players[PLAYER_SEAT.index()]
                            .hand
                            .closed
                            .iter()
                            .position(|t| t.id == drawn.id)
                            .unwrap_or(0);
                    }
                }
                self.last_step_at = Instant::now();
            }
            Phase::AwaitDiscard => {
                if !self.is_player_turn() {
                    let action = ai_choose_discard(&self.game);
                    self.apply_ai_action(action);
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                } else {
                    self.update_self_message();
                    self.set_deadline_if_unset();
                }
            }
            Phase::AwaitCalls => {
                if self.calls_resolved {
                    self.game.advance_turn();
                    self.calls_resolved = false;
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                    return None;
                }
                self.calls_resolved = true;
                let from = self.game.last_discard.map(|(s, _)| s);
                let Some(from) = from else {
                    self.game.advance_turn();
                    return None;
                };

                // 1) 先看 AI 谁能荣和(头跳).
                for s in ron_check_order(from) {
                    if s == PLAYER_SEAT {
                        continue;
                    }
                    if let Some(score) = self.game.try_ron(s) {
                        self.game.declare_ron(s, score);
                        self.message = format!("{} 荣和!", seat_label(s));
                        return None;
                    }
                }

                // 2) 玩家是否有响应选项?
                if from != PLAYER_SEAT {
                    let opts = self.game.legal_calls(PLAYER_SEAT);
                    if opts.any() {
                        let mut hints: Vec<String> = Vec::new();
                        if opts.ron {
                            hints.push("W 和".into());
                        }
                        if opts.pon.is_some() {
                            hints.push("P 碰".into());
                        }
                        if !opts.chi.is_empty() {
                            if opts.chi.len() > 1 {
                                hints.push(format!("A 吃(共{}种)", opts.chi.len()));
                            } else {
                                hints.push("A 吃".into());
                            }
                        }
                        if opts.minkan.is_some() {
                            hints.push("M 杠".into());
                        }
                        hints.push("C 跳过".into());
                        self.message = format!("可响应: {}", hints.join("  "));
                        self.player_calls = Some(opts);
                        self.set_deadline_if_unset();
                        return None;
                    }
                }

                // 3) 无人响应, 推进.
                self.game.advance_turn();
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            Phase::RoundEnd => {
                if !self.message.contains("下一局")
                    && let Some(result) = self.game.last_result.clone()
                {
                    self.message = match &result {
                        RoundResult::Ryuukyoku { .. } => "流局. 按 N 进下一局.".to_string(),
                        RoundResult::Win {
                            winner,
                            score,
                            is_tsumo,
                            ..
                        } => {
                            let mut s = format!(
                                "{} {}: {} 番 {} 符",
                                seat_label(*winner),
                                if *is_tsumo { "自摸" } else { "荣和" },
                                score.han,
                                score.fu,
                            );
                            let yaku_str: Vec<String> = score
                                .yaku
                                .iter()
                                .map(|(y, h)| format!("{}({})", y.name_zh(), h))
                                .collect();
                            if !yaku_str.is_empty() {
                                s.push_str(" | ");
                                s.push_str(&yaku_str.join(" "));
                            }
                            s.push_str(". 按 N 进下一局.");
                            s
                        }
                    };
                }
            }
            Phase::GameEnd => {
                let rankings = final_ranking(&self.game.players, &self.game.config);
                return Some(Transition::EnterGameOver { rankings });
            }
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
                    let len = self.player().hand.closed.len();
                    if self.selected + 1 < len {
                        self.selected += 1;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.try_player_discard();
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.try_player_win();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.try_player_riichi();
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                self.try_player_kan();
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.try_player_pon();
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.try_player_chi();
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                self.try_player_minkan();
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if self.player_calls.is_some() {
                    self.player_calls = None;
                    self.message = "已跳过.".into();
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                if self.game.phase == Phase::RoundEnd {
                    self.game.next_round();
                    self.last_step_at = Instant::now();
                }
            }
            _ => {}
        }
        None
    }

    fn update_self_message(&mut self) {
        let opts = self.game.legal_self_options();
        let mut hints = vec!["←/→ 选".to_string(), "Enter 切".to_string()];
        if opts.tsumo {
            hints.push("W 自摸".into());
        }
        if !opts.riichi_discards.is_empty() {
            hints.push(format!("R 立直({}张可)", opts.riichi_discards.len()));
        }
        if !opts.ankan.is_empty() {
            hints.push("K 暗杠".into());
        }
        if !opts.shouminkan.is_empty() {
            hints.push("K 加杠".into());
        }
        let intro = if opts.tsumo {
            "你可以自摸! "
        } else {
            "你的回合: "
        };
        self.message = format!("{}{}", intro, hints.join("  "));
    }

    fn apply_ai_action(&mut self, action: Action) {
        match action {
            Action::Discard(t) => {
                let _ = self.game.do_discard(t);
            }
            Action::Tsumo => {
                if let Some(score) = self.game.try_tsumo() {
                    let winner = self.game.turn;
                    self.game.declare_tsumo(score);
                    self.message = format!("{} 自摸!", seat_label(winner));
                }
            }
            Action::Ron(seat) => {
                if let Some(score) = self.game.try_ron(seat) {
                    self.game.declare_ron(seat, score);
                    self.message = format!("{} 荣和!", seat_label(seat));
                }
            }
            _ => {}
        }
    }

    fn apply_timeout_default(&mut self) {
        let action = default_action_on_timeout(&self.game);
        match action {
            Action::Discard(t) => {
                if self.game.do_discard(t).is_ok() {
                    self.message = "(超时) 自动切刚摸的牌.".into();
                    self.calls_resolved = false;
                    self.player_calls = None;
                }
            }
            Action::Pass => {
                if self.player_calls.is_some() {
                    self.player_calls = None;
                    self.message = "(超时) 自动跳过.".into();
                }
            }
            _ => {}
        }
        self.last_step_at = Instant::now();
        self.clear_deadline();
    }

    fn set_deadline_if_unset(&mut self) {
        if self.decision_deadline.is_some() {
            return;
        }
        if let Some(secs) = self.game.config.thinking_time_secs {
            self.decision_deadline = Some(Instant::now() + Duration::from_secs(secs as u64));
        }
    }

    fn clear_deadline(&mut self) {
        self.decision_deadline = None;
    }

    /// 剩余思考秒数(向上取整). None = 不限时或不在等候态.
    pub fn remaining_seconds(&self) -> Option<u64> {
        let d = self.decision_deadline?;
        let now = Instant::now();
        if now >= d {
            return Some(0);
        }
        let dur = d.saturating_duration_since(now);
        Some(dur.as_secs() + if dur.subsec_millis() > 0 { 1 } else { 0 })
    }

    fn is_player_turn(&self) -> bool {
        self.game.turn == PLAYER_SEAT
    }

    fn player(&self) -> &crate::game::PlayerState {
        &self.game.players[PLAYER_SEAT.index()]
    }

    fn try_player_discard(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let p = self.player();
        let Some(&t) = p.hand.closed.get(self.selected) else {
            return;
        };
        if self.game.do_discard(t).is_ok() {
            self.calls_resolved = false;
            self.player_calls = None;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn try_player_win(&mut self) {
        if self.is_player_turn()
            && self.game.phase == Phase::AwaitDiscard
            && let Some(score) = self.game.try_tsumo()
        {
            self.game.declare_tsumo(score);
            self.message = format!("{} 自摸!", seat_label(PLAYER_SEAT));
            self.player_calls = None;
            self.clear_deadline();
            return;
        }
        if let Some(opts) = &self.player_calls
            && opts.ron
            && let Some(score) = self.game.try_ron(PLAYER_SEAT)
        {
            self.game.declare_ron(PLAYER_SEAT, score);
            self.message = format!("{} 荣和!", seat_label(PLAYER_SEAT));
            self.player_calls = None;
            self.clear_deadline();
        }
    }

    fn try_player_riichi(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let opts = self.game.legal_self_options();
        if opts.riichi_discards.is_empty() {
            self.message = "不能立直.".into();
            return;
        }
        let p = self.player();
        let Some(&t) = p.hand.closed.get(self.selected) else {
            return;
        };
        if !opts.riichi_discards.iter().any(|x| x.kind == t.kind) {
            self.message = format!("切 {} 后未听牌, 不可立直.", t.kind.short());
            return;
        }
        match self.game.do_riichi(t) {
            Ok(()) => {
                self.message = "立直成立!".into();
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            Err(e) => {
                self.message = format!("立直失败: {}", e);
            }
        }
    }

    fn try_player_kan(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let opts = self.game.legal_self_options();
        if let Some(kind) = opts.ankan.first().copied() {
            if let Err(e) = self.game.do_ankan(kind) {
                self.message = format!("暗杠失败: {}", e);
            } else {
                self.message = format!("暗杠 {}!", kind.short());
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            return;
        }
        if let Some(kind) = opts.shouminkan.first().copied() {
            if let Err(e) = self.game.do_shouminkan(kind) {
                self.message = format!("加杠失败: {}", e);
            } else {
                self.message = format!("加杠 {}!", kind.short());
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            return;
        }
        self.message = "不能杠.".into();
    }

    fn try_player_pon(&mut self) {
        let Some(opts) = self.player_calls.clone() else {
            return;
        };
        let Some(two) = opts.pon else {
            self.message = "不能碰.".into();
            return;
        };
        if let Err(e) = self.game.do_pon(PLAYER_SEAT, two) {
            self.message = format!("碰失败: {}", e);
        } else {
            self.message = "碰!".into();
            self.player_calls = None;
            self.calls_resolved = false;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn try_player_chi(&mut self) {
        let Some(opts) = self.player_calls.clone() else {
            return;
        };
        let Some(&two) = opts.chi.first() else {
            self.message = "不能吃.".into();
            return;
        };
        if let Err(e) = self.game.do_chi(PLAYER_SEAT, two) {
            self.message = format!("吃失败: {}", e);
        } else {
            self.message = "吃!".into();
            self.player_calls = None;
            self.calls_resolved = false;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn try_player_minkan(&mut self) {
        let Some(opts) = self.player_calls.clone() else {
            return;
        };
        let Some(three) = opts.minkan else {
            self.message = "不能明杠.".into();
            return;
        };
        if let Err(e) = self.game.do_minkan(PLAYER_SEAT, three) {
            self.message = format!("明杠失败: {}", e);
        } else {
            self.message = "明杠!".into();
            self.player_calls = None;
            self.calls_resolved = false;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    // ============== 渲染 ==============

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(20),
                Constraint::Length(4),
            ])
            .split(area);

        self.render_header(f, chunks[0]);
        self.render_table(f, chunks[1]);
        self.render_status(f, chunks[2]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let dora = self
            .game
            .wall
            .as_ref()
            .map(|w| w.dora_indicators())
            .unwrap_or_default();
        let dora_str: Vec<String> = dora.iter().map(|t| tile_label(*t).0).collect();
        let remaining = self.game.wall.as_ref().map(|w| w.remaining()).unwrap_or(0);

        let line = Line::from(vec![
            Span::styled(
                format!("{} {} 局", self.game.round_wind.label(), self.game.kyoku),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::raw(format!("本场 {}", self.game.honba)),
            Span::raw("   "),
            Span::raw(format!("立直棒 {}", self.game.riichi_sticks)),
            Span::raw("   "),
            Span::raw(format!("剩余山 {}", remaining)),
            Span::raw("   "),
            Span::styled(
                format!("宝牌指示 {}", dora_str.join(" ")),
                Style::default().fg(Color::Cyan),
            ),
        ]);
        let p = Paragraph::new(line)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" tui-majo "));
        f.render_widget(p, area);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let mut hints: Vec<Span> = Vec::new();
        match self.game.phase {
            Phase::AwaitDiscard if self.is_player_turn() => {
                let opts = self.game.legal_self_options();
                hints.push(Span::raw("←/→ 选  Enter 切"));
                if opts.tsumo {
                    hints.push(Span::raw("  "));
                    hints.push(Span::styled(
                        "W 自摸",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                if !opts.riichi_discards.is_empty() {
                    hints.push(Span::raw("  "));
                    hints.push(Span::styled("R 立直", Style::default().fg(Color::Yellow)));
                }
                if !opts.ankan.is_empty() {
                    hints.push(Span::raw("  "));
                    hints.push(Span::styled("K 暗杠", Style::default().fg(Color::Magenta)));
                }
                if !opts.shouminkan.is_empty() {
                    hints.push(Span::raw("  "));
                    hints.push(Span::styled("K 加杠", Style::default().fg(Color::Magenta)));
                }
            }
            Phase::AwaitCalls if self.player_calls.is_some() => {
                let opts = self.player_calls.as_ref().unwrap();
                if opts.ron {
                    hints.push(Span::styled(
                        "W 和",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                    hints.push(Span::raw("  "));
                }
                if opts.pon.is_some() {
                    hints.push(Span::styled("P 碰", Style::default().fg(Color::Cyan)));
                    hints.push(Span::raw("  "));
                }
                if !opts.chi.is_empty() {
                    hints.push(Span::styled("A 吃", Style::default().fg(Color::Cyan)));
                    hints.push(Span::raw("  "));
                }
                if opts.minkan.is_some() {
                    hints.push(Span::styled("M 杠", Style::default().fg(Color::Magenta)));
                    hints.push(Span::raw("  "));
                }
                hints.push(Span::raw("C 跳过"));
            }
            Phase::RoundEnd => {
                hints.push(Span::styled(
                    "N 下一局",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Phase::GameEnd => {}
            _ => {
                hints.push(Span::styled(
                    "(AI 思考中)",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let lines = vec![Line::from(self.message.clone()), Line::from(hints)];
        let p = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(p, area);
    }

    fn render_table(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Min(6),
            ])
            .split(area);

        self.render_seat(f, rows[0], Seat::North, false);
        self.render_seat(f, rows[1], Seat::West, false);
        self.render_seat(f, rows[2], Seat::South, false);
        self.render_seat(f, rows[3], Seat::East, true);
    }

    fn render_seat(&self, f: &mut Frame, area: Rect, seat: Seat, is_player: bool) {
        let p = &self.game.players[seat.index()];
        let is_current = self.game.turn == seat;
        let is_dealer = seat == self.game.dealer;
        let seat_wind = self.game.seat_wind_of(seat);

        let title = format!(
            " {}{}{} 自风{}{} 点棒 {} ",
            seat_label(seat),
            if is_dealer { "·亲" } else { "" },
            if seat == PLAYER_SEAT { "(你)" } else { "" },
            seat_wind.short(),
            if p.riichi { " R" } else { "" },
            p.score,
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Left)
            .border_style(if is_current {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });
        let inner = block.inner(area);
        f.render_widget(block, area);

        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        let melds_w: u16 = ((parts[0].width as usize) / 2).min(45) as u16;
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(melds_w)])
            .split(parts[0]);

        let hand_spans = self.render_hand_inline(p, is_player);
        f.render_widget(Paragraph::new(Line::from(hand_spans)), row1[0]);

        let meld_spans = render_melds_inline(&p.hand.melds);
        f.render_widget(
            Paragraph::new(Line::from(meld_spans)).alignment(Alignment::Right),
            row1[1],
        );

        let river_lines = render_river_lines(&p.river);
        f.render_widget(Paragraph::new(river_lines), parts[1]);
    }

    fn render_hand_inline(
        &self,
        p: &crate::game::PlayerState,
        is_player: bool,
    ) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(p.hand.closed.len() * 2 + 1);
        let last_drawn_id = p.last_drawn.map(|t| t.id);
        let player_phase = self.is_player_turn() && self.game.phase == Phase::AwaitDiscard;

        if is_player {
            for (i, t) in p.hand.closed.iter().enumerate() {
                let sel = i == self.selected && player_phase;
                let drawn = Some(t.id) == last_drawn_id;
                spans.push(separator_span());
                spans.push(tile_content_span(*t, sel, drawn));
            }
        } else {
            for _ in 0..p.hand.closed.len() {
                spans.push(separator_span());
                spans.push(Span::styled("▒▒", Style::default().fg(Color::DarkGray)));
            }
        }
        spans.push(separator_span());
        spans
    }
}

/// 荣和检查顺序: 从 from 的下家开始(标准头跳).
fn ron_check_order(from: Seat) -> [Seat; 4] {
    let mut s = from.next();
    let mut out = [Seat::East; 4];
    for slot in &mut out {
        *slot = s;
        s = s.next();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn drain_pending(app: &mut GameScreenState) {
        if app.player_calls.is_some() {
            app.player_calls = None;
        }
    }

    #[test]
    fn app_can_complete_a_round_without_panic() {
        let mut app = GameScreenState::new(GameConfig::default(), 0xC0FFEE);
        let backend = TestBackend::new(120, 40);
        let mut term = Terminal::new(backend).unwrap();

        for _ in 0..5000 {
            term.draw(|f| app.render(f, f.area())).unwrap();
            app.last_step_at = Instant::now() - Duration::from_secs(1);
            drain_pending(&mut app);

            if app.is_player_turn() && app.game.phase == Phase::AwaitDiscard {
                let drawn = app.game.players[PLAYER_SEAT.index()].last_drawn;
                if let Some(t) = drawn {
                    let _ = app.game.do_discard(t);
                    app.calls_resolved = false;
                }
            } else {
                let _ = app.advance();
            }
            if app.game.phase == Phase::RoundEnd || app.game.phase == Phase::GameEnd {
                break;
            }
        }
        term.draw(|f| app.render(f, f.area())).unwrap();
        assert!(matches!(app.game.phase, Phase::RoundEnd | Phase::GameEnd));
    }

    #[test]
    fn app_can_advance_through_multiple_rounds() {
        let mut app = GameScreenState::new(GameConfig::default(), 0xC0FFEE);
        let backend = TestBackend::new(120, 40);
        let mut term = Terminal::new(backend).unwrap();

        let mut rounds = 0;
        for _ in 0..30000 {
            term.draw(|f| app.render(f, f.area())).unwrap();
            app.last_step_at = Instant::now() - Duration::from_secs(1);
            drain_pending(&mut app);

            match app.game.phase {
                Phase::AwaitDiscard if app.is_player_turn() => {
                    let drawn = app.game.players[PLAYER_SEAT.index()].last_drawn;
                    if let Some(t) = drawn {
                        let _ = app.game.do_discard(t);
                        app.calls_resolved = false;
                    }
                }
                Phase::RoundEnd => {
                    app.game.next_round();
                    rounds += 1;
                    if rounds >= 3 {
                        break;
                    }
                }
                Phase::GameEnd => break,
                _ => {
                    let _ = app.advance();
                }
            }
        }
        assert!(rounds >= 3 || app.game.phase == Phase::GameEnd);
    }
}
