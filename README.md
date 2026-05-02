# tui-majo · 终端日本麻将

> 工作摸鱼专用。一个终端窗口，一局麻将，老板看过来就 `Ctrl+L` 假装清屏。

基于比赛规则（WRC 2022 主基）的日麻 TUI 实现，Rust + ratatui 写的。全角中文牌张，纯快捷键操作，两套主题。

## 截图

![tui-majo 截图](https://raw.githubusercontent.com/XuanLee-HEALER/tui-majo/master/docs/img/screenshot.png)

## 特性

- **比赛规则**：半庄战 / 东风战，头跳 / 双家荣 / 三家荣 可配，uma + oka 终局结算
- **完整役种**：全部标准役（1-6 番）+ 全部役满 + 古役（默认关闭，可单独打开）
- **全角中文牌**：一萬 二筒 三索 東南西北白發中，等宽字体下视觉舒服
- **Action Modal**：按 `m` 弹浮窗，当前可选行动一目了然
- **多种吃法 picker**：手里同时有两种吃法时弹选择框，避免误吃
- **两套主题**：暗 / 亮，`T` 键循环切换；牌张分色（数字红、萬黑筒蓝索紫、中红發绿白蓝风牌黑），辨识度高
- **操作计时**：每步思考时长可配（10-60 秒 / 不限时），超时执行默认动作
- **自动适配终端**：Windows 走 `wt.exe`，macOS 优先 iTerm2（fallback Terminal.app），启动开新窗口跑游戏不污染当前终端

## 多人游戏（局域网）

主菜单选 **局域网游戏** 即可。架构是房主即 server 的 P2P：房主进程同时跑 axum WebSocket server + 自己的 client，其它玩家通过 ws 连进来。空缺座位自动补 AI。

- **mDNS 自动发现**：同 LAN 下其他人的房间会自动出现在大厅列表，5 秒刷新一次
- **手动 IP fallback**：mDNS 在企业 Wi-Fi / VLAN 隔离下失效时，直接输入 `192.168.1.5:port` 加入
- **空 slot AI 补满**：人不够 4 个，剩下的座位由 AI 接管，开局即可
- **断线重连**：客户端拿到 `reconnect_token`，30 秒内重新连上恢复座位
- **鸣牌优先级裁决**：切牌后 500ms 收响应窗口，按 Ron > Pon=Kan > Chi 头跳裁决
- **房主即 server**：房主退出 = 房间解散，其他人退出 = 回房间

跑过完整 4 人协议级集成测试 + 端到端 ws 双 client 连接。

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

游戏界面快捷键：

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

全局键：

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

## 规则参考

- [docs/spec/README.md](docs/spec/README.md) — 规则索引
- 来源：维基百科 EN/ZH，WRC 2022 规则，灰机 wiki / 凌上开花 wiki

## 路线图

- [x] 单人 vs 3 AI
- [x] 完整役种 + 役满
- [x] Action Modal + 同 kind 牌联动高亮 + 多吃法 picker
- [x] 两套主题 (暗/亮) + 牌张分色 (实物麻将经典配色)
- [x] 操作计时
- [x] 终局 uma + oka 结算
- [x] 局域网联机（mDNS 发现 + 手动 IP fallback + 断线重连 + AI 补位）
- [ ] 互联网联机 / 房间密码 / TLS
- [ ] 振听强制
- [ ] 立直牌横置渲染
- [ ] 中途流局（九种九牌 / 四风连打 / 四杠散了 / 四家立直）
- [ ] 牌谱回放
- [ ] 更聪明的 AI（当前是摸切 + 能和就和）

## 测试

- **算法回归**：`cargo test --lib` 71 个单元测试（役 / 符 / 分解 / 分数分配 / 网络协议）
- **真实牌谱验证**：tests/replay/fixtures/ 下 10 局天凤 mjlog → 解析 → 重放 → 99 局 fu/han/yaku 全对齐 mjx-project 标准
- **协议级集成**：testkit 框架跑 4-player 全流程（lobby / 鸣牌 / 断线 / e2e ws 双客户端）
- **Property-based fuzz**：`PROPTEST_CASES=1000` 跑 1000 局随机会话验证分数守恒等不变量

## 参与开发

欢迎提 [issue](https://github.com/XuanLee-HEALER/tui-majo/issues) 和 PR 一起来折腾。bug 反馈、规则细节修正、新役种、新功能、AI 改进、UI 调整都欢迎。

## 许可证

[GPL-3.0-or-later](LICENSE)
