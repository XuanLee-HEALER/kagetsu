# Workspace 拆分执行计划

将单 crate `tui-majo` 拆为 cargo workspace,共 3 个 crate。

**分支**: `workspace-refactor`
**本次任务范围**: 完成拆分 + 测试全绿 + 留下 `RENAME-TODO.md`
**改名 (→ kage)**: 不在本次任务内,任务结束后用户自己按 `rename-todo.md` 操作

---

## 1. 目标结构

```
tui-majo/                       (workspace root, 仓库名暂不动)
├── Cargo.toml                  [workspace] + [workspace.dependencies]
├── Cargo.lock
├── crates/
│   ├── majo-core/              lib — 一切"非渲染"逻辑
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── engine/         (现 src/engine/)
│   │   │   ├── game_engine.rs  (现 src/game_engine.rs)
│   │   │   ├── ai/             (现 src/ai/)
│   │   │   ├── mental_poker/   (现 src/mental_poker/)
│   │   │   ├── config/         (现 src/config/, Phase 0 已解耦)
│   │   │   ├── net/            (现 src/net/, 含 libp2p)
│   │   │   └── dev/            (现 src/dev/, cfg(feature="dev-tools"))
│   │   └── tests/
│   │       ├── engine_drives_match.rs
│   │       ├── proptest_invariants.rs
│   │       ├── probe_claw_relay.rs
│   │       ├── scenarios_*.rs   (5 个 scenarios 测试, 不依赖 ui)
│   │       ├── scenarios_replay.rs
│   │       ├── common/
│   │       └── replay/
│   │
│   ├── tui-majo/               bin — TUI 客户端
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs          (re-export ui 模块)
│   │       ├── main.rs         (launcher, 现 src/main.rs)
│   │       ├── ui/             (现 src/ui/)
│   │       └── bin/
│   │           ├── game.rs     (现 src/bin/game.rs)
│   │           └── relay.rs    (现 src/bin/relay.rs)
│   │
│   └── web-majo/               bin — 自部署 web 节点 (本次仅骨架)
│       ├── Cargo.toml
│       └── src/
│           └── main.rs         (hello-world + import majo_core 验证接通)
│
├── docs/
│   ├── refactor/
│   │   ├── workspace-plan.md   (本文档)
│   │   └── rename-todo.md      (任务结束后用户执行)
│   ├── design/
│   ├── spec/
│   ├── img/
│   └── release.md
├── justfile
├── README.md
└── LICENSE
```

## 2. 反向耦合点 (Phase 0 解决)

只有 **1 个**:
- `src/config/mod.rs:24` `use crate::ui::theme::ThemeKind;`
- `LocalPrefs.theme: ThemeKind` 字段持久化到 prefs.toml

`net/` 模块经过 grep 验证 **零** 反向耦合(原以为有 `TileSpec`,实际只是注释里出现 "ratatui" 字样)。

## 3. 阶段化执行步骤

### Phase 0: 解耦 ThemeKind (仍在单 crate 形态)

1. `src/ui/theme.rs` 内 `ThemeKind` enum 整体移动到 `src/config/theme_kind.rs` (新文件) 或直接放 `src/config/mod.rs`
   - 移动的部分:`enum ThemeKind { Dark, Light }` + `next()` + `label()` + `Default`/`Serialize`/`Deserialize` derive
   - **不移动**:`theme()` method (返回 `Theme`,持 ratatui Color)
2. `src/ui/theme.rs` 内:
   - 删除 `ThemeKind` 定义
   - 加 `use crate::config::ThemeKind;` (或 `pub use crate::config::ThemeKind;` 让旧引用路径 `crate::ui::theme::ThemeKind` 仍可用 —— 选这个能少改其它文件,推荐)
   - 把 `theme()` method 改成自由函数 `pub fn theme_for(kind: ThemeKind) -> Theme` 或 inherent method on `Theme`:`Theme::from_kind(kind) -> Theme`
3. 全文修引用: 所有 `ThemeKind::Dark.theme()` → `Theme::from_kind(ThemeKind::Dark)` 或 `theme_for(ThemeKind::Dark)`
   - 影响文件: `src/ui/edit_config_modal.rs:334`, `src/ui/chi_picker.rs:222`, `src/ui/confirm.rs:239`
   - `src/ui/screens/online_room.rs` 和 `online_zerotrust_game.rs` 只用了 `ThemeKind` 类型,不用 `theme()`,不受影响
4. `cargo test` 全绿
5. commit: `refactor(config): 解 ThemeKind 反向耦合, 上移裸 enum 到 config`

**预期 diff 大小**: ~50 行(主要是 theme.rs 内部重排 + 3 处调用改写)

### Phase 1+2: 建 workspace + 搬迁代码

这两步必须在**单次 commit 序列**内连续完成(中间状态编译不过),建议如下顺序:

**Step 1: 准备 workspace 根 Cargo.toml**

