//! 局域网游戏 · 局内. 完整 144x40 渲染, 数据源 = `GameStateView`.
//!
//! 与单机 [`crate::ui::screens::game::GameScreenState`] 的关键差异:
//! - 数据通过 `GameStateView` 投影 (他家手牌只有数量, 全是牌背).
//! - 自家位置不固定为东家, `view.my_seat` 决定底/右/对/左 4 家映射.
//! - 不实现 modal / command 模式 (Phase 4b 简化, 只用快捷键).
//! - 一切动作都通过 `NetAction` 上报 server, 客户端不直改 GameState.
//!
//! 操作:
//! - ← → / h l : 选手牌
//! - 1-9       : 直接选第 N 张
//! - Enter / d : 切选中牌
//! - t         : 摸切 (切刚摸到的牌)
//! - R         : 立直 (默认切选中牌)
//! - W         : 自摸
//! - K         : 暗杠 (server 定可暗杠的牌种)
//! - P / A / M : 碰 / 吃 / 明杠
//! - C         : 跳过 (他家弃牌窗口期)
//! - N         : 下一局 (RoundEnd)
//! - L         : 离开房间 → 主菜单

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::engine::domain::meld::{MeldKind, Seat};
use crate::engine::domain::tile::{Tile, TileIndex};
use crate::engine::event::GameEvent;
use crate::engine::phase::Phase;
use crate::net::protocol::{
    ClientMsg, GameStateView, NetAction, PlayerView, RoomLifecycle, ServerMsg,
};
use crate::net::session::NetSession;
use crate::ui::Transition;
use crate::ui::paint::{
    TileState, paint_back_column_wide, paint_back_row_wide, paint_boxed_row_hl,
    paint_discard_grid_wide_hl, paint_fill, paint_hr, paint_hr_accent, paint_meld_row_tight_hl,
    paint_str, paint_tile_tight, paint_tile_wide,
};
use crate::ui::screens::game::TileSpec;
use crate::ui::theme::{Theme, ThemeKind};

/// 自家相对其它三家的方位.
#[derive(Debug, Clone, Copy)]
struct SeatLayout {
    bottom: Seat, // 自家
    right: Seat,  // 下家 (next)
    top: Seat,    // 对家 (next.next)
    left: Seat,   // 上家 (next.next.next)
}

impl SeatLayout {
    fn from_my_seat(my: Seat) -> Self {
        let right = my.next();
        let top = right.next();
        let left = top.next();
        Self {
            bottom: my,
            right,
            top,
            left,
        }
    }
}

/// 把 PlayerView 数组按相对位置取出.
fn player_at(view: &GameStateView, seat: Seat) -> &PlayerView {
    view.players
        .iter()
        .find(|p| p.seat == seat)
        .expect("seat must exist in players[4]")
}

pub struct OnlineGameState {
    pub session: NetSession,
    pub state_view: Option<GameStateView>,
    pub message: String,
    /// 自家手牌选中索引 (与 selectable_tiles() 对应, 不指向 last_drawn).
    pub selected: usize,
    /// 主题 (从 App.local_prefs 拷一份).
    pub theme_kind: ThemeKind,
    /// server 推送的当前 ActionRequired (鸣牌窗口或思考倒计时).
    /// None = 没有未决动作.
    pub current_hints: Option<Vec<NetAction>>,
    /// 当前动作 deadline (unix ms). 0 表示无限期.
    pub current_deadline_ms: i64,
    /// 多选吃法时弹的 picker. 非 None 时优先吃所有按键.
    pub chi_picker: Option<crate::ui::chi_picker::ChiPicker>,
}

impl OnlineGameState {
    pub fn new(session: NetSession, theme_kind: ThemeKind) -> Self {
        Self {
            session,
            state_view: None,
            message: "等待 server 推送状态...".into(),
            selected: 0,
            theme_kind,
            current_hints: None,
            current_deadline_ms: 0,
            chi_picker: None,
        }
    }

    /// 从 my_hand + target tile 枚举可能的吃法 (本家 2 张组合).
    /// 算法与 server-side `engine::state::legal_calls` 对齐.
    fn enumerate_chi_options(my_hand: &[Tile], target: Tile) -> Vec<[Tile; 2]> {
        let kind = target.kind;
        if !kind.is_suupai() {
            return Vec::new();
        }
        let r = (kind.0 % 9) as i32;
        let suit_base = (kind.0 / 9) as i32 * 9;
        let mut counts = [0u8; 34];
        for t in my_hand {
            counts[t.kind.0 as usize] += 1;
        }
        let mut out = Vec::new();
        for (a, b) in [(-2i32, -1i32), (-1, 1), (1, 2)] {
            let na = r + a;
            let nb = r + b;
            if !(0..=8).contains(&na) || !(0..=8).contains(&nb) {
                continue;
            }
            let ka = (suit_base + na) as usize;
            let kb = (suit_base + nb) as usize;
            if counts[ka] > 0 && counts[kb] > 0 {
                let ta = *my_hand.iter().find(|t| t.kind.0 as usize == ka).unwrap();
                let tb = *my_hand.iter().find(|t| t.kind.0 as usize == kb).unwrap();
                out.push([ta, tb]);
            }
        }
        out
    }

    /// 找 events 里最近一条 Discard 事件的 tile (用于 chi target).
    fn last_discard_tile(view: &GameStateView) -> Option<Tile> {
        for ev in view.events.iter().rev() {
            if let GameEvent::Discard { tile, .. } = ev {
                return Some(*tile);
            }
        }
        None
    }

    pub fn my_player_id(&self) -> u32 {
        self.session.player_id
    }

    pub fn advance(&mut self) -> Option<Transition> {
        while let Some(msg) = self.session.try_recv() {
            if let Some(t) = self.handle_msg(msg) {
                return Some(t);
            }
        }
        if self.session.is_disconnected() && !self.message.contains("断开") {
            self.message = "连接断开".into();
        }
        None
    }

