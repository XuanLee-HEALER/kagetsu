# Rename TODO: `tui-majo` → `kage`

任务结束后**用户自己**按此清单执行 — agent 不会替你跑任何一步。

**起点状态**(workspace-refactor 分支完成时):

- 仓库名: `tui-majo` (GitHub: `XuanLee-HEALER/tui-majo`)
- workspace 三个 crate: `majo-core` / `tui-majo` / `web-majo`
- 已发布 crates.io: `tui-majo` v2.1.0 (单 crate 时代)

**终点目标**:

- 仓库名: `kage` (GitHub: `XuanLee-HEALER/kage`)
- workspace 三个 crate: `kage-core` / `kage-tui` / `kage-web`
- crates.io: `kage-core` / `kage-tui` (web 是否发布看 5.5)

按本清单**从上到下顺序执行**,跨阶段会有依赖。每一步都给了可直接复制的 shell;`sed` 用 macOS / BSD 版本(GNU sed 请把 `sed -i ''` 改成 `sed -i`)。

---

## 0. 前置确认

### 0.1 验证 crates.io 命名可用

```bash
cargo search kage-core --limit 5
cargo search kage-tui  --limit 5
cargo search kage-web  --limit 5
```

只要返回结果中**没有** name 完全等于 `kage-core` / `kage-tui` / `kage-web` 的条目就可以用。注意 `kage` 这种短名很容易被人占,任何一个名字被占就走 fallback。

### 0.2 Fallback 命名候选

如果上面任一名字已被占用,按优先级选下面任一套替代(三个名字保持同前缀,别混):

| 候选前缀 | core | tui | web | 备注 |
|---|---|---|---|---|
| `kage-mahjong-` | `kage-mahjong-core` | `kage-mahjong-tui` | `kage-mahjong-web` | 最稳,搜索可发现性好 |
| `riichi-kage-` | `riichi-kage-core` | `riichi-kage-tui` | `riichi-kage-web` | 强调日麻规则 |
| `kage-rs-` | `kage-rs-core` | `kage-rs-tui` | `kage-rs-web` | rust 社区惯用 `-rs` 后缀,但用作前缀偏少 |
| `majo-`(保留) | `majo-core` | `majo-tui` | `majo-web` | 仅去掉 `tui-` 前缀,改动最小,但名字偏抽象 |

下面所有 sed / 命令默认使用 `kage` 主选;选 fallback 请整体替换前缀。

### 0.3 备份 / 新分支

```bash
git checkout main
git pull
git checkout -b rename/kage
```

---

## 1. workspace 内 crate 改名

> ⚠️ 这一节会改非常多文件,**先确保工作区干净**(`git status` 无未提交改动)。

### 1.1 目录改名

```bash
git mv crates/majo-core crates/kage-core
git mv crates/tui-majo  crates/kage-tui
git mv crates/web-majo  crates/kage-web
```

### 1.2 每个 crate 的 `Cargo.toml` 改 `name`

```bash
sed -i '' 's|^name = "majo-core"|name = "kage-core"|' crates/kage-core/Cargo.toml
sed -i '' 's|^name = "tui-majo"|name = "kage-tui"|'  crates/kage-tui/Cargo.toml
sed -i '' 's|^name = "web-majo"|name = "kage-web"|'  crates/kage-web/Cargo.toml
```

### 1.3 各 crate 的 `Cargo.toml` 中 path 依赖

`kage-tui/Cargo.toml` 和 `kage-web/Cargo.toml` 里都有 `majo-core = { path = "..." }`,改为 `kage-core`:

```bash
sed -i '' -E 's|^majo-core = \{ path = "\.\./majo-core" \}|kage-core = { path = "../kage-core" }|' \
    crates/kage-tui/Cargo.toml \
    crates/kage-web/Cargo.toml
```

如 `[features]` 透传引用了 `majo-core/dev-tools`:

```bash
sed -i '' 's|"majo-core/dev-tools"|"kage-core/dev-tools"|g' crates/kage-tui/Cargo.toml
```

### 1.4 根 `Cargo.toml` 的 workspace `members`

```bash
sed -i '' -E \
  's|"crates/majo-core"|"crates/kage-core"|; s|"crates/tui-majo"|"crates/kage-tui"|; s|"crates/web-majo"|"crates/kage-web"|' \
  Cargo.toml
```

