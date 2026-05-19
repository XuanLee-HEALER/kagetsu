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

use crate::ai::dummy::ai_choose_discard;
use crate::ai::timeout::default_action_on_timeout;
use crate::engine::domain::action::Action;
use crate::engine::domain::meld::{MeldKind, Seat};
use crate::engine::domain::tile::{Tile, TileIndex};
use crate::engine::event::GameEvent;
use crate::engine::phase::Phase;
use crate::engine::round_state::RoundResult;
use crate::engine::rules::GameRules;
use crate::engine::score::final_ranking;
use crate::game_engine::{CallOptions, GameEngine};
use crate::ui::Transition;
use crate::ui::paint::{
    TileState, paint_back_column_wide, paint_back_row_wide, paint_boxed_row_hl,
    paint_discard_grid_wide_hl, paint_double_box, paint_fill, paint_hr, paint_hr_accent,
    paint_meld_row_tight_hl, paint_str, paint_tile_tight, paint_tile_wide,
};
use crate::ui::theme::Theme;
use crate::ui::widgets::seat_label;
use unicode_width::UnicodeWidthStr;

const PLAYER_SEAT: Seat = Seat::East;
/// AI 操作的节流时间, 让玩家看清.
const AI_STEP_DELAY_MS: u64 = 350;

pub struct GameScreenState {
    pub engine: GameEngine,
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
    /// Action Modal 是否打开.
    pub modal_open: bool,
    /// Modal 当前选中项.
    pub modal_selected: usize,
    /// 进入 RoundEnd 的时刻, 用于流局后 N 秒自动推进.
    pub round_end_at: Option<Instant>,
    /// 当前主题 (本地偏好, 不绑 GameRules).
    pub theme_kind: crate::ui::theme::ThemeKind,
    /// 多选吃法时弹出的 picker. 非 None 时优先吃所有按键.
    pub chi_picker: Option<crate::ui::chi_picker::ChiPicker>,
    /// 全局录像开关 (来自 LocalPrefs.dev.record_replays). feature 关时
    /// 字段还在但永远不被读, F8 也不会改它.
    pub record_replays: bool,
    /// 当前局的初始 engine snapshot. 局开始时若 record_replays 为
    /// true 则填充, RoundEnd 时连同 engine.recorded_actions 一起 flush.
    #[cfg(feature = "dev-tools")]
    pub recording_initial: Option<GameEngine>,
}

