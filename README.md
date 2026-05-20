# kagetsu

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

<p align="center">
  <img src="hero.svg" alt="kagetsu — 純正九蓮宝燈" width="760">
</p>

> 工作摸鱼专用

**kagetsu** 是一个日本立直麻将实现:核心是一套**纯函数式计算引擎**,外面包终端(TUI)与自托管网页两种前端。local-first —— 可单机离线玩,也能在局域网、或经零信任协议跨互联网与真人对局。

> 📖 其他语言:[English](README.en.md) · [日本語](README.ja.md)

**一句话亮点:**

- 🀄 **纯函数式引擎** —— 规则 / 役 / 算分 / 状态机全是纯函数,无隐藏可变状态
- 🖥️ **TUI 优先** —— ratatui 终端界面,全角中文牌,纯快捷键,自动适配现代终端
- 🌐 **自托管 WebUI** —— Docker 一键起,或 `cargo run`,无中心服务器
- 🔒 **零信任联机** —— 4 人 mental poker 协议对等开局,不需要信任房主
- 📡 **局域网 / 互联网** —— mDNS 自动发现 + QUIC 低延迟传输 + NAT 穿透

## 截图

![终端界面](screenshot.png)

## 细节

### 纯函数式计算引擎

整局对局其实就是事件流上的一次 fold:从初始状态出发,每个事件都经纯函数 `f(state, event) -> state` 折叠推进,全程没有隐藏的可变状态。带来三个直接好处:

- **易测** —— 任意状态转移都能直接断言,算法层 403 个单元测试
- **可回放 / 存档** —— 任意时刻状态可序列化;支持 F5/F9 快速存档与天凤 mjlog 重放
- **确定性** —— 同庄种子 + 同操作必得同结果,便于复盘

设计文档见 [`docs/design/pure-functional-refactor.md`](docs/design/pure-functional-refactor.md)。

### 完整的立直规则

基于比赛规则(WRC 2022 主基):

- 半庄战 / 东风战,uma + oka 终局结算,头跳 / 双家荣 / 三家荣可配
- 全部标准役(1-6 番)+ 全部役满 + 古役(默认关闭,可单独开)
- 食断 / 赤宝牌 / 一发 / 里宝牌 / 西入 / 击飞等细则可配

**真实牌谱验证**:10 局天凤 mjlog 解析 → 重放,99 局的 fu / han / yaku 全部对齐 mjx-project 标准。

### ZeroTrust:零信任 mental poker 联机

v2.0 起支持 ZeroTrust 模式 —— 4 个真人玩家用零信任的 mental poker 协议对等跑一手麻将。牌山由 4 方联合洗牌,谁都不知道完整牌序,每张牌通过门限 ElGamal 解密只让“该看到的人”看到。**不需要信任房主。**

协议 0–7 覆盖 keygen / 联合洗牌 / 摸牌 / 揭示 / 弃牌 / 鸣牌 / 暗杠 / 和牌全流程。底层 [ark-bls12-381](https://github.com/arkworks-rs/algebra) 椭圆曲线 + ChaCha20 RNG,所有 ZK 证明(DLEQ / Schnorr / cut-and-choose shuffle)走 Fiat-Shamir 非交互式。

> 约束:ZeroTrust 模式必须 4 个真人 —— AI 没有私钥,无法参与协议。

### 网络层

- **传输** —— QUIC + TCP 双栈,QUIC 低延迟优先
- **发现** —— 同 LAN 下 mDNS + gossipsub 自动发现房间,5 秒刷新
- **NAT 穿透** —— autonat 探测公网可达性,relay-server / dcutr 升级直连,让零信任对局能跨互联网进行
- **容错** —— 断线 30 秒内带 token 重连恢复座位;鸣牌按 Ron > Pon = Kan > Chi 头跳裁决

Standard 模式另提供房主权威架构 + 空座位 AI 补满。

## 项目结构

cargo workspace,三个 crate:

```text
kagetsu/
├── crates/
│   ├── kagetsu-core/   引擎 —— 规则 / 役 / 算分 / mental poker / 网络 / AI / 回放
│   ├── kagetsu/        终端前端(ratatui)
│   └── kagetsu-web/    网页前端(axum + svelte)
├── docs/               规则 spec / 设计文档
└── compose.yaml        web 自部署
```

| crate | 说明 | 文档 |
|---|---|---|
| [`kagetsu-core`](crates/kagetsu-core/README.md) | 纯函数式引擎,不依赖任何 UI,可独立作库引入 | 模块拆分 / 测试分层 |
| [`kagetsu`](crates/kagetsu/README.md) | 终端版,`cargo install kagetsu` 即装 | 键位 / 字体 / 配置 |
| [`kagetsu-web`](crates/kagetsu-web/README.md) | 自托管网页节点 | 部署 / 设计系统 |

## 部署 / 安装

### 终端版

```sh
cargo install kagetsu
```

或从 [Releases](https://github.com/XuanLee-HEALER/kagetsu/releases) 下载对应平台的二进制压缩包,解压即用。

推荐 WezTerm / kitty / Alacritty 等现代终端;终端字体需支持 **CJK 等宽**,否则全角牌会显示异常。键位、配置项、字体清单见 [kagetsu crate README](crates/kagetsu/README.md)。

### 网页版

自托管,无需中心服务器。

**Docker(推荐)** —— 在仓库根目录:

```sh
docker compose up
```

浏览器打开 <http://localhost:8080/>。或手动构建:

```sh
docker build -f crates/kagetsu-web/Dockerfile -t kagetsu-web .
docker run --rm -p 8080:8080 kagetsu-web
```

**cargo(开发)**:

```sh
cargo run -p kagetsu-web
```

> 网页端目前 serve 的是 SakyaHuman 设计原型,浏览器 ↔ 后端的 WebSocket 对局业务仍在开发中。进度见 [kagetsu-web README](crates/kagetsu-web/README.md)。

## 参与开发

欢迎提 [issue](https://github.com/XuanLee-HEALER/kagetsu/issues) 和 PR 一起来折腾 —— bug 反馈、规则细节修正、新役种、AI 改进、UI 调整都欢迎。

其中**新役种**这块还在打地基:新增一个役要走通定义、计分、合理性验证、测试四个环节,把它们收敛成一套稳定的接入接口仍是进行中的工作 —— 尤其欢迎一起来定这个模式。

```sh
just test    # 跑全部测试
just ci      # fmt + clippy + test
```

## 许可证

本项目以 [GPL-3.0-or-later](LICENSE) 分发。

依赖侧:全部依赖均为宽松许可证,与 GPL-3 兼容,由 [`deny.toml`](deny.toml) 持续校验;release 二进制随包附带 `cargo-about` 生成的第三方许可证汇总 `THIRD-PARTY-LICENSES.html`。
