//! tui-majo launcher.
//!
//! 在合适的终端 emulator 中开新窗口跑 `tui-majo-game`. 找不到合适终端则 fallback
//! 到当前终端 inline 运行.
//!
//! ## 检测策略
//!
//! 1. **TERM_PROGRAM 提示**: 用户当前已经在某个终端里跑 launcher 时, 优先开同款.
//!    - WezTerm / kitty / iTerm.app / Apple_Terminal 这几个会 set 这个变量.
//!    - Alacritty / GNOME Terminal / konsole 不 set, 走下一步.
//! 2. **跨平台 modern 三巨头**: WezTerm → kitty → Alacritty (按命令是否在 PATH).
//! 3. **平台原生**: macOS 走 iTerm2 → Terminal.app; Windows 走 Windows Terminal.
//! 4. **Inline fallback**: 都没有就在当前终端跑.
//!
//! ## 启动选项
//!
//! - `--inline`: 强制 inline 跑 (跳过 launcher, 在当前终端).

use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_COLS: u16 = 144;
const DEFAULT_ROWS: u16 = 40;
const APP_TITLE: &str = "tui-majo";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let inline = args.iter().any(|a| a == "--inline");

    let game = locate_game_binary()?;

    if inline {
        return run_inline(&game);
    }

    match try_launch_external(&game) {
        Ok(true) => Ok(()),
        Ok(false) => run_inline(&game),
        Err(e) => {
            eprintln!("[tui-majo] 外部终端启动失败 ({e}), fallback inline.");
            run_inline(&game)
        }
    }
}

fn locate_game_binary() -> Result<PathBuf> {
    let exe = env::current_exe().context("找不到自身可执行路径")?;
    let dir = exe.parent().context("可执行路径无父目录")?;
    let bin_name = if cfg!(windows) {
        "tui-majo-game.exe"
    } else {
        "tui-majo-game"
    };
    let path = dir.join(bin_name);
    if !path.exists() {
        bail!(
            "找不到游戏内核 binary: {}\n请确认已构建 tui-majo-game (cargo build --bin tui-majo-game)",
            path.display()
        );
    }
    Ok(path)
}

fn run_inline(game: &Path) -> Result<()> {
    let status = Command::new(game)
        .status()
        .with_context(|| format!("运行 {} 失败", game.display()))?;
    if !status.success() {
        bail!("game 退出非零: {:?}", status.code());
    }
    Ok(())
}

