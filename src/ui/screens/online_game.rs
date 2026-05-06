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
