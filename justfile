# tui-majo task runner
#
# 常用:
#   just                列出所有 recipe
#   just play           开始游戏 (release 构建, AI 推进流畅)
#   just play-debug     带 launcher debug log (查窗口位置 / osascript 错误)
#   just dev            debug 构建跑游戏 (启动快, 迭代用)
#   just test           跑全部单元测试 (带 dev-tools feature)
#   just cov            跑覆盖率 + 总览
#   just cov-html       生成 HTML 覆盖率报告并打开
#   just ci             本地预提交检查 (fmt + clippy + test)

# Windows 上用 PowerShell 7 跑 recipe (其它平台仍用默认 sh).
set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

# 默认 RUST_LOG: libp2p 内部 INFO 太冗余 (会撞 TUI), 仅留 warn+; 自家 tui_majo 留 info.
# 日志默认写到 %TEMP%/tui-majo.log (见 src/bin/game.rs::init_tracing).
# 临时覆盖: `RUST_LOG=debug just play` (其它) / `$env:RUST_LOG="debug"; just play` (Windows).
export RUST_LOG := env_var_or_default("RUST_LOG", "warn,tui_majo=info,libp2p=warn")

# ============================================================
# 默认: 列出 recipes
# ============================================================
default:
    @just --list

# ============================================================
# 跑游戏
# ============================================================

# 开始游戏 (release, 推荐) — 默认走 launcher 在新终端窗口开
# launcher 不直接依赖 game, cargo 不会自动重编 game; build --bins 确保两个都最新.
# 默认带 dev-tools feature: F5/F9 savestate + F8 录像开关.
play:
    cargo build --release --bins --features dev-tools
    cargo run --release --bin tui-majo --features dev-tools

# 别名: run = play
run: play

# Launcher debug 模式 (TUI_MAJO_DEBUG=1):
# 在 launcher 终端 stderr 打印 osascript 内容 + stderr, 排查 -10000 / 窗口位置问题.
# 用法:跑此 recipe → 看到 "[launcher/iTerm2] osascript script: ..." 之类输出.
play-debug:
    cargo build --release --bins --features dev-tools
    TUI_MAJO_DEBUG=1 cargo run --release --bin tui-majo --features dev-tools

# 开发模式 (debug 构建, 编译快但运行略慢)
dev:
    cargo build --bins --features dev-tools
    cargo run --bin tui-majo --features dev-tools

# 强制 inline 启动 (跳过 launcher 的新终端 spawn, 在当前终端跑)
play-inline:
    cargo build --release --bins --features dev-tools
    cargo run --release --bin tui-majo --features dev-tools -- --inline

# 直接跑游戏内核, 完全跳过 launcher (开发常用, 不开新窗口, 编译快)
play-game:
    cargo run --bin tui-majo-game --features dev-tools

# release 构建, 不运行
build:
    cargo build --release --bins --features dev-tools

# ============================================================
# 测试
# ============================================================

# 跑全部单元测试 (含 dev-tools feature 下的 recorder/savestate 测试)
test:
    cargo test --lib --features dev-tools

# 跑某个模块的测试: just test-mod engine::round_state
test-mod MOD:
    cargo test --lib --features dev-tools {{MOD}}

# 严格 fuzz: 1000 个随机 seed 跑分数守恒 (release, ~17 分钟)
# 平时 just ci 只跑 16 cases, 发布前 / 重构后跑这个加深保护.
fuzz:
    PROPTEST_CASES=1000 cargo test --release --test proptest_invariants

# ============================================================
# 覆盖率 (需 cargo-llvm-cov: cargo install cargo-llvm-cov +
#                          rustup component add llvm-tools-preview)
# ============================================================

# 总览 — 各文件 region/line/function 百分比
cov:
    cargo llvm-cov --features dev-tools --lib --ignore-run-fail --summary-only

# HTML 报告 — 可点击逐行高亮未覆盖行, 浏览器打开
cov-html:
    cargo llvm-cov --features dev-tools --lib --ignore-run-fail --html --open

# 列出每文件未覆盖行号 (排查薄弱点用)
cov-missing:
    cargo llvm-cov --features dev-tools --lib --ignore-run-fail --show-missing-lines

# ============================================================
# 静态检查 / 格式化
# ============================================================

# 类型检查 (比 build 快)
check:
    cargo check --all-targets --features dev-tools

# clippy lint
lint:
    cargo clippy --all-targets --features dev-tools -- -D warnings

# 自动修可修的 clippy
fix:
    cargo clippy --fix --all-targets --features dev-tools --allow-dirty --allow-staged

# 格式化代码
fmt:
    cargo fmt

# 检查格式 (不改文件)
fmt-check:
    cargo fmt -- --check

# 本地预提交检查
ci: fmt-check lint test
    @echo "✓ ci passed"

# ============================================================
# 杂项
# ============================================================

# 清理 build 产物
clean:
    cargo clean

# 生成并打开 API 文档
docs:
    cargo doc --no-deps --open

# 看 spec 文档列表
spec:
    @ls docs/spec/
