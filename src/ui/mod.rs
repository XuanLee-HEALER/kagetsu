//! TUI 顶层: App + Screen 枚举 + 主循环 + 全局快捷键.
//!
//! 屏间通过 [`Transition`] 切换. App 持有 [`last_config`] / [`last_seed_choice`]
//! 用于"新游戏"复用上次配置.

pub mod confirm;
pub mod edit_config_modal;
pub mod paint;
pub mod screens;
pub mod theme;
pub mod widgets;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

use crate::config::LoadResult;
use crate::config::LocalPrefs;
use crate::engine::rules::GameRules;
use crate::engine::score::Ranking;
use crate::ui::confirm::{ConfirmChoice, ConfirmModal};

/// multiaddr 优先级评分: 含 /p2p-circuit/ + 公网 IP > 含 circuit > 公网 + QUIC > 公网 > LAN.
/// 用于从 host_listen_addrs 选最优 dial_addr 给加入者复制粘贴.
fn addr_score(addr: &libp2p::Multiaddr) -> u32 {
    let mut score = 0u32;
    let mut has_circuit = false;
    let mut has_quic = false;
    let mut public_ip = false;
    for proto in addr.iter() {
        match proto {
            libp2p::multiaddr::Protocol::P2pCircuit => has_circuit = true,
            libp2p::multiaddr::Protocol::QuicV1 => has_quic = true,
            libp2p::multiaddr::Protocol::Ip4(ip) => {
                if !ip.is_private() && !ip.is_loopback() && !ip.is_link_local() {
                    public_ip = true;
                }
            }
            libp2p::multiaddr::Protocol::Ip6(ip) => {
                if !ip.is_loopback() {
                    public_ip = true;
                }
            }
            _ => {}
        }
    }
    if has_circuit {
        score += 1000;
    }
    if public_ip {
        score += 100;
    }
    if has_quic {
        score += 10;
    }
    score
}

pub use screens::config::{ConfigState, SeedChoice};
pub use screens::game::GameScreenState;
pub use screens::gameover::GameOverState;
pub use screens::main_menu::MainMenuState;
pub use screens::online_game::OnlineGameState;
pub use screens::online_lobby::OnlineLobbyState;
pub use screens::online_room::OnlineRoomState;

/// 屏间转换请求.
pub enum Transition {
    Quit,
    EnterMainMenu,
    EnterConfig,
    /// Config 屏 Enter 触发: 用 ConfigState 内的 config + seed_choice 起一局新游戏.
    StartGame,
    /// InGame 检测到 Phase::GameEnd 触发.
    EnterGameOver {
        rankings: [Ranking; 4],
    },
    /// 主菜单进局域网大厅.
    EnterOnlineLobby,
    /// 大厅创建房间 → 进 OnlineRoom.
    CreateOnlineRoom {
        nickname: String,
    },
    /// 大厅加入房间 → 进 OnlineRoom (远程, 走 ws).
    JoinOnlineRoom {
        nickname: String,
        addr: String,
    },
    /// 房间内 server 推送 GameStateView/InGame, 切到 OnlineGame.
    EnterOnlineGame,
    /// 屏请求弹一个 ConfirmModal. App 拦下来 stash, Yes 时执行 ConfirmAction.
    RequestConfirm {
        modal: Box<ConfirmModal>,
        action: ConfirmAction,
    },
}

/// 危险操作的副作用类型. 由 App 在 confirm Yes 后 dispatch 执行.
#[derive(Debug, Clone, Copy)]
pub enum ConfirmAction {
    Quit,
    /// InGame Esc 回主菜单 (丢进度).
    BackToMainMenu,
    /// OnlineRoom L (send Leave + 回主菜单).
    LeaveOnlineRoom,
    /// OnlineRoom Esc (同 LeaveOnlineRoom 但提示文案不同).
    LeaveOnlineRoomViaEsc,
    /// OnlineGame L / Esc (send Leave + 回主菜单).
    LeaveOnlineGame,
    /// OnlineLobby Esc (drop browser + 回主菜单).
    LeaveOnlineLobby,
}

pub enum Screen {
    MainMenu(MainMenuState),
    Config(ConfigState),
    InGame(Box<GameScreenState>),
    GameOver(GameOverState),
    OnlineLobby(OnlineLobbyState),
    OnlineRoom(Box<OnlineRoomState>),
    OnlineGame(Box<OnlineGameState>),
}

