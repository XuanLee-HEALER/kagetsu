//! 房间地理区域 (M3.E).
//!
//! 跨大洲匹配延迟太高 (太平洋 ~150ms RTT, 加 relay 中转 → 300ms+).
//! 让用户在创建房间时选 region, 大厅按此过滤显示.
//!
//! 序列化为 lowercase kebab string (`cn-east` / `jp` 等), 老 prefs.toml 没字段
//! 时 `#[serde(default)]` 给 `Unknown`. 大厅显示 region tag, 过滤器
//! "全部" 显示所有 region 或仅显示用户偏好 region.

use serde::{Deserialize, Serialize};

/// 房间地理区域. UI 让用户从预设列表中选, 也允许 `Unknown`
/// (老房间 / 没填的房间, 默认显示).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Region {
    /// 中国大陆华东 (上海 / 杭州 / 南京 / 苏州).
    CnEast,
    /// 中国大陆华南 (深圳 / 广州).
    CnSouth,
    /// 中国大陆华北 (北京 / 天津).
    CnNorth,
    /// 日本 (东京 / 大阪).
    Jp,
    /// 新加坡 / 马来西亚.
    Sg,
    /// 北美东海岸 (弗吉尼亚 / 纽约).
    UsEast,
    /// 北美西海岸 (加州 / 俄勒冈).
    UsWest,
    /// 欧洲 (法兰克福 / 伦敦 / 巴黎).
    Eu,
    /// 其它区域.
    Other,
    /// 未填 (老房间 / 默认).
    #[default]
    Unknown,
}

impl Region {
    /// 给 UI 选择列表用的所有 variant 顺序 (Unknown 排最后).
    pub const fn all() -> &'static [Region] {
        &[
            Region::CnEast,
            Region::CnSouth,
            Region::CnNorth,
            Region::Jp,
            Region::Sg,
            Region::UsEast,
            Region::UsWest,
            Region::Eu,
            Region::Other,
            Region::Unknown,
        ]
    }

    /// 短 tag 标签 (用于大厅列表显示, 4 字符内).
    pub fn short_tag(self) -> &'static str {
        match self {
            Region::CnEast => "华东",
            Region::CnSouth => "华南",
            Region::CnNorth => "华北",
            Region::Jp => "日本",
            Region::Sg => "新马",
            Region::UsEast => "美东",
            Region::UsWest => "美西",
            Region::Eu => "欧洲",
            Region::Other => "其它",
            Region::Unknown => "?",
        }
    }

    /// 完整中文标签 (config 选择 modal 用).
    pub fn label(self) -> &'static str {
        match self {
            Region::CnEast => "中国华东 (上海/杭州)",
            Region::CnSouth => "中国华南 (深圳/广州)",
            Region::CnNorth => "中国华北 (北京/天津)",
            Region::Jp => "日本 (东京/大阪)",
            Region::Sg => "新加坡 / 东南亚",
            Region::UsEast => "美国东岸 (弗吉尼亚)",
            Region::UsWest => "美国西岸 (加州)",
            Region::Eu => "欧洲 (法兰克福/伦敦)",
            Region::Other => "其它",
            Region::Unknown => "未指定",
        }
    }

    /// kebab-case 字符串形式 (跟 serde 序列化一致). 用于线协议 wire string.
    pub fn as_kebab(self) -> &'static str {
        match self {
            Region::CnEast => "cn-east",
            Region::CnSouth => "cn-south",
            Region::CnNorth => "cn-north",
            Region::Jp => "jp",
            Region::Sg => "sg",
            Region::UsEast => "us-east",
            Region::UsWest => "us-west",
            Region::Eu => "eu",
            Region::Other => "other",
            Region::Unknown => "unknown",
        }
    }

    /// 从字符串反向匹配 (容错, 未识别串返回 Unknown).
    pub fn from_kebab(s: &str) -> Region {
        match s {
            "cn-east" => Region::CnEast,
            "cn-south" => Region::CnSouth,
            "cn-north" => Region::CnNorth,
            "jp" => Region::Jp,
            "sg" => Region::Sg,
            "us-east" => Region::UsEast,
            "us-west" => Region::UsWest,
            "eu" => Region::Eu,
            "other" => Region::Other,
            _ => Region::Unknown,
        }
    }

    /// 大厅过滤器: 房间是否应展示给 user_pref region 的用户.
    /// `Unknown` 用户偏好等同 "全部显示", 任何房间都通过.
    /// 否则: 房间 region 匹配, 或房间是 Unknown (兼容老房间).
    pub fn matches_filter(room_region: Region, user_pref: Region) -> bool {
        if matches!(user_pref, Region::Unknown) {
            return true;
        }
        room_region == user_pref || matches!(room_region, Region::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_round_trip() {
        for r in Region::all() {
            assert_eq!(Region::from_kebab(r.as_kebab()), *r);
        }
    }

    #[test]
    fn unknown_filter_shows_everything() {
        for r in Region::all() {
            assert!(Region::matches_filter(*r, Region::Unknown));
        }
    }

    #[test]
    fn specific_filter_keeps_matching_and_unknown() {
        assert!(Region::matches_filter(Region::CnEast, Region::CnEast));
        assert!(Region::matches_filter(Region::Unknown, Region::CnEast)); // 兼容老房间
        assert!(!Region::matches_filter(Region::Jp, Region::CnEast));
    }

    #[test]
    fn from_kebab_unknown_string_returns_unknown() {
        assert_eq!(Region::from_kebab("garbage"), Region::Unknown);
        assert_eq!(Region::from_kebab(""), Region::Unknown);
    }

    #[test]
    fn serde_kebab_wire_format() {
        // 验证 serde rename_all = kebab-case 跟 as_kebab() 一致.
        let r = Region::CnEast;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"cn-east\"");
        let r2: Region = serde_json::from_str(&json).unwrap();
        assert_eq!(r2, r);
    }

    /// 嵌入 toml table 验证 prefs.toml 可正常序列化反序列化.
    #[test]
    fn serde_via_toml_table() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrapper {
            region: Region,
        }
        let w = Wrapper { region: Region::Jp };
        let s = toml::to_string(&w).unwrap();
        assert!(s.contains("region = \"jp\""));
        let w2: Wrapper = toml::from_str(&s).unwrap();
        assert_eq!(w, w2);
    }
}
