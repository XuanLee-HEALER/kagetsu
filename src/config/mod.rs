//! 软件级用户偏好 (跨房间 + 跨启动持久化).
//!
//! 注意区分: 这一层是 *软件设置* (主题/语言/键位/声音). 游戏规则参数
//! (kuitan/aka_dora/uma/...) 不在这里, 见 [`crate::engine::rules::GameRules`].
//!
//! ## 启动加载策略 ([`LocalPrefs::load`])
//!
//! 启动时调一次, 返回 [`LoadResult`] 含 [`PrefsLoadStatus`] 让 UI 显示一次性 banner.
//! 5 种状态:
//!
//! | 状态 | 触发条件 | 副作用 |
//! |---|---|---|
//! | `Ok` | 文件存在 + 解析成功 + 字段完全匹配当前 schema | 不动 |
//! | `Default` | 文件不存在 (新用户首次启动) | 创建文件 + 写 default |
//! | `Migrated` | 解析成功但有缺/多字段 | atomic rewrite 规范化, 提示用户 |
//! | `Corrupted` | 解析失败 / 文件过大 (>1 MB) | 备份到 `.bak` + atomic rewrite default, 提示用户 |
//! | `Inaccessible` | 路径不可用 / 读权限错 | 内存 default, 不持久化 |

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeKind;

/// 防御: prefs.toml 不该超过 1 MB. 超出视为损坏 (避免恶意大文件).
const MAX_PREFS_SIZE: u64 = 1_048_576;

/// 本地用户偏好 (跨房间 + 持久化到磁盘).
///
/// 路径: 平台标准目录 `tui-majo/prefs.toml`
/// - Windows: `%APPDATA%\tui-majo\prefs.toml`
/// - macOS:   `~/Library/Application Support/tui-majo/prefs.toml`
/// - Linux:   `~/.config/tui-majo/prefs.toml`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LocalPrefs {
    pub theme: ThemeKind,
    // 占位扩展点 (未来加字段时, 旧 prefs.toml 因 #[serde(default)] 仍可解析):
    // pub locale: Locale,        // zh-CN / en-US / ja-JP ...
    // pub keymap: KeymapPreset,   // default / vim ...
    // pub sound: SoundPrefs,
}

/// 启动加载结果.
#[derive(Debug)]
pub struct LoadResult {
    pub prefs: LocalPrefs,
    pub status: PrefsLoadStatus,
}

/// 启动时 prefs 文件的诊断状态.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefsLoadStatus {
    /// 文件存在且 schema 完整匹配, 不动.
    Ok,
    /// 文件不存在 (新用户首次启动), 已创建并写默认值.
    Default,
    /// 解析成功但字段集与当前 schema 不一致 (缺/多字段),
    /// 已 atomic 写规范化版本.
    Migrated,
    /// 解析失败 / 文件过大. 原文件已备份到 `.bak`, 用 default 覆盖.
    Corrupted { reason: String },
    /// 配置目录不可用 (无家目录 / 权限错). 内存中用默认值, 无法持久化.
    Inaccessible { reason: String },
}

impl PrefsLoadStatus {
    /// 是否需要在主菜单显示一次性 banner. Ok / Default 不显示.
    pub fn user_visible_banner(&self) -> Option<String> {
        match self {
            PrefsLoadStatus::Ok | PrefsLoadStatus::Default => None,
            PrefsLoadStatus::Migrated => {
                Some("配置文件 schema 已升级到当前版本 (旧字段保留, 缺失字段补齐)".into())
            }
            PrefsLoadStatus::Corrupted { reason } => Some(format!(
                "配置文件已损坏 ({}), 已备份原文件到 prefs.toml.bak 并重置",
                short_reason(reason)
            )),
            PrefsLoadStatus::Inaccessible { reason } => Some(format!(
                "配置目录不可访问 ({}), 本次会话用默认值",
                short_reason(reason)
            )),
        }
    }
}

fn short_reason(s: &str) -> String {
    // 防止过长 reason 撑爆 banner.
    const MAX_LEN: usize = 80;
    if s.chars().count() <= MAX_LEN {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(MAX_LEN - 3).collect();
        out.push_str("...");
        out
    }
}