```toml
[workspace]
resolver = "2"
members = ["crates/majo-core", "crates/tui-majo", "crates/web-majo"]

[workspace.package]
edition = "2024"
version = "2.1.0"
license = "GPL-3.0-or-later"
repository = "https://github.com/XuanLee-HEALER/tui-majo"
homepage = "https://github.com/XuanLee-HEALER/tui-majo"

[workspace.dependencies]
# common
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# rng / time / fs
rand = "0.9"
rand_chacha = "0.9"
dirs = "5"
toml = "0.8"
time = { version = "0.3", features = ["local-offset"] }
uuid = { version = "1", features = ["v4", "serde"] }

# async runtime + net
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync", "net", "io-util"] }
futures-util = "0.3"
libp2p = { version = "0.56", features = [
    "tokio", "quic", "tcp", "noise", "yamux", "mdns", "identify",
    "autonat", "relay", "dcutr", "request-response", "cbor", "ed25519",
    "macros", "serde", "ping", "gossipsub",
] }

# crypto
ark-ec = "0.5"
ark-ff = "0.5"
ark-std = "0.5"
ark-serialize = { version = "0.5", features = ["derive"] }
ark-bls12-381 = "0.5"
sha2 = "0.10"

# tui
ratatui = "0.30"
crossterm = "0.28"
unicode-width = "0.2"

# dev
pretty_assertions = "1"
proptest = "1"
quick-xml = "0.36"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

**Step 2: `git mv` 搬迁源码** (顺序无所谓,因为是 mv 不是改内容)

```bash
mkdir -p crates/majo-core/src crates/tui-majo/src/bin crates/web-majo/src
git mv src/engine        crates/majo-core/src/engine
git mv src/game_engine.rs crates/majo-core/src/game_engine.rs
git mv src/ai            crates/majo-core/src/ai
git mv src/mental_poker  crates/majo-core/src/mental_poker
git mv src/config        crates/majo-core/src/config
git mv src/net           crates/majo-core/src/net
git mv src/dev           crates/majo-core/src/dev
git mv src/ui            crates/tui-majo/src/ui
git mv src/main.rs       crates/tui-majo/src/main.rs
git mv src/bin/game.rs   crates/tui-majo/src/bin/game.rs
git mv src/bin/relay.rs  crates/tui-majo/src/bin/relay.rs
rmdir src/bin src
```

**Step 3: 搬迁 tests** (依据导入分析: 全部 tests 都不依赖 `crate::ui`,所以全部归 majo-core/tests/)

```bash
mkdir -p crates/majo-core/tests
git mv tests/* crates/majo-core/tests/
rmdir tests
```

**Step 4: 写新 lib.rs / main.rs**

- `crates/majo-core/src/lib.rs`: 复制原 `src/lib.rs` 内容(public mod 声明列表),保留 `#[cfg(feature = "dev-tools")] pub mod dev;`
- `crates/tui-majo/src/lib.rs`: 新建,只 `pub mod ui;`(若 bin/{game,relay} 通过 `tui_majo::ui::App` 访问)。如果不需要 lib,可以省略,直接让 bin 用 `ui` 作为本地 mod。
- `crates/web-majo/src/main.rs`: hello-world,见下面 Step 7。

**Step 5: 各 crate Cargo.toml**

`crates/majo-core/Cargo.toml`:

```toml
[package]
name = "majo-core"
edition.workspace = true
version.workspace = true
license.workspace = true
repository.workspace = true
description = "Riichi mahjong core: engine, AI, mental poker, network protocol."

[features]
default = []
dev-tools = []

[dependencies]
anyhow.workspace = true
thiserror.workspace = true
tracing.workspace = true
serde.workspace = true
serde_json.workspace = true
rand.workspace = true
rand_chacha.workspace = true
dirs.workspace = true
toml.workspace = true
time.workspace = true
uuid.workspace = true
tokio.workspace = true
futures-util.workspace = true
libp2p.workspace = true
ark-ec.workspace = true
ark-ff.workspace = true
ark-std.workspace = true
ark-serialize.workspace = true
ark-bls12-381.workspace = true
sha2.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true
proptest.workspace = true
quick-xml.workspace = true
tracing-subscriber.workspace = true
```

`crates/tui-majo/Cargo.toml`:

```toml
[package]
name = "tui-majo"
edition.workspace = true
version.workspace = true
license.workspace = true
repository.workspace = true
description = "Riichi Mahjong (日本麻将) in your terminal."
default-run = "tui-majo"
exclude = [".design-package/", "docs/scratch.md", "docs/img/", "*.png"]

[features]
default = []
dev-tools = ["majo-core/dev-tools"]

[dependencies]
majo-core = { path = "../majo-core" }
anyhow.workspace = true
ratatui.workspace = true
crossterm.workspace = true
unicode-width.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tokio.workspace = true     # bin/game.rs 起 runtime 用
serde.workspace = true     # 部分 ui 模块可能用
serde_json.workspace = true
time.workspace = true

[[bin]]
name = "tui-majo"
path = "src/main.rs"

[[bin]]
name = "tui-majo-game"
path = "src/bin/game.rs"

[[bin]]
name = "tui-majo-relay"
path = "src/bin/relay.rs"
```

`crates/web-majo/Cargo.toml`:

```toml
[package]
name = "web-majo"
edition.workspace = true
version.workspace = true
license.workspace = true
repository.workspace = true
description = "Self-hosted web frontend for majo (placeholder)."

[dependencies]
majo-core = { path = "../majo-core" }
anyhow.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

**Step 6: 批量改 import path**

仅 `crates/tui-majo/` (含 src/ + bin/) 和 `crates/majo-core/tests/` 需要改:

```bash
# tui-majo crate 内
for kw in engine game_engine ai mental_poker config net dev; do
    # 形如: use crate::engine::X → use majo_core::engine::X
    fd -e rs . crates/tui-majo/ -x sed -i '' -E "s|use crate::${kw}|use majo_core::${kw}|g" {}
done

# tests (原 tests 内用 tui_majo::, 现归 majo-core/tests, 改 tui_majo:: → majo_core::)
for kw in engine game_engine ai mental_poker config net dev; do
    fd -e rs . crates/majo-core/tests/ -x sed -i '' -E "s|tui_majo::${kw}|majo_core::${kw}|g" {}
done
```

**注意**:
- `crates/majo-core/src/` 内部所有 `use crate::xxx` 保持不动(crate 名换了,但 `crate::` 自动指向新 crate)
- `crates/tui-majo/src/` 内部 `use crate::ui::xxx` 保持不动(ui 仍在 tui-majo)
- bin/{game,relay} 原来用 `tui_majo::ui::App` 访问 ui,搬到 `crates/tui-majo/src/bin/` 后**仍可用** `tui_majo::ui::App`(因为 tui-majo crate 本身仍叫 tui-majo,有 lib.rs re-export ui)

**Step 7: web-majo 占位 main.rs**

```rust
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
```

**Step 8: 顺序 commit**

建议:

```
refactor(workspace): 解 ThemeKind 反向耦合 (Phase 0)
refactor(workspace): 切换为 cargo workspace, 搬迁源码到 majo-core / tui-majo
refactor(workspace): 搬迁 tests 到 majo-core, 改写 import path
chore(workspace): web-majo 骨架 + Cargo.toml 配置, 全 workspace cargo test 通过
docs(refactor): 写 workspace-plan.md + rename-todo.md
```

### Phase 3: 验证

按 task #6 描述跑全部验证:

```bash
cargo build --workspace
cargo build --workspace --features dev-tools
cargo test  --workspace
cargo test  --workspace --features dev-tools
```

人工跑一次 TUI 启动确认无 panic:

```bash
cargo run -p tui-majo --bin tui-majo-game -- --inline
```

### Phase 4: RENAME-TODO.md

见 `docs/refactor/rename-todo.md` (任务结束后用户执行)。

## 4. 风险点 + 应对

| 风险 | 应对 |
|---|---|
| `tokio::main` macro 在 bin 内, 但 tokio 版本和 features 走 workspace.dependencies, bin crate 必须显式声明 | 在 tui-majo Cargo.toml 里加 `tokio.workspace = true` |
| `dev-tools` feature 跨 crate 透传忘记加 | tui-majo `[features] dev-tools = ["majo-core/dev-tools"]` |
| ratatui / crossterm 在 majo-core 的 transitive 出现 | 不应该: majo-core 不 depend tui crates; 若 cargo tree 显示有, grep `use ratatui` 全删 |
| sed 替换误改 docstring 内的 `tui_majo::xxx` 示例 | 用 `cargo build` 直接验证, doctest 也会跑 |
| Cargo.lock 大改动 | workspace 切换后 Cargo.lock 会重写, 合并冲突难, 直接 `rm Cargo.lock && cargo build` 重生 (本次 PR 只此一次) |
| dev/ 模块 cfg(feature) 在 majo-core, tui-majo 的 bin/game.rs 引用 `tui_majo::dev` 时需通过 majo-core 暴露 | 改为 `majo_core::dev` (feature 透传后能用) |

## 5. 验收清单

- [ ] Phase 0: `cargo test` 全绿, ThemeKind 上移 commit 完成
- [ ] Phase 1+2: `cargo build --workspace` 通过, `cargo test --workspace` 全绿
- [ ] Phase 3: `cargo run -p tui-majo --bin tui-majo-game -- --inline` 正常启动 TUI
- [ ] Phase 4: `docs/refactor/rename-todo.md` 写完
- [ ] 提交 history 干净, 每个 commit 可独立编译
- [ ] git log 显示 file mv 而非 delete+create (用 git mv, 保 history)
