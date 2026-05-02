//! 房间信任模式 (M4.B).
//!
//! 决定开局时是走房主权威路径 (Standard) 还是 mental-poker 零信任路径
//! (ZeroTrust). 由房主创建房间时指定, 跟随 [`crate::net::p2p::behaviour::LobbyAnnouncement`]
//! 广播让加入者看到房间 mode 后再决定加入.
//!
//! ## 模式语义
//!
//! - **Standard**: 房主进程持权威 GameState, 玩家动作走 RoomActor 验证 + apply.
//!   投影 GameStateView 给 client (隐藏他家手牌). 信任房主. 现状架构.
//! - **ZeroTrust**: 4 玩家对等, mental poker 协议保证开局牌山联合洗牌不可预知,
//!   摸牌走 threshold ElGamal (协议 2), dora 揭示走协议 3.
//!   单方无法预知牌山或看他家手牌. 性能开销: 开局 shuffle proof ~5-10s/玩家
//!   (BLS12-381 G1, K=80 安全参数).
//!
//! 序列化为 lowercase kebab string. 老 schema 没此字段时 [`Default`] = Standard
//! (兼容 M3 房间).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RoomMode {
    /// 房主权威, 现状架构. 信任房主.
    #[default]
    Standard,
    /// 对等零信任, mental poker. 单方无法作弊.
    ZeroTrust,
}

impl RoomMode {
    /// 给 UI 选择列表用.
    pub const fn all() -> &'static [RoomMode] {
        &[RoomMode::Standard, RoomMode::ZeroTrust]
    }

    /// 短 tag (大厅列表行用, 4 字符内).
    pub fn short_tag(self) -> &'static str {
        match self {
            RoomMode::Standard => "标准",
            RoomMode::ZeroTrust => "零信任",
        }
    }

    /// 完整中文标签.
    pub fn label(self) -> &'static str {
        match self {
            RoomMode::Standard => "标准模式 (房主权威)",
            RoomMode::ZeroTrust => "零信任模式 (mental poker)",
        }
    }

    /// kebab-case wire 形式, 跟 serde 一致.
    pub fn as_kebab(self) -> &'static str {
        match self {
            RoomMode::Standard => "standard",
            RoomMode::ZeroTrust => "zero-trust",
        }
    }

    pub fn from_kebab(s: &str) -> RoomMode {
        match s {
            "zero-trust" => RoomMode::ZeroTrust,
            _ => RoomMode::Standard,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_round_trip() {
        for m in RoomMode::all() {
            assert_eq!(RoomMode::from_kebab(m.as_kebab()), *m);
        }
    }

    #[test]
    fn from_kebab_unknown_falls_back_to_standard() {
        assert_eq!(RoomMode::from_kebab("garbage"), RoomMode::Standard);
        assert_eq!(RoomMode::from_kebab(""), RoomMode::Standard);
    }

    #[test]
    fn default_is_standard() {
        assert_eq!(RoomMode::default(), RoomMode::Standard);
    }

    #[test]
    fn serde_via_toml_table() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrapper {
            mode: RoomMode,
        }
        let w = Wrapper {
            mode: RoomMode::ZeroTrust,
        };
        let s = toml::to_string(&w).unwrap();
        assert!(s.contains("mode = \"zero-trust\""));
        let w2: Wrapper = toml::from_str(&s).unwrap();
        assert_eq!(w, w2);
    }
}