impl GameScreenState {
    pub fn new(
        config: GameRules,
        game_seed: u64,
        theme_kind: crate::ui::theme::ThemeKind,
        record_replays: bool,
    ) -> Self {
        let mut g = GameEngine::new(config);
        g.start_round(game_seed ^ 1);
        #[allow(unused_mut)]
        let mut s = Self {
            engine: g,
            selected: 0,
            player_calls: None,
            calls_resolved: false,
            game_seed,
            round_index: 1,
            last_step_at: Instant::now(),
            message: String::from("东 1 局开始. 你是东家(亲)."),
            decision_deadline: None,
            modal_open: false,
            modal_selected: 0,
            round_end_at: None,
            theme_kind,
            chi_picker: None,
            record_replays,
            #[cfg(feature = "dev-tools")]
            recording_initial: None,
        };
        #[cfg(feature = "dev-tools")]
        s.maybe_start_recording();
        // record_replays 在 feature off 时只是个 dead bool, 静默.
        let _ = record_replays;
        s
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

        match self.engine.phase() {
            Phase::Deal => {
                self.round_index += 1;
                let seed = self.game_seed ^ self.round_index;
                self.engine.start_round(seed);
                self.selected = 0;
                self.player_calls = None;
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.clear_deadline();
                #[cfg(feature = "dev-tools")]
                self.maybe_start_recording();
            }
            Phase::Draw => {
                if self.engine.do_draw().is_none() {
                    // engine 内部已自动转 RoundEnd + 填 last_result (山摸尽 → 流局).
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
                    let action = ai_choose_discard(&self.engine.round);
                    self.apply_ai_action(action);
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                } else if self.player().riichi && !self.engine.can_tsumo() {
                    // 立直后强制摸切 (除非可自摸, 留给玩家按 W 决定).
                    // 走 AI 节流让玩家看到摸到的牌再切出.
                    self.update_self_message();
                    if self.last_step_at.elapsed().as_millis() >= AI_STEP_DELAY_MS as u128 {
                        self.try_player_tsumogiri();
                    }
                } else {
                    self.update_self_message();
                    self.set_deadline_if_unset();
                }
            }
            Phase::AwaitCalls => {
                if self.calls_resolved {
                    self.engine.advance_turn();
                    self.calls_resolved = false;
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                    return None;
                }
                self.calls_resolved = true;
                let from = self.engine.last_discard().map(|(s, _)| s);
                let Some(from) = from else {
                    self.engine.advance_turn();
                    return None;
                };

                // 1) 先看 AI 谁能荣和(头跳).
                for s in ron_check_order(from) {
                    if s == PLAYER_SEAT {
                        continue;
                    }
                    if let Some(score) = self.engine.try_ron(s) {
                        self.engine.declare_ron(s, score);
                        self.message = format!("{} 荣和!", seat_label(s));
                        return None;
                    }
                }

                // 2) 玩家是否有响应选项?
                if from != PLAYER_SEAT {
                    let opts = self.engine.legal_calls(PLAYER_SEAT);
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
                self.engine.advance_turn();
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            Phase::RoundEnd => {
                // 首次进入 RoundEnd: 设置 message + flush 录像.
                if self.round_end_at.is_none() {
                    self.round_end_at = Some(Instant::now());
                    #[cfg(feature = "dev-tools")]
                    self.flush_recording_if_any();
                    if let Some(result) = self.engine.last_result.clone() {
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
                // 流局/和牌都等用户按 N 主动推进 (next_round).
            }
            Phase::GameEnd => {
                let rankings = final_ranking(self.engine.players(), self.engine.rules());
                return Some(Transition::EnterGameOver { rankings });
            }
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        // Dev-only savestate (F5/F9). 优先级最高: chi picker / modal / riichi lock
        // 都不拦截, 任何时刻都能存读档.
        #[cfg(feature = "dev-tools")]
        if matches!(key.code, KeyCode::F(5)) {
            self.dev_quick_save();
            return None;
        }
        #[cfg(feature = "dev-tools")]
        if matches!(key.code, KeyCode::F(9)) {
            self.dev_quick_load();
            return None;
        }
        // ChiPicker 打开: 优先吃所有按键.
        if let Some(picker) = self.chi_picker.as_mut() {
            use crate::ui::chi_picker::ChiOutcome;
            match picker.handle_key(key) {
                ChiOutcome::Pick(idx) => {
                    self.chi_picker = None;
                    self.do_chi_at(idx);
                }
                ChiOutcome::Cancel => {
                    self.chi_picker = None;
                    self.message = "取消吃.".into();
                }
                ChiOutcome::Pending => {}
            }
            return None;
        }
        // Modal 打开: 优先处理 modal 键.
        if self.modal_open {
            return self.handle_modal_key(key);
        }
        // 立直后游戏内操作锁: 除 W (自摸/荣和) / C (跳过响应) / Esc (回主菜单)
        // 外其它键全部 noop. 自动摸切由 advance() 处理.
        if self.is_riichi_locked() {
            return self.handle_riichi_locked_key(key);
        }
        match key.code {
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
                if self.is_player_turn() && self.engine.phase() == Phase::AwaitDiscard {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if self.is_player_turn() && self.engine.phase() == Phase::AwaitDiscard {
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
                if self.engine.phase() == Phase::RoundEnd {
                    // 推 mat (kyoku/dealer/honba/riichi_sticks_pool 推进 + 检测整庄结束).
                    self.engine.next_round();
                    // 整庄结束 → 不起新一局, advance() 下次会转 EnterGameOver.
                    if !self.engine.mat.ended {
                        // 起新一局: 算下一 seed + reset 局内 UI 状态.
                        self.round_index += 1;
                        let seed = self.game_seed ^ self.round_index;
                        self.engine.start_round(seed);
                        self.selected = 0;
                        self.player_calls = None;
                        self.calls_resolved = false;
                        #[cfg(feature = "dev-tools")]
                        self.maybe_start_recording();
                    }
                    self.round_end_at = None;
                    self.message.clear();
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                }
            }
            // 数字 1-9 选第 N 张牌 (索引 selectable_tiles, 不含摸到的).
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                if self.is_player_turn() && self.engine.phase() == Phase::AwaitDiscard {
                    let idx = (c.to_digit(10).unwrap() - 1) as usize;
                    let len = self.selectable_count();
                    if idx < len {
                        self.selected = idx;
                    }
                }
            }
            KeyCode::Esc => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "回主菜单",
                        "确定离开当前对局回主菜单? 进度会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::BackToMainMenu,
                });
            }
            _ => {}
        }
        None
    }

    /// 立直后是否锁定游戏内操作键 (AwaitDiscard / AwaitCalls 阶段).
    fn is_riichi_locked(&self) -> bool {
        self.player().riichi
            && matches!(
                self.engine.phase(),
                Phase::AwaitDiscard | Phase::AwaitCalls | Phase::Draw
            )
    }

    /// 立直锁定模式下只允许 W / C / Esc.
    fn handle_riichi_locked_key(&mut self, key: KeyEvent) -> Option<Transition> {
        match key.code {
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.try_player_win();
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if self.player_calls.is_some() {
                    self.player_calls = None;
                    self.message = "已跳过.".into();
                    self.last_step_at = Instant::now();
                    self.clear_deadline();
                }
            }
            KeyCode::Esc => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "回主菜单",
                        "确定离开当前对局回主菜单? 进度会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::BackToMainMenu,
                });
            }
            _ => {}
        }
        None
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
        if !self.is_player_turn() || self.engine.phase() != Phase::AwaitDiscard {
            return;
        }
        let p = self.player();
        if let Some(t) = p.last_drawn
            && self.engine.do_discard(t).is_ok()
        {
            self.calls_resolved = false;
            self.player_calls = None;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn update_self_message(&mut self) {
        let opts = self.engine.legal_self_options();
        // 立直中: 只剩 W 自摸或自动摸切.
        if self.player().riichi {
            let intro = if opts.tsumo {
                "立直中, 可自摸! W 和  T/Enter 摸切"
            } else {
                "立直中: 自动摸切..."
            };
            self.message = intro.into();
            return;
        }
        let mut hints = vec!["←/→ 选".to_string(), "Enter 切".to_string()];
        if opts.tsumo {
            hints.push("W 自摸".into());
        }
        if !opts.riichi_discards.is_empty() {
            hints.push(format!(
                "R 立直 (高亮 {} 张可立, ←/→ 选后按 R)",
                opts.riichi_discards.len()
            ));
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
                let _ = self.engine.do_discard(t);
            }
            Action::Tsumo => {
                if let Some(score) = self.engine.try_tsumo() {
                    let winner = self.engine.turn();
                    self.engine.declare_tsumo(score);
                    self.message = format!("{} 自摸!", seat_label(winner));
                }
            }
            Action::Ron(seat) => {
                if let Some(score) = self.engine.try_ron(seat) {
                    self.engine.declare_ron(seat, score);
                    self.message = format!("{} 荣和!", seat_label(seat));
                }
            }
            _ => {}
        }
    }

    fn apply_timeout_default(&mut self) {
        let action = default_action_on_timeout(&self.engine.round);
        match action {
            Action::Discard(t) => {
                if self.engine.do_discard(t).is_ok() {
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
        if let Some(secs) = self.engine.rules().thinking_time_secs {
            self.decision_deadline = Some(Instant::now() + Duration::from_secs(secs as u64));
        }
    }

    fn clear_deadline(&mut self) {
        self.decision_deadline = None;
    }

    /// 切换主题 (供全局 T 键调用).
    pub fn set_theme(&mut self, kind: crate::ui::theme::ThemeKind) {
        self.theme_kind = kind;
    }

    /// F5: dev quick save. 失败时把错误写到 message.
    #[cfg(feature = "dev-tools")]
    fn dev_quick_save(&mut self) {
        match crate::dev::savestate::save(&self.engine, "quick") {
            Ok(path) => {
                self.message = format!("[DEV] 存档 → {}", path.display());
            }
            Err(e) => {
                self.message = format!("[DEV] 存档失败: {}", e);
            }
        }
    }

    /// F9: dev quick load. 替换 self.game 并复位 UI 派生 state.
    #[cfg(feature = "dev-tools")]
    fn dev_quick_load(&mut self) {
        match crate::dev::savestate::load("quick") {
            Ok(g) => {
                self.engine = g;
                // UI 派生 state 全部复位, 因为切屏/局面变了.
                self.selected = 0;
                self.player_calls = None;
                self.calls_resolved = false;
                self.last_step_at = Instant::now();
                self.decision_deadline = None;
                self.modal_open = false;
                self.modal_selected = 0;
                self.round_end_at = None;
                self.chi_picker = None;
                self.message = "[DEV] 已读档.".into();
            }
            Err(e) => {
                self.message = format!("[DEV] 读档失败: {}", e);
            }
        }
    }

    /// 局开始时 (start_round 后) 若开启录像则 snapshot 初始 engine + 启 actions buf.
    #[cfg(feature = "dev-tools")]
    fn maybe_start_recording(&mut self) {
        if !self.record_replays {
            return;
        }
        // 录像 hook 由 GameEngine.apply 自动 push, 这里只做 snapshot + 启 buffer.
        self.recording_initial = Some(self.engine.clone());
        self.engine.recorded_actions = Some(Vec::new());
    }

    /// RoundEnd 时把当前局 (initial snapshot + actions) 写到 recordings/
    /// 目录, 以 unix 时间戳作 filename.
    #[cfg(feature = "dev-tools")]
    fn flush_recording_if_any(&mut self) {
        let Some(initial) = self.recording_initial.take() else {
            self.engine.recorded_actions = None;
            return;
        };
        let actions = self.engine.recorded_actions.take().unwrap_or_default();
        let rec = crate::dev::recorder::RoundRecording {
            initial_state: initial,
            actions,
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let filename = format!("round_{}", ts);
        let _ = crate::dev::recorder::save(&rec, &filename);
    }

    /// F8 (App 层调用) 切换录像开关. 当前局已开始的录像不变 (flag 仅
    /// 影响下一局).
    #[cfg(feature = "dev-tools")]
    pub fn set_record_replays(&mut self, v: bool) {
        self.record_replays = v;
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
        self.engine.turn() == PLAYER_SEAT
    }

    fn player(&self) -> &crate::engine::player::PlayerState {
        &self.engine.players()[PLAYER_SEAT.index()]
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

    /// 当前 selected 手牌的 kind (用于河/副露/手牌联动高亮).
    /// 仅自家 AwaitDiscard 阶段返回 Some, 其它阶段返回 None.
    fn highlight_kind(&self) -> Option<crate::engine::domain::tile::TileIndex> {
        if !self.is_player_turn() || self.engine.phase() != Phase::AwaitDiscard {
            return None;
        }
        self.selectable_tiles().get(self.selected).map(|t| t.kind)
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
        if !self.is_player_turn() || self.engine.phase() != Phase::AwaitDiscard {
            return;
        }
        // 立直后只能摸切: Enter/Space/D 退化为摸切刚摸到的牌.
        if self.player().riichi {
            self.try_player_tsumogiri();
            return;
        }
        let tiles = self.selectable_tiles();
        let Some(&t) = tiles.get(self.selected) else {
            return;
        };
        if self.engine.do_discard(t).is_ok() {
            self.calls_resolved = false;
            self.player_calls = None;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    fn try_player_win(&mut self) {
        if self.is_player_turn()
            && self.engine.phase() == Phase::AwaitDiscard
            && let Some(score) = self.engine.try_tsumo()
        {
            self.engine.declare_tsumo(score);
            self.message = format!("{} 自摸!", seat_label(PLAYER_SEAT));
            self.player_calls = None;
            self.clear_deadline();
            return;
        }
        if let Some(opts) = &self.player_calls
            && opts.ron
            && let Some(score) = self.engine.try_ron(PLAYER_SEAT)
        {
            self.engine.declare_ron(PLAYER_SEAT, score);
            self.message = format!("{} 荣和!", seat_label(PLAYER_SEAT));
            self.player_calls = None;
            self.clear_deadline();
        }
    }

    fn try_player_riichi(&mut self) {
        if !self.is_player_turn() || self.engine.phase() != Phase::AwaitDiscard {
            return;
        }
        let opts = self.engine.legal_self_options();
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
        match self.engine.do_riichi(t) {
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
        if !self.is_player_turn() || self.engine.phase() != Phase::AwaitDiscard {
            return;
        }
        let opts = self.engine.legal_self_options();
        if let Some(kind) = opts.ankan.first().copied() {
            if let Err(e) = self.engine.do_ankan(kind) {
                self.message = format!("暗杠失败: {}", e);
            } else {
                self.message = format!("暗杠 {}!", kind.short());
                self.selected = 0;
                self.last_step_at = Instant::now();
                self.clear_deadline();
            }
            return;
        }
        if let Some(kind) = opts.shouminkan.first().copied() {
            if let Err(e) = self.engine.do_shouminkan(kind) {
                self.message = format!("加杠失败: {}", e);
            } else {
                self.message = format!("加杠 {}!", kind.short());
                self.selected = 0;
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
        if let Err(e) = self.engine.do_pon(PLAYER_SEAT, two) {
            self.message = format!("碰失败: {}", e);
        } else {
            self.message = "碰!".into();
            self.selected = 0;
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
        if opts.chi.is_empty() {
            self.message = "不能吃.".into();
            return;
        }
        if opts.chi.len() == 1 {
            self.do_chi_at(0);
            return;
        }
        // ≥ 2 种吃法 → 弹 picker. target = 别人切的牌.
        let Some((_, target)) = self.engine.last_discard() else {
            self.message = "找不到弃牌目标.".into();
            return;
        };
        self.chi_picker = Some(crate::ui::chi_picker::ChiPicker::new(
            opts.chi.clone(),
            target,
        ));
    }

    fn do_chi_at(&mut self, idx: usize) {
        let Some(opts) = self.player_calls.clone() else {
            return;
        };
        let Some(&two) = opts.chi.get(idx) else {
            self.message = "无效吃法 idx.".into();
            return;
        };
        if let Err(e) = self.engine.do_chi(PLAYER_SEAT, two) {
            self.message = format!("吃失败: {}", e);
        } else {
            self.message = "吃!".into();
            self.selected = 0;
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
        if let Err(e) = self.engine.do_minkan(PLAYER_SEAT, three) {
            self.message = format!("明杠失败: {}", e);
        } else {
            self.message = "明杠!".into();
            self.selected = 0;
            self.player_calls = None;
            self.calls_resolved = false;
            self.last_step_at = Instant::now();
            self.clear_deadline();
        }
    }

    // ============== 渲染 (HiFi-05 设计稿坐标) ==============

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = Theme::from_kind(self.theme_kind);
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
        if let Some(picker) = &self.chi_picker {
            picker.render(buf, area, &theme);
        }
    }

    /// row 0-1: 顶部 status bar.
    fn paint_top_status(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let g = &self.engine;
        // 局 / 本场 / 立直棒
        let round_label = format!("{} {} 局", g.round_wind().label(), g.kyoku());
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
            &format!("{}本", g.honba()),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if g.riichi_sticks() > 0 {
            paint_str(
                buf,
                ox + 15,
                oy,
                &format!("{}供", g.riichi_sticks()),
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
        let junme = g.players()[0].river.len() + 1;
        let wall_left = g.wall().as_ref().map(|w| w.remaining()).unwrap_or(0);
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
        if let Some(wall) = g.wall().as_ref()
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
            (
                Seat::East,
                "東",
                g.players()[0].score,
                g.players()[0].riichi,
            ),
            (
                Seat::South,
                "南",
                g.players()[1].score,
                g.players()[1].riichi,
            ),
            (
                Seat::West,
                "西",
                g.players()[2].score,
                g.players()[2].riichi,
            ),
            (
                Seat::North,
                "北",
                g.players()[3].score,
                g.players()[3].riichi,
            ),
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
        // dev-tools 录像指示 (col 116-118): 当前局正在录时显示 REC.
        #[cfg(feature = "dev-tools")]
        if self.recording_initial.is_some() {
            paint_str(
                buf,
                ox + 116,
                oy,
                "REC",
                Style::default()
                    .fg(theme.danger)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
        // 时钟 / 标题
        paint_str(
            buf,
            ox + 120,
            oy,
            "tui-majo",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        paint_str(
            buf,
            ox + 130,
            oy,
            &format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second()),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_hr(buf, ox, oy + 1, 144, theme);
    }

    /// row 3-9: 对家 (West).
    fn paint_opponent_top(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.engine.players()[Seat::West.index()];
        // 标题行 row 3
        paint_str(
            buf,
            ox + 66,
            oy + 3,
            "─── 对家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let wind = self.engine.seat_wind_of(Seat::West).short();
        paint_str(
            buf,
            ox + 75,
            oy + 3,
            &wind,
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
                paint_meld_row_tight_hl(buf, col, oy + 5, &tiles, theme, self.highlight_kind());
                col += (tiles.len() as u16) * 3 + 1;
            }
        }
        // 牌河 row 6-9
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide_hl(
            buf,
            ox + 54,
            oy + 6,
            &p.river,
            theme,
            riichi_at,
            self.highlight_kind(),
        );
    }

    /// row 6-19 左侧: 上家 (North).
    fn paint_opponent_left(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.engine.players()[Seat::North.index()];
        paint_str(
            buf,
            ox + 2,
            oy + 6,
            "上家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let wind = self.engine.seat_wind_of(Seat::North).short();
        let label = if p.riichi {
            format!("{} {}★", wind, p.score)
        } else {
            format!("{} {}", wind, p.score)
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
                paint_meld_row_tight_hl(buf, col, row, &tiles, theme, self.highlight_kind());
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
        paint_discard_grid_wide_hl(
            buf,
            ox + 20,
            oy + 12,
            &p.river,
            theme,
            riichi_at,
            self.highlight_kind(),
        );
    }

    /// row 6-19 右侧: 下家 (South).
    fn paint_opponent_right(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let p = &self.engine.players()[Seat::South.index()];
        paint_str(
            buf,
            ox + 132,
            oy + 6,
            "下家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let wind = self.engine.seat_wind_of(Seat::South).short();
        let label = if p.riichi {
            format!("{} {}★", wind, p.score)
        } else {
            format!("{} {}", wind, p.score)
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
                paint_meld_row_tight_hl(buf, col, row, &tiles, theme, self.highlight_kind());
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
        paint_discard_grid_wide_hl(
            buf,
            ox + 92,
            oy + 12,
            &p.river,
            theme,
            riichi_at,
            self.highlight_kind(),
        );
    }

    /// row 17-18: 中央 dora + 山数提示.
    fn paint_center_info(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        let g = &self.engine;
        paint_str(
            buf,
            ox + 66,
            oy + 17,
            "宝",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if let Some(wall) = g.wall().as_ref()
            && let Some(t) = wall.dora_indicators().first()
        {
            paint_tile_wide(buf, ox + 70, oy + 17, Some(t), theme, TileState::Normal);
        }
        let wall_left = g.wall().as_ref().map(|w| w.remaining()).unwrap_or(0);
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
        let p = &self.engine.players()[PLAYER_SEAT.index()];
        let riichi_at = riichi_index_in_river(p);
        paint_discard_grid_wide_hl(
            buf,
            ox + 54,
            oy + 23,
            &p.river,
            theme,
            riichi_at,
            self.highlight_kind(),
        );
    }

    /// row 28-29: 自家分割线 + 状态行.
    fn paint_my_status(&self, buf: &mut Buffer, ox: u16, oy: u16, theme: &Theme) {
        paint_hr_accent(buf, ox + 2, oy + 28, 140, theme);
        let p = &self.engine.players()[PLAYER_SEAT.index()];
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
        // 玩家固定坐东侧 (PLAYER_SEAT = East), 但自风随庄家轮转: 东1=東, 东2=北, …
        let player_wind = self.engine.seat_wind_of(PLAYER_SEAT);
        let dealer_str = if PLAYER_SEAT == self.engine.dealer() {
            format!("{} ◆庄", player_wind.short())
        } else {
            player_wind.short().to_string()
        };
        paint_str(
            buf,
            ox + 9,
            oy + 29,
            &dealer_str,
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
        // 听牌检测.
        // 13 张型 (刚切完): closed + melds*3 = 13. 直接算 tenpai.
        // 14 张型 (摸完未切): closed + melds*3 = 14. 排除 last_drawn 那张后是 13 张型,
        //                                            按那个算 (相当于"摸切立即听")
        // 副露 0/1/2/3/4 → closed 13/10/7/4/1, 加 last_drawn 摸完是 14/11/8/5/2.
        // 杠虽 4 张但占 1 面子, 公式仍是 *3.
        let total = p.hand.closed.len() + p.hand.melds.len() * 3;
        let waits = if total == 13 {
            crate::engine::domain::decompose::tenpai_tiles(
                &crate::engine::domain::tile::count_by_kind(&p.hand.closed),
                &p.hand.melds,
            )
        } else if total == 14 {
            // 摸完未切: 把 last_drawn 排除后剩 13 张算 tenpai.
            if let Some(drawn) = p.last_drawn {
                let mut counts = crate::engine::domain::tile::count_by_kind(&p.hand.closed);
                counts[drawn.kind.0 as usize] = counts[drawn.kind.0 as usize].saturating_sub(1);
                crate::engine::domain::decompose::tenpai_tiles(&counts, &p.hand.melds)
            } else {
                // 极端: 14 张但无 last_drawn (鸣牌后状态不应到此, 但兜底).
                Vec::new()
            }
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
        let any_riichi = (1..=3).any(|i| self.engine.players()[i].riichi);
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
            let style = match self.engine.phase() {
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
        let p = &self.engine.players()[PLAYER_SEAT.index()];
        let mut display: Vec<Tile> = self.selectable_tiles();
        let drawn_idx = p.last_drawn.map(|t| {
            display.push(t);
            display.len() - 1
        });
        let selected_player = self.is_player_turn() && self.engine.phase() == Phase::AwaitDiscard;
        let selectable_len = drawn_idx.unwrap_or(display.len());
        let selected = if selected_player && self.selected < selectable_len {
            Some(self.selected)
        } else {
            None
        };
        paint_boxed_row_hl(
            buf,
            ox + 4,
            oy + 31,
            &display,
            theme,
            selected,
            drawn_idx,
            self.highlight_kind(),
        );
        // 编号 row 35 (与 paint_boxed_row 同样的间隙规则: drawn 前留 3 cells).
        // 切后能进入听牌的牌 (legal_self_options.riichi_discards) 用 danger
        // 高亮, 玩家用 ←/→ 选中再 R 立直.
        let opts = self.engine.legal_self_options();
        let riichi_kinds: std::collections::HashSet<u8> =
            opts.riichi_discards.iter().map(|t| t.kind.0).collect();
        let drawn_gap = 3u16;
        let mut cx = ox + 4 + 1;
        for (i, t) in display.iter().enumerate() {
            if Some(i) == drawn_idx && i > 0 {
                cx += drawn_gap;
            }
            let style = if riichi_kinds.contains(&t.kind.0) {
                Style::default()
                    .fg(theme.danger)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim).bg(theme.bg)
            };
            paint_str(buf, cx, oy + 35, &format!("{:>2}", i + 1), style);
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
            .engine
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
        // row 38: 主操作快捷键 hint
        paint_str(
            buf,
            ox + 2,
            oy + 38,
            "提示",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        self.paint_key_hint(buf, ox + 8, oy + 38, "m", theme.line, theme.fg);
        paint_str(
            buf,
            ox + 11,
            oy + 38,
            "菜单 ·",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        paint_str(
            buf,
            ox + 23,
            oy + 38,
            "[1-9]",
            Style::default().fg(theme.ok).bg(theme.panel),
        );
        paint_str(
            buf,
            ox + 29,
            oy + 38,
            "选牌 ·",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        self.paint_key_hint(buf, ox + 37, oy + 38, "d", theme.line, theme.fg);
        paint_str(
            buf,
            ox + 40,
            oy + 38,
            "切 ·",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        self.paint_key_hint(buf, ox + 46, oy + 38, "t", theme.line, theme.fg);
        paint_str(
            buf,
            ox + 49,
            oy + 38,
            "摸切 ·",
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        self.paint_key_hint(buf, ox + 57, oy + 38, "R", theme.danger, theme.danger);
        paint_str(
            buf,
            ox + 60,
            oy + 38,
            "立直",
            Style::default().fg(theme.danger).bg(theme.panel),
        );
        self.paint_key_hint(buf, ox + 66, oy + 38, "W", theme.ok, theme.ok);
        paint_str(
            buf,
            ox + 69,
            oy + 38,
            "自摸",
            Style::default().fg(theme.ok).bg(theme.panel),
        );
        paint_str(
            buf,
            ox + 132,
            oy + 38,
            "←→ 选牌",
            Style::default().fg(theme.dim).bg(theme.panel),
        );

        // row 39: 鸣牌响应 + 全局
        paint_str(
            buf,
            ox + 2,
            oy + 39,
            "响应",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        #[cfg(feature = "dev-tools")]
        let pairs: &[(&str, &str)] = &[
            ("P", "碰"),
            ("A", "吃"),
            ("M", "明杠"),
            ("K", "暗杠"),
            ("C", "跳过"),
            ("N", "下一局"),
            ("F5", "存档"),
            ("F8", "录像"),
            ("F9", "读档"),
            ("Esc", "回"),
            ("Q", "退"),
        ];
        #[cfg(not(feature = "dev-tools"))]
        let pairs: &[(&str, &str)] = &[
            ("P", "碰"),
            ("A", "吃"),
            ("M", "明杠"),
            ("K", "暗杠"),
            ("C", "跳过"),
            ("N", "下一局"),
            ("L", "离开"),
            ("Esc", "回主菜单"),
            ("Q", "退出"),
        ];
        let mut col = ox + 9;
        let cmd_style = Style::default().fg(theme.fg).bg(theme.bg);
        let hint_style = Style::default().fg(theme.dim).bg(theme.bg);
        for (key, label) in pairs {
            let key_w = UnicodeWidthStr::width(*key) as u16;
            let lbl_text = format!(" {} ·", label);
            let lbl_w = UnicodeWidthStr::width(lbl_text.as_str()) as u16;
            if col + key_w + lbl_w + 1 >= ox + 144 {
                break;
            }
            paint_str(buf, col, oy + 39, key, cmd_style);
            paint_str(buf, col + key_w, oy + 39, &lbl_text, hint_style);
            col += key_w + lbl_w + 1;
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
                .bg(Theme::from_kind(self.theme_kind).panel)
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
        let g = &self.engine;
        let junme = g.players()[0].river.len() + 1;
        paint_str(
            buf,
            mx + 2,
            my + 2,
            &format!("巡 {}", junme),
            Style::default().fg(theme.dim).bg(theme.panel),
        );
        if let Some(t) = g.players()[PLAYER_SEAT.index()].last_drawn {
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

/// 找到玩家立直时弃出的牌在 river 里的索引. PlayerState.riichi_river_idx
/// 由 do_riichi 写入, 此处直接转发.
fn riichi_index_in_river(p: &crate::engine::player::PlayerState) -> Option<usize> {
    p.riichi_river_idx
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
        if self.engine.phase() == Phase::AwaitDiscard && self.is_player_turn() {
            let opts = self.engine.legal_self_options();
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
            let in_riichi = self.engine.players()[PLAYER_SEAT.index()].riichi;
            out.push(ModalAction {
                key: 'D',
                label: "切牌",
                detail: if in_riichi {
                    "立直后只能摸切".into()
                } else {
                    "选择手牌中一张打出".into()
                },
                enabled: !in_riichi,
            });
            out.push(ModalAction {
                key: 'T',
                label: "摸切",
                detail: "切出刚摸到的牌(不变手牌)".into(),
                enabled: self.engine.players()[PLAYER_SEAT.index()]
                    .last_drawn
                    .is_some(),
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
        let mut app = GameScreenState::new(
            GameRules::default(),
            0xC0FFEE,
            crate::ui::theme::ThemeKind::Dark,
            false,
        );
        let backend = TestBackend::new(144, 40);
        let mut term = Terminal::new(backend).unwrap();

        for _ in 0..5000 {
            term.draw(|f| app.render(f, f.area())).unwrap();
            app.last_step_at = Instant::now() - Duration::from_secs(1);
            drain_pending(&mut app);

            if app.is_player_turn() && app.engine.phase() == Phase::AwaitDiscard {
                let drawn = app.engine.players()[PLAYER_SEAT.index()].last_drawn;
                if let Some(t) = drawn {
                    let _ = app.engine.do_discard(t);
                    app.calls_resolved = false;
                }
            } else {
                let _ = app.advance();
            }
            if app.engine.phase() == Phase::RoundEnd || app.engine.phase() == Phase::GameEnd {
                break;
            }
        }
        term.draw(|f| app.render(f, f.area())).unwrap();
        assert!(matches!(
            app.engine.phase(),
            Phase::RoundEnd | Phase::GameEnd
        ));
    }

    #[test]
    fn app_can_advance_through_multiple_rounds() {
        let mut app = GameScreenState::new(
            GameRules::default(),
            0xC0FFEE,
            crate::ui::theme::ThemeKind::Dark,
            false,
        );
        let backend = TestBackend::new(144, 40);
        let mut term = Terminal::new(backend).unwrap();

        let mut rounds = 0;
        for _ in 0..30000 {
            term.draw(|f| app.render(f, f.area())).unwrap();
            app.last_step_at = Instant::now() - Duration::from_secs(1);
            drain_pending(&mut app);

            match app.engine.phase() {
                Phase::AwaitDiscard if app.is_player_turn() => {
                    let drawn = app.engine.players()[PLAYER_SEAT.index()].last_drawn;
                    if let Some(t) = drawn {
                        let _ = app.engine.do_discard(t);
                        app.calls_resolved = false;
                    }
                }
                Phase::RoundEnd => {
                    app.engine.next_round();
                    rounds += 1;
                    if rounds >= 3 || app.engine.mat.ended {
                        break;
                    }
                    // 起新一局 (跟 N 键 handler 行为一致).
                    app.round_index += 1;
                    let seed = app.game_seed ^ app.round_index;
                    app.engine.start_round(seed);
                }
                Phase::GameEnd => break,
                _ => {
                    let _ = app.advance();
                }
            }
        }
        assert!(rounds >= 3 || app.engine.phase() == Phase::GameEnd);
    }

    #[test]
    fn modal_actions_in_await_discard() {
        let mut app = GameScreenState::new(
            GameRules::default(),
            0xC0FFEE,
            crate::ui::theme::ThemeKind::Dark,
            false,
        );
        // 模拟玩家摸牌阶段
        let _ = app.advance(); // Deal -> Draw
        let _ = app.advance(); // Draw 自家
        // 此时 phase 应是 AwaitDiscard, 是自家 (East)
        if app.is_player_turn() && app.engine.phase() == Phase::AwaitDiscard {
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
