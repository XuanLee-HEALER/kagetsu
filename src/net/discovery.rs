//! mDNS 服务广告 / 浏览 (LAN 房间发现).
//!
//! - 房主: [`DiscoveryAd::advertise`] 注册一个 mDNS service, 让局域网内
//!   其它 client 能发现.
//! - 加入者: [`DiscoveryBrowser::start`] 启动 browse, 通过 [`DiscoveryBrowser::poll`]
//!   非阻塞读取发现到的房间.
//!
//! ### 协议
//!
//! - service type: `_tui-majo._tcp.local.`
//! - TXT records:
//!   - `room_id`     房间 UUID 字符串
//!   - `host_nick`   房主昵称
//!   - `players`     当前人数 (1-4)
//!   - `state`       `lobby` | `in_game`

use std::collections::HashMap;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

const SERVICE_TYPE: &str = "_tui-majo._tcp.local.";

/// 房主端: 广告自家房间.
pub struct DiscoveryAd {
    daemon: ServiceDaemon,
    fullname: String,
}

impl DiscoveryAd {
    /// 注册 mDNS 广告. `port` 是房主 ws server 监听端口.
    /// `room_id` / `host_nick` / `players` / `lifecycle` 写入 TXT records.
    pub fn advertise(
        host_nick: &str,
        port: u16,
        room_id: &str,
        players: u8,
        lifecycle: &str,
    ) -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        // mdns instance + host name 必须 valid DNS chars.
        // 用 sanitize 后的 nick + room_id 短前缀做 instance name.
        let safe_nick: String = host_nick
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect();
        let prefix: String = room_id.chars().take(6).collect();
        let instance = if safe_nick.is_empty() {
            format!("host-{prefix}")
        } else {
            format!("{safe_nick}-{prefix}")
        };
        let host_local = format!("{instance}.local.");

        let mut props: HashMap<String, String> = HashMap::new();
        props.insert("room_id".into(), room_id.into());
        props.insert("host_nick".into(), host_nick.into());
        props.insert("players".into(), players.to_string());
        props.insert("state".into(), lifecycle.into());

        // ip = 空数组, 用 enable_addr_auto 让 mdns-sd 自动选可用接口.
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host_local, "", port, props)?
            .enable_addr_auto();

        let fullname = info.get_fullname().to_string();
        daemon.register(info)?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for DiscoveryAd {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        // shutdown 可能阻塞, 简单 best-effort.
        let _ = self.daemon.shutdown();
    }
}

/// 加入者端: 浏览发现的房间.
pub struct DiscoveryBrowser {
    daemon: ServiceDaemon,
    receiver: mdns_sd::Receiver<ServiceEvent>,
    /// 已知房间 (按 fullname 索引).
    rooms: HashMap<String, RoomEntry>,
}

/// 发现到的一个房间.
#[derive(Debug, Clone)]
pub struct RoomEntry {
    pub fullname: String,
    pub addr: String, // host:port
    pub room_id: String,
    pub host_nick: String,
    pub players: u8,
    pub state: String,
}

impl DiscoveryBrowser {
    pub fn start() -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        let receiver = daemon.browse(SERVICE_TYPE)?;
        Ok(Self {
            daemon,
            receiver,
            rooms: HashMap::new(),
        })
    }

    /// 非阻塞拉取 mDNS 事件, 更新内部 rooms 表.
    pub fn poll(&mut self) {
        loop {
            match self.receiver.recv_timeout(Duration::from_millis(0)) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let entry = make_entry(&info);
                    if let Some(e) = entry {
                        self.rooms.insert(e.fullname.clone(), e);
                    }
                }
                Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                    self.rooms.remove(&fullname);
                }
                Ok(_) => {}
                Err(_) => break, // empty / disconnected
            }
        }
    }

    pub fn rooms(&self) -> Vec<RoomEntry> {
        let mut v: Vec<_> = self.rooms.values().cloned().collect();
        v.sort_by(|a, b| a.host_nick.cmp(&b.host_nick));
        v
    }
}

impl Drop for DiscoveryBrowser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

fn make_entry(info: &ServiceInfo) -> Option<RoomEntry> {
    let port = info.get_port();
    let v4 = info.get_addresses_v4();
    let ip = v4.iter().next()?;
    let addr = format!("{}:{}", ip, port);

    let room_id = info
        .get_property_val_str("room_id")
        .unwrap_or("")
        .to_string();
    let host_nick = info
        .get_property_val_str("host_nick")
        .unwrap_or("?")
        .to_string();
    let players = info
        .get_property_val_str("players")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let state = info
        .get_property_val_str("state")
        .unwrap_or("?")
        .to_string();

    Some(RoomEntry {
        fullname: info.get_fullname().to_string(),
        addr,
        room_id,
        host_nick,
        players,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertise_then_browse_finds_room() {
        // 启动一个广告.
        let _ad =
            DiscoveryAd::advertise("Tester", 12345, "room-uuid-12345678", 1, "lobby").expect("ad");

        // 启动一个 browser, 等几秒看是否发现.
        let mut br = DiscoveryBrowser::start().expect("browse");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && br.rooms().is_empty() {
            br.poll();
            std::thread::sleep(Duration::from_millis(100));
        }
        let rooms = br.rooms();
        // mDNS 在 CI / 容器环境可能无 multicast, 不强制要求发现.
        // 但本地开发环境可手动看是否能收到.
        if rooms.is_empty() {
            eprintln!("[discovery test] 未发现 - 可能是无 mDNS 多播的环境, 跳过");
            return;
        }
        let r = &rooms[0];
        assert_eq!(r.host_nick, "Tester");
        assert_eq!(r.room_id, "room-uuid-12345678");
        assert_eq!(r.state, "lobby");
        assert_eq!(r.players, 1);
    }
}
