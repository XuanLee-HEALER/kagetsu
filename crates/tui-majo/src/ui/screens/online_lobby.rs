//! 在线大厅: 输入 nickname → 创建房间 / 加入房间.
//!
//! 房间发现:
//! - LAN 走 mDNS (libp2p mdns Behaviour) 自动发现同 WiFi 房间
//! - 公网走 DHT/gossipsub (M3.B 后) 发现 bootstrap relay 注册的房间
//! - 手动输入 multiaddr (含 /p2p-circuit/ 走 relay 中转, 不含则尝试直连)

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use majo_core::net::p2p::discovery::{RoomBrowser, RoomEntry};
use majo_core::net::p2p::{Region, RoomMode};
use crate::ui::Transition;

/// 大厅项目焦点.
const FOCUS_NICKNAME: usize = 0;
const FOCUS_CREATE: usize = 1;
const FOCUS_DISCOVERED: usize = 2;
const FOCUS_ADDR: usize = 3;
const FOCUS_JOIN: usize = 4;
const ITEM_COUNT: usize = 5;

pub struct OnlineLobbyState {
    pub nickname: String,
    /// 加入房间用的 host 地址, 形如 `192.168.1.5:34567`.
    pub addr: String,
    pub focus: usize,
    pub message: String,
    /// mDNS browser, 启动失败时 None (e.g. 容器/受限网络).
    pub browser: Option<RoomBrowser>,
    /// 当前发现到的房间列表, 已按 region_filter 过滤 (每帧 poll 更新).
    pub discovered: Vec<RoomEntry>,
    /// discovered 列表里选中行 (focus=FOCUS_DISCOVERED 时生效).
    pub discovered_selected: usize,
    /// 大厅 region 过滤器 (M3.E). 初值取自 prefs.network.region;
    /// 'R' 键 cycle 切换. Unknown 表示不过滤显示全部.
    pub region_filter: Region,
    /// 过滤前总房间数 (UI 显示 "3/5" 用).
    pub discovered_total: usize,
    /// 创建房间时用的模式. 'M' 键切换 (Standard ↔ ZeroTrust).
    /// 初值取自 prefs.network.default_room_mode.
    pub room_mode: RoomMode,
}

/// 给一个发现到的房间打 tag: [LAN] / [中转] / [远程].
/// LAN: 私网 IP (192.168/10/172.16-31, 169.254 等)
/// 中转: multiaddr 含 /p2p-circuit/
/// 远程: 公网 IP 直连
fn room_addr_tag(room: &RoomEntry) -> &'static str {
    use libp2p::multiaddr::Protocol;
    let Some(addr) = room.primary_addr() else {
        return "?";
    };
    let mut has_circuit = false;
    let mut public_ip = false;
    let mut private_ip = false;
    for p in addr.iter() {
        match p {
            Protocol::P2pCircuit => has_circuit = true,
            Protocol::Ip4(ip) => {
                if ip.is_private() || ip.is_loopback() || ip.is_link_local() {
                    private_ip = true;
                } else {
                    public_ip = true;
                }
            }
            Protocol::Ip6(ip) => {
                if ip.is_loopback() {
                    private_ip = true;
                } else {
                    public_ip = true;
                }
            }
            _ => {}
        }
    }
    if has_circuit {
        "中转"
    } else if public_ip {
        "远程"
    } else if private_ip {
        "LAN"
    } else {
        "?"
    }
}

/// 'R' 键切换时, 按 Region::all() 顺序循环到下一个.
fn next_region(current: Region) -> Region {
    let all = Region::all();
    let pos = all.iter().position(|r| *r == current).unwrap_or(0);
    all[(pos + 1) % all.len()]
}

impl OnlineLobbyState {
    pub fn new(
        runtime: &tokio::runtime::Handle,
        bootstrap_relays: Vec<libp2p::Multiaddr>,
        region_filter: Region,
        room_mode: RoomMode,
    ) -> Self {
        let browser = RoomBrowser::start(runtime, bootstrap_relays).ok();
        Self {
            nickname: String::new(),
            addr: String::new(),
            focus: FOCUS_NICKNAME,
            message: String::new(),
            browser,
            discovered: Vec::new(),
            discovered_selected: 0,
            region_filter,
            discovered_total: 0,
            room_mode,
        }
    }