确认:

```bash
grep '^members' Cargo.toml
# members = ["crates/kage-core", "crates/kage-tui", "crates/kage-web"]
```

### 1.5 bin 名

`crates/kage-tui/Cargo.toml` 里的 `[[bin]]` 段以及 `default-run`:

```bash
sed -i '' \
  -e 's|default-run = "tui-majo"|default-run = "kage"|' \
  -e 's|name = "tui-majo"$|name = "kage"|' \
  -e 's|name = "tui-majo-game"|name = "kage-game"|' \
  -e 's|name = "tui-majo-relay"|name = "kage-relay"|' \
  crates/kage-tui/Cargo.toml
```

> bin 名是用户安装后实际命令名 (`cargo install kage-tui` → `~/.cargo/bin/kage`),改名后老用户的 `tui-majo` 命令会失效,**release notes 里要明确说明**。

### 1.6 全文 sed:rust 源码 `use` / 路径

`majo_core::` → `kage_core::`,`tui_majo::` → `kage_tui::`:

```bash
# rs 源文件
find crates -name '*.rs' -type f -exec sed -i '' \
    -e 's/majo_core::/kage_core::/g' \
    -e 's/tui_majo::/kage_tui::/g' \
    -e 's/\bextern crate majo_core\b/extern crate kage_core/g' \
    -e 's/\bextern crate tui_majo\b/extern crate kage_tui/g' \
    {} +
```

`RUST_LOG` 默认值里的 crate 名(`justfile`、`bin/*` 的 init_tracing 等):

```bash
find crates -name '*.rs' -type f -exec sed -i '' \
    -e 's/\btui_majo=info/kage_tui=info/g' \
    -e 's/\btui_majo=debug/kage_tui=debug/g' \
    -e 's/\btui_majo=warn/kage_tui=warn/g' \
    {} +
```

### 1.7 验证 workspace 全绿

```bash
rm -f Cargo.lock     # workspace 改名后 lock 文件大改,重生
cargo build --workspace
cargo build --workspace --features dev-tools
cargo test  --workspace
cargo test  --workspace --features dev-tools
```

四条全绿才能继续。中间任意一条挂了:

- 编译错通常是漏改的 `use majo_core::` / `use tui_majo::` — 用 `grep -rn "majo_core\|tui_majo" crates/` 找剩余
- doctest 挂通常是文档注释里的示例代码 — 一并 sed

### 1.8 commit

```bash
git add Cargo.toml crates/
git commit -m "refactor(workspace): rename crates majo-core/tui-majo/web-majo → kage-core/kage-tui/kage-web"
```

---

## 2. GitHub 仓库改名

### 2.1 用 gh CLI 一键改名

```bash
gh repo rename kage --repo XuanLee-HEALER/tui-majo
```

GitHub 自动:

- 把 `XuanLee-HEALER/tui-majo` → `XuanLee-HEALER/kage`
- 设置 **301 永久重定向**,老 URL (`github.com/XuanLee-HEALER/kagetsu`, releases、issues、PRs 链接)继续可用
- `git clone` 老地址会拿到 redirect,不需要现有用户立刻改 remote

### 2.2 更新本地 git remote(可选但推荐)

```bash
git remote set-url origin git@github.com:XuanLee-HEALER/kage.git
git remote -v   # 验证
```

### 2.3 更新 `Cargo.toml` 中的 repo / homepage 字段

根 `Cargo.toml` 的 `[workspace.package]`:

```bash
sed -i '' \
  -e 's|https://github.com/XuanLee-HEALER/kagetsu|https://github.com/XuanLee-HEALER/kage|g' \
  Cargo.toml
```

`crates/*/Cargo.toml` 里如果有显式 `repository = ...` / `homepage = ...` 而非 `.workspace = true` 的,也同步改(应该没有,workspace 化后都该走 `.workspace = true`)。验证:

```bash
grep -rn "tui-majo" Cargo.toml crates/*/Cargo.toml
# 应该只剩下 description / keywords / 等业务字段里的语义文字, URL 都换完了
```

---

## 3. crates.io 处理

### 3.1 yank 老版本

**⚠️ yank 不删除版本,只让 `cargo install` 默认看不到。已有依赖此版本的 `Cargo.lock` 仍能解析。**

