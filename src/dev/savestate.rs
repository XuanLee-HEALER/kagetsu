//! Savestate (dev-only).
//!
//! F5 = quick save → `savestates/quick.json`,
//! F9 = quick load ← 同文件.
//! 路径: `dirs::config_dir() / tui-majo / savestates / <slot>.json`.
//!
//! 泛型实现, 不绑定具体 state 类型: caller (UI 层) 决定要序列化什么 — 现在是
//! GameEngine, 早期是 GameState. 只要实现 Serialize / DeserializeOwned 即可.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::PathBuf;

/// savestates 目录, 不存在时创建.
pub fn savestates_dir() -> std::io::Result<PathBuf> {
    let mut dir = dirs::config_dir().ok_or_else(|| {
        std::io::Error::other("无可用配置目录 (dirs::config_dir 返回 None)")
    })?;
    dir.push("tui-majo");
    dir.push("savestates");
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn slot_path(slot: &str) -> std::io::Result<PathBuf> {
    let mut p = savestates_dir()?;
    p.push(format!("{}.json", slot));
    Ok(p)
}

/// 写一份 state 到 `<slot>.json`, 返回最终路径.
pub fn save<T: Serialize>(state: &T, slot: &str) -> std::io::Result<PathBuf> {
    let path = slot_path(slot)?;
    let s = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(&path, s)?;
    Ok(path)
}

/// 从 `<slot>.json` 读 state.
pub fn load<T: DeserializeOwned>(slot: &str) -> std::io::Result<T> {
    let path = slot_path(slot)?;
    let s = std::fs::read_to_string(&path)?;
    serde_json::from_str(&s).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct DummyState {
        seed: u64,
        kyoku: u8,
    }

    #[test]
    fn roundtrip_dummy_state() {
        let g = DummyState {
            seed: 42,
            kyoku: 3,
        };
        let slot = format!("__test_{}", std::process::id());
        save(&g, &slot).unwrap();
        let g2: DummyState = load(&slot).unwrap();
        assert_eq!(g, g2);
        let _ = std::fs::remove_file(slot_path(&slot).unwrap());
    }
}
