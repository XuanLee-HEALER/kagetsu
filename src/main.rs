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
/// 强制覆盖 exit_behavior=Close 让游戏退出后窗口自动关 (用户全局 config 可能设了 Hold).
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
            "--config",
            "exit_behavior='Close'",
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
/// 加 `window.position.{x,y}=0` 让窗口出现在屏幕左上.
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
            "-o",
            "window.position.x=0",
            "-o",
            "window.position.y=0",
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
    // append "; exit" 让 shell 在游戏退出后主动 exit, 触发 iTerm2 默认
    // profile "When the session ends: Close" 关闭窗口.
    let cmd = format!("{}; exit", path_str);
    let escaped = applescript_escape(&cmd);
    // try chain: 优先 newWindow 引用, 失败 fallback 到 front window. 某些
    // iTerm2 版本里 create window 返回的不是 window object, set position
    // 会触发 -10000 errAEEventNotHandled. 用 try 包起来 + diag string 报告
    // 实际是哪个分支生效, debug 模式下从 stdout 读到.
    // iTerm2 的 window 对象 *不响应* position (-10000), 只响应 bounds (NSRect).
    // 策略: 先读当前 bounds 算出宽高, 再 set bounds 保持宽高把 origin 拖到 (0, 0).
    // 用 str::replace 而非 format! 因为 AppleScript 含大量 {} 跟 format! 占位符冲突.
    let script = ITERM_SCRIPT_TEMPLATE
        .replace("__COLS__", &DEFAULT_COLS.to_string())
        .replace("__ROWS__", &DEFAULT_ROWS.to_string())
        .replace("__TITLE__", APP_TITLE)
        .replace("__PATH__", &escaped);
    run_osascript(&script, "iTerm2")
}

// iTerm2 (3.6.x) sdef 关键事实:
// - window class 支持 bounds (rectangle, QDRect = {left, top, right, bottom},
//   屏幕左上原点). position 已废弃, frame/origin 也已废弃.
// - application 级只暴露 `current window`, 不是 `front window` — 用 front window
//   会触发 -10000 errAEEventNotHandled.
// - create window with default profile 返回 window 类型, 但变量要在 tell
//   application 块内立即用; 保险起见我们用 current window 引用最稳.
#[cfg(target_os = "macos")]
const ITERM_SCRIPT_TEMPLATE: &str = r#"tell application "iTerm"
    activate
    set newWindow to (create window with default profile)
    set diag to "newWindow class=" & ((class of newWindow) as string)
    tell current session of newWindow
        set columns to __COLS__
        set rows to __ROWS__
        set name to "__TITLE__"
        write text "__PATH__"
    end tell
    try
        set b to bounds of current window
        set winW to (item 3 of b) - (item 1 of b)
        set winH to (item 4 of b) - (item 2 of b)
        set diag to diag & "; before bounds={" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b) & "} (w=" & winW & ",h=" & winH & ")"
        set bounds of current window to {0, 0, winW, winH}
        set b2 to bounds of current window
        set diag to diag & "; after bounds={" & (item 1 of b2) & "," & (item 2 of b2) & "," & (item 3 of b2) & "," & (item 4 of b2) & "}"
    on error errMsg number errNum
        set diag to diag & "; bounds set failed " & errNum & " " & errMsg
    end try
    return diag
end tell"#;

#[cfg(target_os = "macos")]
fn launch_terminal_app(game: &Path) -> Result<bool> {
    if !terminal_app_installed() {
        return Ok(false);
    }
    let path_str = game.to_str().context("game path utf8")?;
    // append "; exit" 让 shell 在游戏退出后主动 exit, 触发 Terminal.app
    // 默认 profile "When the shell exits: Close if the shell exited cleanly".
    let cmd = format!("{}; exit", path_str);
    let escaped = applescript_escape(&cmd);
    // Terminal.app 跟 iTerm 一致用 bounds. 用 str::replace 避免 format! 跟 {} 冲突.
    let script = TERMINAL_APP_SCRIPT_TEMPLATE
        .replace("__TITLE__", APP_TITLE)
        .replace("__PATH__", &escaped);
    run_osascript(&script, "Terminal.app")
}

#[cfg(target_os = "macos")]
const TERMINAL_APP_SCRIPT_TEMPLATE: &str = r#"tell application "Terminal"
    activate
    do script "__PATH__"
    set custom title of front window to "__TITLE__"
    set diag to "front window class=" & ((class of front window) as string)
    try
        set b to bounds of front window
        set winW to (item 3 of b) - (item 1 of b)
        set winH to (item 4 of b) - (item 2 of b)
        set diag to diag & "; before bounds={" & (item 1 of b) & "," & (item 2 of b) & "," & (item 3 of b) & "," & (item 4 of b) & "} (w=" & winW & ",h=" & winH & ")"
        set bounds of front window to {0, 0, winW, winH}
        set b2 to bounds of front window
        set diag to diag & "; after bounds={" & (item 1 of b2) & "," & (item 2 of b2) & "," & (item 3 of b2) & "," & (item 4 of b2) & "}"
    on error errMsg number errNum
        set diag to diag & "; bounds set failed " & errNum & " " & errMsg
    end try
    return diag
end tell"#;

/// 跑 osascript, 捕获 stderr. 失败时打到 launcher 终端让 user 能看到原因 +
/// 返 Ok(false) 让 caller fallback 到下一个 launcher.
/// TUI_MAJO_DEBUG=1 时打印 script 内容供 debug.
#[cfg(target_os = "macos")]
fn run_osascript(script: &str, label: &str) -> Result<bool> {
    let debug = std::env::var("TUI_MAJO_DEBUG").is_ok();
    if debug {
        eprintln!("[launcher/{}] osascript script:\n{}", label, script);
    }
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .with_context(|| format!("spawn osascript ({})", label))?;
    if !output.status.success() {
        eprintln!(
            "[launcher/{}] osascript failed (status {}):\n{}",
            label,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(false);
    }
    if debug {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            eprintln!("[launcher/{}] stdout: {}", label, stdout.trim());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("[launcher/{}] stderr (status 0): {}", label, stderr.trim());
        }
    }
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
            "--pos",
            "0,0",
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