```bash
cargo yank --vers 2.1.0 tui-majo
# 想取消: cargo yank --vers 2.1.0 --undo tui-majo
```

### 3.2 是否保留 `tui-majo` 名字防 squatting

**建议保留**。理由:

- crates.io **永远不允许删除 crate**,你只能放弃 owner;放弃后被别人接手发恶意包,坑老用户
- 占着不发新版,成本几乎为 0
- 可以发一个 v2.1.1 的占位版本(`lib.rs` 只 re-export `kage_core` + `eprintln!("[DEPRECATED] please use kage-* crates")`),帮老用户 migrate

可选的占位 publish 步骤(在另一个临时目录):

```bash
mkdir -p /tmp/tui-majo-stub && cd /tmp/tui-majo-stub
cat > Cargo.toml <<'EOF'
[package]
name = "tui-majo"
version = "2.1.1"
edition = "2024"
license = "GPL-3.0-or-later"
description = "DEPRECATED — renamed to kage-tui. See https://github.com/XuanLee-HEALER/kage"
repository = "https://github.com/XuanLee-HEALER/kage"

[[bin]]
name = "tui-majo"
path = "src/main.rs"
EOF
mkdir src
cat > src/main.rs <<'EOF'
fn main() {
    eprintln!("[tui-majo] This crate has been renamed to `kage-tui`.");
    eprintln!("Please run: cargo install kage-tui");
    eprintln!("See https://github.com/XuanLee-HEALER/kage");
    std::process::exit(1);
}
EOF
cargo publish --dry-run   # 先 dry run
cargo publish
cd -
```

### 3.3 发布 `kage-core` (lib)

```bash
cargo publish --dry-run -p kage-core
cargo publish -p kage-core
```

### 3.4 发布 `kage-tui` (bin)

`kage-tui` 依赖 `kage-core` 的 path,publish 时 cargo 会要求 `version` 字段;workspace 已经走 `version.workspace`,publish 前确认 path 依赖会自动加 `version = "..."`(workspace inheritance 自动处理):

```bash
cargo publish --dry-run -p kage-tui
cargo publish -p kage-tui
```

### 3.5 `kage-web` 是否 publish

**目前是 skeleton,建议先不发。** 等到真正接 axum + WebSocket gateway 后再 publish 第一版。

如果你想现在就占名字防 squatting,publish 一个最小版本:

```bash
cargo publish --dry-run -p kage-web
cargo publish -p kage-web
```

### 3.6 publish 顺序约束

`kage-tui` / `kage-web` 都依赖 `kage-core`,所以**必须先发 `kage-core`,等 1-2 分钟 crates.io index 同步后再发 bin crate**。否则会报 `kage-core` not found。

---

## 4. docs / 文档全文替换

### 4.1 docs/ 内所有 markdown

```bash
# tui-majo (slug) → kage; tui_majo (rust path) → kage_tui
find docs -name '*.md' -type f -exec sed -i '' \
    -e 's/tui-majo/kage/g' \
    -e 's/tui_majo/kage_tui/g' \
    {} +
```

⚠️ **审查 sed 结果**!这种 broad replace 会把"专有名词 tui-majo"和"作为路径片段的 tui-majo"全部换掉。看一遍 diff:

```bash
git diff docs/
```

特别检查:

- `docs/refactor/workspace-plan.md` — 是历史记录,可考虑**不动**(保留原始上下文),或者只在文末加个 "**注**: 改名已完成,见 `rename-todo.md`"
- `docs/release.md` — 大量出现 `tui-majo-${version}-${target}.tar.gz` artifact 命名,如果你想保持 v2.1.0 之前的 artifact 命名历史,只改 v2.1.1+ 的示例;否则全替也行
- `docs/design/pure-functional-refactor.md` — 是设计文档,改 import path 示例足够

### 4.2 README.md

README 第一段从「tui-majo · 终端日本麻将」改为「kage · 终端日本麻将」,并加一段三 crate 概览。建议手写而非 sed,因为还要解释**为什么改名 + 三 crate 结构 + 老用户怎么迁移**。

参考模板(改完用 `git diff README.md` 校验):

