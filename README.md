# tui-majo · 终端日本麻将

> 工作摸鱼专用。一个终端窗口，一局麻将，老板看过来就 `Ctrl+L` 假装清屏。

基于比赛规则（WRC 2022 主基）的日麻 TUI 实现，Rust + ratatui 写的。全角中文牌张，纯快捷键操作，两套主题。**v2.0 起支持 ZeroTrust 模式** —— 4 玩家用 libp2p mental poker 协议对等联机，无需信任房主。

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
- **自动适配终端**：跨平台优先 WezTerm / kitty / Alacritty，再 fallback 到 Windows Terminal / iTerm2 / Terminal.app；启动开新窗口跑游戏不污染当前终端，找不到合适终端则 inline 兜底

## 多人游戏

主菜单选 **局域网游戏** 进入大厅，房主创建房间时可选 **Standard** 或 **ZeroTrust** 模式。

### Standard 模式（房主权威）

房主即 server 的 P2P 架构：房主进程同时跑 libp2p swarm + 自己的 client，其他玩家通过 request-response 协议连进来。空缺座位自动补 AI。

- **mDNS + gossipsub 自动发现**：同 LAN 下其他人的房间会自动出现在大厅列表，5 秒刷新一次
- **NAT 穿透**：QUIC + TCP 双 transport，autonat 探测自己是否公网可达，relay-server / dcutr 升级直连
- **手动 multiaddr fallback**：mDNS 失效时直接粘贴 `/ip4/.../udp/.../quic-v1/p2p/...` 加入
- **空 slot AI 补满**：人不够 4 个，剩下的座位由 AI 接管
- **断线重连**：客户端拿到 `reconnect_token`，30 秒内重新连上恢复座位
- **鸣牌优先级裁决**：切牌后 500ms 收响应窗口，按 Ron > Pon=Kan > Chi 头跳裁决

### ZeroTrust 模式（P2P mental poker）

**v2.0 新增。** 4 个真人玩家用零信任的 mental poker 协议对等跑一手麻将 —— 牌山由 4 方联合洗牌，谁也不知道全部牌序，每张牌通过门限 ElGamal 解密只让"该看到的人"看到。**不需要信任房主**。

协议层实现：
- **协议 0** —— 4 方 keygen + Schnorr DLEQ 验证联合公钥
- **协议 1** —— Sako-Killian Cut-and-Choose 联合洗牌（K=80，Fiat-Shamir transcript）
- **协议 2** —— 摸牌（DrawShareRequest/Response + threshold 解密）
- **协议 3** —— 公开揭示（dora indicator）
- **协议 4** —— 弃牌（plaintext 公开广播）
- **协议 5** —— 鸣牌（吃 / 碰 / 明杠）
- **协议 6** —— 暗杠（选项 C：监督方反查 plaintext 验证 4 张同 kind）
- **协议 7** —— 和牌（Tsumo / Ron + ownership 验证）
- **加杠** —— 已碰刻子加自摸第 4 张升级 Kan

底层用 [ark-bls12-381](https://github.com/arkworks-rs/algebra) G1 椭圆曲线 + ChaCha20 RNG。所有 ZK 证明（DLEQ / Schnorr / cnc shuffle）走 Fiat-Shamir 非交互式。

UI 渲染：4 家弃牌池 / 自家手牌 cursor / 副露 / dora 指示 / 协议进度 / 事件日志 / 和牌详情（含 yaku 算分）。

约束：
- ZeroTrust 模式必须 4 个真人玩家（mental poker 协议无法跟 AI 协作 —— AI 没私钥）
- 单局耗时较 Standard 模式略长（联合洗牌 + cnc proof 验证需要 ~10s）

跑过完整 4 swarm libp2p 集成测试 + 16 个 ZeroTrust UI e2e 测试。

## 安装

[crates.io](https://crates.io/crates/tui-majo) 装：

```sh
cargo install tui-majo
```

或者从源码：

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

## 架构

```text
src/
├── domain/         # 牌 / 副露 / 役 / 算分 / 拆解
├── engine/         # 游戏状态机 / 规则 / 牌山 / 事件
├── mental_poker/   # ZeroTrust 协议 0-7 + Sako-Killian shuffle + ElGamal/DLEQ/Schnorr
├── net/
│   ├── room.rs     # RoomActor (Standard 房主权威 + ZeroTrust 路由)
│   ├── session.rs  # NetSession (统一 client 抽象 + mp 边带)
│   ├── mp/         # MpPlayerActor (ZeroTrust 协议状态机)
│   └── p2p/        # libp2p swarm + mp_bridge + mp_swarm
├── ui/             # ratatui 屏 + ZeroTrustGameState
├── ai/             # AI 决策 (Standard 模式空座位补)
└── replay/         # 天凤 mjlog 解析 + 重放
```

## 规则参考

- [docs/spec/README.md](docs/spec/README.md) —— 规则索引
- 来源：维基百科 EN/ZH，WRC 2022 规则，灰机 wiki / 凌上开花 wiki
- ZeroTrust 协议参考：Bayer-Groth shuffle、Sako-Killian cut-and-choose、threshold ElGamal

## 测试

- **算法回归**：`cargo test --lib` 403 个单元测试（牌型 / 役 / 符 / 分解 / 分数分配 / mental poker 协议 / actor 状态机 / UI 状态机）
- **真实牌谱验证**：`tests/replay/fixtures/` 下 10 局天凤 mjlog → 解析 → 重放 → 99 局 fu/han/yaku 全对齐 mjx-project 标准
- **协议级集成**：4 player 全流程（lobby / 鸣牌 / 断线 / e2e ws 双客户端）
- **ZeroTrust e2e** 16 个测试 4 层覆盖：
  - actor 协议 0-7 mpsc 直连一手 e2e
  - mp_bridge MockTransport 抽象层一手 e2e
  - SwarmTransport in-memory dispatcher 一手 e2e
  - **真 libp2p 4-swarm TCP localhost** 集成 e2e（噪声加密 + yamux 复用 + gossipsub mesh + rr_mp protocol）
  - UI ZeroTrustGameState 完整 gameplay e2e（摸 / 弃 / 吃碰杠 / 立直 / 自摸荣和 / 加杠 / 多回合 / yaku 算分）
- **Property-based fuzz**：`PROPTEST_CASES=1000` 跑 1000 局随机会话验证分数守恒等不变量

## 参与开发

欢迎提 [issue](https://github.com/XuanLee-HEALER/tui-majo/issues) 和 PR 一起来折腾。bug 反馈、规则细节修正、新役种、新功能、AI 改进、UI 调整都欢迎。

## 许可证

[GPL-3.0-or-later](LICENSE)
