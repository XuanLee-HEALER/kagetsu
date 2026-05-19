//! web-majo: 自部署 web 节点 (骨架).
//!
//! 当前为占位实现, 仅验证 workspace dependency 接通 + 起 tokio runtime.
//! 后续 milestone 接 axum + WebSocket gateway, browser 走 WS 连本地后端,
//! 跨节点对局复用 majo_core::net (libp2p).

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("web-majo skeleton — majo-core linked OK");
    // 触发对 majo_core 的引用, 验证 link 通畅.
    let _rules = majo_core::engine::rules::GameRules::default();
    Ok(())
}