    pub fn with_message(
        runtime: &tokio::runtime::Handle,
        bootstrap_relays: Vec<libp2p::Multiaddr>,
        region_filter: Region,
        room_mode: RoomMode,
        message: String,
    ) -> Self {
        Self {
            message,
            ..Self::new(runtime, bootstrap_relays, region_filter, room_mode)
        }
    }

    /// App.tick 调用: 让 browser poll mDNS 事件, 按 region_filter 过滤后赋给 discovered.
    pub fn advance(&mut self) -> Option<Transition> {
        if let Some(b) = self.browser.as_mut() {
            b.poll();
            let all = b.rooms();
            self.discovered_total = all.len();
            self.discovered = all
                .into_iter()
                .filter(|r| Region::matches_filter(r.region, self.region_filter))
                .collect();
            if self.discovered_selected >= self.discovered.len() && !self.discovered.is_empty() {
                self.discovered_selected = self.discovered.len() - 1;
            }
        }
        None
    }

    pub fn handle_event(&mut self, key: KeyEvent) -> Option<Transition> {
        // 在 discovered 列表内, 上下/J/K 改 discovered_selected
        if self.focus == FOCUS_DISCOVERED && !self.discovered.is_empty() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Char('J') => {
                    if self.discovered_selected + 1 < self.discovered.len() {
                        self.discovered_selected += 1;
                    }
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Char('K') => {
                    self.discovered_selected = self.discovered_selected.saturating_sub(1);
                    return None;
                }
                _ => {}
            }
        }
        match key.code {
            // 'R' / 'r': cycle 切换 region 过滤器 (任意 focus 下都生效, 但
            // 排除 nickname/addr 输入焦点免抢字符输入).
            KeyCode::Char('r') | KeyCode::Char('R')
                if !matches!(self.focus, FOCUS_NICKNAME | FOCUS_ADDR) =>
            {
                self.region_filter = next_region(self.region_filter);
                None
            }
            // 'M' / 'm': 切换创建房间的模式 (Standard ↔ ZeroTrust).
            // 排除 nickname/addr 输入焦点.
            KeyCode::Char('m') | KeyCode::Char('M')
                if !matches!(self.focus, FOCUS_NICKNAME | FOCUS_ADDR) =>
            {
                self.room_mode = match self.room_mode {
                    RoomMode::Standard => RoomMode::ZeroTrust,
                    RoomMode::ZeroTrust => RoomMode::Standard,
                };
                None
            }
            KeyCode::Tab | KeyCode::Down => {
                self.focus = (self.focus + 1) % ITEM_COUNT;
                None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = (self.focus + ITEM_COUNT - 1) % ITEM_COUNT;
                None
            }
            KeyCode::Char(c) if self.focus == FOCUS_NICKNAME => {
                if self.nickname.chars().count() < 16 {
                    self.nickname.push(c);
                }
                None
            }
            KeyCode::Backspace if self.focus == FOCUS_NICKNAME => {
                self.nickname.pop();
                None
            }
            KeyCode::Char(c) if self.focus == FOCUS_ADDR => {
                if self.addr.chars().count() < 64 {
                    self.addr.push(c);
                }
                None
            }
            KeyCode::Backspace if self.focus == FOCUS_ADDR => {
                self.addr.pop();
                None
            }
            KeyCode::Enter => match self.focus {
                FOCUS_NICKNAME => {
                    self.focus = FOCUS_CREATE;
                    None
                }
                FOCUS_CREATE => {
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    Some(Transition::CreateOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                        mode: self.room_mode,
                    })
                }
                FOCUS_DISCOVERED => {
                    if self.discovered.is_empty() {
                        self.message = "暂未发现房间, 用下方手动输 IP".into();
                        self.focus = FOCUS_ADDR;
                        return None;
                    }
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    let entry = &self.discovered[self.discovered_selected];
                    let addr = entry
                        .dial_multiaddr()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| entry.addr());
                    Some(Transition::JoinOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                        addr,
                    })
                }
                FOCUS_ADDR => {
                    self.focus = FOCUS_JOIN;
                    None
                }
                FOCUS_JOIN => {
                    if self.nickname.trim().is_empty() {
                        self.message = "请输入昵称".into();
                        self.focus = FOCUS_NICKNAME;
                        return None;
                    }
                    if self.addr.trim().is_empty() {
                        self.message = "请输入房间地址 (形如 192.168.1.5:34567)".into();
                        self.focus = FOCUS_ADDR;
                        return None;
                    }
                    Some(Transition::JoinOnlineRoom {
                        nickname: self.nickname.trim().to_string(),
                        addr: self.addr.trim().to_string(),
                    })
                }
                _ => None,
            },
            KeyCode::Esc => Some(Transition::RequestConfirm {
                modal: Box::new(crate::ui::confirm::ConfirmModal::new(
                    "回主菜单",
                    "确定离开局域网大厅?",
                )),
                action: crate::ui::ConfirmAction::LeaveOnlineLobby,
            }),
            _ => None,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 在线游戏 · 大厅 ")
            .title_alignment(Alignment::Center);
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));

        // 昵称输入
        let nickname_label = if self.focus == FOCUS_NICKNAME {
            "▶ 昵称: "
        } else {
            "  昵称: "
        };
        let mut nickname_text = self.nickname.clone();
        if self.focus == FOCUS_NICKNAME {
            nickname_text.push('_');
        }
        lines.push(Line::from(vec![
            Span::raw(nickname_label),
            Span::styled(
                nickname_text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));

        // 模式显示 + 'M' 键提示.
        {
            let (mode_label, mode_color) = match self.room_mode {
                RoomMode::Standard => ("Standard (房主权威)", Color::Cyan),
                RoomMode::ZeroTrust => ("ZeroTrust (P2P mental poker, 需 4 真人)", Color::Magenta),
            };
            lines.push(Line::from(vec![
                Span::raw("  模式: "),
                Span::styled(
                    mode_label,
                    Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   ('M' 键切换)", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
        }

        // 创建房间按钮
        {
            let prefix = if self.focus == FOCUS_CREATE {
                "▶ "
            } else {
                "  "
            };
            let mut style = Style::default();
            if self.focus == FOCUS_CREATE {
                style = style
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!(
                    "{}创建房间 (本机做房主, 同时支持 LAN mDNS + 公网 relay 中转)",
                    prefix
                ),
                style,
            )));
        }
        lines.push(Line::from(""));

        // mDNS 发现到的房间列表
        let discovered_focus = self.focus == FOCUS_DISCOVERED;
        let header_prefix = if discovered_focus { "▶ " } else { "  " };
        let header_style = if discovered_focus {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        let filter_label = if matches!(self.region_filter, Region::Unknown) {
            "全部".to_string()
        } else {
            self.region_filter.short_tag().to_string()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{}发现的房间 ({}/{}) · 筛选: {} [R 切换]",
                header_prefix,
                self.discovered.len(),
                self.discovered_total,
                filter_label,
            ),
            header_style,
        )));
        if self.browser.is_none() {
            lines.push(Line::from(Span::styled(
                "    (mDNS 启动失败, 用下方手动 IP)",
                Style::default().fg(Color::DarkGray),
            )));
        } else if self.discovered.is_empty() {
            lines.push(Line::from(Span::styled(
                "    暂无发现, 等几秒或手动输 IP",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (i, room) in self.discovered.iter().enumerate() {
                let cursor = if discovered_focus && i == self.discovered_selected {
                    "  ▶"
                } else {
                    "   "
                };
                let style = if discovered_focus && i == self.discovered_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let tag = room_addr_tag(room);
                let region_tag = room.region.short_tag();
                let mode_tag = room.mode.short_tag();
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} [{}][{}][{}] {} @ {} · {}/4 · {}",
                        cursor,
                        tag,
                        region_tag,
                        mode_tag,
                        room.host_nick,
                        room.addr(),
                        room.players,
                        room.state
                    ),
                    style,
                )));
            }
            if discovered_focus {
                lines.push(Line::from(Span::styled(
                    "    (j/k 选, Enter 加入)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines.push(Line::from(""));

        // 房间地址输入
        let addr_label = if self.focus == FOCUS_ADDR {
            "▶ 地址: "
        } else {
            "  地址: "
        };
        let mut addr_text = if self.addr.is_empty() {
            "(multiaddr, 例 /ip4/.../udp/.../quic-v1/p2p/...)".to_string()
        } else {
            self.addr.clone()
        };
        if self.focus == FOCUS_ADDR {
            addr_text.push('_');
        }
        let addr_style = if self.addr.is_empty() && self.focus != FOCUS_ADDR {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        };
        lines.push(Line::from(vec![
            Span::raw(addr_label),
            Span::styled(addr_text, addr_style),
        ]));

        // 加入房间按钮
        {
            let prefix = if self.focus == FOCUS_JOIN {
                "▶ "
            } else {
                "  "
            };
            let mut style = Style::default();
            if self.focus == FOCUS_JOIN {
                style = style
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!("{}加入房间 (输入地址后回车)", prefix),
                style,
            )));
        }

        if !self.message.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                self.message.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑↓/Tab 切焦点 · 回车 确认 (输入框时回车前进焦点)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "Esc 回主菜单 · Q 退出",
            Style::default().fg(Color::DarkGray),
        )));

        f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use majo_core::net::p2p::discovery::RoomEntry;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use libp2p::{Multiaddr, PeerId};

    fn make_state() -> OnlineLobbyState {
        OnlineLobbyState {
            nickname: String::new(),
            addr: String::new(),
            focus: FOCUS_NICKNAME,
            message: String::new(),
            browser: None,
            discovered: Vec::new(),
            discovered_selected: 0,
            region_filter: Region::Unknown,
            discovered_total: 0,
            room_mode: RoomMode::Standard,
        }
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn keycode(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn make_room_entry(addr: &str, region: Region, mode: RoomMode) -> RoomEntry {
        let kp = libp2p::identity::Keypair::generate_ed25519();
        let peer = PeerId::from(&kp.public());
        let multi: Multiaddr = addr.parse().unwrap();
        RoomEntry {
            peer_id: peer,
            addrs: vec![multi],
            host_nick: "h".into(),
            players: 1,
            state: "lobby".into(),
            room_id: "r".into(),
            region,
            mode,
        }
    }

    // ============================================================================
    // 纯函数: room_addr_tag / next_region
    // ============================================================================

    #[test]
    fn room_addr_tag_lan_for_private_ipv4() {
        let r = make_room_entry(
            "/ip4/192.168.1.5/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        );
        assert_eq!(room_addr_tag(&r), "LAN");
    }

    #[test]
    fn room_addr_tag_remote_for_public_ipv4() {
        let r = make_room_entry(
            "/ip4/8.8.8.8/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        );
        assert_eq!(room_addr_tag(&r), "远程");
    }

    #[test]
    fn room_addr_tag_circuit_for_relayed_addr() {
        // /p2p-circuit 含中转
        let r = make_room_entry(
            "/ip4/8.8.8.8/udp/4001/quic-v1/p2p-circuit",
            Region::Unknown,
            RoomMode::Standard,
        );
        assert_eq!(room_addr_tag(&r), "中转");
    }

    #[test]
    fn room_addr_tag_question_when_no_addr() {
        let r = RoomEntry {
            peer_id: PeerId::from(&libp2p::identity::Keypair::generate_ed25519().public()),
            addrs: vec![],
            host_nick: "h".into(),
            players: 0,
            state: "lobby".into(),
            room_id: "r".into(),
            region: Region::Unknown,
            mode: RoomMode::Standard,
        };
        assert_eq!(room_addr_tag(&r), "?");
    }

    #[test]
    fn next_region_cycles_through_all() {
        let mut r = Region::all()[0];
        for _ in 0..Region::all().len() {
            r = next_region(r);
        }
        assert_eq!(r, Region::all()[0], "完整循环一周应回到起点");
    }

    // ============================================================================
    // handle_event: focus 切换 / 输入 / Enter / Esc / 'R' / 'M'
    // ============================================================================

    #[test]
    fn tab_cycles_focus_through_5_items() {
        let mut s = make_state();
        let initial = s.focus;
        for _ in 0..ITEM_COUNT {
            s.handle_event(keycode(KeyCode::Tab));
        }
        assert_eq!(s.focus, initial, "5 次 Tab 应回到起点");
    }

    #[test]
    fn back_tab_goes_backwards() {
        let mut s = make_state();
        s.handle_event(keycode(KeyCode::BackTab));
        assert_eq!(s.focus, FOCUS_JOIN, "Up 从 NICKNAME 应到 JOIN");
    }

    #[test]
    fn nickname_focus_accepts_chars() {
        let mut s = make_state();
        s.handle_event(key('a'));
        s.handle_event(key('b'));
        assert_eq!(s.nickname, "ab");
    }

    #[test]
    fn nickname_max_16_chars() {
        let mut s = make_state();
        for _ in 0..20 {
            s.handle_event(key('x'));
        }
        assert_eq!(s.nickname.chars().count(), 16, "上限 16 字符");
    }

    #[test]
    fn nickname_backspace_pops() {
        let mut s = make_state();
        s.nickname = "abc".into();
        s.handle_event(keycode(KeyCode::Backspace));
        assert_eq!(s.nickname, "ab");
    }

    #[test]
    fn addr_focus_accepts_chars_with_64_limit() {
        let mut s = make_state();
        s.focus = FOCUS_ADDR;
        for _ in 0..70 {
            s.handle_event(key('x'));
        }
        assert_eq!(s.addr.chars().count(), 64);
    }

    #[test]
    fn addr_backspace_pops() {
        let mut s = make_state();
        s.focus = FOCUS_ADDR;
        s.addr = "host".into();
        s.handle_event(keycode(KeyCode::Backspace));
        assert_eq!(s.addr, "hos");
    }

    #[test]
    fn enter_on_nickname_advances_to_create() {
        let mut s = make_state();
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_CREATE);
    }

    #[test]
    fn enter_on_create_with_empty_nickname_resets_focus_and_messages() {
        let mut s = make_state();
        s.focus = FOCUS_CREATE;
        s.nickname.clear();
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_NICKNAME);
        assert!(!s.message.is_empty());
    }

    #[test]
    fn enter_on_create_with_valid_nickname_emits_create_transition() {
        let mut s = make_state();
        s.nickname = "Alice".into();
        s.focus = FOCUS_CREATE;
        s.room_mode = RoomMode::ZeroTrust;
        let t = s.handle_event(keycode(KeyCode::Enter));
        match t {
            Some(Transition::CreateOnlineRoom { nickname, mode }) => {
                assert_eq!(nickname, "Alice");
                assert_eq!(mode, RoomMode::ZeroTrust);
            }
            _ => panic!("应返回 CreateOnlineRoom"),
        }
    }

    #[test]
    fn enter_on_join_with_empty_nickname_resets_to_nickname_focus() {
        let mut s = make_state();
        s.focus = FOCUS_JOIN;
        s.nickname.clear();
        s.addr = "/ip4/1.2.3.4/udp/4001/quic-v1".into();
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_NICKNAME);
    }

    #[test]
    fn enter_on_join_with_empty_addr_resets_to_addr_focus() {
        let mut s = make_state();
        s.focus = FOCUS_JOIN;
        s.nickname = "Alice".into();
        s.addr.clear();
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_ADDR);
    }

    #[test]
    fn enter_on_join_valid_emits_join_transition() {
        let mut s = make_state();
        s.focus = FOCUS_JOIN;
        s.nickname = "Bob".into();
        s.addr = "/ip4/1.2.3.4/udp/4001/quic-v1/p2p/Q".into();
        let t = s.handle_event(keycode(KeyCode::Enter));
        match t {
            Some(Transition::JoinOnlineRoom { nickname, addr }) => {
                assert_eq!(nickname, "Bob");
                assert!(addr.starts_with("/ip4"));
            }
            _ => panic!("应返回 JoinOnlineRoom"),
        }
    }

    #[test]
    fn enter_on_addr_advances_to_join_focus() {
        let mut s = make_state();
        s.focus = FOCUS_ADDR;
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_JOIN);
    }

    #[test]
    fn enter_on_discovered_empty_list_falls_back_to_addr_focus() {
        let mut s = make_state();
        s.focus = FOCUS_DISCOVERED;
        s.discovered.clear();
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_ADDR);
        assert!(!s.message.is_empty());
    }

    #[test]
    fn enter_on_discovered_with_room_emits_join_transition() {
        let mut s = make_state();
        s.focus = FOCUS_DISCOVERED;
        s.nickname = "Alice".into();
        s.discovered.push(make_room_entry(
            "/ip4/1.2.3.4/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        ));
        let t = s.handle_event(keycode(KeyCode::Enter));
        match t {
            Some(Transition::JoinOnlineRoom { nickname, addr }) => {
                assert_eq!(nickname, "Alice");
                assert!(addr.contains("p2p"), "discovered 路径应注入 dial_multiaddr");
            }
            _ => panic!("应返回 JoinOnlineRoom"),
        }
    }

    #[test]
    fn enter_on_discovered_with_empty_nickname_resets_to_nickname() {
        let mut s = make_state();
        s.focus = FOCUS_DISCOVERED;
        s.nickname.clear();
        s.discovered.push(make_room_entry(
            "/ip4/1.2.3.4/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        ));
        let t = s.handle_event(keycode(KeyCode::Enter));
        assert!(t.is_none());
        assert_eq!(s.focus, FOCUS_NICKNAME);
    }

    #[test]
    fn region_key_cycles_when_not_in_text_input() {
        let mut s = make_state();
        s.focus = FOCUS_CREATE;
        let before = s.region_filter;
        s.handle_event(key('R'));
        assert_ne!(s.region_filter, before);
    }

    #[test]
    fn region_key_ignored_in_nickname_input() {
        let mut s = make_state();
        s.focus = FOCUS_NICKNAME;
        let before = s.region_filter;
        s.handle_event(key('R'));
        assert_eq!(s.region_filter, before);
        assert!(s.nickname.contains('R'), "应作为字符输入到 nickname");
    }

    #[test]
    fn mode_key_toggles_room_mode_when_not_in_text_input() {
        let mut s = make_state();
        s.focus = FOCUS_CREATE;
        s.room_mode = RoomMode::Standard;
        s.handle_event(key('M'));
        assert_eq!(s.room_mode, RoomMode::ZeroTrust);
        s.handle_event(key('M'));
        assert_eq!(s.room_mode, RoomMode::Standard);
    }

    #[test]
    fn discovered_list_j_k_navigates_selection() {
        let mut s = make_state();
        s.focus = FOCUS_DISCOVERED;
        s.discovered.push(make_room_entry(
            "/ip4/1.2.3.4/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        ));
        s.discovered.push(make_room_entry(
            "/ip4/5.6.7.8/udp/4001/quic-v1",
            Region::Unknown,
            RoomMode::Standard,
        ));
        s.handle_event(key('j'));
        assert_eq!(s.discovered_selected, 1);
        s.handle_event(key('j')); // 边界, 不增
        assert_eq!(s.discovered_selected, 1);
        s.handle_event(key('k'));
        assert_eq!(s.discovered_selected, 0);
        s.handle_event(key('k')); // 已为 0, 仍为 0
        assert_eq!(s.discovered_selected, 0);
    }

    #[test]
    fn esc_returns_request_confirm_transition() {
        let mut s = make_state();
        let t = s.handle_event(keycode(KeyCode::Esc));
        assert!(matches!(t, Some(Transition::RequestConfirm { .. })));
    }

    #[test]
    fn advance_with_no_browser_returns_none() {
        let mut s = make_state();
        let t = s.advance();
        assert!(t.is_none());
        assert!(s.discovered.is_empty());
    }

    // ============================================================================
    // render smoke — 不 panic 即可
    // ============================================================================

    #[test]
    fn render_empty_does_not_panic() {
        let s = make_state();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area())).unwrap();
    }

    #[test]
    fn render_with_discovered_rooms_does_not_panic() {
        let mut s = make_state();
        s.nickname = "Alice".into();
        s.addr = "/ip4/1.2.3.4/udp/4001/quic-v1".into();
        s.message = "test".into();
        s.discovered.push(make_room_entry(
            "/ip4/192.168.1.5/udp/4001/quic-v1",
            Region::CnEast,
            RoomMode::Standard,
        ));
        s.discovered.push(make_room_entry(
            "/ip4/8.8.8.8/udp/4001/quic-v1/p2p-circuit",
            Region::Jp,
            RoomMode::ZeroTrust,
        ));
        s.discovered_total = 2;
        s.focus = FOCUS_DISCOVERED;
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f, f.area())).unwrap();
    }

    /// 各 focus 各跑一遍 render, 确保不同分支都被路过.
    #[test]
    fn render_each_focus_does_not_panic() {
        for f_idx in 0..ITEM_COUNT {
            let mut s = make_state();
            s.focus = f_idx;
            s.message = "msg".into();
            s.region_filter = Region::CnEast;
            s.room_mode = if f_idx % 2 == 0 {
                RoomMode::Standard
            } else {
                RoomMode::ZeroTrust
            };
            let backend = ratatui::backend::TestBackend::new(120, 40);
            let mut term = ratatui::Terminal::new(backend).unwrap();
            term.draw(|frame| s.render(frame, frame.area())).unwrap();
        }
    }
}
