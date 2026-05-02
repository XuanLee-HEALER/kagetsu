//! 游戏规则配置.
//!
//! [`GameConfig`] 是规则相关 (绑房间, 多人模式下房主控). [`LocalPrefs`] 是
//! client 本地偏好 (主题等), 不绑房间, 每人独立.
//!
//! 默认采用 WRC 2022 主基, 古役默认关闭(用户可开启).
//! 详见 docs/spec/README.md

use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiRonRule {
    /// 头跳: 仅最近一家可和.
    Atamahane,
    /// 双家荣和.
    DoubleRon,
    /// 三家荣和.
    TripleRon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthRule {
    /// 半庄(东+南).
    Hanchan,
    /// 东风战.
    Tonpuusen,
}

/// 房间共享的规则配置 (多人模式下房主控制, 单机时玩家自己改).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// 食断(鸣牌后断幺九成立).
    pub kuitan: bool,
    /// 赤宝牌(各花色五各 1 张赤).
    pub aka_dora: bool,
    /// 一发.
    pub ippatsu: bool,
    /// 里 dora.
    pub ura_dora: bool,
    /// 13+ 番视为役满.
    pub kazoe_yakuman: bool,
    /// 双倍役满(国士13面/纯九莲/四暗刻单骑/大四喜).
    pub double_yakuman: bool,
    /// 多家荣和规则.
    pub multi_ron: MultiRonRule,
    /// 半庄/东风.
    pub length: LengthRule,
    /// 西入(南 4 < 30000 → 进入西场).
    pub west_round: bool,
    /// 击飞(任一家 < 0 强制结束).
    pub minus_score_end: bool,
    /// 古役总开关.
    pub kotekisai: bool,
    /// 古役细分(仅在 kotekisai = true 下生效).
    pub kotekisai_renhou: bool,
    pub kotekisai_sanrenkou: bool,
    pub kotekisai_surenkou: bool,
    pub kotekisai_daisharin: bool,
    pub kotekisai_daichisei: bool,
    pub kotekisai_parenchan: bool,
    pub kotekisai_shisanputaa: bool,
    /// 起始点棒.
    pub starting_score: i32,
    /// 目标点棒(用于 oka).
    pub target_score: i32,
    /// uma (顺位奖罚).
    pub uma: [i32; 4],
    /// 玩家单步思考时长(秒). None = 不限时.
    pub thinking_time_secs: Option<u32>,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            kuitan: true,
            aka_dora: true,
            ippatsu: true,
            ura_dora: true,
            kazoe_yakuman: true,
            double_yakuman: true,
            multi_ron: MultiRonRule::Atamahane,
            length: LengthRule::Hanchan,
            west_round: true,
            minus_score_end: false,
            kotekisai: false,
            kotekisai_renhou: false,
            kotekisai_sanrenkou: false,
            kotekisai_surenkou: false,
            kotekisai_daisharin: false,
            kotekisai_daichisei: false,
            kotekisai_parenchan: false,
            kotekisai_shisanputaa: false,
            starting_score: 25_000,
            target_score: 30_000,
            uma: [15, 5, -5, -15],
            thinking_time_secs: Some(30),
        }
    }
}

/// 本地用户偏好 (跨房间 + 持久化到磁盘).
///
/// 路径: 平台标准目录 `tui-majo/prefs.toml`
/// - Windows: `%APPDATA%\tui-majo\prefs.toml`
/// - macOS:   `~/Library/Application Support/tui-majo/prefs.toml`
/// - Linux:   `~/.config/tui-majo/prefs.toml`
///
/// 加载失败 (路径错 / 解析错 / 文件不存在) 一律返回默认值, 不崩溃.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalPrefs {
    pub theme: ThemeKind,
    // 占位扩展点 (未来加字段时, 旧 prefs.toml 因 #[serde(default)] 仍可解析):
    // pub locale: Locale,        // zh-CN / en-US / ja-JP ...
    // pub keymap: KeymapPreset,   // default / vim ...
    // pub sound: SoundPrefs,
}

impl LocalPrefs {
    /// 从磁盘加载. 失败时返回 default 并写 warn log, 永不崩溃.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<Self>(&s) {
                Ok(p) => {
                    tracing::info!("loaded prefs from {}", path.display());
                    p
                }
                Err(e) => {
                    tracing::warn!("解析 prefs.toml 失败 ({}): {e}, 用默认值", path.display());
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!("读取 prefs.toml 失败 ({}): {e}, 用默认值", path.display());
                Self::default()
            }
        }
    }

    /// 保存到磁盘 (含 mkdir -p 父目录).
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "无可用配置目录")
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&path, s)
    }

    /// 平台标准 prefs.toml 路径. 极少数无家目录环境返回 None.
    pub fn path() -> Option<std::path::PathBuf> {
        let mut dir = dirs::config_dir()?;
        dir.push("tui-majo");
        dir.push("prefs.toml");
        Some(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_prefs_round_trip_toml() {
        let p = LocalPrefs {
            theme: ThemeKind::Light,
        };
        let s = toml::to_string_pretty(&p).unwrap();
        let back: LocalPrefs = toml::from_str(&s).unwrap();
        assert_eq!(back.theme, ThemeKind::Light);
    }

    #[test]
    fn local_prefs_missing_field_uses_default() {
        // 模拟旧 prefs.toml 缺少新字段
        let s = "";
        let p: LocalPrefs = toml::from_str(s).unwrap();
        assert_eq!(p.theme, ThemeKind::default());
    }
}
