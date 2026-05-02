//! Bootstrap relay 节点列表 (Tier 1).
//!
//! 这些节点是公网可达的 libp2p relay-server, 用于:
//! - AutoNAT: 客户端通过它们探测自己是否公网可达
//! - Circuit Relay v2: NAT 后的房主向它们注册 reservation, 让加入者能连过来
//! - DCUtR: 协调两端打洞升级直连
//!
//! 加新节点编辑 `DEFAULT_BOOTSTRAP_RELAYS`. 用户也可在 prefs.toml 里 override:
//! ```toml
//! [network]
//! bootstrap_relays = [
//!     "/dns4/your-relay.example.com/udp/4001/quic-v1/p2p/12D3KooW...",
//! ]
//! ```

use libp2p::Multiaddr;

/// 默认 bootstrap relay 列表 (硬编码). 编译期常量字符串, 运行时解析.
///
/// **占位**: claw (新加坡 ECS) 部署后, 把它的 multiaddr 加进来.
/// 临时为空数组以便编译; 客户端无 bootstrap 时落到纯 LAN mDNS 模式.
pub const DEFAULT_BOOTSTRAP_RELAYS: &[&str] = &[
    // 示例 (部署后替换):
    // "/dns4/claw.example.com/udp/4001/quic-v1/p2p/12D3KooW...",
];

/// 解析字符串数组为 Multiaddr, 跳过解析失败的条目并写 warn 日志.
pub fn parse_bootstrap_addrs(raw: &[String]) -> Vec<Multiaddr> {
    raw.iter()
        .filter_map(|s| match s.parse::<Multiaddr>() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("bootstrap multiaddr 解析失败 ({s}): {e}");
                None
            }
        })
        .collect()
}

/// 拿到当前生效的 bootstrap relay 列表.
/// 优先级: prefs.toml override > 硬编码 default. (空 prefs 用 default)
pub fn effective_bootstrap_relays(override_list: &[String]) -> Vec<Multiaddr> {
    if !override_list.is_empty() {
        return parse_bootstrap_addrs(override_list);
    }
    DEFAULT_BOOTSTRAP_RELAYS
        .iter()
        .filter_map(|s| s.parse::<Multiaddr>().ok())
        .collect()
}

/// M3.D: 把静态 (Tier 1 prefs) + 动态 (Tier 2 玩家贡献池) relay 列表合并去重.
///
/// 顺序保留 static 优先 — host swarm dial bootstrap 时按列表顺序探, Tier 1
/// 通常更稳, 应优先尝试. dynamic 追加在后作 fallback.
pub fn merge_relay_pool(
    static_relays: Vec<Multiaddr>,
    dynamic_relays: Vec<Multiaddr>,
) -> Vec<Multiaddr> {
    let mut out = static_relays;
    for addr in dynamic_relays {
        if !out.contains(&addr) {
            out.push(addr);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_invalid_entries() {
        let raw = vec![
            "/ip4/1.2.3.4/tcp/4001".into(),
            "garbage not multiaddr".into(),
            "/ip4/5.6.7.8/udp/4001/quic-v1".into(),
        ];
        let parsed = parse_bootstrap_addrs(&raw);
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn override_takes_priority() {
        let override_list = vec!["/ip4/127.0.0.1/tcp/4001".into()];
        let result = effective_bootstrap_relays(&override_list);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn merge_dedup_keeps_static_first() {
        let s: Multiaddr = "/ip4/1.1.1.1/udp/4001/quic-v1".parse().unwrap();
        let d1: Multiaddr = "/ip4/2.2.2.2/udp/4001/quic-v1".parse().unwrap();
        let d2: Multiaddr = "/ip4/1.1.1.1/udp/4001/quic-v1".parse().unwrap(); // 跟 s 重复
        let merged = merge_relay_pool(vec![s.clone()], vec![d1.clone(), d2]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], s);
        assert_eq!(merged[1], d1);
    }

    #[test]
    fn empty_override_falls_back_to_default() {
        let override_list: Vec<String> = vec![];
        let result = effective_bootstrap_relays(&override_list);
        // 当前 DEFAULT_BOOTSTRAP_RELAYS 是空, 所以返回空 (占位).
        // 部署后此断言会变 result.len() > 0.
        assert_eq!(result.len(), DEFAULT_BOOTSTRAP_RELAYS.len());
    }
}
