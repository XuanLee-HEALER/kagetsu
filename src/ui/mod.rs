//! TUI 顶层: App + Screen 枚举 + 主循环 + 全局快捷键.
//!
//! 屏间通过 [`Transition`] 切换. App 持有 [`last_config`] / [`last_seed_choice`]
//! 用于"新游戏"复用上次配置.

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

use crate::config::{GameConfig, LocalPrefs};
use crate::score::Ranking;

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
    pub last_config: GameConfig,
    pub last_seed_choice: SeedChoice,
    /// 本地 UI 偏好 (主题等), 不绑房间.
    pub local_prefs: LocalPrefs,
    /// tokio runtime handle, 用于 sync UI 线程调用 async net 操作.
    pub runtime: tokio::runtime::Handle,
    /// 房主 mode: 当前活跃 ws server 的端口 (用于显示给加入者).
    pub host_port: Option<u16>,
    /// 房主 mode: mDNS 广告. 房间结束 / 退出 lobby 时 drop.
    pub discovery_ad: Option<crate::net::discovery::DiscoveryAd>,
}

impl App {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            screen: Screen::MainMenu(MainMenuState::new()),
            running: true,
            last_config: GameConfig::default(),
            last_seed_choice: SeedChoice::Random,
            local_prefs: LocalPrefs::default(),
            runtime,
            host_port: None,
            discovery_ad: None,
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

    /// 单步: poll 事件 → 全局/屏处理 → InGame advance → 应用 transition.
    fn tick(&mut self) -> Result<()> {
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
        // 大写 T: 全局切换主题 (避免与 InGame 的小写 t 冲突).
        // COMMAND 模式下放行让命令缓冲区接受字符.
        if key.code == KeyCode::Char('T') && !self.is_in_command_mode() {
            self.cycle_theme();
            return None;
        }
        // 全局快捷键: Q 总是退出 (但 COMMAND 模式下当字符).
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) && !self.is_in_command_mode()
        {
            return Some(Transition::Quit);
        }
        // Esc: 主菜单上不响应; 其它屏回主菜单.
        // 例外: COMMAND 模式 / Modal 打开时交给屏处理 (取消命令 / 关闭 modal).
        if key.code == KeyCode::Esc && !self.is_in_command_mode() && !self.is_in_modal() {
            return match self.screen {
                Screen::MainMenu(_) => None,
                _ => Some(Transition::EnterMainMenu),
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
        // 同步到当前屏幕 (InGame 缓存了 theme_kind).
        if let Screen::InGame(s) = &mut self.screen {
            s.set_theme(next);
        }
    }

    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::Quit => {
                self.running = false;
            }
            Transition::EnterMainMenu => {
                // 房主退出 → 关 mDNS 广告 + 房间 (RoomHandle 会随 OnlineRoomState drop).
                self.discovery_ad.take();
                self.host_port = None;
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
                self.screen = Screen::OnlineLobby(OnlineLobbyState::new());
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
        }
    }

    /// 创建本地 RoomActor (房主), 同时启动 ws server 让远程玩家可加入,
    /// 自己用 LocalSession 直连 RoomActor. 同时 mDNS 广告该房间.
    fn create_online_room(&mut self, nickname: String) {
        use crate::net::discovery::DiscoveryAd;
        use crate::net::room::spawn_room;
        use crate::net::server::spawn_ws_server;
        use crate::net::session::spawn_local_session;

        // spawn_room 内部用 tokio::spawn, 必须在 runtime context 中调用.
        // 整个 setup (spawn_room + spawn_ws_server + spawn_local_session)
        // 包在一次 block_on 内确保都在 runtime 内.
        let setup_result = self.runtime.block_on(async {
            let handle = spawn_room(nickname.clone(), self.last_config.clone());
            let port_addr = spawn_ws_server(handle.clone(), 0)
                .await
                .map_err(|e| format!("ws server 启动失败: {e}"))?;
            let session = spawn_local_session(handle.clone(), nickname.clone())
                .await
                .map_err(|e| format!("房主 join 失败: {e}"))?;
            Ok::<_, String>((port_addr.port(), session))
        });
        let (port, session) = match setup_result {
            Ok(v) => (Some(v.0), v.1),
            Err(e) => {
                tracing::error!("创建房间失败: {e}");
                self.screen =
                    Screen::OnlineLobby(OnlineLobbyState::with_message(format!("创建失败: {e}")));
                return;
            }
        };
        self.host_port = port;

        // 启动 mDNS 广告 (port 必须存在才有意义).
        let room_id = format!("{}", uuid::Uuid::new_v4());
        if let Some(p) = port {
            match DiscoveryAd::advertise(&nickname, p, &room_id, 1, "lobby") {
                Ok(ad) => self.discovery_ad = Some(ad),
                Err(e) => tracing::warn!("mDNS 广告失败: {e}"),
            }
        }

        let placeholder_view = crate::net::protocol::RoomView {
            room_id: match port {
                Some(p) => format!("LAN @ :{p}"),
                None => "LAN".into(),
            },
            host_id: session.player_id,
            config: self.last_config.clone(),
            players: vec![],
            state: crate::net::protocol::RoomLifecycle::Lobby,
        };
        self.screen = Screen::OnlineRoom(Box::new(OnlineRoomState::new(session, placeholder_view)));
    }

    /// 远程加入房间 (走 ws).
    fn join_online_room(&mut self, nickname: String, addr: String) {
        use crate::net::server::join_remote;
        let r = self
            .runtime
            .block_on(async { join_remote(&addr, nickname).await });
        match r {
            Ok(session) => {
                let placeholder_view = crate::net::protocol::RoomView {
                    room_id: format!("远程 @ {addr}"),
                    host_id: 0,
                    config: GameConfig::default(),
                    players: vec![],
                    state: crate::net::protocol::RoomLifecycle::Lobby,
                };
                self.screen =
                    Screen::OnlineRoom(Box::new(OnlineRoomState::new(session, placeholder_view)));
            }
            Err(e) => {
                self.screen =
                    Screen::OnlineLobby(OnlineLobbyState::with_message(format!("加入失败: {e}")));
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
            Screen::MainMenu(s) => s.render(f, area),
            Screen::Config(s) => s.render(f, area),
            Screen::InGame(s) => s.render(f, area),
            Screen::GameOver(s) => s.render(f, area),
            Screen::OnlineLobby(s) => s.render(f, area),
            Screen::OnlineRoom(s) => s.render(f, area),
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