    fn handle_msg(&mut self, msg: ServerMsg) -> Option<Transition> {
        match msg {
            ServerMsg::GameStateView(view) => {
                let new_view = *view;
                let max = self.selectable_count(&new_view);
                if self.selected >= max && max > 0 {
                    self.selected = max - 1;
                }
                // 收到新 state, 旧 hints 失效 (新一轮才会再发 ActionRequired)
                if !matches!(new_view.phase, crate::engine::phase::Phase::AwaitCalls) {
                    self.current_hints = None;
                    self.current_deadline_ms = 0;
                }
                self.state_view = Some(new_view);
                self.message.clear();
            }
            ServerMsg::ActionRequired {
                hints,
                deadline_unix_ms,
            } => {
                self.current_hints = Some(hints);
                self.current_deadline_ms = deadline_unix_ms;
            }
            ServerMsg::RoundResult(r) => {
                self.message = format!("局结算: {} | 分数 {:?}", r.message, r.scores);
                self.current_hints = None;
            }
            ServerMsg::GameEnd(_) => {
                self.message = "整庄结束, 按 N 回房间, L 回主菜单".into();
                self.current_hints = None;
            }
            ServerMsg::BackToRoom => {
                self.message = "回房间".into();
            }
            ServerMsg::RoomUpdate(view) => {
                if view.state == RoomLifecycle::Lobby {
                    return Some(Transition::EnterMainMenu);
                }
            }
            ServerMsg::Error { message } => {
                self.message = message;
            }
            _ => {}
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        // ChiPicker 打开时优先吃所有按键.
        if let Some(picker) = self.chi_picker.as_mut() {
            use crate::ui::chi_picker::ChiOutcome;
            match picker.handle_key(key) {
                ChiOutcome::Pick(idx) => {
                    self.chi_picker = None;
                    self.session.send(ClientMsg::Action(NetAction::Chi(idx)));
                }
                ChiOutcome::Cancel => {
                    self.chi_picker = None;
                    self.message = "取消吃.".into();
                }
                ChiOutcome::Pending => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.move_select(-1),
            KeyCode::Right | KeyCode::Char('l') if !key.modifiers.is_empty() => self.move_select(1),
            KeyCode::Right => self.move_select(1),
            KeyCode::Char('1')
            | KeyCode::Char('2')
            | KeyCode::Char('3')
            | KeyCode::Char('4')
            | KeyCode::Char('5')
            | KeyCode::Char('6')
            | KeyCode::Char('7')
            | KeyCode::Char('8')
            | KeyCode::Char('9') => {
                if let KeyCode::Char(c) = key.code
                    && let Some(d) = c.to_digit(10)
                {
                    let idx = (d as usize).saturating_sub(1);
                    let max = self
                        .state_view
                        .as_ref()
                        .map(|v| self.selectable_count(v))
                        .unwrap_or(0);
                    if idx < max {
                        self.selected = idx;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('D') => {
                self.do_discard_selected();
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                self.do_tsumogiri();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.do_riichi_selected();
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.session.send(ClientMsg::Action(NetAction::Tsumo));
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                self.session.send(ClientMsg::Action(NetAction::Pon));
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.do_chi();
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                self.session.send(ClientMsg::Action(NetAction::Minkan));
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                self.do_ankan();
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.session.send(ClientMsg::Action(NetAction::Pass));
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.session.send(ClientMsg::Action(NetAction::NextRound));
            }
            KeyCode::Char('L') => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "离开对局",
                        "确定离开当前对局回主菜单? 进度会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::LeaveOnlineGame,
                });
            }
            KeyCode::Esc => {
                return Some(Transition::RequestConfirm {
                    modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                        "回主菜单",
                        "确定离开当前对局回主菜单? 进度会丢失.",
                    )),
                    action: crate::ui::ConfirmAction::LeaveOnlineGame,
                });
            }
            _ => {}
        }
        None
    }