/// 总调度: 按策略试一连串终端, 第一个成功的 spawn 即返回 Ok(true).
/// 任何 launcher 拿到 Err 直接冒泡 (不掩盖); 拿到 Ok(false) 表示该 launcher
/// 不适用 (binary 不在 / app bundle 不存在), 继续试下一个.
fn try_launch_external(game: &Path) -> Result<bool> {
    // 1. TERM_PROGRAM 提示: 用户当前在啥终端 launcher 就开啥.
    if let Ok(hint) = env::var("TERM_PROGRAM") {
        let r = match hint.as_str() {
            "WezTerm" => launch_wezterm(game),
            "kitty" => launch_kitty(game),
            #[cfg(target_os = "macos")]
            "iTerm.app" => launch_iterm2(game),
            #[cfg(target_os = "macos")]
            "Apple_Terminal" => launch_terminal_app(game),
            _ => Ok(false),
        };
        if matches!(r, Ok(true)) {
            return Ok(true);
        }
    }

    // 2. 跨平台 modern 三巨头 (用户选择 modern-first).
    if launch_wezterm(game)? {
        return Ok(true);
    }
    if launch_kitty(game)? {
        return Ok(true);
    }
    if launch_alacritty(game)? {
        return Ok(true);
    }

    // 3. 平台原生.
    #[cfg(target_os = "macos")]
    {
        if launch_iterm2(game)? {
            return Ok(true);
        }
        if launch_terminal_app(game)? {
            return Ok(true);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if launch_windows_terminal(game)? {
            return Ok(true);
        }
    }

    Ok(false)
}

// ============================================================
// 跨平台 modern 三巨头
// ============================================================

/// WezTerm: 命令 `wezterm start --config initial_cols=N --config initial_rows=M -- <cmd>`.
/// 跨 mac / linux / windows.
fn launch_wezterm(game: &Path) -> Result<bool> {
    if !binary_in_path(wezterm_bin_name()) {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    Command::new(wezterm_bin_name())
        .args([
            "start",
            "--config",
            &format!("initial_cols={DEFAULT_COLS}"),
            "--config",
            &format!("initial_rows={DEFAULT_ROWS}"),
            "--",
            path_str,
        ])
        .spawn()
        .context("spawn wezterm")?;
    Ok(true)
}

/// kitty: `kitty -o initial_window_width=Nc -o initial_window_height=Mc --title <t> <cmd>`.
/// `Nc` 后缀表示按列/行数计算 (不是像素).
fn launch_kitty(game: &Path) -> Result<bool> {
    if !binary_in_path(kitty_bin_name()) {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    Command::new(kitty_bin_name())
        .args([
            "-o",
            &format!("initial_window_width={DEFAULT_COLS}c"),
            "-o",
            &format!("initial_window_height={DEFAULT_ROWS}c"),
            "--title",
            APP_TITLE,
            path_str,
        ])
        .spawn()
        .context("spawn kitty")?;
    Ok(true)
}

/// Alacritty: `alacritty -o window.dimensions.{columns,lines}=N --title <t> -e <cmd>`.
fn launch_alacritty(game: &Path) -> Result<bool> {
    if !binary_in_path(alacritty_bin_name()) {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    Command::new(alacritty_bin_name())
        .args([
            "-o",
            &format!("window.dimensions.columns={DEFAULT_COLS}"),
            "-o",
            &format!("window.dimensions.lines={DEFAULT_ROWS}"),
            "--title",
            APP_TITLE,
            "-e",
            path_str,
        ])
        .spawn()
        .context("spawn alacritty")?;
    Ok(true)
}

// ============================================================
// 平台原生
// ============================================================

#[cfg(target_os = "macos")]
fn launch_iterm2(game: &Path) -> Result<bool> {
    if !iterm2_installed() {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    let escaped = applescript_escape(path_str);
    let script = format!(
        r#"tell application "iTerm"
    activate
    set newWindow to (create window with default profile)
    tell current session of newWindow
        set columns to {cols}
        set rows to {rows}
        set name to "{title}"
        write text "{path}"
    end tell
end tell"#,
        cols = DEFAULT_COLS,
        rows = DEFAULT_ROWS,
        title = APP_TITLE,
        path = escaped,
    );
    Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .context("spawn osascript (iTerm2)")?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn launch_terminal_app(game: &Path) -> Result<bool> {
    if !terminal_app_installed() {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    let escaped = applescript_escape(path_str);
    let script = format!(
        r#"tell application "Terminal"
    activate
    do script "{path}"
    set custom title of front window to "{title}"
end tell"#,
        path = escaped,
        title = APP_TITLE,
    );
    Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .context("spawn osascript (Terminal.app)")?;
    Ok(true)
}

#[cfg(target_os = "windows")]
fn launch_windows_terminal(game: &Path) -> Result<bool> {
    if !binary_in_path("wt.exe") {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    Command::new("wt.exe")
        .args([
            "--size",
            &format!("{DEFAULT_COLS},{DEFAULT_ROWS}"),
            "--title",
            APP_TITLE,
            path_str,
        ])
        .spawn()
        .context("spawn wt.exe")?;
    Ok(true)
}

// ============================================================
// 平台无关 helpers
// ============================================================

fn wezterm_bin_name() -> &'static str {
    if cfg!(windows) { "wezterm.exe" } else { "wezterm" }
}

fn kitty_bin_name() -> &'static str {
    if cfg!(windows) { "kitty.exe" } else { "kitty" }
}

fn alacritty_bin_name() -> &'static str {
    if cfg!(windows) {
        "alacritty.exe"
    } else {
        "alacritty"
    }
}

fn binary_in_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|p| env::split_paths(&p).any(|d| d.join(name).exists()))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn iterm2_installed() -> bool {
    if Path::new("/Applications/iTerm.app").exists() {
        return true;
    }
    env::var_os("HOME")
        .map(|h| Path::new(&h).join("Applications/iTerm.app").exists())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn terminal_app_installed() -> bool {
    Path::new("/System/Applications/Utilities/Terminal.app").exists()
        || Path::new("/Applications/Utilities/Terminal.app").exists()
}

/// 转义 AppleScript 字符串里的 `\` 和 `"`.
#[cfg(target_os = "macos")]
fn applescript_escape(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', r#"\""#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escape_basic() {
        assert_eq!(applescript_escape("simple"), "simple");
        assert_eq!(applescript_escape(r"path\with\back"), r"path\\with\\back");
        assert_eq!(applescript_escape(r#"a"b"#), r#"a\"b"#);
    }

    #[test]
    fn binary_in_path_returns_false_for_nonexistent() {
        assert!(!binary_in_path("definitely-not-a-binary-xyzzy"));
    }

    #[test]
    fn bin_names_have_correct_suffix() {
        if cfg!(windows) {
            assert_eq!(wezterm_bin_name(), "wezterm.exe");
            assert_eq!(kitty_bin_name(), "kitty.exe");
            assert_eq!(alacritty_bin_name(), "alacritty.exe");
        } else {
            assert_eq!(wezterm_bin_name(), "wezterm");
            assert_eq!(kitty_bin_name(), "kitty");
            assert_eq!(alacritty_bin_name(), "alacritty");
        }
    }
}