```markdown
# kage · 终端日本麻将

> (原 tui-majo,v2.2 起改名为 kage,详见 [迁移指南](#迁移自-tui-majo).)

基于比赛规则(WRC 2022 主基)的日麻 TUI 实现,Rust + ratatui 写的。**workspace 三 crate**:

- `kage-core` — 引擎、AI、mental poker、libp2p 网络协议
- `kage-tui`  — TUI 客户端 (本仓库主 binary `kage`)
- `kage-web`  — 自部署 web 节点(WIP)

## 截图

![kage 截图](https://raw.githubusercontent.com/XuanLee-HEALER/kage/master/docs/img/screenshot.png)

## 安装

```bash
cargo install kage-tui
kage              # 启动游戏
```

## 迁移自 tui-majo

老的 `tui-majo` crate 已 deprecated,bin 名也从 `tui-majo` 改成 `kage`。手动迁移:

```bash
cargo uninstall tui-majo
cargo install kage-tui
```
\`\`\`

(rest of README 保持原内容,把所有 `tui-majo` / `tui_majo` 引用按 4.1 规则全文替换。)

### 4.3 LICENSE

GPLv3 全文里**没有项目名占位符**,copyright holder (`XuanLee-HEALER`) 保持不变。**LICENSE 不需要改**。

如果你额外维护了 NOTICE / COPYRIGHT 这类文件提到 "tui-majo",同步改。

### 4.4 justfile

```bash
sed -i '' \
    -e 's/\btui-majo\b/kage/g' \
    -e 's/\btui_majo\b/kage_tui/g' \
    -e 's/--bin tui-majo\b/--bin kage/g' \
    -e 's/--bin tui-majo-game\b/--bin kage-game/g' \
    -e 's/--bin tui-majo-relay\b/--bin kage-relay/g' \
    justfile
```

特别注意:

- `RUST_LOG := env_var_or_default("RUST_LOG", "warn,tui_majo=info,libp2p=warn")` → 把 `tui_majo` 改成 `kage_tui`(已在 sed 命令覆盖)
- 注释里提到 `%TEMP%/tui-majo.log` 这种路径,你可能想顺便改 log filename;改 log 文件名要同时改 `crates/kage-tui/src/bin/game.rs::init_tracing` 内的硬编码字符串

校验:

```bash
just --list   # 命令全部能跑出来
just dev      # 跑通
```

---

## 5. 其它

### 5.1 GitHub Actions

`.github/workflows/release.yml` 里所有 `tui-majo` 引用要改:

```bash
sed -i '' \
    -e 's|tui-majo-\${version}|kage-${version}|g' \
    -e 's|tui-majo-\$version|kage-$version|g' \
    -e 's|release/tui-majo\b|release/kage|g' \
    -e 's|release/tui-majo-game|release/kage-game|g' \
    -e 's|release/tui-majo-relay|release/kage-relay|g' \
    -e 's|tui-majo\.exe|kage.exe|g' \
    -e 's|tui-majo-game\.exe|kage-game.exe|g' \
    -e 's|tui-majo-relay\.exe|kage-relay.exe|g' \
    -e 's|XuanLeeHEALER\.tui-majo|XuanLeeHEALER.kage|g' \
    .github/workflows/release.yml
