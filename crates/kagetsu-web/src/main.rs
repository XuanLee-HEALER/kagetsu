//! kagetsu-web: 自部署 web 节点.
//!
//! 当前阶段: 起 axum server, 静态 serve `static/` 目录下的设计原型
//! (SakyaHuman design system, 10 个 1440×900 artboard, React + Babel
//! inline 渲染). 浏览器打开 `http://localhost:8080/` 即可看完整设计稿.
//!
//! 后续 milestone:
//! - 接 WebSocket gateway: browser ↔ 本地 backend
//! - 把 React 原型转 svelte 组件 (见 README.md "Roadmap")
//! - 跨节点对局复用 `kagetsu_core::net` (libp2p)

use anyhow::{Context, Result};
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    // kagetsu-core link verification — 把它装进一个 _ binding 即可让 cargo
    // 知道我们真的用了这个 crate (后续接 WS gateway 时会真正调用).
    let _rules = kagetsu_core::engine::rules::GameRules::default();

    let static_dir = resolve_static_dir()?;
    info!(static_dir = %static_dir.display(), "serving static prototype");

    let app = Router::new()
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = std::env::var("WEB_MAJO_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("WEB_MAJO_ADDR 不是合法 SocketAddr (例: 127.0.0.1:8080)")?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr} 失败"))?;
    info!(%addr, "kagetsu-web listening on http://{}", addr);

    axum::serve(listener, app).await.context("axum::serve")?;
    Ok(())
}

/// 解析 static 目录位置:
/// 1. `WEB_MAJO_STATIC` env var 优先 (部署/容器场景)
/// 2. 否则取 `CARGO_MANIFEST_DIR/static` (开发时 `cargo run -p kagetsu-web` 即可)
fn resolve_static_dir() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("WEB_MAJO_STATIC") {
        let pb = PathBuf::from(p);
        if !pb.exists() {
            anyhow::bail!("WEB_MAJO_STATIC={} 不存在", pb.display());
        }
        return Ok(pb);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir.join("static");
    if !candidate.exists() {
        anyhow::bail!(
            "未找到 static 目录: {} (也未设 WEB_MAJO_STATIC env)",
            candidate.display()
        );
    }
    Ok(candidate)
}
