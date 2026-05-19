//! tui-majo: 终端日本麻将 TUI 客户端.
//!
//! 渲染层依赖 ratatui + crossterm. 域逻辑 / 网络层在 `majo_core`.
//!
//! 这里仅 re-export `ui` 供 `bin/game.rs` 用 `tui_majo::ui::App` 访问.
//! `bin/main.rs` (launcher) 不进 lib, 直接走外部进程模型起 `tui-majo-game`.

pub mod ui;
