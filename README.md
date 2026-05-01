# tui-majo · 终端日本麻将

> 工作摸鱼专用。一个终端窗口，一局麻将，老板看过来就 `Ctrl+L` 假装清屏。

基于比赛规则（WRC 2022 主基）的日麻 TUI 实现，Rust + ratatui 写的。全角中文牌张，vim 风格命令行驱动，三套主题。

## 截图

![tui-majo 截图](https://raw.githubusercontent.com/XuanLee-HEALER/tui-majo/master/docs/img/screenshot.png)

## 特性

- **比赛规则**：半庄战 / 东风战，头跳 / 双家荣 / 三家荣 可配，uma + oka 终局结算
- **完整役种**：全部标准役（1-6 番）+ 全部役满 + 古役（默认关闭，可单独打开）
- **全角中文牌**：一萬 二筒 三索 東南西北白發中，等宽字体下视觉舒服
- **vim 命令模式**：`:` 进命令行，Tab 补全，唯一前缀 ghost text 提示，`:discard 5p` `:riichi 4m` `:tsumo` 等
- **Action Modal**：按 `m` 弹浮窗，当前可选行动一目了然
- **三套主题**：暗 / 亮 / 单色，任意 `T` 键循环切换
- **操作计时**：每步思考时长可配（10-60 秒 / 不限时），超时执行默认动作
- **自动适配终端**：Windows 走 `wt.exe`，macOS 优先 iTerm2（fallback Terminal.app），启动开新窗口跑游戏不污染当前终端

## 多人游戏

🚧 **正在施工中**。当前是 1 人 + 3 AI。多人 P2P 联机正在排期，暂时只能跟 AI 玩。

## 安装

需要 Rust 工具链（`rustup` + `cargo`）：

```sh
git clone https://github.com/XuanLee-HEALER/tui-majo.git
cd tui-majo
cargo install --path .
```

`tui-majo` 和 `tui-majo-game` 两个二进制都会装到 `~/.cargo/bin/`。直接 `tui-majo` 启动。

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

游戏界面（NORMAL 模式）：

| 键 | 动作 |
|---|---|
| `←` / `→` | 选手牌 |
| `1`-`9` | 直接选第 N 张 |
| `Enter` / `D` | 切选中牌 |
| `T` | 摸切（切刚摸的那张） |
| `R` | 立直（切选中牌成立直） |
| `K` | 暗杠 / 加杠 |
| `W` | 自摸 / 荣和 |
| `P` / `A` / `M` | 碰 / 吃 / 明杠（响应他家弃牌） |
| `C` | 跳过响应 |
| `N` | 下一局 / 整庄结束按这个走完 |
| `m` | 唤起 Action Modal |
| `:` | 进入 COMMAND 模式 |

COMMAND 模式（`:` 之后输入）：

| 命令 | 别名 | 动作 |
|---|---|---|
| `:discard <牌>` | `:d` | 切某张牌（`5p` / `p5` / `五筒`） |
| `:riichi <牌>` | `:r` | 立直 |
| `:tsumo` | `:t` | 自摸 |
| `:pon` / `:kan` / `:chi` | `:p` `:k` `:a` | 碰 / 杠 / 吃 |
| `:pass` | `:c` `:skip` | 跳过响应 |
| `:menu` | `:m` | 唤起 Action Modal |

Tab 键补全到唯一前缀，唯一匹配时光标后会显示灰色 ghost text 提示。

全局键：

| 键 | 动作 |
|---|---|
| `T`（大写） | 切换主题（暗 → 亮 → 单色） |
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
- 主题（暗/亮/单色）
- 庄种子（随机 / 3 个固定预设，用于复盘）

## 终端尺寸

需要至少 **144 × 40** 字符。启动时会尝试 `SetSize(144, 40)` 让终端自动放大。不够就显示提示屏，拉大窗口自动恢复。

## 规则参考

- [docs/spec/README.md](docs/spec/README.md) — 规则索引
- 来源：维基百科 EN/ZH，WRC 2022 规则，灰机 wiki / 凌上开花 wiki

## 路线图

- [x] 单人 vs 3 AI
- [x] 完整役种 + 役满
- [x] vim 命令模式 + Action Modal
- [x] 三套主题
- [x] 操作计时
- [x] 终局 uma + oka 结算
- [ ] 多人游戏（P2P 联机）
- [ ] 振听强制
- [ ] 立直牌横置渲染
- [ ] 中途流局（九种九牌 / 四风连打 / 四杠散了 / 四家立直）
- [ ] 牌谱回放
- [ ] 更聪明的 AI（当前是摸切 + 能和就和）

## 参与开发

欢迎提 [issue](https://github.com/XuanLee-HEALER/tui-majo/issues) 和 PR 一起来折腾。bug 反馈、规则细节修正、新役种、新功能、AI 改进、UI 调整都欢迎。

## 许可证

[GPL-3.0-or-later](LICENSE)
