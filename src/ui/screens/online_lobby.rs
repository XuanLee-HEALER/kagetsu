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

use crate::net::p2p::Region;
use crate::net::p2p::discovery::{RoomBrowser, RoomEntry};
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
        }
    }

    pub fn with_message(
        runtime: &tokio::runtime::Handle,
        bootstrap_relays: Vec<libp2p::Multiaddr>,
        region_filter: Region,
        message: String,
    ) -> Self {
        Self {
            message,
            ..Self::new(runtime, bootstrap_relays, region_filter)
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
