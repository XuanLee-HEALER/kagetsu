//! 牌谱回归测试 (B 层).
//!
//! 各 phase 实施进度:
//! - P1 GameState/Wall 注入 API ✓ (in lib)
//! - P2 mjai pai 双向映射 ✓
//! - P3 mjai NDJSON parser (TODO)
//! - P4 ReplayLog IR (TODO)
//! - P5 ReplayDriver (TODO)
//! - P6 第一个 fixture 跑通 (TODO)
//! - P7 30-50 公开 sample 集 (TODO)

mod common;
#[path = "replay/mod.rs"]
mod replay;

// P2 mjai_pai 模块的测试在 `replay/mjai_pai.rs` 内, 通过 `cargo test mjai_pai`
// 自动跑. 这里留作主入口.
