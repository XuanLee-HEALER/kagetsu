//! 软件级用户偏好 (跨房间 + 跨启动持久化).
//!
//! 注意区分: 这一层是 *软件设置* (主题/语言/键位/声音). 游戏规则参数
//! (kuitan/aka_dora/uma/...) 不在这里, 见 [`crate::engine::rules::GameRules`].

use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeKind;

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
        let path = Self::path()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无可用配置目录"))?;
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
        let s = "";
        let p: LocalPrefs = toml::from_str(s).unwrap();
        assert_eq!(p.theme, ThemeKind::default());
    }
}