```

⚠️ **`identifier: XuanLeeHEALER.tui-majo` 在 winget 里改了意味着发的是一个新 package**,老的 `XuanLeeHEALER.tui-majo` 安装的用户**不会**自动收到 `XuanLeeHEALER.kage` 的更新。两种选择:

1. (推荐)保留 `XuanLeeHEALER.tui-majo` identifier 直到出大版本,在 release notes 通告改名后逐渐过渡;同时让新 identifier `XuanLeeHEALER.kage` 也发,过渡期两个 identifier 都发
2. 直接换,所有 winget 用户得手动 `winget uninstall XuanLeeHEALER.tui-majo && winget install XuanLeeHEALER.kage`

如选 (1),winget step 内复制一份,一个 identifier=...tui-majo 一个 ...kage,两份都跑。

校验:

```bash
cat .github/workflows/release.yml | grep -n "kage\|tui-majo"
```

### 5.2 brew tap 公式(`docs/release.md` 提到的)

如果你已经维护了 brew tap (`Formula/tui-majo.rb`),发新版前:

1. 把 formula 重命名 `Formula/kage.rb`
2. 内容里把 `class TuiMajo < Formula` → `class Kage < Formula`
3. install 块的 bin 名跟 1.5 保持一致

老的 `Formula/tui-majo.rb` 保留(指向最后一个 tui-majo release),给老用户兜底。

### 5.3 winget 提交

按 5.1 (1) 方案,首次发 `XuanLeeHEALER.kage` 时 winget-releaser 会自动建 PR 到 microsoft/winget-pkgs;manifest 模板会从老 identifier 复用,核对一下 PackageName / Publisher 字段。

### 5.4 README 的 keywords / Cargo.toml description

每个 crate 的 description 在 1.1 内被改名命令带过,再走一遍确认:

```bash
grep -n "^description" crates/*/Cargo.toml
# crates/kage-core/Cargo.toml:description = "Riichi mahjong core: engine, AI, mental poker, network protocol."
# crates/kage-tui/Cargo.toml:description = "Riichi Mahjong (日本麻将) in your terminal."
# crates/kage-web/Cargo.toml:description = "Self-hosted web frontend for kage (placeholder)."
```

注意 `web-majo` description 里如有 `for majo` 文字要改成 `for kage`,sed 漏网。

### 5.5 截图 / 海报 / 设计资源

`docs/img/screenshot.png` 等图片**文件名内**没有 `tui-majo`,内容也无需重拍。如果你海报 / 社媒物料里印了 "tui-majo" 文字,自行更新。

### 5.6 release notes

打 v2.2.0 tag 前在 GitHub release draft 顶端醒目位置加:

```markdown
## ⚠️ Breaking: 项目改名 tui-majo → kage

- 仓库 URL: github.com/XuanLee-HEALER/kagetsu → github.com/XuanLee-HEALER/kage (301 redirect 自动跳转)
- crates.io: `tui-majo` (deprecated) → `kage-tui` + `kage-core`
- 安装命令: `cargo install tui-majo` → `cargo install kage-tui`
- 启动命令: `tui-majo` → `kage`
- log 文件: `%TEMP%/tui-majo.log` → `%TEMP%/kage.log`(如改了)

老用户迁移:
\`\`\`bash
cargo uninstall tui-majo
cargo install kage-tui
\`\`\`
```

---

## 6. 完成判据

全部跑通才算改名完成:

- [ ] `cargo search kage-core` / `kage-tui` 能搜到自己刚发的版本
- [ ] `cargo install kage-tui` 在干净环境(`~/.cargo/bin/kage` 不存在)能装上,`kage --version` 输出 2.2.0(或你的新 tag 版本)
- [ ] `gh repo view XuanLee-HEALER/kage` 显示新仓库,老 URL `XuanLee-HEALER/tui-majo` 浏览器访问自动 301 到新地址
- [ ] `cargo yank --list tui-majo` 显示 v2.1.0 已 yanked
- [ ] (如发了 placeholder)`cargo install tui-majo` 装上后跑会打印 deprecation 提示
- [ ] CI 跑通 v2.2.0 tag 的 release workflow,artifact 命名为 `kage-2.2.0-x86_64-unknown-linux-gnu.tar.gz` 等
- [ ] README, justfile, docs/* 里 `grep -rn "tui-majo\|tui_majo" .` 只在以下地方残留:
  - `CHANGELOG` / `docs/release.md` 历史段落
  - `docs/refactor/workspace-plan.md`(本次拆分计划,作为档案保留)
  - 本文件 `docs/refactor/rename-todo.md`(自我引用 + 历史说明)
- [ ] `cargo test --workspace` 全绿,包括 doctest
- [ ] 至少一个用户(可以是自己另一台机器)能从干净环境装上新版并跑通 `kage` 启动

---

## 7. 不可逆操作 callouts

| 操作 | 不可逆性 | 建议 |
|---|---|---|
| `cargo publish` | crates.io 永远不能删 version,只能 yank | 先 `--dry-run` |
| `cargo yank` | yank 可 `--undo`,但已经看到 yank 状态的下游 `Cargo.lock` 不会回滚 | 可逆,放心做 |
| `gh repo rename` | 301 redirect 自动建立,改回去也是 301,但来回切换会让用户混乱 | 一次性,改完别折腾 |
| `git remote set-url` | 纯本地,随时改 | 无风险 |
| 全文 sed | 改坏一片代码 | `git diff` 仔细审,挂了 `git checkout -- file` 局部撤回 |
| 删除 / 重命名 `Cargo.lock` | 锁文件重生,本地依赖版本会更新 | 改名 PR 内做一次即可,别频繁 |