    fn move_select(&mut self, delta: i32) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        let max = self.selectable_count(view);
        if max == 0 {
            return;
        }
        let cur = self.selected as i32;
        let new = (cur + delta).rem_euclid(max as i32);
        self.selected = new as usize;
    }

    /// 自家可选牌数 = 手牌中除 last_drawn 之外的张数.
    /// 当前 selected 手牌的 kind (用于河/副露/手牌联动高亮).
    /// 仅自家 AwaitDiscard 阶段返回 Some, 其它阶段返回 None.
    fn highlight_kind(&self, view: &GameStateView) -> Option<TileIndex> {
        if view.turn != view.my_seat || view.phase != Phase::AwaitDiscard {
            return None;
        }
        let (sel, _) = Self::split_hand(view);
        sel.get(self.selected).map(|t| t.kind)
    }

    fn selectable_count(&self, view: &GameStateView) -> usize {
        if let Some(d) = view.my_last_drawn {
            // 找一张匹配 d 的位置, 那一张就是 last_drawn (不可选).
            let mut found = false;
            let mut count = 0;
            for t in &view.my_hand {
                if !found && tiles_eq(t, &d) {
                    found = true;
                    continue;
                }
                count += 1;
            }
            if !found { view.my_hand.len() } else { count }
        } else {
            view.my_hand.len()
        }
    }

    /// 返回 (selectable_tiles, drawn_tile) 用于渲染.
    fn split_hand(view: &GameStateView) -> (Vec<Tile>, Option<Tile>) {
        if let Some(d) = view.my_last_drawn {
            let mut sel: Vec<Tile> = Vec::with_capacity(view.my_hand.len());
            let mut drawn_extracted = false;
            for t in &view.my_hand {
                if !drawn_extracted && tiles_eq(t, &d) {
                    drawn_extracted = true;
                    continue;
                }
                sel.push(*t);
            }
            sel.sort_by_key(|t| t.kind.0);
            if drawn_extracted {
                (sel, Some(d))
            } else {
                let mut all = view.my_hand.clone();
                all.sort_by_key(|t| t.kind.0);
                (all, None)
            }
        } else {
            let mut all = view.my_hand.clone();
            all.sort_by_key(|t| t.kind.0);
            (all, None)
        }
    }

    fn do_discard_selected(&mut self) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        let (sel_tiles, _drawn) = Self::split_hand(view);
        if let Some(t) = sel_tiles.get(self.selected) {
            self.session
                .send(ClientMsg::Action(NetAction::Discard(TileSpec {
                    kind: t.kind,
                })));
        }
    }

    fn do_tsumogiri(&mut self) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        if let Some(d) = view.my_last_drawn {
            self.session
                .send(ClientMsg::Action(NetAction::Discard(TileSpec {
                    kind: d.kind,
                })));
        }
    }

    fn do_riichi_selected(&mut self) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        let (sel_tiles, drawn) = Self::split_hand(view);
        let kind = if let Some(t) = sel_tiles.get(self.selected) {
            Some(t.kind)
        } else {
            drawn.map(|d| d.kind)
        };
        if let Some(k) = kind {
            self.session
                .send(ClientMsg::Action(NetAction::Riichi(TileSpec { kind: k })));
        }
    }

    /// 暗杠: 简化版, 选自家手牌中第一种数量 ≥ 4 的牌种发上去.
    /// (server 端会校验, 不合法回 Error.)
    fn do_chi(&mut self) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        let Some(target) = Self::last_discard_tile(view) else {
            self.message = "找不到弃牌目标.".into();
            return;
        };
        let options = Self::enumerate_chi_options(&view.my_hand, target);
        match options.len() {
            0 => {
                self.message = "不能吃.".into();
            }
            1 => {
                self.session.send(ClientMsg::Action(NetAction::Chi(0)));
            }
            _ => {
                self.chi_picker = Some(crate::ui::chi_picker::ChiPicker::new(options, target));
            }
        }
    }

    fn do_ankan(&mut self) {
        let Some(view) = self.state_view.as_ref() else {
            return;
        };
        let mut counts = [0u8; 34];
        for t in &view.my_hand {
            counts[t.kind.0 as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            if c >= 4 {
                self.session
                    .send(ClientMsg::Action(NetAction::Ankan(TileIndex(i as u8))));
                return;
            }
        }
        self.message = "无可暗杠".into();
    }

    // ============== 渲染 ==============

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = self.theme_kind.theme();
        let buf = f.buffer_mut();
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

        let Some(view) = self.state_view.as_ref() else {
            paint_str(
                buf,
                ox + 4,
                oy + 4,
                "等待 server 推送状态...",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            return;
        };
        let layout = SeatLayout::from_my_seat(view.my_seat);

        self.paint_top_status(buf, ox, oy, &theme, view);
        self.paint_opponent_top(buf, ox, oy, &theme, view, layout.top);
        self.paint_opponent_left(buf, ox, oy, &theme, view, layout.left);
        self.paint_opponent_right(buf, ox, oy, &theme, view, layout.right);
        self.paint_center_info(buf, ox, oy, &theme, view);
        self.paint_my_river(buf, ox, oy, &theme, view, layout.bottom);
        self.paint_my_status(buf, ox, oy, &theme, view, layout.bottom);
        self.paint_my_message_and_melds(buf, ox, oy, &theme, view, layout.bottom);
        self.paint_my_hand(buf, ox, oy, &theme, view);
        self.paint_bottom(buf, ox, oy, &theme, view);
        if let Some(picker) = &self.chi_picker {
            picker.render(buf, area, &theme);
        }
    }

    fn paint_top_status(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
    ) {
        // 局 / 本场 / 立直棒
        let round_label = format!("{} {} 局", view.round_wind.label(), view.kyoku);
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
            &format!("{}本", view.honba),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if view.riichi_sticks > 0 {
            paint_str(
                buf,
                ox + 15,
                oy,
                &format!("{}供", view.riichi_sticks),
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
        let me = player_at(view, view.my_seat);
        let junme = me.river.len() + 1;
        paint_str(
            buf,
            ox + 21,
            oy,
            &format!("巡 {} · 山 {}", junme, view.wall_remaining),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_str(
            buf,
            ox + 36,
            oy,
            "│",
            Style::default().fg(theme.line).bg(theme.bg),
        );
        // 宝牌
        paint_str(
            buf,
            ox + 38,
            oy,
            "宝 ",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if let Some(t) = view.dora_indicators.first() {
            paint_tile_wide(buf, ox + 41, oy, Some(t), theme, TileState::Normal);
        }
        paint_str(
            buf,
            ox + 46,
            oy,
            "│",
            Style::default().fg(theme.line).bg(theme.bg),
        );
        // 4 家分数 (按 East/South/West/North 绝对座位排, 自家高亮)
        let mut col = ox + 48;
        for seat in Seat::ALL {
            let p = player_at(view, seat);
            let label = match seat {
                Seat::East => "東",
                Seat::South => "南",
                Seat::West => "西",
                Seat::North => "北",
            };
            let star = if p.riichi { "★" } else { "" };
            let style = if seat == view.my_seat {
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD)
            } else if p.riichi {
                Style::default().fg(theme.danger).bg(theme.bg)
            } else {
                Style::default().fg(theme.dim).bg(theme.bg)
            };
            paint_str(
                buf,
                col,
                oy,
                &format!("{} {}{}", label, p.score, star),
                style,
            );
            col += 11;
        }
        paint_str(
            buf,
            ox + 120,
            oy,
            "tui-majo · LAN",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_hr(buf, ox, oy + 1, 144, theme);
    }

    /// 对家 (top): 牌背 + 副露 + 河
    fn paint_opponent_top(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        seat: Seat,
    ) {
        let p = player_at(view, seat);
        let label = absolute_seat_label(seat);
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
            label,
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
            &format!("─ {}", &p.nickname),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        paint_back_row_wide(buf, ox + 42, oy + 4, p.hand_count, theme);
        if !p.melds.is_empty() {
            paint_str(
                buf,
                ox + 42,
                oy + 5,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 48;
            for meld in &p.melds {
                let tiles: Vec<Tile> = meld_tiles(meld);
                paint_meld_row_tight_hl(buf, col, oy + 5, &tiles, theme, self.highlight_kind(view));
                col += (tiles.len() as u16) * 3 + 1;
            }
        }
        paint_discard_grid_wide_hl(
            buf,
            ox + 54,
            oy + 6,
            &p.river,
            theme,
            p.riichi_river_idx,
            self.highlight_kind(view),
        );
    }

    /// 上家 (left).
    fn paint_opponent_left(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        seat: Seat,
    ) {
        let p = player_at(view, seat);
        let label = absolute_seat_label(seat);
        paint_str(
            buf,
            ox + 2,
            oy + 6,
            "上家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let line = if p.riichi {
            format!("{} {}★", label, p.score)
        } else {
            format!("{} {}", label, p.score)
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
        paint_str(buf, ox + 2, oy + 7, &line, style);
        paint_str(
            buf,
            ox + 2,
            oy + 8,
            &p.nickname,
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if !p.melds.is_empty() {
            paint_str(
                buf,
                ox + 2,
                oy + 9,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 2;
            let mut row = oy + 10;
            for meld in &p.melds {
                let tiles: Vec<Tile> = meld_tiles(meld);
                paint_meld_row_tight_hl(buf, col, row, &tiles, theme, self.highlight_kind(view));
                col += (tiles.len() as u16) * 3 + 1;
                if col > ox + 14 {
                    col = ox + 2;
                    row += 1;
                }
            }
        }
        paint_back_column_wide(buf, ox + 14, oy + 6, p.hand_count.min(13), theme);
        paint_discard_grid_wide_hl(
            buf,
            ox + 20,
            oy + 12,
            &p.river,
            theme,
            p.riichi_river_idx,
            self.highlight_kind(view),
        );
    }

    /// 下家 (right).
    fn paint_opponent_right(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        seat: Seat,
    ) {
        let p = player_at(view, seat);
        let label = absolute_seat_label(seat);
        paint_str(
            buf,
            ox + 132,
            oy + 6,
            "下家",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let line = if p.riichi {
            format!("{} {}★", label, p.score)
        } else {
            format!("{} {}", label, p.score)
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
        paint_str(buf, ox + 132, oy + 7, &line, style);
        paint_str(
            buf,
            ox + 132,
            oy + 8,
            &p.nickname,
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if !p.melds.is_empty() {
            paint_str(
                buf,
                ox + 126,
                oy + 9,
                "副露",
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            let mut col = ox + 126;
            let mut row = oy + 10;
            for meld in &p.melds {
                let tiles: Vec<Tile> = meld_tiles(meld);
                paint_meld_row_tight_hl(buf, col, row, &tiles, theme, self.highlight_kind(view));
                col += (tiles.len() as u16) * 3 + 1;
                if col > ox + 138 {
                    col = ox + 126;
                    row += 1;
                }
            }
        }
        paint_back_column_wide(buf, ox + 120, oy + 6, p.hand_count.min(13), theme);
        paint_discard_grid_wide_hl(
            buf,
            ox + 92,
            oy + 12,
            &p.river,
            theme,
            p.riichi_river_idx,
            self.highlight_kind(view),
        );
    }

    fn paint_center_info(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
    ) {
        paint_str(
            buf,
            ox + 66,
            oy + 17,
            "宝",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        if let Some(t) = view.dora_indicators.first() {
            paint_tile_wide(buf, ox + 70, oy + 17, Some(t), theme, TileState::Normal);
        }
        paint_str(
            buf,
            ox + 68,
            oy + 18,
            &format!("山 {}", view.wall_remaining),
            Style::default().fg(theme.dim).bg(theme.bg),
        );
    }

    fn paint_my_river(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        my: Seat,
    ) {
        let p = player_at(view, my);
        paint_discard_grid_wide_hl(
            buf,
            ox + 54,
            oy + 23,
            &p.river,
            theme,
            p.riichi_river_idx,
            self.highlight_kind(view),
        );
    }

    fn paint_my_status(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        my: Seat,
    ) {
        paint_hr_accent(buf, ox + 2, oy + 28, 140, theme);
        let me = player_at(view, my);
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
        let dealer_str = if my == view.dealer {
            format!("{} ◆庄", absolute_seat_label(my))
        } else {
            absolute_seat_label(my).to_string()
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
            &format!("{}", me.score),
            Style::default()
                .fg(theme.fg)
                .bg(theme.bg)
                .add_modifier(Modifier::BOLD),
        );
        // 听牌检测.
        // 13 张型 (刚切完): my_hand + melds*3 = 13. 直接算 tenpai.
        // 14 张型 (摸完未切): my_hand + melds*3 = 14. 排除 my_last_drawn 那张后算.
        let total = view.my_hand.len() + me.melds.len() * 3;
        let waits = if total == 13 {
            let counts = crate::engine::domain::tile::count_by_kind(&view.my_hand);
            crate::engine::domain::decompose::tenpai_tiles(&counts, &me.melds)
        } else if total == 14 {
            if let Some(drawn) = view.my_last_drawn {
                let mut counts = crate::engine::domain::tile::count_by_kind(&view.my_hand);
                counts[drawn.kind.0 as usize] = counts[drawn.kind.0 as usize].saturating_sub(1);
                crate::engine::domain::decompose::tenpai_tiles(&counts, &me.melds)
            } else {
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
        let any_riichi = Seat::ALL
            .iter()
            .any(|&s| s != my && player_at(view, s).riichi);
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

    fn paint_my_message_and_melds(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
        my: Seat,
    ) {
        if !self.message.is_empty() {
            let style = match view.phase {
                Phase::RoundEnd => Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default().fg(theme.fg).bg(theme.bg),
            };
            let mut msg = self.message.clone();
            let max_w = 74usize;
            while UnicodeWidthStr::width(msg.as_str()) > max_w {
                msg.pop();
            }
            paint_str(buf, ox + 4, oy + 30, &msg, style);
        }
        let me = player_at(view, my);
        if me.melds.is_empty() {
            return;
        }
        let mut col = ox + 82;
        for meld in &me.melds {
            let (label, label_color) = match &meld.kind {
                MeldKind::Chi { .. } => ("[吃]", theme.info),
                MeldKind::Pon { .. } => ("[碰]", theme.info),
                MeldKind::Minkan { .. } => ("[明杠]", theme.accent),
                MeldKind::Shouminkan { .. } => ("[加杠]", theme.accent),
                MeldKind::Ankan { .. } => ("[暗杠]", theme.dim),
            };
            let label_w = UnicodeWidthStr::width(label) as u16;
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
            let tiles = meld_tiles(meld);
            for tile in &tiles {
                paint_tile_tight(buf, tx, oy + 30, Some(tile), theme, TileState::Normal);
                tx += 3;
            }
            col += total_w;
        }
    }

    fn paint_my_hand(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
    ) {
        let (mut display, drawn) = Self::split_hand(view);
        let drawn_idx = drawn.map(|t| {
            display.push(t);
            display.len() - 1
        });
        let is_my_turn_discard = view.turn == view.my_seat && view.phase == Phase::AwaitDiscard;
        let selectable_len = drawn_idx.unwrap_or(display.len());
        let selected = if is_my_turn_discard && self.selected < selectable_len {
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
            self.highlight_kind(view),
        );
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

    fn paint_bottom(
        &self,
        buf: &mut Buffer,
        ox: u16,
        oy: u16,
        theme: &Theme,
        view: &GameStateView,
    ) {
        paint_hr(buf, ox, oy + 36, 144, theme);
        // last 事件
        paint_str(
            buf,
            ox + 2,
            oy + 37,
            "last",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let mut col = ox + 7;
        for ev in view
            .events
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .iter()
            .rev()
        {
            let (text, style) = format_event(ev, view.my_seat, theme);
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
        // 提示行
        paint_fill(
            buf,
            ox,
            oy + 38,
            144,
            1,
            Style::default().bg(theme.panel).fg(theme.fg),
        );
        let phase_label = match view.phase {
            Phase::Deal => "配牌",
            Phase::Draw => "摸牌",
            Phase::AwaitDiscard => "切牌",
            Phase::AwaitCalls => "鸣牌窗口",
            Phase::RoundEnd => "局结算",
            Phase::GameEnd => "整庄结束",
        };
        let your_turn = view.turn == view.my_seat;
        let has_hints = self.current_hints.is_some();
        let phase_text = format!(
            " {} · 当前 {} · {}",
            phase_label,
            absolute_seat_label(view.turn),
            if has_hints {
                "**等你响应**"
            } else if your_turn {
                "你的回合"
            } else {
                "他家行动"
            }
        );
        paint_str(
            buf,
            ox + 2,
            oy + 38,
            &phase_text,
            Style::default()
                .fg(if has_hints || your_turn {
                    theme.accent
                } else {
                    theme.dim
                })
                .bg(theme.panel)
                .add_modifier(if has_hints || your_turn {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        );
        // 倒计时 (col ~ 80)
        if self.current_deadline_ms > 0 {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let remaining_ms = (self.current_deadline_ms - now_ms).max(0);
            let remaining_s = (remaining_ms + 999) / 1000;
            let countdown_style = if remaining_s <= 1 {
                Style::default()
                    .fg(theme.bg)
                    .bg(theme.danger)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.danger)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD)
            };
            paint_str(
                buf,
                ox + 80,
                oy + 38,
                &format!(" ⏱  {} 秒 ", remaining_s),
                countdown_style,
            );
        }
        paint_str(
            buf,
            ox + 120,
            oy + 38,
            " LAN ",
            Style::default()
                .fg(theme.bg)
                .bg(theme.ok)
                .add_modifier(Modifier::BOLD),
        );
        // row 39: 按键速查
        paint_str(
            buf,
            ox + 2,
            oy + 39,
            "操作",
            Style::default().fg(theme.dim).bg(theme.bg),
        );
        let cmds: &[(&str, &str)] = &[
            ("[1-9/←→]", "选牌"),
            ("[d]", "切"),
            ("[t]", "摸切"),
            ("[R]", "立直"),
            ("[W]", "自摸"),
            ("[K]", "暗杠"),
            ("[P]", "碰"),
            ("[A]", "吃"),
            ("[M]", "明杠"),
            ("[C]", "跳过"),
            ("[N]", "下一局"),
            ("[L]", "离开"),
        ];
        let mut col = ox + 11;
        for (k, h) in cmds {
            let kw = UnicodeWidthStr::width(*k) as u16;
            let hw = UnicodeWidthStr::width(*h) as u16;
            if col + kw + hw + 2 >= ox + 144 {
                break;
            }
            paint_str(
                buf,
                col,
                oy + 39,
                k,
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.bg)
                    .add_modifier(Modifier::BOLD),
            );
            paint_str(
                buf,
                col + kw,
                oy + 39,
                h,
                Style::default().fg(theme.dim).bg(theme.bg),
            );
            col += kw + hw + 1;
        }
    }
}

// ============== Helpers ==============

fn tiles_eq(a: &Tile, b: &Tile) -> bool {
    a.kind == b.kind && a.red == b.red
}

fn meld_tiles(meld: &crate::engine::domain::meld::Meld) -> Vec<Tile> {
    match &meld.kind {
        MeldKind::Chi { tiles } | MeldKind::Pon { tiles } => tiles.to_vec(),
        MeldKind::Minkan { tiles } | MeldKind::Shouminkan { tiles } | MeldKind::Ankan { tiles } => {
            tiles.to_vec()
        }
    }
}

fn absolute_seat_label(s: Seat) -> &'static str {
    match s {
        Seat::East => "東",
        Seat::South => "南",
        Seat::West => "西",
        Seat::North => "北",
    }
}

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

/// 把 GameEvent 渲染成 last 行文本.  不同于单机版: 我们用绝对座位 (東/南/西/北),
/// 自家事件标 "你".
fn format_event(ev: &GameEvent, my_seat: Seat, theme: &Theme) -> (String, Style) {
    let s = Style::default().bg(theme.bg);
    let who_str = |w: Seat| -> String {
        if w == my_seat {
            "你".into()
        } else {
            absolute_seat_label(w).to_string()
        }
    };
    match ev {
        GameEvent::Discard { who, tile } => (
            format!("{} 打 {}", who_str(*who), kind_label_tight(tile.kind)),
            s.fg(theme.dim),
        ),
        GameEvent::Draw { who, .. } => (format!("{} 摸", who_str(*who)), s.fg(theme.info)),
        GameEvent::Pon { who, tile } => (
            format!("{} 碰 {}", who_str(*who), kind_label_tight(tile.kind)),
            s.fg(theme.info),
        ),
        GameEvent::Chi { who, tile } => (
            format!("{} 吃 {}", who_str(*who), kind_label_tight(tile.kind)),
            s.fg(theme.info),
        ),
        GameEvent::Minkan { who, tile } => (
            format!("{} 杠 {}", who_str(*who), kind_label_tight(tile.kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Ankan { who, kind } => (
            format!("{} 暗杠 {}", who_str(*who), kind_label_tight(*kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Shouminkan { who, kind } => (
            format!("{} 加杠 {}", who_str(*who), kind_label_tight(*kind)),
            s.fg(theme.accent),
        ),
        GameEvent::Riichi { who, .. } => (
            format!("{} 立直", who_str(*who)),
            s.fg(theme.danger).add_modifier(Modifier::BOLD),
        ),
        GameEvent::Tsumo { who } => (
            format!("{} 自摸", who_str(*who)),
            s.fg(theme.ok).add_modifier(Modifier::BOLD),
        ),
        GameEvent::Ron { who, .. } => (
            format!("{} 荣和", who_str(*who)),
            s.fg(theme.ok).add_modifier(Modifier::BOLD),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::domain::meld::Seat;
    use crate::engine::round_state::RoundWind;
    use crate::net::protocol::{GameStateView, PlayerView};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn tile(kind_idx: u8, id: u16) -> Tile {
        Tile {
            id,
            kind: TileIndex(kind_idx),
            red: false,
        }
    }

    fn make_pv(seat: Seat) -> PlayerView {
        PlayerView {
            seat,
            nickname: format!("{seat:?}"),
            score: 25_000,
            hand_count: 13,
            melds: Vec::new(),
            river: Vec::new(),
            riichi: false,
            riichi_river_idx: None,
        }
    }

    fn make_view(my_seat: Seat, my_hand: Vec<Tile>, last_drawn: Option<Tile>) -> GameStateView {
        GameStateView {
            round_wind: RoundWind::East,
            kyoku: 1,
            honba: 0,
            riichi_sticks: 0,
            dealer: Seat::East,
            turn: my_seat,
            phase: Phase::AwaitDiscard,
            my_seat,
            my_hand,
            my_last_drawn: last_drawn,
            players: [
                make_pv(Seat::East),
                make_pv(Seat::South),
                make_pv(Seat::West),
                make_pv(Seat::North),
            ],
            wall_remaining: 70,
            dora_indicators: Vec::new(),
            events: Vec::new(),
        }
    }

    fn make_state(
        my_id: u32,
    ) -> (
        OnlineGameState,
        mpsc::UnboundedReceiver<ClientMsg>,
        mpsc::UnboundedSender<ServerMsg>,
    ) {
        let (out_tx, out_rx) = mpsc::unbounded_channel::<ClientMsg>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<ServerMsg>();
        let session = NetSession::from_channels(my_id, Uuid::new_v4(), out_tx, in_rx);
        let state = OnlineGameState::new(session, ThemeKind::default());
        (state, out_rx, in_tx)
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn keycode(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    // ============================================================================
    // 纯函数: SeatLayout / player_at / tiles_eq / kind_label_tight /
    // absolute_seat_label / meld_tiles
    // ============================================================================

    #[test]
    fn seat_layout_from_my_east() {
        let l = SeatLayout::from_my_seat(Seat::East);
        assert_eq!(l.bottom, Seat::East);
        assert_eq!(l.right, Seat::South);
        assert_eq!(l.top, Seat::West);
        assert_eq!(l.left, Seat::North);
    }

    #[test]
    fn seat_layout_from_my_south() {
        let l = SeatLayout::from_my_seat(Seat::South);
        assert_eq!(l.bottom, Seat::South);
        assert_eq!(l.right, Seat::West);
        assert_eq!(l.top, Seat::North);
        assert_eq!(l.left, Seat::East);
    }

    #[test]
    fn player_at_returns_correct_seat() {
        let view = make_view(Seat::East, Vec::new(), None);
        assert_eq!(player_at(&view, Seat::West).seat, Seat::West);
    }

    #[test]
    fn tiles_eq_true_for_same_kind_and_red() {
        let a = tile(5, 1);
        let mut b = tile(5, 2);
        b.red = false;
        assert!(tiles_eq(&a, &b));
    }

    #[test]
    fn tiles_eq_false_for_different_red() {
        let a = tile(5, 1);
        let b = Tile {
            id: 2,
            kind: TileIndex(5),
            red: true,
        };
        assert!(!tiles_eq(&a, &b));
    }

    #[test]
    fn kind_label_tight_man_tiles() {
        assert_eq!(kind_label_tight(TileIndex(0)), "1萬");
        assert_eq!(kind_label_tight(TileIndex(8)), "9萬");
    }

    #[test]
    fn kind_label_tight_pin_tiles() {
        assert_eq!(kind_label_tight(TileIndex(9)), "1筒");
        assert_eq!(kind_label_tight(TileIndex(17)), "9筒");
    }

    #[test]
    fn kind_label_tight_sou_tiles() {
        assert_eq!(kind_label_tight(TileIndex(18)), "1索");
        assert_eq!(kind_label_tight(TileIndex(26)), "9索");
    }

    #[test]
    fn kind_label_tight_winds_dragons() {
        assert_eq!(kind_label_tight(TileIndex(27)).trim(), "東");
        assert_eq!(kind_label_tight(TileIndex(33)).trim(), "中");
    }

    #[test]
    fn kind_label_tight_out_of_range_falls_back() {
        assert_eq!(kind_label_tight(TileIndex(100)).trim(), "??");
    }

    #[test]
    fn absolute_seat_label_chinese() {
        assert_eq!(absolute_seat_label(Seat::East), "東");
        assert_eq!(absolute_seat_label(Seat::South), "南");
        assert_eq!(absolute_seat_label(Seat::West), "西");
        assert_eq!(absolute_seat_label(Seat::North), "北");
    }

    // ============================================================================
    // enumerate_chi_options 边界
    // ============================================================================

    #[test]
    fn enumerate_chi_options_honor_tile_returns_empty() {
        let hand = vec![tile(27, 1), tile(28, 2), tile(29, 3)];
        assert!(OnlineGameState::enumerate_chi_options(&hand, tile(30, 99)).is_empty());
    }

    #[test]
    fn enumerate_chi_options_middle_tile_three_combos() {
        // 5m target, 持 3m 4m 6m 7m → 应有 3-4, 4-6, 6-7 (三种).
        let hand = vec![tile(2, 1), tile(3, 2), tile(5, 3), tile(6, 4)];
        let opts = OnlineGameState::enumerate_chi_options(&hand, tile(4, 99));
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn enumerate_chi_options_edge_tile_one_combo() {
        // 1m target, 持 2m 3m → 只能 2-3 一种.
        let hand = vec![tile(1, 1), tile(2, 2)];
        let opts = OnlineGameState::enumerate_chi_options(&hand, tile(0, 99));
        assert_eq!(opts.len(), 1);
    }

    #[test]
    fn enumerate_chi_options_no_match_returns_empty() {
        let hand = vec![tile(8, 1)];
        let opts = OnlineGameState::enumerate_chi_options(&hand, tile(0, 99));
        assert!(opts.is_empty());
    }

    #[test]
    fn enumerate_chi_options_does_not_cross_suit() {
        // 9m target, 持 1p 2p (跨花色), 不应给 1p-2p 当作 9m 邻居.
        let hand = vec![tile(9, 1), tile(10, 2)];
        let opts = OnlineGameState::enumerate_chi_options(&hand, tile(8, 99));
        assert!(opts.is_empty());
    }

    // ============================================================================
    // last_discard_tile / selectable_count / split_hand
    // ============================================================================

    #[test]
    fn last_discard_tile_empty_returns_none() {
        let view = make_view(Seat::East, Vec::new(), None);
        assert!(OnlineGameState::last_discard_tile(&view).is_none());
    }

    #[test]
    fn last_discard_tile_returns_most_recent() {
        let mut view = make_view(Seat::East, Vec::new(), None);
        view.events = vec![
            GameEvent::Discard {
                who: Seat::East,
                tile: tile(0, 1),
            },
            GameEvent::Draw {
                who: Seat::South,
                tile: tile(0, 7),
            },
            GameEvent::Discard {
                who: Seat::South,
                tile: tile(5, 9),
            },
        ];
        let last = OnlineGameState::last_discard_tile(&view).unwrap();
        assert_eq!(last.kind, TileIndex(5));
    }

    #[test]
    fn selectable_count_with_drawn_excludes_drawn() {
        let (state, _, _) = make_state(1);
        let drawn = tile(5, 100);
        let view = make_view(
            Seat::East,
            vec![tile(0, 1), tile(1, 2), drawn],
            Some(drawn),
        );
        assert_eq!(state.selectable_count(&view), 2);
    }

    #[test]
    fn selectable_count_no_drawn_returns_full_hand() {
        let (state, _, _) = make_state(1);
        let view = make_view(Seat::East, vec![tile(0, 1), tile(1, 2), tile(2, 3)], None);
        assert_eq!(state.selectable_count(&view), 3);
    }

    #[test]
    fn split_hand_separates_drawn_tile() {
        let drawn = tile(5, 100);
        let view = make_view(
            Seat::East,
            vec![tile(0, 1), drawn, tile(1, 2)],
            Some(drawn),
        );
        let (sel, d) = OnlineGameState::split_hand(&view);
        assert_eq!(sel.len(), 2);
        assert!(d.is_some());
        assert_eq!(d.unwrap().id, 100);
    }

    #[test]
    fn split_hand_no_drawn_returns_full_sorted() {
        let view = make_view(Seat::East, vec![tile(5, 1), tile(0, 2), tile(2, 3)], None);
        let (sel, d) = OnlineGameState::split_hand(&view);
        assert_eq!(sel.len(), 3);
        assert!(d.is_none());
        // 排序后第一张应为 kind 0.
        assert_eq!(sel[0].kind, TileIndex(0));
    }

    // ============================================================================
    // handle_event: 各按键 → ClientMsg::Action
    // ============================================================================

    #[test]
    fn enter_sends_discard_action() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.state_view = Some(make_view(Seat::East, vec![tile(2, 1)], None));
        s.handle_event(keycode(KeyCode::Enter));
        let m = out_rx.try_recv().expect("应发 Action");
        assert!(matches!(
            m,
            ClientMsg::Action(NetAction::Discard(TileSpec { kind: TileIndex(2) }))
        ));
    }

    #[test]
    fn t_key_tsumogiri_sends_drawn_kind() {
        let (mut s, mut out_rx, _) = make_state(1);
        let drawn = tile(7, 9);
        s.state_view = Some(make_view(Seat::East, vec![drawn], Some(drawn)));
        s.handle_event(key('t'));
        let m = out_rx.try_recv().expect("应发 tsumogiri");
        assert!(matches!(
            m,
            ClientMsg::Action(NetAction::Discard(TileSpec { kind: TileIndex(7) }))
        ));
    }

    #[test]
    fn t_key_tsumogiri_no_drawn_does_nothing() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.state_view = Some(make_view(Seat::East, vec![tile(0, 1)], None));
        s.handle_event(key('t'));
        assert!(out_rx.try_recv().is_err());
    }

    #[test]
    fn r_key_riichi_uses_selected_kind() {
        let (mut s, mut out_rx, _) = make_state(1);
        let drawn = tile(5, 100);
        s.state_view = Some(make_view(
            Seat::East,
            vec![tile(0, 1), tile(1, 2), drawn],
            Some(drawn),
        ));
        s.selected = 1;
        s.handle_event(key('R'));
        let m = out_rx.try_recv().expect("应发 Riichi");
        match m {
            ClientMsg::Action(NetAction::Riichi(spec)) => {
                // selected=1 -> sorted hand[1] (排序后 0,1)
                assert_eq!(spec.kind, TileIndex(1));
            }
            _ => panic!("expected Riichi action"),
        }
    }

    #[test]
    fn w_key_sends_tsumo() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.handle_event(key('W'));
        assert!(matches!(
            out_rx.try_recv(),
            Ok(ClientMsg::Action(NetAction::Tsumo))
        ));
    }

    #[test]
    fn p_key_sends_pon() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.handle_event(key('P'));
        assert!(matches!(
            out_rx.try_recv(),
            Ok(ClientMsg::Action(NetAction::Pon))
        ));
    }

    #[test]
    fn m_key_sends_minkan() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.handle_event(key('M'));
        assert!(matches!(
            out_rx.try_recv(),
            Ok(ClientMsg::Action(NetAction::Minkan))
        ));
    }

    #[test]
    fn c_key_sends_pass() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.handle_event(key('C'));
        assert!(matches!(
            out_rx.try_recv(),
            Ok(ClientMsg::Action(NetAction::Pass))
        ));
    }

    #[test]
    fn n_key_sends_next_round() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.handle_event(key('N'));
        assert!(matches!(
            out_rx.try_recv(),
            Ok(ClientMsg::Action(NetAction::NextRound))
        ));
    }

    #[test]
    fn k_key_ankan_with_4_tiles_sends_action() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.state_view = Some(make_view(
            Seat::East,
            vec![tile(5, 1), tile(5, 2), tile(5, 3), tile(5, 4)],
            None,
        ));
        s.handle_event(key('K'));
        match out_rx.try_recv() {
            Ok(ClientMsg::Action(NetAction::Ankan(idx))) => {
                assert_eq!(idx, TileIndex(5));
            }
            other => panic!("应发 Ankan, got {other:?}"),
        }
    }

    #[test]
    fn k_key_ankan_without_4_tiles_sets_message() {
        let (mut s, mut out_rx, _) = make_state(1);
        s.state_view = Some(make_view(
            Seat::East,
            vec![tile(5, 1), tile(5, 2)],
            None,
        ));
        s.handle_event(key('K'));
        assert!(out_rx.try_recv().is_err(), "无 4 张同 kind 不应发 Ankan");
        assert!(s.message.contains("无可暗杠"));
    }

    #[test]
    fn a_key_chi_no_target_sets_message() {
        let (mut s, _out_rx, _) = make_state(1);
        s.state_view = Some(make_view(Seat::East, vec![tile(0, 1)], None));
        s.handle_event(key('a'));
        assert!(s.message.contains("找不到弃牌"));
    }

    #[test]
    fn a_key_chi_one_option_auto_sends() {
        let (mut s, mut out_rx, _) = make_state(1);
        let mut v = make_view(Seat::East, vec![tile(1, 1), tile(2, 2)], None);
        v.events = vec![GameEvent::Discard {
            who: Seat::North,
            tile: tile(0, 99),
        }];
        s.state_view = Some(v);
        s.handle_event(key('A'));
        // 只有一种吃法 → 直接发 Chi(0)
        match out_rx.try_recv() {
            Ok(ClientMsg::Action(NetAction::Chi(idx))) => assert_eq!(idx, 0),
            other => panic!("应发 Chi(0), got {other:?}"),
        }
    }

    #[test]
    fn a_key_chi_no_options_sets_cannot_chi() {
        let (mut s, _, _) = make_state(1);
        let mut v = make_view(Seat::East, vec![tile(8, 1)], None);
        v.events = vec![GameEvent::Discard {
            who: Seat::North,
            tile: tile(0, 99),
        }];
        s.state_view = Some(v);
        s.handle_event(key('a'));
        assert!(s.message.contains("不能吃"));
    }

    #[test]
    fn digit_keys_select_index_within_range() {
        let (mut s, _, _) = make_state(1);
        s.state_view = Some(make_view(
            Seat::East,
            vec![tile(0, 1), tile(1, 2), tile(2, 3)],
            None,
        ));
        s.handle_event(key('2'));
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn digit_keys_out_of_range_kept_intact() {
        let (mut s, _, _) = make_state(1);
        s.state_view = Some(make_view(Seat::East, vec![tile(0, 1)], None));
        s.selected = 0;
        s.handle_event(key('5'));
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn left_right_arrows_move_selection() {
        let (mut s, _, _) = make_state(1);
        s.state_view = Some(make_view(
            Seat::East,
            vec![tile(0, 1), tile(1, 2), tile(2, 3)],
            None,
        ));
        s.handle_event(keycode(KeyCode::Right));
        assert_eq!(s.selected, 1);
        s.handle_event(keycode(KeyCode::Left));
        assert_eq!(s.selected, 0);
        // 边界 wrap: Left from 0 → 2
        s.handle_event(keycode(KeyCode::Left));
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn move_select_no_view_no_panic() {
        let (mut s, _, _) = make_state(1);
        s.handle_event(keycode(KeyCode::Right));
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn capital_l_returns_leave_request_confirm() {
        let (mut s, _, _) = make_state(1);
        let t = s.handle_event(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
        assert!(matches!(t, Some(Transition::RequestConfirm { .. })));
    }

    #[test]
    fn esc_returns_request_confirm() {
        let (mut s, _, _) = make_state(1);
        let t = s.handle_event(keycode(KeyCode::Esc));
        assert!(matches!(t, Some(Transition::RequestConfirm { .. })));
    }

    // ============================================================================
    // handle_msg: 各 ServerMsg
    // ============================================================================

    #[test]
    fn game_state_view_replaces_state_and_clears_hints() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        s.current_hints = Some(vec![NetAction::Pon]);
        s.current_deadline_ms = 999;
        let v = make_view(Seat::East, vec![tile(0, 1)], None);
        in_tx.send(ServerMsg::GameStateView(Box::new(v))).unwrap();
        let _ = s.advance();
        assert!(s.state_view.is_some());
        assert!(s.current_hints.is_none(), "GameStateView 应清旧 hints");
        assert_eq!(s.current_deadline_ms, 0);
    }

    #[test]
    fn game_state_view_in_await_calls_keeps_hints() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        s.current_hints = Some(vec![NetAction::Pon]);
        let mut v = make_view(Seat::East, vec![tile(0, 1)], None);
        v.phase = Phase::AwaitCalls;
        in_tx.send(ServerMsg::GameStateView(Box::new(v))).unwrap();
        let _ = s.advance();
        // 仍在 AwaitCalls → hints 应保留
        assert!(s.current_hints.is_some());
    }

    #[test]
    fn action_required_stores_hints_and_deadline() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        in_tx
            .send(ServerMsg::ActionRequired {
                hints: vec![NetAction::Pon, NetAction::Pass],
                deadline_unix_ms: 12345,
            })
            .unwrap();
        let _ = s.advance();
        assert_eq!(s.current_hints.as_ref().unwrap().len(), 2);
        assert_eq!(s.current_deadline_ms, 12345);
    }

    #[test]
    fn round_result_sets_message_and_clears_hints() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        s.current_hints = Some(vec![NetAction::Pon]);
        in_tx
            .send(ServerMsg::RoundResult(crate::net::protocol::RoundResultView {
                message: "流局".into(),
                scores: [25_000, 25_000, 25_000, 25_000],
            }))
            .unwrap();
        let _ = s.advance();
        assert!(s.message.contains("局结算"));
        assert!(s.current_hints.is_none());
    }

    #[test]
    fn game_end_sets_message_and_clears_hints() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        s.current_hints = Some(vec![NetAction::Pon]);
        let dummy = crate::engine::score::Ranking {
            seat: Seat::East,
            place: 1,
            raw_score: 25_000,
            return_diff_k: 0,
            uma: 0,
            oka: 0,
            final_score: 0,
        };
        in_tx
            .send(ServerMsg::GameEnd(crate::net::protocol::GameOverView {
                rankings: [
                    dummy,
                    crate::engine::score::Ranking {
                        seat: Seat::South,
                        place: 2,
                        ..dummy
                    },
                    crate::engine::score::Ranking {
                        seat: Seat::West,
                        place: 3,
                        ..dummy
                    },
                    crate::engine::score::Ranking {
                        seat: Seat::North,
                        place: 4,
                        ..dummy
                    },
                ],
            }))
            .unwrap();
        let _ = s.advance();
        assert!(s.message.contains("整庄结束"));
        assert!(s.current_hints.is_none());
    }

    #[test]
    fn back_to_room_sets_message() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        in_tx.send(ServerMsg::BackToRoom).unwrap();
        let _ = s.advance();
        assert_eq!(s.message, "回房间");
    }

    #[test]
    fn room_update_to_lobby_returns_main_menu_transition() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        let view = crate::net::protocol::RoomView {
            room_id: "x".into(),
            host_id: 1,
            config: crate::engine::rules::GameRules::default(),
            players: Vec::new(),
            state: RoomLifecycle::Lobby,
            mode: crate::net::p2p::RoomMode::Standard,
        };
        in_tx.send(ServerMsg::RoomUpdate(Box::new(view))).unwrap();
        let t = s.advance();
        assert!(matches!(t, Some(Transition::EnterMainMenu)));
    }

    #[test]
    fn error_message_sets_state_message() {
        let (mut s, _out_rx, in_tx) = make_state(1);
        in_tx
            .send(ServerMsg::Error {
                message: "boom".into(),
            })
            .unwrap();
        let _ = s.advance();
        assert_eq!(s.message, "boom");
    }

    #[test]
    fn advance_when_disconnected_sets_message() {
        let (mut s, out_rx, _) = make_state(1);
        drop(out_rx);
        let _ = s.advance();
        assert!(s.message.contains("断开"));
    }

    // ============================================================================
    // ChiPicker 路径
    // ============================================================================

    #[test]
    fn chi_picker_pick_sends_chi_and_clears_picker() {
        let (mut s, mut out_rx, _) = make_state(1);
        let target = tile(0, 99);
        let opts = vec![[tile(1, 1), tile(2, 2)], [tile(2, 3), tile(3, 4)]];
        s.chi_picker = Some(crate::ui::chi_picker::ChiPicker::new(opts, target));
        // ChiPicker Enter 选第 0 项.
        s.handle_event(keycode(KeyCode::Enter));
        assert!(s.chi_picker.is_none());
        match out_rx.try_recv() {
            Ok(ClientMsg::Action(NetAction::Chi(0))) => {}
            other => panic!("应发 Chi(0), got {other:?}"),
        }
    }

    #[test]
    fn chi_picker_cancel_clears_picker_with_message() {
        let (mut s, mut out_rx, _) = make_state(1);
        let target = tile(0, 99);
        let opts = vec![[tile(1, 1), tile(2, 2)]];
        s.chi_picker = Some(crate::ui::chi_picker::ChiPicker::new(opts, target));
        s.handle_event(keycode(KeyCode::Esc));
        assert!(s.chi_picker.is_none());
        assert!(s.message.contains("取消"));
        assert!(out_rx.try_recv().is_err());
    }

    // ============================================================================
    // render smoke
    // ============================================================================

    #[test]
    fn render_no_state_view_does_not_panic() {
        let (s, _out, _in) = make_state(1);
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area())).unwrap();
    }

    #[test]
    fn render_with_state_view_does_not_panic() {
        let (mut s, _out, _in) = make_state(1);
        // 13 张手牌 + 1 张 last_drawn.
        let hand: Vec<Tile> = (0..13).map(|i| tile(i as u8, i as u16)).collect();
        let drawn = tile(13, 100);
        let mut hand_with_drawn = hand.clone();
        hand_with_drawn.push(drawn);
        let mut view = make_view(Seat::East, hand_with_drawn, Some(drawn));
        view.events = vec![
            GameEvent::Draw {
                who: Seat::East,
                tile: drawn,
            },
            GameEvent::Discard {
                who: Seat::South,
                tile: tile(5, 1),
            },
        ];
        view.dora_indicators = vec![tile(0, 200)];
        view.riichi_sticks = 1;
        view.honba = 2;
        s.state_view = Some(view);
        s.message = "msg".into();
        s.current_hints = Some(vec![NetAction::Pon, NetAction::Pass]);
        s.current_deadline_ms = 0;
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area())).unwrap();
    }

    #[test]
    fn render_with_chi_picker_does_not_panic() {
        let (mut s, _out, _in) = make_state(1);
        s.state_view = Some(make_view(Seat::East, vec![tile(0, 1)], None));
        let opts = vec![[tile(1, 1), tile(2, 2)], [tile(2, 3), tile(3, 4)]];
        s.chi_picker = Some(crate::ui::chi_picker::ChiPicker::new(opts, tile(0, 99)));
        let backend = ratatui::backend::TestBackend::new(144, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area())).unwrap();
    }

    #[test]
    fn render_for_each_my_seat_layout_does_not_panic() {
        for my_seat in [Seat::East, Seat::South, Seat::West, Seat::North] {
            let (mut s, _out, _in) = make_state(1);
            s.state_view = Some(make_view(my_seat, vec![tile(0, 1), tile(1, 2)], None));
            let backend = ratatui::backend::TestBackend::new(144, 40);
            let mut term = ratatui::Terminal::new(backend).unwrap();
            term.draw(|f| s.render(f, f.area())).unwrap();
        }
    }

    #[test]
    fn format_event_renders_each_variant() {
        // 仅检查不 panic + 输出非空.
        let theme = ThemeKind::default().theme();
        let events = vec![
            GameEvent::Discard {
                who: Seat::East,
                tile: tile(0, 1),
            },
            GameEvent::Draw {
                who: Seat::South,
                tile: tile(0, 2),
            },
            GameEvent::Pon {
                who: Seat::West,
                tile: tile(0, 3),
            },
            GameEvent::Chi {
                who: Seat::North,
                tile: tile(0, 4),
            },
            GameEvent::Minkan {
                who: Seat::East,
                tile: tile(0, 5),
            },
            GameEvent::Ankan {
                who: Seat::East,
                kind: TileIndex(0),
            },
            GameEvent::Shouminkan {
                who: Seat::East,
                kind: TileIndex(0),
            },
            GameEvent::Riichi {
                who: Seat::East,
                tile: tile(0, 6),
            },
            GameEvent::Tsumo { who: Seat::East },
            GameEvent::Ron {
                who: Seat::East,
                from: Seat::South,
            },
        ];
        for ev in &events {
            let (text, _) = format_event(ev, Seat::East, &theme);
            assert!(!text.is_empty());
        }
    }
}
