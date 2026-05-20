# 发布流程

打 tag → CI 自动建 3 平台二进制 + 发 GitHub Release + 提 winget PR。
brew tap 走单独的 git push 流程 (你自己的 tap 仓库)。

## 一次性配置

发版前先把这些 secret 配好 (Settings → Secrets and variables → Actions):

### `WINGET_TOKEN`

提 winget-pkgs PR 用的 GitHub PAT。

1. https://github.com/settings/tokens 创建 **classic** PAT (不能用 fine-grained)。
2. 勾选 **`public_repo`** scope (够了, 不需要 `repo`)。
3. 命名建议 `winget-releaser-tui-majo`, expiry 1 年或 no expiration。
4. 复制 token, 在 repo Settings 加为 secret 命名 `WINGET_TOKEN`。
5. 用这个 PAT 的账号要先去 https://github.com/microsoft/winget-pkgs **fork** 一份。
   `vedantmgoyal2009/winget-releaser` 会 push 到那个 fork 然后开 PR。

### (可选) brew 自动 bump 不在本流程

brew tap 是你自己的 git 仓库, 发版后手动 push 即可。如果以后想自动化,
加一个 secret `HOMEBREW_TAP_TOKEN` (PAT, `repo` scope), 在 release.yml
末尾追加一个 job 用 `mislav/bump-homebrew-formula-action`。

## 每次发版的步骤

### 1. 更新版本号

`Cargo.toml` 的 `version` 字段必须和 tag 对应:

```toml
[package]
version = "2.1.0"   # 对应 tag v2.1.0
```

也更新 `Cargo.lock` (`cargo build` 自动)。

### 2. 跑本地 CI 检查

```pwsh
just ci   # fmt-check + clippy + test 全套
```

确保 main 是干净的, lock 已 commit。

### 3. 打 tag 推 origin

```bash
git tag v2.1.0
git push origin v2.1.0
```

正式版 tag 形如 `v2.1.0`。预发版用 `v2.1.0-rc1` / `v2.1.0-beta1` 之类
带 `-` 的形式, CI 自动判定为 prerelease 且**跳过 winget PR** (winget
不收预发版)。

### 4. 等 CI 跑完

GitHub Actions 上能看到 `Release` workflow 在跑。三平台 build 大概
4-6 分钟, release + winget PR 再 1-2 分钟。

完成后:
- GitHub Release 页面会出现新版本, 含 4 个 archive + 4 个 .sha256 sidecar:
  - `tui-majo-2.1.0-x86_64-unknown-linux-gnu.tar.gz`
  - `tui-majo-2.1.0-x86_64-apple-darwin.tar.gz`
  - `tui-majo-2.1.0-aarch64-apple-darwin.tar.gz`
  - `tui-majo-2.1.0-x86_64-pc-windows-msvc.zip`
- winget-pkgs 仓库会出现一个 PR 待 MS team review (1-2 个工作日, 首次提交可能更久)。

### 5. 更新 brew tap (手动)

新 macOS archive 的 SHA256 在 release 页 sidecar 文件里, 也可以本地算:

```bash
# 拿 arm64 macOS archive (Apple Silicon)
curl -L -o /tmp/tm-arm.tar.gz https://github.com/XuanLee-HEALER/kagetsu/releases/download/v2.1.0/tui-majo-2.1.0-aarch64-apple-darwin.tar.gz
shasum -a 256 /tmp/tm-arm.tar.gz

# x86_64 macOS archive (Intel)
curl -L -o /tmp/tm-x64.tar.gz https://github.com/XuanLee-HEALER/kagetsu/releases/download/v2.1.0/tui-majo-2.1.0-x86_64-apple-darwin.tar.gz
shasum -a 256 /tmp/tm-x64.tar.gz
```

把这两个 sha256 写进你 brew tap 的 `Formula/tui-majo.rb` (示例):

```ruby
class TuiMajo < Formula
  desc "Riichi Mahjong (日本麻将) in your terminal"
  homepage "https://github.com/XuanLee-HEALER/kagetsu"
  version "2.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/XuanLee-HEALER/kagetsu/releases/download/v2.1.0/tui-majo-2.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "<arm64 sha>"
    else
      url "https://github.com/XuanLee-HEALER/kagetsu/releases/download/v2.1.0/tui-majo-2.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "<x64 sha>"
    end
  end

  def install
    bin.install "tui-majo"
    bin.install "tui-majo-game"
    bin.install "tui-majo-relay"
  end

  test do
    assert_match "tui-majo", shell_output("#{bin}/tui-majo --version", 0)
  end
end
```

push 到你 tap 的 main 分支即生效, 用户 `brew upgrade tui-majo` 拿到新版。

## 回滚 / 重发

### tag 推错或 CI 跑挂了想重来

```bash
# 删本地 tag
git tag -d v2.1.0
# 删远程 tag
git push origin :refs/tags/v2.1.0
# 修代码 / 重新打 tag
git tag v2.1.0
git push origin v2.1.0
```

注意: GitHub Release 不会自动跟着删, 需要手动到 release 页面 delete。
winget PR 如果已经开了, 要去 winget-pkgs 那 close 掉再重新触发。

### 想撤回某个版本 (已发出)

1. 在 GitHub Release 页面把版本 mark as draft 或 delete (用户客户端
   不会自动删, 但 download URL 会 404)。
2. 让 brew tap 把 formula 回滚到上个版本, push。
3. 已合进 winget-pkgs 的没法撤, 只能开 PR 把 manifest 删掉, 风险自负。

实践上: **避免发出有问题的版本**。打 tag 前 `just ci` 必跑过。

## 故障排查

### winget PR 没出现

- 检查 `WINGET_TOKEN` secret 是否过期。
- 检查那个 PAT 的账号是否已经 fork 了 microsoft/winget-pkgs。
- 看 Actions 日志里 `winget-releaser` 步骤的输出, 通常会写明原因。
- tag 是预发版 (含 `-`) 时本就跳过, 是预期行为。

### build 失败 (某个 target)

- macOS aarch64 build 现在 `macos-latest` runner 默认就是 arm, 应该原生
  就过。x86_64 是 cross-compile, 出错通常是 link 步骤 — 看 cargo 日志。
- Linux glibc 兼容: ubuntu-latest 用的是较新 glibc, 老 Linux 系统可能
  跑不动。如果有用户反馈, 改用 `ubuntu-22.04` 或 `ubuntu-20.04` 锁低
  glibc 版本。

### Release 创建失败

- `softprops/action-gh-release` 需要 `contents: write` 权限, 已在
  workflow 顶层声明。
- 如果 tag 已存在但 release 不存在, 会创建新 release; 都存在会
  报错。删旧的或 force 模式 (workflow 里加 `make_latest: legacy`)。

## 加平台 / 加渠道

未来想覆盖更多:
- **Linux aarch64**: matrix 加一项, 用 `cross` 或 `cargo-zigbuild` 跨编。
- **Linux musl** (Alpine 友好): 加 `x86_64-unknown-linux-musl` target,
  装 musl 工具链。
- **AUR** (Arch 用户): 写 PKGBUILD 推到 AUR, 也是单独 git 仓库。
- **Cargo install** 已经天然支持 (`cargo install tui-majo` 从 crates.io
  拉源码编译), 发版步骤再加 `cargo publish`。
- **brew 自动 bump**: 在 release.yml 末尾 append `mislav/bump-homebrew-formula-action`
  job, 见上面 "一次性配置" 末尾。
