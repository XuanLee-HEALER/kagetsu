//! 真实 mjlog fixture → replay → 报告 diff.
//!
//! 流程: 读 .mjlog 文件 → tenhou_parser → tenhou_to_mjai → mjai_parser
//! → build_replay_log → ReplayDriver.replay → 收集 ReplayDiff.
//!
//! ## 三档输出
//!
//! - **Pass**: driver.replay() 返回空 diffs (本局事件流 + 结算完全一致)
//! - **EventFailed**: 事件流跑不通 (e.g. 解码错的 m, 缺役实现)
//! - **ResultMismatch**: 事件流通过但结算不一致 (fu/han/yaku 算法 bug)

use std::path::Path;

use tui_majo::config::GameConfig;

use super::driver::{ReplayDiff, ReplayDriver};
use super::replay_log::build_replay_log;
use super::tenhou_parser::parse_mjlog;
use super::tenhou_to_mjai::tenhou_to_mjai;

#[derive(Debug)]
pub struct FixtureResult {
    pub fixture: String,
    pub kyoku_results: Vec<KyokuOutcome>,
}

#[derive(Debug)]
pub enum KyokuOutcome {
    Pass,
    Diffs(Vec<ReplayDiff>),
    /// 转换 / 解析期失败 (轻量)
    PreReplayError(String),
}

/// 跑一个 mjlog fixture, 返回每局结果.
pub fn run_fixture(path: impl AsRef<Path>) -> Result<FixtureResult, String> {
    let path = path.as_ref();
    let xml = std::fs::read_to_string(path).map_err(|e| format!("读 {}: {e}", path.display()))?;
    let tenhou_events = parse_mjlog(&xml).map_err(|e| format!("parse_mjlog: {e}"))?;
    let mjai_events = tenhou_to_mjai(&tenhou_events).map_err(|e| format!("tenhou_to_mjai: {e}"))?;
    let replay_log = build_replay_log(mjai_events).map_err(|e| format!("build_replay_log: {e}"))?;
    let cfg = tenhou_default_config();

    let mut kyoku_results = Vec::new();
    for k in &replay_log.kyokus {
        let driver = ReplayDriver::new(k, &cfg);
        let diffs = driver.replay();
        if diffs.is_empty() {
            kyoku_results.push(KyokuOutcome::Pass);
        } else {
            kyoku_results.push(KyokuOutcome::Diffs(diffs));
        }
    }
    Ok(FixtureResult {
        fixture: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .into(),
        kyoku_results,
    })
}

/// 天凤凤凰房默认配置 (按 mjx-project tests_py 假设): 半庄, kuitan, ippatsu, ura, aka.
pub fn tenhou_default_config() -> GameConfig {
    GameConfig {
        kuitan: true,
        aka_dora: true,
        ippatsu: true,
        ura_dora: true,
        ..GameConfig::default()
    }
}