pub struct App {
    pub screen: Screen,
    pub running: bool,
    pub last_config: GameRules,
    pub last_seed_choice: SeedChoice,
    /// 本地 UI 偏好 (主题等), 不绑房间.
    pub local_prefs: LocalPrefs,
    /// tokio runtime handle, 用于 sync UI 线程调用 async net 操作.
    pub runtime: tokio::runtime::Handle,
    /// 房主 mode: 当前 listener 的 multiaddr (含 peer-id, 给加入者拷贝用).
    /// 自动从 [`Self::host_listen_addrs`] 选最优 (含 /p2p-circuit/ > 公网 > LAN).
    pub host_dial_addr: Option<libp2p::Multiaddr>,
    /// 房主 mode: 全部 listen 地址 (LAN + 公网 + relay 中转), 累积自 swarm 事件.
    pub host_listen_addrs: Vec<libp2p::Multiaddr>,
    /// 房主 mode: P2P listener 句柄. drop = 关闭 swarm + mDNS 广告.
    pub host_listener: Option<crate::net::p2p::host::HostHandle>,
    /// 房主 AutoNAT 探测出的可达性状态 (None = 还没探测).
    pub host_nat: Option<crate::net::p2p::host::NatReachability>,
    /// 当前正在显示的 ConfirmModal (含待执行 action). 拦截一切按键直到关闭.
    pub pending_confirm: Option<(ConfirmModal, ConfirmAction)>,
    /// 启动时 prefs 加载状态产生的一次性 banner (主菜单显示).
    /// 第一次按键后清掉, 不再显示.
    pub startup_banner: Option<String>,
}

