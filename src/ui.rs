//! ratatui TUI 渲染与输入.
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
//! - Q / Esc: 退出

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::time::{Duration, Instant};

use crate::config::GameConfig;
use crate::game::{CallOptions, GameState, Phase, RoundResult, RyuukyokuKind};
use crate::meld::{Meld, MeldKind, Seat};
use crate::player::ai_choose_discard;
use crate::action::Action;
use crate::tile::Tile;

const PLAYER_SEAT: Seat = Seat::East;
/// AI 操作的节流时间, 让玩家看清.
const AI_STEP_DELAY_MS: u64 = 350;
/// 河的列宽(每行最多几张).
const RIVER_COLS: usize = 6;

pub struct App {
    pub game: GameState,
    pub running: bool,
    /// 玩家选中的手牌索引.
    pub selected: usize,
    /// 当玩家有可执行的鸣牌/荣和时缓存.
    pub player_calls: Option<CallOptions>,
    /// 已扫描过 AwaitCalls 阶段(避免重复检查).
    pub calls_resolved: bool,
    /// 用于种子: 每局递增.
    pub round_index: u64,
    /// 上次 AI 操作的时间, 用于节流.
    pub(crate) last_step_at: Instant,
    /// 状态栏临时消息.
    pub message: String,
}

impl App {
    pub fn new() -> Self {
        let mut g = GameState::new(GameConfig::default());
        g.start_round(0xC0FFEE);
        Self {
            game: g,
            running: true,
            selected: 0,
            player_calls: None,
            calls_resolved: false,
            round_index: 1,
            last_step_at: Instant::now(),
            message: String::from("东 1 局开始. 你是东家(亲)."),
        }
    }