impl LocalPrefs {
    /// 启动时调一次, 返回 [`LoadResult`].
    /// 永不 panic, 永不返回 Err: 任何意外都映射到一个 [`PrefsLoadStatus`] 变体.
    pub fn load() -> LoadResult {
        let Some(path) = Self::path() else {
            return LoadResult {
                prefs: Self::default(),
                status: PrefsLoadStatus::Inaccessible {
                    reason: "无可用配置目录 (dirs::config_dir 返回 None)".into(),
                },
            };
        };

        // 清理上次 atomic_write 残留的 .tmp (上次写中途崩溃).
        let tmp_path = path.with_extension("toml.tmp");
        if tmp_path.exists() {
            let _ = std::fs::remove_file(&tmp_path);
        }

        // 文件状态分流.
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 新用户: 写一份默认.
                let prefs = Self::default();
                if let Err(save_err) = prefs.save() {
                    tracing::warn!("初次创建 prefs 失败: {save_err}");
                    return LoadResult {
                        prefs,
                        status: PrefsLoadStatus::Inaccessible {
                            reason: save_err.to_string(),
                        },
                    };
                }
                tracing::info!("首次创建 prefs at {}", path.display());
                return LoadResult {
                    prefs,
                    status: PrefsLoadStatus::Default,
                };
            }
            Err(e) => {
                tracing::warn!("无法访问 prefs ({}): {e}", path.display());
                return LoadResult {
                    prefs: Self::default(),
                    status: PrefsLoadStatus::Inaccessible {
                        reason: e.to_string(),
                    },
                };
            }
        };

        if meta.len() > MAX_PREFS_SIZE {
            return Self::recover_from_corruption(
                &path,
                format!("文件过大 {} bytes (上限 {})", meta.len(), MAX_PREFS_SIZE),
            );
        }

        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                return LoadResult {
                    prefs: Self::default(),
                    status: PrefsLoadStatus::Inaccessible {
                        reason: e.to_string(),
                    },
                };
            }
        };

        let parsed = match toml::from_str::<Self>(&raw) {
            Ok(p) => p,
            Err(e) => {
                return Self::recover_from_corruption(&path, format!("toml 解析: {e}"));
            }
        };

        // schema 一致性检查: 比较 raw / normalized 的顶层字段集.
        // BTreeSet 比较与字段顺序无关.
        let known_keys: BTreeSet<String> = toml::to_string_pretty(&parsed)
            .ok()
            .as_deref()
            .and_then(top_keys)
            .unwrap_or_default();
        let raw_keys = top_keys(&raw).unwrap_or_default();

        if known_keys != raw_keys {
            // 缺字段 (serde default 补) 或 多余未知字段 (serde 忽略). 规范化.
            tracing::info!(
                "prefs schema migration: raw_keys={:?}, expected={:?}",
                raw_keys,
                known_keys
            );
            if let Err(e) = parsed.save() {
                tracing::warn!("规范化 prefs 写入失败: {e}");
                // 写失败不致命 — 仍返回正确的 in-memory parsed.
            }
            return LoadResult {
                prefs: parsed,
                status: PrefsLoadStatus::Migrated,
            };
        }

        tracing::debug!("prefs loaded ok from {}", path.display());
        LoadResult {
            prefs: parsed,
            status: PrefsLoadStatus::Ok,
        }
    }

    fn recover_from_corruption(path: &Path, reason: String) -> LoadResult {
        let bak = path.with_extension("toml.bak");
        match std::fs::rename(path, &bak) {
            Ok(_) => tracing::warn!("损坏的 prefs 已备份到 {} (原因: {reason})", bak.display()),
            Err(e) => tracing::warn!(
                "备份损坏 prefs 到 {} 失败: {e} (原 reason: {reason})",
                bak.display()
            ),
        }

        let prefs = Self::default();
        if let Err(e) = prefs.save() {
            tracing::warn!("覆盖损坏 prefs 写默认值失败: {e}");
            return LoadResult {
                prefs,
                status: PrefsLoadStatus::Inaccessible {
                    reason: format!("{reason} (重置写入也失败: {e})"),
                },
            };
        }

        LoadResult {
            prefs,
            status: PrefsLoadStatus::Corrupted { reason },
        }
    }

    /// 保存到磁盘. atomic write (写到 .tmp 再 rename), 减少程序崩溃中途留半截文件的风险.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无可用配置目录"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        atomic_write(&path, &s)
    }

    /// 平台标准 prefs.toml 路径. 极少数无家目录环境返回 None.
    pub fn path() -> Option<std::path::PathBuf> {
        let mut dir = dirs::config_dir()?;
        dir.push("tui-majo");
        dir.push("prefs.toml");
        Some(dir)
    }
}

/// 提取 toml 文本顶层 key 集合. 解析失败返回 None.
fn top_keys(s: &str) -> Option<BTreeSet<String>> {
    let v: toml::Value = toml::from_str(s).ok()?;
    let table = v.as_table()?;
    Some(table.keys().cloned().collect())
}

/// 原子写: 写到 `<path>.tmp` 再 rename. 减少程序崩溃中途留半截文件.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, content)?;
    // rename 在 Windows 上原子性有限制 (目标存在时 fail), 用 fs::rename 但若失败 fallback 到 write.
    match std::fs::rename(&tmp, path) {
        Ok(_) => Ok(()),
        Err(_) => {
            // Windows: 目标存在 rename 失败. 移除目标后再 rename.
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path)
        }
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

    #[test]
    fn top_keys_ignores_order_and_extracts_set() {
        let a = top_keys("theme = \"Dark\"\nfuture = 42").unwrap();
        let b = top_keys("future = 42\ntheme = \"Light\"").unwrap();
        assert_eq!(a, b);
        assert!(a.contains("theme"));
        assert!(a.contains("future"));
    }

    #[test]
    fn top_keys_returns_none_for_invalid_toml() {
        assert!(top_keys("not toml ===").is_none());
    }

    #[test]
    fn user_visible_banner_for_each_status() {
        assert!(PrefsLoadStatus::Ok.user_visible_banner().is_none());
        assert!(PrefsLoadStatus::Default.user_visible_banner().is_none());
        assert!(PrefsLoadStatus::Migrated.user_visible_banner().is_some());
        assert!(
            PrefsLoadStatus::Corrupted { reason: "x".into() }
                .user_visible_banner()
                .is_some()
        );
        assert!(
            PrefsLoadStatus::Inaccessible { reason: "x".into() }
                .user_visible_banner()
                .is_some()
        );
    }

    #[test]
    fn short_reason_truncates_long_string() {
        let long = "x".repeat(200);
        let s = short_reason(&long);
        assert!(s.chars().count() <= 80);
        assert!(s.ends_with("..."));
    }
}