impl App {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let LoadResult { prefs, status } = LocalPrefs::load();
        let startup_banner = status.user_visible_banner();
        Self {
            screen: Screen::MainMenu(MainMenuState::new()),
            running: true,
            last_config: GameRules::default(),
            last_seed_choice: SeedChoice::Random,
            local_prefs: prefs,
            runtime,
            host_dial_addr: None,
            host_listen_addrs: Vec::new(),
            host_listener: None,
            host_nat: None,
            pending_confirm: None,
            startup_banner,
        }
    }

    pub fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        while self.running {
            terminal.draw(|f| self.render(f))?;
            self.tick()?;
        }
        Ok(())
    }

    /// 拉空 host_listener.event_rx, 把 NAT/DCUtR/listen-addr 状态更新到 App.
    fn drain_host_events(&mut self) {
        use crate::net::p2p::host::HostEvent;
        let Some(listener) = self.host_listener.as_mut() else {
            return;
        };
        let mut addr_changed = false;
        while let Ok(ev) = listener.event_rx.try_recv() {
            match ev {
                HostEvent::NatStatusChanged { reachability } => {
                    self.host_nat = Some(reachability);
                }
                HostEvent::DcutrResult { peer_id, upgraded } => {
                    tracing::info!("dcutr peer={peer_id} upgraded={upgraded}");
                }
                HostEvent::PeerJoined {
                    peer_id, player_id, ..
                } => {
                    tracing::debug!("peer joined: {peer_id} as player {player_id}");
                }
                HostEvent::PeerLeft { peer_id } => {
                    tracing::debug!("peer left: {peer_id}");
                }
                HostEvent::NewListenAddr { addr } => {
                    if !self.host_listen_addrs.contains(&addr) {
                        self.host_listen_addrs.push(addr);
                        addr_changed = true;
                    }
                }
            }
        }
        if addr_changed {
            self.host_dial_addr = self
                .host_listen_addrs
                .iter()
                .max_by_key(|a| addr_score(a))
                .cloned();
        }
    }

    /// 单步: poll 事件 → 全局/屏处理 → InGame advance → 应用 transition.
    fn tick(&mut self) -> Result<()> {
        self.drain_host_events();
        let timeout = self.poll_timeout();
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && let Some(t) = self.handle_key(key)
        {
            self.apply_transition(t);
            return Ok(());
        }
        // 推进各屏的 advance (InGame 主推, Online 屏轮询 transport)
        let transition = match &mut self.screen {
            Screen::InGame(s) => s.advance(),
            Screen::OnlineLobby(s) => s.advance(),
            Screen::OnlineRoom(s) => s.advance(),
            Screen::OnlineGame(s) => s.advance(),
            _ => None,
        };
        if let Some(t) = transition {
            self.apply_transition(t);
        }
        Ok(())
    }

    fn poll_timeout(&self) -> Duration {
        const FRAME_MS: u64 = 80;
        if let Screen::InGame(s) = &self.screen
            && let Some(d) = s.decision_deadline
        {
            let now = Instant::now();
            let remaining = d.saturating_duration_since(now);
            return remaining.min(Duration::from_millis(FRAME_MS));
        }
        Duration::from_millis(FRAME_MS)
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Transition> {
        // 第一次任何按键后清启动 banner.
        self.startup_banner = None;
        // pending_confirm 优先吃所有按键.
        if let Some((modal, action)) = self.pending_confirm.as_mut() {
            if let Some(choice) = modal.handle_key(key) {
                let action = *action;
                self.pending_confirm = None;
                if choice == ConfirmChoice::Yes {
                    return Some(self.execute_confirm_action(action));
                }
            }
            return None;
        }
        // 大写 T: 全局切换主题 (避免与 InGame 的小写 t 冲突).
        // COMMAND 模式下放行让命令缓冲区接受字符.
        if key.code == KeyCode::Char('T') && !self.is_in_command_mode() {
            self.cycle_theme();
            return None;
        }
        // 全局快捷键: Q 弹确认 modal (主菜单除外, 直接退).
        // COMMAND 模式下当字符.
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) && !self.is_in_command_mode()
        {
            return Some(Transition::RequestConfirm {
                modal: Box::new(ConfirmModal::new("退出程序", "确定退出 tui-majo?")),
                action: ConfirmAction::Quit,
            });
        }
        // Esc: 主菜单不响应; Config/GameOver 直接回主菜单 (无副作用);
        // 其它 4 屏派发给屏自己 (屏决定要不要弹 confirm).
        if key.code == KeyCode::Esc && !self.is_in_command_mode() && !self.is_in_modal() {
            return match &mut self.screen {
                Screen::MainMenu(_) => None,
                Screen::Config(_) | Screen::GameOver(_) => Some(Transition::EnterMainMenu),
                Screen::InGame(s) => s.handle_event(key),
                Screen::OnlineLobby(s) => s.handle_event(key),
                Screen::OnlineRoom(s) => s.handle_event(key),
                Screen::OnlineGame(s) => s.handle_event(key),
            };
        }
        // 派发到屏.
        match &mut self.screen {
            Screen::MainMenu(s) => s.handle_event(key),
            Screen::Config(s) => s.handle_event(key),
            Screen::InGame(s) => s.handle_event(key),
            Screen::GameOver(s) => s.handle_event(key),
            Screen::OnlineLobby(s) => s.handle_event(key),
            Screen::OnlineRoom(s) => s.handle_event(key),
            Screen::OnlineGame(s) => s.handle_event(key),
        }
    }

    fn execute_confirm_action(&mut self, action: ConfirmAction) -> Transition {
        match action {
            ConfirmAction::Quit => Transition::Quit,
            ConfirmAction::BackToMainMenu | ConfirmAction::LeaveOnlineLobby => {
                Transition::EnterMainMenu
            }
            ConfirmAction::LeaveOnlineRoom | ConfirmAction::LeaveOnlineRoomViaEsc => {
                if let Screen::OnlineRoom(s) = &mut self.screen {
                    s.session.send(crate::net::protocol::ClientMsg::Leave);
                }
                Transition::EnterMainMenu
            }
            ConfirmAction::LeaveOnlineGame => {
                if let Screen::OnlineGame(s) = &mut self.screen {
                    s.session.send(crate::net::protocol::ClientMsg::Leave);
                }
                Transition::EnterMainMenu
            }
        }
    }

    fn is_in_command_mode(&self) -> bool {
        if let Screen::InGame(s) = &self.screen {
            s.is_command_mode()
        } else {
            false
        }
    }

    fn is_in_modal(&self) -> bool {
        if let Screen::InGame(s) = &self.screen {
            s.modal_open
        } else {
            false
        }
    }

    fn cycle_theme(&mut self) {
        let next = self.local_prefs.theme.next();
        self.local_prefs.theme = next;
        if let Err(e) = self.local_prefs.save() {
            tracing::warn!("保存 prefs 失败: {e}");
        }
        // 同步到当前屏幕 (各屏自己缓存了 theme_kind).
        match &mut self.screen {
            Screen::InGame(s) => s.set_theme(next),
            Screen::OnlineRoom(s) => s.set_theme(next),
            Screen::OnlineGame(s) => s.theme_kind = next,
            _ => {}
        }
    }

    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::Quit => {
                self.running = false;
            }
            Transition::EnterMainMenu => {
                // 房主退出 → drop listener (含 mDNS 广告 + swarm).
                // RoomHandle 会随 OnlineRoomState drop.
                self.host_listener.take();
                self.host_dial_addr = None;
                self.host_listen_addrs.clear();
                self.host_nat = None;
                self.screen = Screen::MainMenu(MainMenuState::new());
            }
            Transition::EnterConfig => {
                self.screen =
                    Screen::Config(ConfigState::from(&self.last_config, &self.last_seed_choice));
            }
            Transition::StartGame => {
                if let Screen::Config(c) = &self.screen {
                    self.last_config = c.config.clone();
                    self.last_seed_choice = c.seed_choice;
                }
                let seed = screens::config::resolve_seed(self.last_seed_choice);
                self.screen = Screen::InGame(Box::new(GameScreenState::new(
                    self.last_config.clone(),
                    seed,
                    self.local_prefs.theme,
                )));
            }
            Transition::EnterGameOver { rankings } => {
                self.screen = Screen::GameOver(GameOverState::new(rankings));
            }
            Transition::EnterOnlineLobby => {
                self.screen = Screen::OnlineLobby(OnlineLobbyState::new(&self.runtime));
            }
            Transition::CreateOnlineRoom { nickname } => {
                self.create_online_room(nickname);
            }
            Transition::JoinOnlineRoom { nickname, addr } => {
                self.join_online_room(nickname, addr);
            }
            Transition::EnterOnlineGame => {
                self.transition_room_to_game();
            }
            Transition::RequestConfirm { modal, action } => {
                self.pending_confirm = Some((*modal, action));
            }
        }
    }

    /// 创建本地 RoomActor (房主), 同时启动 P2P listener 让远程玩家可加入,
    /// 自己用 LocalSession 直连 RoomActor. listener 内部跑 mDNS 广告 + libp2p swarm.
    fn create_online_room(&mut self, nickname: String) {
        use crate::net::p2p::bootstrap::effective_bootstrap_relays;
        use crate::net::p2p::discovery::encode_metadata;
        use crate::net::p2p::host::spawn_p2p_listener;
        use crate::net::room::spawn_room;
        use crate::net::session::spawn_local_session;

        let room_id = format!("{}", uuid::Uuid::new_v4());
        let metadata = encode_metadata(&nickname, 1, "lobby", &room_id);
        let bootstrap = effective_bootstrap_relays(&self.local_prefs.network.bootstrap_relays);

        // spawn_room 内部用 tokio::spawn, 必须在 runtime context 中调用.
        let setup_result = self.runtime.block_on(async {
            let handle = spawn_room(nickname.clone(), self.last_config.clone());
            let listener = spawn_p2p_listener(handle.clone(), metadata, bootstrap)
                .await
                .map_err(|e| format!("P2P listener 启动失败: {e}"))?;
            let session = spawn_local_session(handle.clone(), nickname.clone())
                .await
                .map_err(|e| format!("房主 join 失败: {e}"))?;
            Ok::<_, String>((listener, session))
        });
        let (listener, session) = match setup_result {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("创建房间失败: {e}");
                self.screen = Screen::OnlineLobby(OnlineLobbyState::with_message(
                    &self.runtime,
                    format!("创建失败: {e}"),
                ));
                return;
            }
        };
        let dial_addr = listener.dial_addr.clone();
        self.host_dial_addr = dial_addr.clone();
        self.host_listener = Some(listener);

        let room_id_display = match &dial_addr {
            Some(a) => format!("LAN @ {a}"),
            None => "LAN".into(),
        };
        let placeholder_view = crate::net::protocol::RoomView {
            room_id: room_id_display,
            host_id: session.player_id,
            config: self.last_config.clone(),
            players: vec![],
            state: crate::net::protocol::RoomLifecycle::Lobby,
        };
        let mut room_state = OnlineRoomState::new(session, placeholder_view);
        room_state.set_theme(self.local_prefs.theme);
        self.screen = Screen::OnlineRoom(Box::new(room_state));
    }

    /// 远程加入房间. `addr` 是 multiaddr 字符串 (含 /p2p/<peer-id>).
    fn join_online_room(&mut self, nickname: String, addr: String) {
        use crate::net::p2p::join::join_remote;
        let multiaddr: libp2p::Multiaddr = match addr.parse() {
            Ok(m) => m,
            Err(e) => {
                self.screen = Screen::OnlineLobby(OnlineLobbyState::with_message(
                    &self.runtime,
                    format!("地址格式错误: {e}"),
                ));
                return;
            }
        };
        let r = self
            .runtime
            .block_on(async { join_remote(&multiaddr, nickname).await });
        match r {
            Ok(session) => {
                let placeholder_view = crate::net::protocol::RoomView {
                    room_id: format!("远程 @ {addr}"),
                    host_id: 0,
                    config: GameRules::default(),
                    players: vec![],
                    state: crate::net::protocol::RoomLifecycle::Lobby,
                };
                let mut room_state = OnlineRoomState::new(session, placeholder_view);
                room_state.set_theme(self.local_prefs.theme);
                self.screen = Screen::OnlineRoom(Box::new(room_state));
            }
            Err(e) => {
                self.screen = Screen::OnlineLobby(OnlineLobbyState::with_message(
                    &self.runtime,
                    format!("加入失败: {e}"),
                ));
            }
        }
    }

    /// OnlineRoom → OnlineGame: 移交 session.
    fn transition_room_to_game(&mut self) {
        let prev = std::mem::replace(&mut self.screen, Screen::MainMenu(MainMenuState::new()));
        if let Screen::OnlineRoom(state) = prev {
            let s = *state;
            self.screen = Screen::OnlineGame(Box::new(OnlineGameState::new(
                s.session,
                self.local_prefs.theme,
            )));
        }
    }

    fn render(&self, f: &mut ratatui::Frame) {
        let area = f.area();
        const MIN_W: u16 = 144;
        const MIN_H: u16 = 40;
        if area.width < MIN_W || area.height < MIN_H {
            self.render_size_warning(f, area, MIN_W, MIN_H);
            return;
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(1)])
            .split(area);
        self.render_main(f, chunks[0]);
        self.render_global_footer(f, chunks[1]);
        // 全局叠加 ConfirmModal (在所有屏内容之上).
        if let Some((modal, _)) = &self.pending_confirm {
            let theme = self.local_prefs.theme.theme();
            modal.render(f.buffer_mut(), area, &theme);
        }
    }

    fn render_size_warning(&self, f: &mut ratatui::Frame, area: Rect, min_w: u16, min_h: u16) {
        let theme = self.local_prefs.theme.theme();
        // 整屏背景色.
        let buf = f.buffer_mut();
        for y in area.y..(area.y + area.height) {
            for x in area.x..(area.x + area.width) {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ').set_bg(theme.bg).set_fg(theme.fg);
                }
            }
        }
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "终端窗口太小",
                Style::default()
                    .fg(theme.danger)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(format!("当前尺寸: {} × {}", area.width, area.height)),
            Line::from(format!("需要尺寸: {} × {}", min_w, min_h)),
            Line::from(""),
            Line::from(Span::styled(
                "请放大窗口 (或按 F11 全屏)",
                Style::default().fg(theme.fg),
            )),
            Line::from(""),
            Line::from(Span::styled("Q 退出", Style::default().fg(theme.dim))),
        ];
        let p = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(p, area);
    }

    fn render_main(&self, f: &mut ratatui::Frame, area: Rect) {
        match &self.screen {
            Screen::MainMenu(s) => s.render(f, area, self.startup_banner.as_deref()),
            Screen::Config(s) => s.render(f, area),
            Screen::InGame(s) => s.render(f, area),
            Screen::GameOver(s) => s.render(f, area),
            Screen::OnlineLobby(s) => s.render(f, area),
            Screen::OnlineRoom(s) => s.render(
                f,
                area,
                self.host_nat.as_ref(),
                self.host_dial_addr.as_ref(),
            ),
            Screen::OnlineGame(s) => s.render(f, area),
        }
    }

    fn render_global_footer(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        match &self.screen {
            Screen::MainMenu(_) => {
                spans.push(Span::styled(
                    "  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Screen::InGame(s) => {
                spans.push(Span::styled(
                    "  Esc 回主菜单  |  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
                if let Some(secs) = s.remaining_seconds() {
                    let color = if secs <= 5 { Color::Red } else { Color::Yellow };
                    spans.push(Span::styled(
                        format!("|  ⏱ 剩 {}s  ", secs),
                        Style::default().fg(color),
                    ));
                }
            }
            _ => {
                spans.push(Span::styled(
                    "  Esc 回主菜单  |  Q 退出  ",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        let p = Paragraph::new(Line::from(spans));
        f.render_widget(p, area);
    }
}
