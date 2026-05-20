# kagetsu-core

[![CI](https://github.com/XuanLee-HEALER/kagetsu/actions/workflows/ci.yml/badge.svg)](https://github.com/XuanLee-HEALER/kagetsu/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/XuanLee-HEALER/kagetsu/branch/main/graph/badge.svg)](https://codecov.io/gh/XuanLee-HEALER/kagetsu)

> [kagetsu](../../README.md) 项目的麻将引擎核心。

日本麻将的规则引擎、AI、mental poker 协议与网络层 —— 不依赖任何 UI，纯算法 + 网络。kagetsu 的两个 UI（[`kagetsu`](../kagetsu) 终端版、[`kagetsu-web`](../kagetsu-web) 网页版）都建在它之上，也可作为独立库单独引入。

## 模块

```text
src/
├── domain/         牌 / 副露 / 役 / 算分 / 拆解
├── engine/         游戏状态机 / 规则 / 牌山 / 事件
├── mental_poker/   ZeroTrust 协议 0-7 + Sako-Killian shuffle + ElGamal/DLEQ/Schnorr
├── net/
│   ├── room.rs     RoomActor（Standard 房主权威 + ZeroTrust 路由）
│   ├── session.rs  NetSession（统一 client 抽象 + mp 边带）
│   ├── mp/         MpPlayerActor（ZeroTrust 协议状态机）
│   └── p2p/        libp2p swarm + mp_bridge + mp_swarm
├── ai/             AI 决策（Standard 模式空座位补）
└── replay/         天凤 mjlog 解析 + 重放
```

## 特性开关

| feature | 说明 |
|---|---|
| `dev-tools` | F5/F9 快速存档 + replay 录制。release 不带保持纯净。 |

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
