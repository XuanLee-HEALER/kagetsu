//! GameState savestate (dev-only).
//!
//! F5 = quick save → `savestates/quick.json`,
//! F9 = quick load ← 同文件.
//! 路径: `dirs::config_dir() / tui-majo / savestates / <slot>.json`.

use crate::engine::state::GameState;
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

/// 写一份 GameState 到 `<slot>.json`, 返回最终路径.
pub fn save(state: &GameState, slot: &str) -> std::io::Result<PathBuf> {
    let path = slot_path(slot)?;
    let s = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(&path, s)?;
    Ok(path)
}

/// 从 `<slot>.json` 读 GameState.
pub fn load(slot: &str) -> std::io::Result<GameState> {
    let path = slot_path(slot)?;
    let s = std::fs::read_to_string(&path)?;
    serde_json::from_str(&s).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rules::GameRules;

    #[test]
    fn roundtrip_initial_state() {
        let mut g = GameState::new(GameRules::default());
        g.start_round(42);
        // 用临时 slot 名避免冲突.
        let slot = format!("__test_{}", std::process::id());
        save(&g, &slot).unwrap();
        let g2 = load(&slot).unwrap();
        assert_eq!(g.round_seed, g2.round_seed);
        assert_eq!(g.kyoku, g2.kyoku);
        assert_eq!(g.players[0].hand.closed.len(), g2.players[0].hand.closed.len());
        // 清理.
        let _ = std::fs::remove_file(slot_path(&slot).unwrap());
    }
}
