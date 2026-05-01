//! tui-majo launcher.
//!
//! 在合适的终端 emulator 中开新窗口跑 `tui-majo-game`. 找不到合适终端则 fallback
//! 到当前终端 inline 运行.
//!
//! 目前支持:
//! - Windows: Windows Terminal (wt.exe)
//! - macOS:   iTerm2 优先, Terminal.app fallback
//! - 其他:    inline (向后兼容)
//!
//! 启动选项:
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

#[cfg(target_os = "windows")]
fn try_launch_external(game: &Path) -> Result<bool> {
    if !binary_in_path("wt.exe") {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    Command::new("wt.exe")
        .args([
            "--size",
            &format!("{},{}", DEFAULT_COLS, DEFAULT_ROWS),
            "--title",
            APP_TITLE,
            path_str,
        ])
        .spawn()
        .context("spawn wt.exe")?;
    Ok(true)
}

#[cfg(target_os = "macos")]
fn try_launch_external(game: &Path) -> Result<bool> {
    let path_str = game.to_str().context("game path utf8")?;
    let escaped = applescript_escape(path_str);

    if iterm2_installed() {
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
        return Ok(true);
    }

    if terminal_app_installed() {
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
        return Ok(true);
    }

    Ok(false)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn try_launch_external(_: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escape_basic() {
        assert_eq!(applescript_escape("simple"), "simple");
        assert_eq!(applescript_escape(r"path\with\back"), r"path\\with\\back");
        assert_eq!(applescript_escape(r#"a"b"#), r#"a\"b"#);
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn binary_in_path_returns_false_for_nonexistent() {
        assert!(!super::binary_in_path("definitely-not-a-binary-xyzzy"));
    }
}