/// 统计 fixture 结果 — pass / diff 计数.
pub fn summarize(result: &FixtureResult) -> (usize, usize) {
    let mut pass = 0;
    let mut diff = 0;
    for o in &result.kyoku_results {
        match o {
            KyokuOutcome::Pass => pass += 1,
            _ => diff += 1,
        }
    }
    (pass, diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跑 fixtures/ 下所有 mjlog, 验证 parser / converter 不崩溃.
    /// 不要求 driver.replay 全过 (yaku 实现可能有缺漏).
    #[test]
    fn all_fixtures_parse_and_convert() {
        let dir = std::path::Path::new("tests/replay/fixtures");
        if !dir.exists() {
            eprintln!("[skip] fixtures 目录不存在");
            return;
        }
        let mut failed = Vec::new();
        let mut ok_count = 0;
        for entry in std::fs::read_dir(dir).expect("read fixtures dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mjlog") {
                continue;
            }
            match run_fixture(&path) {
                Ok(_) => ok_count += 1,
                Err(e) => failed.push((path.display().to_string(), e)),
            }
        }
        assert!(
            failed.is_empty(),
            "{} fixtures 解析/转换失败:\n{}",
            failed.len(),
            failed
                .iter()
                .map(|(p, e)| format!("  - {p}: {e}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(ok_count > 0, "没找到 fixture");
    }

    /// 报告 driver.replay 在所有 fixture 上的 diff 数 (用 eprintln 输出概览).
    /// 不 fail 测试 — 这是 phase 8 起步, yaku 实现可能有缺漏.
    #[test]
    #[ignore = "用 cargo test ... --ignored 显式跑, 输出 diff 概览"]
    fn replay_all_fixtures_report() {
        let dir = std::path::Path::new("tests/replay/fixtures");
        if !dir.exists() {
            return;
        }
        let mut total_pass = 0;
        let mut total_diff = 0;
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mjlog") {
                continue;
            }
            match run_fixture(&path) {
                Ok(r) => {
                    let (p, d) = summarize(&r);
                    total_pass += p;
                    total_diff += d;
                    eprintln!("[fixture] {:48} pass={:3} diff={:3}", r.fixture, p, d);
                }
                Err(e) => eprintln!("[fixture FAIL] {}: {e}", path.display()),
            }
        }
        eprintln!("\n=== 总计 ===\n  pass: {total_pass} 局\n  diff: {total_diff} 局");
    }

    /// 按 diff reason 分组统计, 找最常见的 bug 模式优先修.
    #[test]
    #[ignore = "用 cargo test ... --ignored 显式跑"]
    fn diff_reasons_breakdown() {
        let dir = std::path::Path::new("tests/replay/fixtures");
        if !dir.exists() {
            return;
        }
        let mut reason_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut event_failed = 0;
        for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mjlog") {
                continue;
            }
            let Ok(r) = run_fixture(&path) else { continue };
            for o in &r.kyoku_results {
                if let KyokuOutcome::Diffs(diffs) = o {
                    for d in diffs {
                        match d {
                            ReplayDiff::EventFailed { reason, .. } => {
                                event_failed += 1;
                                let key = format!("EventFailed: {}", classify(reason));
                                *reason_counts.entry(key).or_insert(0) += 1;
                            }
                            ReplayDiff::ResultMismatch { reason } => {
                                let key = format!("ResultMismatch: {}", classify(reason));
                                *reason_counts.entry(key).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
        let mut sorted: Vec<_> = reason_counts.into_iter().collect();
        sorted.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        eprintln!("\n=== Diff reason 分布 ===");
        for (reason, count) in &sorted {
            eprintln!("  {count:4} × {reason}");
        }
        eprintln!("\n  EventFailed total: {event_failed}");
    }

    /// 把具体 reason 简化成桶 (前几个词).
    fn classify(reason: &str) -> String {
        // 取前 50 字符 + 替换具体数字
        let r = reason.replace(|c: char| c.is_ascii_digit(), "N");
        r.chars().take(60).collect()
    }

    /// 输出每个 diff 的完整细节, 含 fixture/kyoku 索引.
    /// 用于调试 ResultMismatch 找具体 case 修.
    #[test]
    #[ignore = "用 cargo test ... --ignored 显式跑, 输出每个 diff 的完整原文"]
    fn diff_full_details() {
        let dir = std::path::Path::new("tests/replay/fixtures");
        if !dir.exists() {
            return;
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mjlog") {
                continue;
            }
            let Ok(r) = run_fixture(&path) else { continue };
            for (idx, o) in r.kyoku_results.iter().enumerate() {
                if let KyokuOutcome::Diffs(diffs) = o {
                    for d in diffs {
                        match d {
                            ReplayDiff::ResultMismatch { reason } => {
                                eprintln!("[{}#{}] {}", r.fixture, idx, reason);
                            }
                            ReplayDiff::EventFailed {
                                idx: ev_idx,
                                reason,
                            } => {
                                eprintln!(
                                    "[{}#{}] EventFailed at ev {}: {}",
                                    r.fixture, idx, ev_idx, reason
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
