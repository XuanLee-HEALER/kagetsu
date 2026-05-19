//! tui-majo-game: 实际游戏内核 binary.
//!
//! 一般通过 launcher (tui-majo) 启动. 单独跑也可: `cargo run --bin tui-majo-game`.

use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, SetSize, SetTitle, disable_raw_mode,
    enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;

fn main() -> Result<()> {
    init_tracing();

    // 显式构造 tokio runtime, 把 handle 传给 App.
    // (sync UI 主循环 + 后台异步 net 任务共存的最简方案.)
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // 改终端 title (终端不支持时 SetTitle 是 no-op, 不会失败).
    let _ = execute!(stdout, SetTitle("tui-majo · 终端日麻"));
    let _ = execute!(stdout, SetSize(144, 40));
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = tui_majo::ui::App::new(handle).run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // 显式 drop, 触发 graceful shutdown
    drop(runtime);
    result
}

/// 初始化 tracing.
///
/// 默认写文件 (临时目录 / tui-majo.log), 不打到 stderr 避免污染 ratatui TUI.
/// 默认过滤 `warn,tui_majo=info,libp2p=warn` — libp2p 各 crate 的 INFO 太冗余, 仅保留 warn+.
/// 用户可通过 `RUST_LOG` 环境变量覆盖, 例: `RUST_LOG=debug` 或 `RUST_LOG=tui_majo=trace,libp2p=info`.
/// 文件打不开时静默降级 (绝不 fallback 到 stderr).
fn init_tracing() {
    let log_path = std::env::temp_dir().join("tui-majo.log");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,tui_majo=info,libp2p=warn"));

    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .init();
        tracing::info!("tui-majo started, log = {}", log_path.display());
    }
    // 文件打不开 → 完全静默, 不走 stderr (会撞 TUI).
}