    pub fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        while self.running {
            terminal.draw(|f| self.render(f))?;
            self.handle_events()?;
            self.advance_state();
        }
        Ok(())
    }

    pub(crate) fn advance_state(&mut self) {
        // 玩家有未决定的鸣牌/和牌选项时, 等输入.
        if self.player_calls.is_some() {
            return;
        }

        // AI 节流.
        if !self.is_player_turn()
            && self.last_step_at.elapsed().as_millis() < AI_STEP_DELAY_MS as u128
        {
            return;
        }

        match self.game.phase {
            Phase::Deal => {
                self.round_index += 1;
                let seed = 0xC0FFEE_u64 ^ self.round_index;
                self.game.start_round(seed);
                self.selected = 0;
                self.player_calls = None;
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
            }
            Phase::Draw => {
                if self.game.do_draw().is_none() {
                    self.game.phase = Phase::RoundEnd;
                    self.game.last_result = Some(RoundResult::Ryuukyoku {
                        kind: RyuukyokuKind::Howaipai,
                    });
                    self.message = String::from("流局.");
                    return;
                }
                if self.is_player_turn() {
                    self.update_self_message();
                    // 选中刚摸的那张
                    if let Some(drawn) = self.game.players[PLAYER_SEAT.index()].last_drawn {
                        self.selected = self
                            .game
                            .players[PLAYER_SEAT.index()]
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
                } else {
                    self.update_self_message();
                }
            }
            Phase::AwaitCalls => {
                if self.calls_resolved {
                    // 玩家已 pass 或没人响应 → 推进
                    self.game.advance_turn();
                    self.calls_resolved = false;
                    self.last_step_at = Instant::now();
                    return;
                }
                self.calls_resolved = true;
                let from = self.game.last_discard.map(|(s, _)| s);
                let Some(from) = from else {
                    self.game.advance_turn();
                    return;
                };

                // 1) 先看 AI 谁能荣和(头跳).
                for s in ron_check_order(from) {
                    if s == PLAYER_SEAT {
                        continue;
                    }
                    if let Some(score) = self.game.try_ron(s) {
                        self.game.declare_ron(s, score);
                        self.message = format!("{} 荣和!", seat_label(s));
                        return;
                    }
                }

                // 2) 玩家是否有响应选项(碰/吃/杠/和)?
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
                        return;
                    }
                }

                // 3) AI 不主动鸣牌, 直接 advance.
                self.game.advance_turn();
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
            }
            Phase::RoundEnd => {
                if !self.message.contains("下一局") {
                    if let Some(result) = self.game.last_result.clone() {
                        self.message = match &result {
                            RoundResult::Ryuukyoku { .. } => "流局. 按 N 进下一局.".to_string(),
                            RoundResult::Win { winner, score, is_tsumo, .. } => {
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
            }
            Phase::GameEnd => {
                self.message = String::from("半庄结束. 按 Q 退出.");
            }
        }
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

    fn is_player_turn(&self) -> bool {
        self.game.turn == PLAYER_SEAT
    }

    fn player(&self) -> &crate::game::PlayerState {
        &self.game.players[PLAYER_SEAT.index()]
    }

    fn handle_events(&mut self) -> Result<()> {
        let timeout = Duration::from_millis(80);
        if !event::poll(timeout)? {
            return Ok(());
        }
        let ev = event::read()?;
        let Event::Key(key) = ev else { return Ok(()) };
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                self.running = false;
            }
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
        Ok(())
    }

    fn try_player_discard(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let p = self.player();
        let Some(&t) = p.hand.closed.get(self.selected) else { return };
        if self.game.do_discard(t).is_ok() {
            self.calls_resolved = false;
            self.player_calls = None;
            self.last_step_at = Instant::now();
        }
    }

    fn try_player_win(&mut self) {
        // 自摸优先.
        if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
            if let Some(score) = self.game.try_tsumo() {
                self.game.declare_tsumo(score);
                self.message = format!("{} 自摸!", seat_label(PLAYER_SEAT));
                self.player_calls = None;
                return;
            }
        }
        // 荣和.
        if let Some(opts) = &self.player_calls {
            if opts.ron {
                if let Some(score) = self.game.try_ron(PLAYER_SEAT) {
                    self.game.declare_ron(PLAYER_SEAT, score);
                    self.message = format!("{} 荣和!", seat_label(PLAYER_SEAT));
                    self.player_calls = None;
                }
            }
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
        let Some(&t) = p.hand.closed.get(self.selected) else { return };
        // 检查选中牌是否在 riichi_discards 里(按 kind).
        if !opts.riichi_discards.iter().any(|x| x.kind == t.kind) {
            self.message = format!("切 {} 后未听牌, 不可立直.", t.kind.short());
            return;
        }
        match self.game.do_riichi(t) {
            Ok(()) => {
                self.message = "立直成立!".into();
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
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
            }
            return;
        }
        if let Some(kind) = opts.shouminkan.first().copied() {
            if let Err(e) = self.game.do_shouminkan(kind) {
                self.message = format!("加杠失败: {}", e);
            } else {
                self.message = format!("加杠 {}!", kind.short());
                self.last_step_at = Instant::now();
            }
            return;
        }
        self.message = "不能杠.".into();
    }

    fn try_player_pon(&mut self) {
        let Some(opts) = self.player_calls.clone() else { return };
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
        }
    }

    fn try_player_chi(&mut self) {
        let Some(opts) = self.player_calls.clone() else { return };
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
        }
    }

    fn try_player_minkan(&mut self) {
        let Some(opts) = self.player_calls.clone() else { return };
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
        }
    }

    // ============== 渲染 ==============

    fn render(&self, f: &mut ratatui::Frame) {
        let area = f.area();
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
        self.render_footer(f, chunks[2]);
    }

    fn render_header(&self, f: &mut ratatui::Frame, area: Rect) {
        let dora = self
            .game
            .wall
            .as_ref()
            .map(|w| w.dora_indicators())
            .unwrap_or_default();
        let dora_str: Vec<String> = dora.iter().map(|t| tile_label(*t).0).collect();
        let remaining = self
            .game
            .wall
            .as_ref()
            .map(|w| w.remaining())
            .unwrap_or(0);

        let line = Line::from(vec![
            Span::styled(
                format!("{} {} 局", self.game.round_wind.label(), self.game.kyoku),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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

    fn render_footer(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut hints: Vec<Span> = Vec::new();
        match self.game.phase {
            Phase::AwaitDiscard if self.is_player_turn() => {
                let opts = self.game.legal_self_options();
                hints.push(Span::raw("←/→ 选  Enter 切"));
                if opts.tsumo {
                    hints.push(Span::raw("  "));
                    hints.push(Span::styled(
                        "W 自摸",
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
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
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
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
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ));
            }
            Phase::GameEnd => {}
            _ => {
                hints.push(Span::styled("(AI 思考中)", Style::default().fg(Color::DarkGray)));
            }
        }
        hints.push(Span::raw("    Q 退出"));

        let lines = vec![Line::from(self.message.clone()), Line::from(hints)];
        let p = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(p, area);
    }

    fn render_table(&self, f: &mut ratatui::Frame, area: Rect) {
        // 4 家垂直堆叠. 每家手牌 1 行 + 河 3 行 + 边框 2 = 6 行.
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),  // 北
                Constraint::Length(6),  // 西
                Constraint::Length(6),  // 南
                Constraint::Min(6),     // 东(自家, 占据剩余空间)
            ])
            .split(area);

        self.render_seat(f, rows[0], Seat::North, false);
        self.render_seat(f, rows[1], Seat::West, false);
        self.render_seat(f, rows[2], Seat::South, false);
        self.render_seat(f, rows[3], Seat::East, true);
    }

    fn render_seat(&self, f: &mut ratatui::Frame, area: Rect, seat: Seat, is_player: bool) {
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

        // 内部布局: 上 = 手牌+副露(1 行), 下 = 河(剩余).
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        // 第一行: 左 = 手牌, 右 = 副露.
        let melds_w: u16 = ((parts[0].width as usize) / 2).min(45) as u16;
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(melds_w)])
            .split(parts[0]);

        // 手牌 (单行 outline).
        let hand_spans = self.render_hand_inline(p, is_player);
        f.render_widget(Paragraph::new(Line::from(hand_spans)), row1[0]);

        // 副露.
        let meld_spans = render_melds_inline(&p.hand.melds);
        f.render_widget(
            Paragraph::new(Line::from(meld_spans)).alignment(Alignment::Right),
            row1[1],
        );

        // 河 (多行 outline).
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
                spans.push(Span::styled(
                    "▒▒",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        spans.push(separator_span());
        spans
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// 返回(显示文本, 默认颜色). 文本永远 2 列宽: 数牌 "5p", 字牌单字符宽中文(占 2 列).
fn tile_label(t: Tile) -> (String, Color) {
    let suit_color = match t.kind.0 {
        0..=8 => Color::Yellow,
        9..=17 => Color::Cyan,
        18..=26 => Color::Green,
        27..=30 => Color::White, // 风牌
        31 => Color::White,      // 白
        32 => Color::Green,      // 發
        33 => Color::Red,        // 中
        _ => Color::DarkGray,
    };
    let text = match t.kind.0 {
        0..=8 => {
            let n = if t.red && t.kind.0 == 4 { 0 } else { t.kind.0 + 1 };
            format!("{}m", n)
        }
        9..=17 => {
            let n = if t.red && t.kind.0 == 13 { 0 } else { t.kind.0 - 9 + 1 };
            format!("{}p", n)
        }
        18..=26 => {
            let n = if t.red && t.kind.0 == 22 { 0 } else { t.kind.0 - 18 + 1 };
            format!("{}s", n)
        }
        27 => "東".into(),
        28 => "南".into(),
        29 => "西".into(),
        30 => "北".into(),
        31 => "白".into(),
        32 => "發".into(),
        33 => "中".into(),
        _ => "??".into(),
    };
    (text, suit_color)
}

fn separator_span() -> Span<'static> {
    Span::styled("│", Style::default().fg(Color::DarkGray))
}

fn tile_content_span(t: Tile, selected: bool, drawn: bool) -> Span<'static> {
    let (text, color) = tile_label(t);
    let mut style = Style::default().fg(color);
    if t.red {
        style = style.fg(Color::Red).add_modifier(Modifier::BOLD);
    }
    if drawn {
        style = style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
    }
    if selected {
        style = Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

/// 副露区(单行 inline). 每组用 `[标签 牌 牌 牌]` 的样式, 暗杠中间两张盖牌.
fn render_melds_inline(melds: &[Meld]) -> Vec<Span<'static>> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for meld in melds {
        let (label, label_color) = match &meld.kind {
            MeldKind::Chi { .. } => ("吃", Color::Cyan),
            MeldKind::Pon { .. } => ("碰", Color::Cyan),
            MeldKind::Minkan { .. } => ("明杠", Color::Magenta),
            MeldKind::Shouminkan { .. } => ("加杠", Color::Magenta),
            MeldKind::Ankan { .. } => ("暗杠", Color::DarkGray),
        };
        out.push(Span::styled(
            format!("[{}", label),
            Style::default().fg(label_color),
        ));
        let mut sorted: Vec<Tile> = meld.tiles().to_vec();
        sorted.sort_by_key(|t| t.kind.0);
        for (i, t) in sorted.iter().enumerate() {
            let show_back = matches!(meld.kind, MeldKind::Ankan { .. }) && (i == 0 || i == 3);
            out.push(Span::raw(" "));
            if show_back {
                out.push(Span::styled("▒▒", Style::default().fg(Color::DarkGray)));
            } else {
                let (text, color) = tile_label(*t);
                let style = if t.red {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                out.push(Span::styled(text, style));
            }
        }
        out.push(Span::styled("] ", Style::default().fg(label_color)));
    }
    out
}

/// 河(多行 outline). 按弃牌顺序 6 列分行, 每张牌 `│xx` 紧贴.
fn render_river_lines(river: &[Tile]) -> Vec<Line<'static>> {
    if river.is_empty() {
        return vec![Line::from(Span::styled(
            "(空)",
            Style::default().fg(Color::DarkGray),
        ))];
    }
    river
        .chunks(RIVER_COLS)
        .map(|chunk| {
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(chunk.len() * 2 + 1);
            for t in chunk {
                spans.push(separator_span());
                let (text, color) = tile_label(*t);
                let style = if t.red {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                };
                spans.push(Span::styled(text, style));
            }
            spans.push(separator_span());
            Line::from(spans)
        })
        .collect()
}

fn seat_label(s: Seat) -> &'static str {
    match s {
        Seat::East => "东",
        Seat::South => "南",
        Seat::West => "西",
        Seat::North => "北",
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

    fn drain_pending(app: &mut App) {
        // 消耗玩家未决定的鸣牌选项, 选择 pass.
        if app.player_calls.is_some() {
            app.player_calls = None;
        }
    }

    #[test]
    fn app_can_complete_a_round_without_panic() {
        let mut app = App::new();
        let backend = TestBackend::new(120, 40);
        let mut term = Terminal::new(backend).unwrap();

        for _ in 0..5000 {
            term.draw(|f| app.render(f)).unwrap();
            app.last_step_at = Instant::now() - Duration::from_secs(1);
            drain_pending(&mut app);

            if app.is_player_turn() && app.game.phase == Phase::AwaitDiscard {
                let drawn = app.game.players[PLAYER_SEAT.index()].last_drawn;
                if let Some(t) = drawn {
                    let _ = app.game.do_discard(t);
                    app.calls_resolved = false;
                }
            } else {
                app.advance_state();
            }
            if app.game.phase == Phase::RoundEnd || app.game.phase == Phase::GameEnd {
                break;
            }
        }
        term.draw(|f| app.render(f)).unwrap();
        assert!(matches!(
            app.game.phase,
            Phase::RoundEnd | Phase::GameEnd
        ));
    }

    #[test]
    fn app_can_advance_through_multiple_rounds() {
        let mut app = App::new();
        let backend = TestBackend::new(120, 40);
        let mut term = Terminal::new(backend).unwrap();

        let mut rounds = 0;
        for _ in 0..30000 {
            term.draw(|f| app.render(f)).unwrap();
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
                _ => app.advance_state(),
            }
        }
        assert!(rounds >= 3 || app.game.phase == Phase::GameEnd);
    }
}
