# kagetsu

> [kagetsu](../../README.md) 项目的终端版 —— Rust + ratatui 的日本麻将 TUI。

全角中文牌张，纯快捷键操作，两套主题。规则、役种、多人模式说明见[主项目 README](../../README.md)。

## 特性

- **Action Modal**：按 `m` 弹浮窗，当前可选行动一目了然
- **多种吃法 picker**：手里同时有两种吃法时弹选择框，避免误吃
- **两套主题**：暗 / 亮，`T` 键循环切换；牌张分色（数字红、萬黑筒蓝索紫、中红發绿白蓝风牌黑），辨识度高
- **操作计时**：每步思考时长可配（10-60 秒 / 不限时），超时执行默认动作
- **终端自动适配**：跨平台优先 WezTerm / kitty / Alacritty，再 fallback 到 Windows Terminal / iTerm2 / Terminal.app；启动开新窗口跑游戏不污染当前终端，找不到合适终端则 inline 兜底

## 安装

[crates.io](https://crates.io/crates/kagetsu) 装：

```sh
cargo install kagetsu
```

或者从源码：

```sh
git clone https://github.com/XuanLee-HEALER/kagetsu.git
cd kagetsu
cargo install --path crates/kagetsu
```

二进制 `kagetsu` / `kagetsu-game` / `kagetsu-relay` 都会装到 `~/.cargo/bin/`。直接 `kagetsu` 启动。

或者用 [just](https://github.com/casey/just) 跑开发命令：

```sh
just play          # release 构建跑游戏 (推荐)
just play-inline   # 在当前终端跑 (不开新窗口)
just play-game     # 跳过 launcher 直接跑游戏内核
just dev           # debug 构建
just test          # 跑全部测试
just ci            # fmt + clippy + test
```

## 字体要求

终端字体需要支持 **CJK 等宽**，否则全角牌会显示成 `??` 或被截断。推荐：

- [Sarasa Mono SC](https://github.com/be5invis/Sarasa-Gothic)（中日文等宽，强推）
- [JetBrains Mono](https://www.jetbrains.com/lp/mono/) + 系统中文 fallback
- macOS 自带 SF Mono / 中文回退 PingFang
- Windows Terminal 默认 Cascadia Mono PL 也行

## 操作

### 单人 / Standard 模式游戏内

| 键 | 动作 |
|---|---|
| `←` / `→` | 选手牌（同 kind 牌联动高亮，方便看河里出过几张） |
| `1`-`9` | 直接选第 N 张 |
| `Enter` / `D` | 切选中牌 |
| `T` | 摸切（切刚摸的那张） |
| `R` | 立直（切选中牌成立直） |
| `K` | 暗杠 / 加杠 |
| `W` | 自摸 / 荣和 |
| `P` / `A` / `M` | 碰 / 吃 / 明杠（响应他家弃牌；多种吃法时弹 picker） |
| `C` | 跳过响应 |
| `N` | 下一局 / 整庄结束按这个走完 |
| `m` | 唤起 Action Modal |

### ZeroTrust 模式游戏内

| 键 | 动作 |
|---|---|
| `D` | 摸下一张 |
| `Space` / `Enter` | 弃 cursor 牌 |
| `R` | 揭示下一张 dora |
| `C` | 吃 |
| `P` | 碰 |
| `K` | 明杠 |
| `X` | 加杠（升级已碰 Pon → Kan） |
| `A` | 暗杠 |
| `I` | 立直 |
| `T` | 自摸 |
| `N` | 荣和 |
| `←/→` (`h/j`) | 移动手牌 cursor |
| `Esc` / `L` | 离开 |

### 全局键

| 键 | 动作 |
|---|---|
| `T`（大写） | 切换主题（暗 ↔ 亮） |
| `Esc` | 回主菜单（除主菜单外） |
| `Q` | 退出 |

## 配置

游戏前的配置页可调：

- 赛制（半庄战 / 东风战）
- 食断 / 赤宝牌 / 一发 / 里宝牌 / 数役满 / 双倍役满
- 多家荣和（头跳 / 双家荣 / 三家荣）
- 西入 / 击飞
- 古役（master + 7 项细分）
- 起始/目标点棒（默认 25000 / 30000）
- Uma 4 种预设
- 思考时长（10/20/30/60 秒 / 不限时）
- 主题（暗/亮）
- 庄种子（随机 / 3 个固定预设，用于复盘）

## 终端尺寸

需要至少 **144 × 40** 字符。启动时会尝试 `SetSize(144, 40)` 让终端自动放大。不够就显示提示屏，拉大窗口自动恢复。
