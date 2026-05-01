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
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::config::GameConfig;
use crate::game::{CallOptions, GameEvent, GameState, Phase, RoundResult, RyuukyokuKind};
use crate::meld::{MeldKind, Seat};
use crate::player::{ai_choose_discard, default_action_on_timeout};
use crate::score::final_ranking;
use crate::tile::{Tile, TileIndex};
use crate::ui::Transition;
use crate::ui::paint::{
    TileState, paint_back_column_wide, paint_back_row_wide, paint_boxed_row,
    paint_discard_grid_wide, paint_double_box, paint_fill, paint_hr, paint_hr_accent,
    paint_meld_row_tight, paint_str, paint_tile_tight, paint_tile_wide,
};
use crate::ui::theme::Theme;
use crate::ui::widgets::seat_label;
use unicode_width::UnicodeWidthStr;

const PLAYER_SEAT: Seat = Seat::East;
/// AI 操作的节流时间, 让玩家看清.
const AI_STEP_DELAY_MS: u64 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
}

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
    /// vim 风格输入模式.
    pub input_mode: InputMode,
    /// COMMAND 模式下的命令缓冲区.
    pub command_buffer: String,
    /// Action Modal 是否打开.
    pub modal_open: bool,
    /// Modal 当前选中项.
    pub modal_selected: usize,
    /// 进入 RoundEnd 的时刻, 用于流局后 N 秒自动推进.
    pub round_end_at: Option<Instant>,
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
            input_mode: InputMode::Normal,
            command_buffer: String::new(),
            modal_open: false,
            modal_selected: 0,
            round_end_at: None,
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
                    // 摸到的牌单独显示, 不参与 selected. selected 保留上巡位置, 钳到合法范围.
                    self.clamp_selected();
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
                // 首次进入 RoundEnd: 设置 message + 计时起点.
                if self.round_end_at.is_none() {
                    self.round_end_at = Some(Instant::now());
                    if let Some(result) = self.game.last_result.clone() {
                        self.message = match &result {
                            RoundResult::Ryuukyoku { .. } => {
                                "流局. 2 秒后自动进下一局 (或按 N).".to_string()
                            }
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
                // 流局自动推进 (2 秒后); 和牌等用户按 N.
                let is_ryuukyoku =
                    matches!(self.game.last_result, Some(RoundResult::Ryuukyoku { .. }));
                if is_ryuukyoku
                    && self
                        .round_end_at
                        .is_some_and(|t| t.elapsed().as_secs() >= 2)
                {
                    self.game.next_round();
                    self.round_end_at = None;
                    self.message.clear();
                    self.last_step_at = Instant::now();
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
        // COMMAND 模式: 字符进 buffer.
        if self.input_mode == InputMode::Command {
            return self.handle_command_key(key);
        }
        // Modal 打开: 优先处理 modal 键.
        if self.modal_open {
            return self.handle_modal_key(key);
        }
        // NORMAL 模式.
        match key.code {
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.command_buffer.clear();
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                // 注意: AwaitCalls 时 m 是明杠键, 这里改用 modal 唤起.
                // 若有 player_calls 且包含 minkan, 仍保留旧行为.
                if self.player_calls.is_some()
                    && self.player_calls.as_ref().unwrap().minkan.is_some()
                {
                    self.try_player_minkan();
                } else {
                    self.modal_open = true;
                    self.modal_selected = 0;
                }
            }
            KeyCode::Left => {
                if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
                    let len = self.selectable_count();
                    if self.selected + 1 < len {
                        self.selected += 1;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.try_player_discard();
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.try_player_discard();
            }
            KeyCode::Char('t') => {
                // 摸切: 切刚摸的那张.
                self.try_player_tsumogiri();
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
                    self.round_end_at = None;
                    self.message.clear();
                    self.last_step_at = Instant::now();
                }
            }
            // 数字 1-9 选第 N 张牌 (索引 selectable_tiles, 不含摸到的).
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                if self.is_player_turn() && self.game.phase == Phase::AwaitDiscard {
                    let idx = (c.to_digit(10).unwrap() - 1) as usize;
                    let len = self.selectable_count();
                    if idx < len {
                        self.selected = idx;
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.command_buffer.clear();
            }
            KeyCode::Enter => {
                let cmd = self.command_buffer.clone();
                self.command_buffer.clear();
                self.input_mode = InputMode::Normal;
                self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                self.command_buffer.pop();
            }
            KeyCode::Tab => {
                self.try_complete_command();
            }
            KeyCode::Char(c) => {
                if self.command_buffer.chars().count() < 32 {
                    self.command_buffer.push(c);
                }
            }
            _ => {}
        }
        None
    }

    /// Tab 补全: 取 buffer 第一个 token 的候选命令, 补到最长公共前缀.
    /// 唯一匹配则补全到完整名 + 1 空格 (方便接参数).
    fn try_complete_command(&mut self) {
        // 只补头一个 token (空格前). 已有空格则不补.
        if self.command_buffer.contains(' ') {
            return;
        }
        let cands = command_candidates(&self.command_buffer);
        match cands.len() {
            0 => {}
            1 => {
                self.command_buffer = cands[0].to_string();
                // 接受参数的命令补完后加空格
                if matches!(cands[0], "discard" | "riichi") {
                    self.command_buffer.push(' ');
                }
            }
            _ => {
                let prefix = longest_common_prefix(&cands);
                if prefix.len() > self.command_buffer.len() {
                    self.command_buffer = prefix;
                }
            }
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Option<Transition> {
        let actions = self.collect_modal_actions();
        match key.code {
            KeyCode::Esc => {
                self.modal_open = false;
            }
            KeyCode::Up => {
                let mut i = self.modal_selected;
                loop {
                    if i == 0 {
                        break;
                    }
                    i -= 1;
                    if actions.get(i).is_some_and(|a| a.enabled) {
                        self.modal_selected = i;
                        break;
                    }
                }
            }
            KeyCode::Down => {
                let mut i = self.modal_selected + 1;
                while i < actions.len() {
                    if actions[i].enabled {
                        self.modal_selected = i;
                        break;
                    }
                    i += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(action) = actions.get(self.modal_selected).cloned()
                    && action.enabled
                {
                    self.modal_open = false;
                    self.execute_modal_action(action.key);
                }
            }
            KeyCode::Char(c) => {
                let upper = c.to_ascii_uppercase();
                if let Some(action) = actions.iter().find(|a| a.key == upper).cloned()
                    && action.enabled
                {
                    self.modal_open = false;
                    self.execute_modal_action(action.key);
                }
            }
            _ => {}
        }
        None
    }

    fn execute_modal_action(&mut self, key: char) {
        match key {
            'R' => self.try_player_riichi(),
            'W' => self.try_player_win(),
            'K' => self.try_player_kan(),
            'D' => self.try_player_discard(),
            'T' => self.try_player_tsumogiri(),
            'P' => self.try_player_pon(),
            'A' => self.try_player_chi(),
            'M' => self.try_player_minkan(),
            'C' => {
                if self.player_calls.is_some() {
                    self.player_calls = None;
                    self.message = "已跳过.".into();
                    self.clear_deadline();
                }
            }
            _ => {}
        }
    }

    /// 摸切: 切刚摸的那张.
    fn try_player_tsumogiri(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let p = self.player();
        if let Some(t) = p.last_drawn
            && self.game.do_discard(t).is_ok()
        {
            self.calls_resolved = false;
            self.player_calls = None;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let parsed = parse_command(cmd);
        match parsed {
            ParsedCommand::Discard(spec) => {
                if let Some(idx) = self.find_tile_in_hand(&spec) {
                    let p = self.player();
                    if let Some(&t) = p.hand.closed.get(idx)
                        && self.game.do_discard(t).is_ok()
                    {
                        self.calls_resolved = false;
                        self.player_calls = None;
                        self.last_step_at = Instant::now();
                        self.clear_deadline();
                    }
                } else {
                    self.message = format!(":discard 失败: 手中无 {}", cmd);
                }
            }
            ParsedCommand::Riichi(spec) => {
                if let Some(idx) = self.find_tile_in_hand(&spec) {
                    let p = self.player();
                    if let Some(&t) = p.hand.closed.get(idx) {
                        match self.game.do_riichi(t) {
                            Ok(()) => {
                                self.message = "立直成立!".into();
                                self.calls_resolved = false;
                                self.last_step_at = Instant::now();
                                self.clear_deadline();
                            }
                            Err(e) => self.message = format!("立直失败: {}", e),
                        }
                    }
                } else {
                    self.message = format!(":riichi 失败: 手中无 {}", cmd);
                }
            }
            ParsedCommand::Tsumo => self.try_player_win(),
            ParsedCommand::Pon => self.try_player_pon(),
            ParsedCommand::Kan => self.try_player_kan(),
            ParsedCommand::Chi => self.try_player_chi(),
            ParsedCommand::Pass => {
                if self.player_calls.is_some() {
                    self.player_calls = None;
                    self.message = "已跳过.".into();
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                } else {
                    self.message = "无可跳过的响应.".into();
                }
            }
            ParsedCommand::Menu => {
                self.modal_open = true;
                self.modal_selected = 0;
            }
            ParsedCommand::Resign => {
                // MVP: 不实际投降, 仅清息.
                self.message = "(暂未支持 :resign)".into();
            }
            ParsedCommand::Unknown(s) => {
                self.message = format!("未知命令: {}", s);
            }
        }
    }

    fn find_tile_in_hand(&self, spec: &TileSpec) -> Option<usize> {
        let p = self.player();
        p.hand.closed.iter().position(|t| spec.matches(t.kind))
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

    /// 是否在 vim COMMAND 模式 (task 10 后会真正切换状态).
    pub fn is_command_mode(&self) -> bool {
        self.input_mode == InputMode::Command
    }

    /// 切换主题 (供全局 T 键调用).
    pub fn set_theme(&mut self, kind: crate::ui::theme::ThemeKind) {
        self.game.config.theme = kind;
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

    /// 可选牌列表 = 自家手牌中除去 last_drawn (摸到的牌). 排序保留 closed 顺序.
    /// 选牌索引 (`self.selected`) 永远指这个列表, 不会落在摸到的牌上.
    /// 摸到的牌通过 T 键摸切, 不参与 hjkl 移动.
    fn selectable_tiles(&self) -> Vec<Tile> {
        let p = self.player();
        let drawn_id = p.last_drawn.map(|t| t.id);
        p.hand
            .closed
            .iter()
            .filter(|t| Some(t.id) != drawn_id)
            .copied()
            .collect()
    }

    fn selectable_count(&self) -> usize {
        let p = self.player();
        let drawn_id = p.last_drawn.map(|t| t.id);
        p.hand
            .closed
            .iter()
            .filter(|t| Some(t.id) != drawn_id)
            .count()
    }

    /// 切牌等导致 selectable 长度变化后, 钳 selected 到合法范围.
    fn clamp_selected(&mut self) {
        let len = self.selectable_count();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn try_player_discard(&mut self) {
        if !self.is_player_turn() || self.game.phase != Phase::AwaitDiscard {
            return;
        }
        let tiles = self.selectable_tiles();
        let Some(&t) = tiles.get(self.selected) else {
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
        let tiles = self.selectable_tiles();
        let Some(&t) = tiles.get(self.selected) else {
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

    // ============== 渲染 (HiFi-05 设计稿坐标) ==============

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = self.game.config.theme.theme();
        let buf = f.buffer_mut();
        // 整屏背景填充.
        paint_fill(
            buf,
            area.x,
            area.y,
            area.width,
            area.height,
            Style::default().bg(theme.bg).fg(theme.fg),
        );
        let ox = area.x;
        let oy = area.y;

        self.paint_top_status(buf, ox, oy, &theme);
        self.paint_opponent_top(buf, ox, oy, &theme);
        self.paint_opponent_left(buf, ox, oy, &theme);
        self.paint_opponent_right(buf, ox, oy, &theme);
        self.paint_center_info(buf, ox, oy, &theme);
        self.paint_my_river(buf, ox, oy, &theme);
        self.paint_my_status(buf, ox, oy, &theme);
        self.paint_my_message_and_melds(buf, ox, oy, &theme);
        self.paint_my_hand(buf, ox, oy, &theme);
        self.paint_bottom(buf, ox, oy, &theme);

        if self.modal_open {
            self.paint_modal(buf, ox, oy, &theme);
        }
    }

    /// row 0-1: 顶部 status bar.
    fn paint_top_status(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let g = &self.game;
        // 局 / 本场 / 立直棒
        let round_label = format!("{} {} 局", g.round_wind.label(), g.kyoku);
        paint_str(
            buf,
            ox + 2,
            oy,
            &round_label,
            Style::default()
                .fg(theme.accent)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        paint_str(
            buf,
            ox + 11,
            oy,
            &format!("{}本", g.honba),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if g.riichi_sticks > 0 {
            paint_str(
                buf,
                ox + 15,
                oy,
                &format!("{}供", g.riichi_sticks),
                Style::default().fg(theme.danger).bg(theme.bg),
            );
        }
        paint_str(
            buf,
            ox + 19,
            oy,
            "│",
            Style::default().fg(theme.line).bg(theme.bg),
        );
        // 巡 / 山
        let junme = g.players[0].river.len() + 1;
        let wall_left = g.wall.as_ref().map(|w| w.remaining()).unwrap_or(0);
        paint_str(
            buf,
            ox + 21,
            oy,
            &format!("巡 {} · 山 {}", junme, wall_left),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_str(
            buf,
            ox + 36,
            oy,
            "│",
            Style::default().fg(theme.line).bg(theme.bg),
        );
        // 宝牌指示
        paint_str(
            buf,
            ox + 38,
            oy,
            "宝 ",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if let Some(wall) = g.wall.as_ref()
            && let Some(t) = wall.dora_indicators().first()
        {
            paint_tile_wide(buf, ox + 41, oy, Some(t), theme, TileState::Normal);
        }
        paint_str(
            buf,
            ox + 46,
            oy,
            "│",
            Style::default().fg(theme.line).bg(theme.bg),
        );
        // 4 家分数 (相对自家位置: 东=自家, 南=下家, 西=对家, 北=上家)
        let scores = [
            (Seat::East, "東", g.players[0].score, g.players[0].riichi),
            (Seat::South, "南", g.players[1].score, g.players[1].riichi),
            (Seat::West, "西", g.players[2].score, g.players[2].riichi),
            (Seat::North, "北", g.players[3].score, g.players[3].riichi),
        ];
        let mut col = ox + 48;
        for (i, (_seat, label, score, riichi)) in scores.iter().enumerate() {
            let star = if *riichi { "★" } else { "" };
            let style = if i == 0 {
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD)
            } else if *riichi {
                Style::default().fg(theme.danger).bg(theme.bg)
            } else {
                Style::default().fg(theme.dim).bg(theme.bg)
            };
            paint_str(buf, col, oy, &format!("{} {}{}", label, score, star), style);
            col += 11;
        }
        // 时钟 / 标题
        paint_str(
            buf,
            ox + 120,
            oy,
            "tui-majo",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_hr(buf, ox, oy + 1, 144, theme);
    }

    /// row 3-9: 对家 (West).
    fn paint_opponent_top(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.game.players[Seat::West.index()];
        // 标题行 row 3
        paint_str(
            buf,
            ox + 66,
            oy + 3,
            "─── 对家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_str(
            buf,
            ox + 75,
            oy + 3,
            "西",
            Style::default()
                .fg(theme.info)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        if p.riichi {
            paint_str(
                buf,
                ox + 78,
                oy + 3,
                "★立直",
                Style::default().fg(theme.danger).bg(theme.bg),
            );
        } else {
            paint_str(
                buf,
                ox + 78,
                oy + 3,
                &format!("{}", p.score),
                Style::default().fg(theme.fg).bg(theme.bg),
            );
        }
        paint_str(
            buf,
            ox + 85,
            oy + 3,
            "───",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        // 手牌行 row 4: 13 张牌背 wide
        paint_back_row_wide(buf, ox + 42, oy + 4, p.hand.closed.len(), theme);
        // 副露 row 5
        if !p.hand.melds.is_empty() {
            paint_str(
                buf,
                ox + 42,
                oy + 5,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 48;
            for meld in &p.hand.melds {
                let tiles: Vec<Tile> = meld.tiles().to_vec();
                paint_meld_row_tight(buf, col, oy + 5, &tiles, theme);
                col += (tiles.len() as u16) * 3 + 1;
            }
        }
        // 牌河 row 6-9
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide(buf, ox + 54, oy + 6, &p.river, theme, riichi_at);
    }

    /// row 6-19 左侧: 上家 (North).
    fn paint_opponent_left(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.game.players[Seat::North.index()];
        paint_str(
            buf,
            ox + 2,
            oy + 6,
            "上家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let label = if p.riichi {
            format!("北 {}★", p.score)
        } else {
            format!("北 {}", p.score)
        };
        let style = if p.riichi {
            Style::default()
                .fg(theme.danger)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.info)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD)
        };
        paint_str(buf, ox + 2, oy + 7, &label, style);
        if !p.hand.melds.is_empty() {
            paint_str(
                buf,
                ox + 2,
                oy + 9,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 2;
            let mut row = oy + 10;
            for meld in &p.hand.melds {
                let tiles: Vec<Tile> = meld.tiles().to_vec();
                paint_meld_row_tight(buf, col, row, &tiles, theme);
                col += (tiles.len() as u16) * 3 + 1;
                if col > ox + 14 {
                    col = ox + 2;
                    row += 1;
                }
            }
        }
        // 手牌竖排 col 14, row 6 起 (减去副露数 ×3)
        let hand_count = p.hand.closed.len();
        paint_back_column_wide(buf, ox + 14, oy + 6, hand_count.min(13), theme);
        // 牌河 6 列 wide, col 20
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide(buf, ox + 20, oy + 12, &p.river, theme, riichi_at);
    }

    /// row 6-19 右侧: 下家 (South).
    fn paint_opponent_right(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.game.players[Seat::South.index()];
        paint_str(
            buf,
            ox + 132,
            oy + 6,
            "下家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let label = if p.riichi {
            format!("南 {}★", p.score)
        } else {
            format!("南 {}", p.score)
        };
        let style = if p.riichi {
            Style::default()
                .fg(theme.danger)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.info)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD)
        };
        paint_str(buf, ox + 132, oy + 7, &label, style);
        if !p.hand.melds.is_empty() {
            paint_str(
                buf,
                ox + 126,
                oy + 9,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 126;
            let mut row = oy + 10;
            for meld in &p.hand.melds {
                let tiles: Vec<Tile> = meld.tiles().to_vec();
                paint_meld_row_tight(buf, col, row, &tiles, theme);
                col += (tiles.len() as u16) * 3 + 1;
                if col > ox + 138 {
                    col = ox + 126;
                    row += 1;
                }
            }
        }
        let hand_count = p.hand.closed.len();
        paint_back_column_wide(buf, ox + 120, oy + 6, hand_count.min(13), theme);
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide(buf, ox + 92, oy + 12, &p.river, theme, riichi_at);
    }

    /// row 17-18: 中央 dora + 山数提示.
    fn paint_center_info(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let g = &self.game;
        paint_str(
            buf,
            ox + 66,
            oy + 17,
            "宝",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if let Some(wall) = g.wall.as_ref()
            && let Some(t) = wall.dora_indicators().first()
        {
            paint_tile_wide(buf, ox + 70, oy + 17, Some(t), theme, TileState::Normal);
        }
        let wall_left = g.wall.as_ref().map(|w| w.remaining()).unwrap_or(0);
        paint_str(
            buf,
            ox + 68,
            oy + 18,
            &format!("山 {}", wall_left),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
    }

    /// row 23-26: 自家牌河.
    fn paint_my_river(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.game.players[PLAYER_SEAT.index()];
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide(buf, ox + 54, oy + 23, &p.river, theme, riichi_at);
    }

    /// row 28-29: 自家分割线 + 状态行.
    fn paint_my_status(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        paint_hr_accent(buf, ox + 2, oy + 28, 140, theme);
        let p = &self.game.players[PLAYER_SEAT.index()];
        paint_str(
            buf,
            ox + 4,
            oy + 29,
            "自家",
            Style::default()
                .fg(theme.accent)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        let dealer_str = if PLAYER_SEAT == self.game.dealer {
            "東 ◆庄"
        } else {
            "東"
        };
        paint_str(
            buf,
            ox + 9,
            oy + 29,
            dealer_str,
            Style::default().fg(theme.accent).bg(theme.bg),
        );
        paint_str(
            buf,
            ox + 18,
            oy + 29,
            &format!("{}", p.score),
            Style::default()
                .fg(theme.fg)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        // 听牌检测
        let waits = if p.hand.closed.len() == 13 {
            crate::decompose::tenpai_tiles(
                &crate::tile::count_by_kind(&p.hand.closed),
                &p.hand.melds,
            )
        } else {
            Vec::new()
        };
        if !waits.is_empty() {
            paint_str(
                buf,
                ox + 28,
                oy + 29,
                "聴牌 (已听)",
                Style::default()
                    .fg(theme.ok)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
            paint_str(
                buf,
                ox + 45,
                oy + 29,
                "聴 ",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            // 显示前 3 个待牌
            let mut col = ox + 48;
            for kind in waits.iter().take(3) {
                paint_str(
                    buf,
                    col,
                    oy + 29,
                    &kind_label_tight(*kind),
                    Style::default().fg(theme.tile_fg).bg(theme.tile_bg),
                );
                col += 4;
            }
        } else {
            paint_str(
                buf,
                ox + 28,
                oy + 29,
                "未聴 (未听)",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
        }
        // 危険提示: 任意他家立直时
        let any_riichi = (1..=3).any(|i| self.game.players[i].riichi);
        if any_riichi {
            paint_str(
                buf,
                ox + 63,
                oy + 29,
                "│",
                Style::default().fg(theme.line).bg(theme.bg),
            );
            paint_str(
                buf,
                ox + 65,
                oy + 29,
                "! 危険",
                Style::default()
                    .fg(theme.danger)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }

    /// row 30: 左侧 self.message (临时消息), 右侧自家副露.
    fn paint_my_message_and_melds(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        // ==== 左侧 message (col 4..78) ====
        if !self.message.is_empty() {
            let style = match self.game.phase {
                Phase::RoundEnd => Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default().fg(theme.fg).bg(theme.bg),
            };
            // 截断长 message 到 col 78 (~74 cells = 37 中文 / 74 半角)
            let mut msg = self.message.clone();
            let max_w = 74usize;
            while UnicodeWidthStr::width(msg.as_str()) > max_w {
                msg.pop();
            }
            paint_str(buf, ox + 4, oy + 30, &msg, style);
        }

        // ==== 右侧自家副露 (col 82+) ====
        let p = self.player();
        if p.hand.melds.is_empty() {
            return;
        }
        let mut col = ox + 82;
        for meld in &p.hand.melds {
            let (label, label_color) = match &meld.kind {
                MeldKind::Chi { .. } => ("[吃]", theme.info),
                MeldKind::Pon { .. } => ("[碰]", theme.info),
                MeldKind::Minkan { .. } => ("[明杠]", theme.accent),
                MeldKind::Shouminkan { .. } => ("[加杠]", theme.accent),
                MeldKind::Ankan { .. } => ("[暗杠]", theme.dim),
            };
            let label_w = UnicodeWidthStr::width(label) as u16;
            // 牌数 (吃/碰 3, 杠 4)
            let tile_count = match &meld.kind {
                MeldKind::Chi { .. } | MeldKind::Pon { .. } => 3u16,
                _ => 4,
            };
            let total_w = label_w + tile_count * 3 + 1;
            if col + total_w >= ox + 142 {
                break;
            }
            paint_str(
                buf,
                col,
                oy + 30,
                label,
                Style::default()
                    .fg(label_color)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
            let mut tx = col + label_w;
            for tile in meld.tiles() {
                paint_tile_tight(buf, tx, oy + 30, Some(tile), theme, TileState::Normal);
                tx += 3;
            }
            col += total_w;
        }
    }

    /// row 31-35: 自家手牌 BoxedRow + 编号.
    /// display = selectable_tiles (sorted, 不含摸到的) + 末尾追加 last_drawn (如有).
    /// selected 直接对应 selectable_tiles 索引, 永不指向摸到的牌.
    fn paint_my_hand(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.game.players[PLAYER_SEAT.index()];
        let mut display: Vec<Tile> = self.selectable_tiles();
        let drawn_idx = p.last_drawn.map(|t| {
            display.push(t);
            display.len() - 1
        });
        let selected_player = self.is_player_turn() && self.game.phase == Phase::AwaitDiscard;
        let selectable_len = drawn_idx.unwrap_or(display.len());
        let selected = if selected_player && self.selected < selectable_len {
            Some(self.selected)
        } else {
            None
        };
        paint_boxed_row(buf, ox + 4, oy + 31, &display, theme, selected, drawn_idx);
        // 编号 row 35 (与 paint_boxed_row 同样的间隙规则: drawn 前留 3 cells)
        let drawn_gap = 3u16;
        let mut cx = ox + 4 + 1;
        for i in 0..display.len() {
            if Some(i) == drawn_idx && i > 0 {
                cx += drawn_gap;
            }
            paint_str(
                buf,
                cx,
                oy + 35,
                &format!("{:>2}", i + 1),
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            cx += 5;
        }
    }

    /// row 36-39: 底部 last 日志 / 模式 status / 使い方 (用法) 速查.
    fn paint_bottom(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        paint_hr(buf, ox, oy + 36, 144, theme);
        // row 37: last 动作日志 (取最近 4-6 个事件)
        paint_str(
            buf,
            ox + 2,
            oy + 37,
            "last",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let mut col = ox + 7;
        for ev in self
            .game
            .events
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .iter()
            .rev()
        {
            let (text, style) = format_event(ev, theme);
            let w = UnicodeWidthStr::width(text.as_str()) as u16;
            if col + w + 2 >= ox + 118 {
                break;
            }
            paint_str(buf, col, oy + 37, &text, style);
            col += w + 1;
            paint_str(
                buf,
                col,
                oy + 37,
                "·",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            col += 2;
        }

        // row 38: 模式 status + 提示 / COMMAND 输入框
        // 整行 panel 背景
        paint_fill(
            buf,
            ox,
            oy + 38,
            144,
            1,
            Style::default().bg(theme.panel).fg(theme.fg),
        );
        if self.input_mode == InputMode::Command {
            paint_str(
                buf,
                ox + 2,
                oy + 38,
                ":",
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD),
            );
            paint_str(
                buf,
                ox + 3,
                oy + 38,
                &self.command_buffer,
                Style::default().fg(theme.fg).bg(theme.panel),
            );
            // ghost text (唯一前缀时灰显补全建议)
            let buf_w = self.command_buffer.chars().count() as u16;
            let cur_x = ox + 3 + buf_w;
            let mut painted_ghost = false;
            if !self.command_buffer.contains(' ') && !self.command_buffer.is_empty() {
                let cands = command_candidates(&self.command_buffer);
                if cands.len() == 1 && cands[0] != self.command_buffer {
                    let suggestion = &cands[0][self.command_buffer.len()..];
                    paint_str(
                        buf,
                        cur_x,
                        oy + 38,
                        suggestion,
                        Style::default().fg(theme.dim).bg(theme.panel),
                    );
                    painted_ghost = true;
                }
            }
            // 光标: 没 ghost 时画 "_" 提示位置
            if !painted_ghost {
                paint_str(
                    buf,
                    cur_x,
                    oy + 38,
                    "_",
                    Style::default()
                        .fg(theme.accent)
                        .bg(theme.panel)
                        .add_modifier(Modifier::BOLD),
                );
            }
        } else {
            paint_str(
                buf,
                ox + 2,
                oy + 38,
                "提示  按",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 11, oy + 38, ":", theme.line, theme.fg);
            paint_str(
                buf,
                ox + 14,
                oy + 38,
                "命令模式 ·",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 30, oy + 38, "m", theme.line, theme.fg);
            paint_str(
                buf,
                ox + 33,
                oy + 38,
                "菜单 ·",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            paint_str(
                buf,
                ox + 45,
                oy + 38,
                "[1-9]",
                Style::default().fg(theme.ok).bg(theme.panel),
            );
            paint_str(
                buf,
                ox + 51,
                oy + 38,
                "选牌 ·",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 59, oy + 38, "d", theme.line, theme.fg);
            paint_str(
                buf,
                ox + 62,
                oy + 38,
                "切 ·",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 68, oy + 38, "t", theme.line, theme.fg);
            paint_str(
                buf,
                ox + 71,
                oy + 38,
                "摸切 ·",
                Style::default().fg(theme.dim).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 79, oy + 38, "R", theme.danger, theme.danger);
            paint_str(
                buf,
                ox + 82,
                oy + 38,
                "立直",
                Style::default().fg(theme.danger).bg(theme.panel),
            );
            self.paint_key_hint(buf, ox + 88, oy + 38, "W", theme.ok, theme.ok);
            paint_str(
                buf,
                ox + 91,
                oy + 38,
                "自摸",
                Style::default().fg(theme.ok).bg(theme.panel),
            );
        }
        // 模式徽章 (col 120)
        let badge_label = match self.input_mode {
            InputMode::Normal => " NORMAL ",
            InputMode::Command => " COMMAND ",
        };
        let badge_bg = if self.input_mode == InputMode::Command {
            theme.accent
        } else {
            theme.ok
        };
        paint_str(
            buf,
            ox + 120,
            oy + 38,
            badge_label,
            Style::default()
                .fg(theme.bg)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        );
        paint_str(
            buf,
            ox + 132,
            oy + 38,
            "←→ 选牌",
            Style::default().fg(theme.dim).bg(theme.panel),
        );

        // row 39:
        // - COMMAND 模式 → 显示候选命令 (Tab 补全)
        // - NORMAL  模式 → 命令速查
        if self.input_mode == InputMode::Command {
            paint_str(
                buf,
                ox + 2,
                oy + 39,
                "候选",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let cands = command_candidates(&self.command_buffer);
            let mut col = ox + 7;
            if cands.is_empty() {
                paint_str(
                    buf,
                    col,
                    oy + 39,
                    "(无匹配)",
                    Style::default().fg(theme.danger).bg(theme.bg),
                );
            } else {
                for (i, name) in cands.iter().enumerate() {
                    if col + (name.len() as u16) + 2 >= ox + 130 {
                        break;
                    }
                    let style = if i == 0 {
                        Style::default()
                            .fg(theme.accent)
                            .bg(theme.bg)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.fg).bg(theme.bg)
                    };
                    paint_str(buf, col, oy + 39, name, style);
                    col += (name.chars().count() as u16) + 2;
                }
            }
            paint_str(
                buf,
                ox + 130,
                oy + 39,
                "Tab 补全",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
        } else {
            paint_str(
                buf,
                ox + 2,
                oy + 39,
                "使い方",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            // 命令 + 中文注释 (动态计算 col, 避免 wide-char 错位)
            let cmds: &[(&str, &str)] = &[
                (":discard", "切"),
                (":riichi", "立直"),
                (":tsumo", "自摸"),
                (":pon", "碰"),
                (":kan", "杠"),
                (":chi", "吃"),
                (":pass", "跳过"),
                (":menu", "菜单"),
                (":resign", "退出"),
            ];
            let cmd_style = Style::default().fg(theme.fg).bg(theme.bg);
            let hint_style = Style::default().fg(theme.dim).bg(theme.bg);
            let mut col = ox + 11;
            for (cmd, hint) in cmds {
                let cmd_w = UnicodeWidthStr::width(*cmd) as u16;
                let hint_text = format!(" ({})", hint);
                let hint_w = UnicodeWidthStr::width(hint_text.as_str()) as u16;
                if col + cmd_w + hint_w + 1 >= ox + 144 {
                    break;
                }
                paint_str(buf, col, oy + 39, cmd, cmd_style);
                paint_str(buf, col + cmd_w, oy + 39, &hint_text, hint_style);
                col += cmd_w + hint_w + 1;
            }
        }
    }

    /// 单个键位提示 (圆角框包裹).
    fn paint_key_hint(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        label: &str,
        border_color: ratatui::style::Color,
        fg: ratatui::style::Color,
    ) {
        // 用 [X] 简化样式
        paint_str(
            buf,
            x,
            y,
            &format!("[{}]", label),
            Style::default()
                .fg(fg)
                .bg(self.game.config.theme.theme().panel)
                .add_modifier(Modifier::BOLD),
        );
        // border_color 暂未使用 (简化 [X] 风格), 留参数以备后续装饰
        let _ = border_color;
    }

    /// Action Modal 浮窗.
    fn paint_modal(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let w: u16 = 56;
        let h: u16 = 20;
        let mx = ox + 44;
        let my = oy + 10;
        // 背景填充 (panel 色)
        paint_fill(
            buf,
            mx,
            my,
            w,
            h,
            Style::default().bg(theme.panel).fg(theme.fg),
        );
        paint_double_box(buf, mx, my, w, h, theme, Some("Action ・ 行动"));

        let actions = self.collect_modal_actions();
        // 信息行
        let g = &self.game;
        let junme = g.players[0].river.len() + 1;
        paint_str(
            buf,
            mx + 2,
            my + 2,
            &format!("巡 {}", junme),
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        if let Some(t) = g.players[PLAYER_SEAT.index()].last_drawn {
            paint_str(
                buf,
                mx + 8,
                my + 2,
                "你摸到",
                Style::default().fg(theme.fg).bg(theme.panel),
            );
            paint_tile_wide(buf, mx + 14, my + 2, Some(&t), theme, TileState::Normal);
        }
        paint_str(
            buf,
            mx + 2,
            my + 4,
            &"─".repeat((w - 4) as usize),
            Style::default().fg(theme.line).bg(theme.panel),
        );

        // 选项列表 (从 row+5 起, 每项 2 行)
        for (i, action) in actions.iter().enumerate() {
            let row = my + 5 + (i as u16) * 2;
            if row + 1 >= my + h - 2 {
                break;
            }
            let highlight = i == self.modal_selected;
            let fg = if !action.enabled {
                theme.dim
            } else if highlight {
                theme.bg
            } else {
                theme.fg
            };
            let bg = if highlight && action.enabled {
                theme.accent
            } else {
                theme.panel
            };
            // 整行 highlight 背景
            if highlight && action.enabled {
                paint_fill(
                    buf,
                    mx + 1,
                    row,
                    w - 2,
                    1,
                    Style::default().bg(theme.accent_soft).fg(theme.fg),
                );
            }
            // [Key]
            paint_str(
                buf,
                mx + 2,
                row,
                &format!(" {} ", action.key),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            );
            // label
            let label_style = if !action.enabled {
                Style::default().fg(theme.dim).bg(theme.panel)
            } else {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD)
            };
            paint_str(buf, mx + 7, row, action.label, label_style);
            // detail
            paint_str(
                buf,
                mx + 13,
                row,
                &action.detail,
                Style::default().fg(theme.dim).bg(theme.panel),
            );
        }

        // 帮助行
        paint_str(
            buf,
            mx + 2,
            my + h - 2,
            "↑↓ 选择 ・ Enter 确认 ・ Esc 关闭",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
    }
}

/// 找到玩家立直时弃出的牌在 river 里的索引. 简化: 当 player.riichi 时找最早的弃牌索引.
fn riichi_index_in_river(p: &crate::game::PlayerState) -> Option<usize> {
    if p.riichi && !p.river.is_empty() {
        // MVP: 立直牌 = 立直时切的那张, 但当前 PlayerState 没存. 用 None 暂不标记.
        // 后续 task 可加 riichi_river_idx 字段.
        None
    } else {
        None
    }
}

/// 把 TileIndex 渲染成 tight 文本 (3 cells: "1萬" / "東 ").
fn kind_label_tight(kind: TileIndex) -> String {
    let n = kind.0;
    match n {
        0..=8 => format!("{}萬", n + 1),
        9..=17 => format!("{}筒", n - 9 + 1),
        18..=26 => format!("{}索", n - 18 + 1),
        27 => "東 ".into(),
        28 => "南 ".into(),
        29 => "西 ".into(),
        30 => "北 ".into(),
        31 => "白 ".into(),
        32 => "發 ".into(),
        33 => "中 ".into(),
        _ => "?? ".into(),
    }
}

fn format_event(ev: &GameEvent, theme: &Theme) -> (String, Style) {
    let s = Style::default().bg(theme.bg);
    match ev {
        GameEvent::Discard { who, tile } => (
            format!("{} 打 {}", seat_short(*who), kind_label_tight(tile.kind)),
            s.fg(theme.dim),
        ),
        GameEvent::Draw { who, .. } => (format!("{} 摸", seat_short(*who)), s.fg(theme.info)),
        GameEvent::Pon { who, tile } => (
            format!("{} 碰 {}", seat_short(*who), kind_label_tight(tile.kind)),
            s.fg(theme.info),
        ),
        GameEvent::Chi { who, tile } => (
            format!("{} 吃 {}", seat_short(*who), kind_label_tight(tile.kind)),
            s.fg(theme.info),
        ),
        GameEvent::Minkan { who, tile } => (
            format!("{} 杠 {}", seat_short(*who), kind_label_tight(tile.kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Ankan { who, kind } => (
            format!("{} 暗杠 {}", seat_short(*who), kind_label_tight(*kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Shouminkan { who, kind } => (
            format!("{} 加杠 {}", seat_short(*who), kind_label_tight(*kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Riichi { who, .. } => (
            format!("{} 立直", seat_short(*who)),
            s.fg(theme.danger).add_modifier(Modifier::BOLD),
        ),
        GameEvent::Tsumo { who } => (
            format!("{} 自摸", seat_short(*who)),
            s.fg(theme.ok).add_modifier(Modifier::BOLD),
        ),
        GameEvent::Ron { who, .. } => (
            format!("{} 荣和", seat_short(*who)),
            s.fg(theme.ok).add_modifier(Modifier::BOLD),
        ),
    }
}

fn seat_short(s: Seat) -> &'static str {
    match s {
        Seat::East => "你",
        Seat::South => "下家",
        Seat::West => "对家",
        Seat::North => "上家",
    }
}

#[derive(Debug, Clone)]
pub struct ModalAction {
    pub key: char,
    pub label: &'static str,
    pub detail: String,
    pub enabled: bool,
}

impl GameScreenState {
    /// 收集 modal 中要显示的动作清单.
    pub fn collect_modal_actions(&self) -> Vec<ModalAction> {
        let mut out = Vec::new();
        // AwaitDiscard 自家
        if self.game.phase == Phase::AwaitDiscard && self.is_player_turn() {
            let opts = self.game.legal_self_options();
            out.push(ModalAction {
                key: 'R',
                label: "立直",
                detail: if opts.riichi_discards.is_empty() {
                    "未聴牌 (未听), 或无法立直".into()
                } else {
                    format!("{} 张可立直", opts.riichi_discards.len())
                },
                enabled: !opts.riichi_discards.is_empty(),
            });
            out.push(ModalAction {
                key: 'W',
                label: "自摸",
                detail: if opts.tsumo {
                    "已聴牌 (已听), 直接和牌".into()
                } else {
                    "未満和牌役 (役不满, 无 ツモ/自摸 役)".into()
                },
                enabled: opts.tsumo,
            });
            out.push(ModalAction {
                key: 'K',
                label: "暗杠",
                detail: if opts.ankan.is_empty() {
                    "无可暗杠组".into()
                } else {
                    format!("{} 种可暗杠", opts.ankan.len())
                },
                enabled: !opts.ankan.is_empty(),
            });
            out.push(ModalAction {
                key: 'D',
                label: "切牌",
                detail: "选择手牌中一张打出".into(),
                enabled: true,
            });
            out.push(ModalAction {
                key: 'T',
                label: "摸切",
                detail: "切出刚摸到的牌(不变手牌)".into(),
                enabled: self.game.players[PLAYER_SEAT.index()].last_drawn.is_some(),
            });
        }
        // AwaitCalls (玩家有响应)
        if let Some(opts) = self.player_calls.as_ref() {
            out.push(ModalAction {
                key: 'W',
                label: "荣和",
                detail: if opts.ron {
                    "可和牌".into()
                } else {
                    "无役不可和".into()
                },
                enabled: opts.ron,
            });
            out.push(ModalAction {
                key: 'P',
                label: "碰",
                detail: if opts.pon.is_some() {
                    "组成刻子".into()
                } else {
                    "无对子".into()
                },
                enabled: opts.pon.is_some(),
            });
            out.push(ModalAction {
                key: 'A',
                label: "吃",
                detail: if !opts.chi.is_empty() {
                    format!("共 {} 种吃法", opts.chi.len())
                } else {
                    "无连续".into()
                },
                enabled: !opts.chi.is_empty(),
            });
            out.push(ModalAction {
                key: 'M',
                label: "明杠",
                detail: if opts.minkan.is_some() {
                    "三同张".into()
                } else {
                    "无三张".into()
                },
                enabled: opts.minkan.is_some(),
            });
            out.push(ModalAction {
                key: 'C',
                label: "跳过",
                detail: "放弃响应".into(),
                enabled: true,
            });
        }
        out
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

// ============== vim 命令解析 ==============

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Discard(TileSpec),
    Riichi(TileSpec),
    Tsumo,
    Pon,
    Kan,
    Chi,
    /// 跳过响应他家弃牌(碰/吃/杠/和).
    Pass,
    Menu,
    Resign,
    Unknown(String),
}

/// 全部主命令名 (顺序固定, 用于 Tab 补全和速查).
pub const COMMAND_NAMES: &[&str] = &[
    "discard", "riichi", "tsumo", "pon", "kan", "chi", "pass", "menu", "resign",
];

/// 牌种说明符: 接受 "5p" / "p5" / "五筒" / "東" 等.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSpec {
    pub kind: TileIndex,
}

impl TileSpec {
    pub fn matches(&self, k: TileIndex) -> bool {
        self.kind == k
    }
}

pub fn parse_command(s: &str) -> ParsedCommand {
    let s = s.trim();
    if s.is_empty() {
        return ParsedCommand::Unknown(String::new());
    }
    let mut parts = s.splitn(2, ' ');
    let head = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim();
    match head.as_str() {
        "discard" | "d" => match parse_tile_spec(arg) {
            Some(spec) => ParsedCommand::Discard(spec),
            None => ParsedCommand::Unknown(s.to_string()),
        },
        "riichi" | "r" => match parse_tile_spec(arg) {
            Some(spec) => ParsedCommand::Riichi(spec),
            None => ParsedCommand::Unknown(s.to_string()),
        },
        "tsumo" | "t" => ParsedCommand::Tsumo,
        "pon" | "p" => ParsedCommand::Pon,
        "kan" | "k" => ParsedCommand::Kan,
        "chi" | "a" => ParsedCommand::Chi,
        "pass" | "skip" | "c" => ParsedCommand::Pass,
        "menu" | "m" => ParsedCommand::Menu,
        "resign" => ParsedCommand::Resign,
        _ => ParsedCommand::Unknown(s.to_string()),
    }
}

/// 找出所有以 prefix 开头的命令名 (按 [`COMMAND_NAMES`] 顺序).
pub fn command_candidates(prefix: &str) -> Vec<&'static str> {
    let p = prefix.to_lowercase();
    COMMAND_NAMES
        .iter()
        .filter(|n| n.starts_with(&p))
        .copied()
        .collect()
}

/// 多个候选的最长公共前缀.
pub fn longest_common_prefix(strs: &[&str]) -> String {
    if strs.is_empty() {
        return String::new();
    }
    let first = strs[0];
    let mut end = first.len();
    for s in &strs[1..] {
        let common = first
            .chars()
            .zip(s.chars())
            .take_while(|(a, b)| a == b)
            .count();
        // count 是 char 数, 但我们要 byte 长度. 因为这里全是 ASCII, char count == byte count.
        end = end.min(common);
    }
    first[..end].to_string()
}

/// 接受的牌输入:
/// - ASCII: "5p" / "p5" / "5m" / "9s" / "1z"-"7z" (z = 字牌, 1=東 .. 7=中)
/// - 中文数字 + 花色: "五筒" / "三索" / "九萬"
/// - 字牌: "东南西北白发中" / "東南西北白發中"
pub fn parse_tile_spec(s: &str) -> Option<TileSpec> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // 单字符字牌
    let kind = match s {
        "東" | "东" | "1z" | "z1" => Some(TileIndex::EAST),
        "南" | "2z" | "z2" => Some(TileIndex::SOUTH),
        "西" | "3z" | "z3" => Some(TileIndex::WEST),
        "北" | "4z" | "z4" => Some(TileIndex::NORTH),
        "白" | "5z" | "z5" => Some(TileIndex::HAKU),
        "發" | "发" | "6z" | "z6" => Some(TileIndex::HATSU),
        "中" | "7z" | "z7" => Some(TileIndex::CHUN),
        _ => None,
    };
    if let Some(k) = kind {
        return Some(TileSpec { kind: k });
    }
    // ASCII 数字 + 花色 (5p / p5)
    let ascii_lo = s.to_lowercase();
    let (n, suit) = parse_num_suit_ascii(&ascii_lo)?;
    if !(1..=9).contains(&n) {
        return None;
    }
    let base = match suit {
        'm' => 0u8,
        'p' => 9,
        's' => 18,
        _ => return None,
    };
    Some(TileSpec {
        kind: TileIndex(base + (n - 1) as u8),
    })
}

fn parse_num_suit_ascii(s: &str) -> Option<(u32, char)> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() != 2 {
        // 试中文数字 + 花色 (e.g. "五筒")
        return parse_cn_num_suit(s);
    }
    let (a, b) = (chars[0], chars[1]);
    if a.is_ascii_digit() {
        Some((a.to_digit(10)?, b))
    } else if b.is_ascii_digit() {
        Some((b.to_digit(10)?, a))
    } else {
        parse_cn_num_suit(s)
    }
}

fn parse_cn_num_suit(s: &str) -> Option<(u32, char)> {
    const CN_NUM: &[(&str, u32)] = &[
        ("一", 1),
        ("二", 2),
        ("三", 3),
        ("四", 4),
        ("五", 5),
        ("六", 6),
        ("七", 7),
        ("八", 8),
        ("九", 9),
    ];
    for (cn, n) in CN_NUM {
        if let Some(rest) = s.strip_prefix(cn) {
            let suit = match rest {
                "萬" | "万" => 'm',
                "筒" | "饼" => 'p',
                "索" | "条" => 's',
                _ => return None,
            };
            return Some((*n, suit));
        }
    }
    None
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
        let backend = TestBackend::new(144, 40);
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
        let backend = TestBackend::new(144, 40);
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

    #[test]
    fn parse_command_basic() {
        assert!(matches!(parse_command("tsumo"), ParsedCommand::Tsumo));
        assert!(matches!(parse_command("t"), ParsedCommand::Tsumo));
        assert!(matches!(parse_command("pon"), ParsedCommand::Pon));
        assert!(matches!(parse_command("kan"), ParsedCommand::Kan));
        assert!(matches!(parse_command("menu"), ParsedCommand::Menu));
        assert!(matches!(parse_command("resign"), ParsedCommand::Resign));
        assert!(matches!(parse_command("pass"), ParsedCommand::Pass));
        assert!(matches!(parse_command("skip"), ParsedCommand::Pass));
        assert!(matches!(parse_command("c"), ParsedCommand::Pass));
        assert!(matches!(parse_command("nope"), ParsedCommand::Unknown(_)));
    }

    #[test]
    fn command_completion() {
        // 唯一前缀
        assert_eq!(command_candidates("ts"), vec!["tsumo"]);
        assert_eq!(command_candidates("res"), vec!["resign"]);
        // 多候选 + 共同前缀
        let p_cands = command_candidates("p");
        assert!(p_cands.contains(&"pon"));
        assert!(p_cands.contains(&"pass"));
        assert_eq!(longest_common_prefix(&p_cands), "p");
        // 无匹配
        assert!(command_candidates("xyz").is_empty());
        // 空 → 全部
        assert_eq!(command_candidates("").len(), COMMAND_NAMES.len());
    }

    #[test]
    fn parse_command_discard_riichi() {
        // 5p
        let p5 = TileIndex(13);
        match parse_command("discard 5p") {
            ParsedCommand::Discard(spec) => assert_eq!(spec.kind, p5),
            other => panic!("期望 Discard, 得到 {:?}", other),
        }
        match parse_command("d p5") {
            ParsedCommand::Discard(spec) => assert_eq!(spec.kind, p5),
            other => panic!("期望 Discard, 得到 {:?}", other),
        }
        // 中文
        match parse_command("discard 五筒") {
            ParsedCommand::Discard(spec) => assert_eq!(spec.kind, p5),
            other => panic!("期望 Discard, 得到 {:?}", other),
        }
        // 字牌
        match parse_command("riichi 東") {
            ParsedCommand::Riichi(spec) => assert_eq!(spec.kind, TileIndex::EAST),
            other => panic!("期望 Riichi, 得到 {:?}", other),
        }
    }

    #[test]
    fn parse_tile_spec_variants() {
        assert_eq!(parse_tile_spec("5p").unwrap().kind, TileIndex(13));
        assert_eq!(parse_tile_spec("p5").unwrap().kind, TileIndex(13));
        assert_eq!(parse_tile_spec("9m").unwrap().kind, TileIndex(8));
        assert_eq!(parse_tile_spec("1s").unwrap().kind, TileIndex(18));
        assert_eq!(parse_tile_spec("中").unwrap().kind, TileIndex::CHUN);
        assert_eq!(parse_tile_spec("发").unwrap().kind, TileIndex::HATSU);
        assert!(parse_tile_spec("0p").is_none());
        assert!(parse_tile_spec("xx").is_none());
    }

    #[test]
    fn modal_actions_in_await_discard() {
        let mut app = GameScreenState::new(GameConfig::default(), 0xC0FFEE);
        // 模拟玩家摸牌阶段
        let _ = app.advance(); // Deal -> Draw
        let _ = app.advance(); // Draw 自家
        // 此时 phase 应是 AwaitDiscard, 是自家 (East)
        if app.is_player_turn() && app.game.phase == Phase::AwaitDiscard {
            let actions = app.collect_modal_actions();
            // 至少含 R/W/K/D/T 五项
            assert!(actions.iter().any(|a| a.key == 'R'));
            assert!(actions.iter().any(|a| a.key == 'W'));
            assert!(actions.iter().any(|a| a.key == 'K'));
            assert!(actions.iter().any(|a| a.key == 'D'));
            assert!(actions.iter().any(|a| a.key == 'T'));
        }
    }
}
