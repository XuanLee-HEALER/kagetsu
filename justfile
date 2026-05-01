# tui-majo task runner
#
# 常用:
#   just            列出所有 recipe
#   just play       开始游戏 (release 构建, AI 推进流畅)
#   just dev        debug 构建跑游戏 (启动快, AI 略慢)
#   just test       跑全部单元测试
#   just ci         本地预提交检查 (fmt + clippy + test)

# Windows 上用 PowerShell 7 跑 recipe (其它平台仍用默认 sh).
set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

# 默认: 列出 recipes
default:
    @just --list

# 开始游戏 (release, 推荐) — 默认走 launcher 在新终端窗口开
# 注意: launcher 不直接依赖 game, cargo 不会自动重编 game.
#       这里显式 build --bins 确保两个 binary 都最新.
play:
    cargo build --release --bins
    cargo run --release --bin tui-majo

# 别名: run = play
run: play

# 开发模式 (debug 构建, 编译快但运行略慢)
dev:
    cargo build --bins
    cargo run --bin tui-majo

# 强制 inline 启动 (跳过 launcher, 在当前终端跑)
play-inline:
    cargo build --release --bins
    cargo run --release --bin tui-majo -- --inline

# 直接跑游戏内核, 跳过 launcher (开发常用, 不开新窗口)
play-game:
    cargo run --release --bin tui-majo-game

# release 构建, 不运行
build:
    cargo build --release

# 跑全部测试
test:
    cargo test

# 跑某个模块的测试: just test-mod decompose
test-mod MOD:
    cargo test --lib {{MOD}}

# 严格 fuzz: 1000 个随机 seed 跑分数守恒 (release, ~17 分钟)
# 平时 just ci 只跑 16 cases, 发布前 / 重构后跑这个加深保护
fuzz:
    $env:PROPTEST_CASES = "1000"; cargo test --release --test proptest_invariants

# 类型检查 (比 build 快)
check:
    cargo check --all-targets

# clippy lint
lint:
    cargo clippy --all-targets -- -D warnings

# 自动修可修的 clippy
fix:
    cargo clippy --fix --all-targets --allow-dirty --allow-staged

# 格式化代码
fmt:
    cargo fmt

# 检查格式 (不改文件)
fmt-check:
    cargo fmt -- --check

# 本地预提交检查
ci: fmt-check lint test
    @echo "✓ ci passed"

# 清理 build 产物
clean:
    cargo clean

# 生成并打开 API 文档
docs:
    cargo doc --no-deps --open

# 看 spec 文档列表
spec:
    @ls docs/spec/
