# kagetsu-web

> [kagetsu](../../README.md) 项目的网页版节点。

自部署 web 节点 —— browser 通过 WebSocket 连本地 backend,跨节点对局
复用 `kagetsu_core::net` (libp2p) 加入同一个 P2P 网络。规则、役种、
多人模式说明见[主项目 README](../../README.md)。

## 当前状态 (workspace-refactor 分支)

**仅启动阶段**:axum HTTP server + 静态 serve 设计原型。WebSocket
gateway / 业务路由尚未接入。

## 跑

```bash
cargo run -p kagetsu-web
# 浏览器打开 http://localhost:8080/
```

支持的环境变量:

| 变量 | 默认 | 说明 |
|---|---|---|
| `WEB_MAJO_ADDR` | `0.0.0.0:8080` | listen 地址 |
| `WEB_MAJO_STATIC` | `CARGO_MANIFEST_DIR/static` | 静态目录 (部署场景外置) |
| `RUST_LOG` | `info,tower_http=debug` | 日志过滤 |

## 目录

```
crates/kagetsu-web/
├── Cargo.toml
├── README.md            (本文档)
├── src/
│   └── main.rs          axum server + ServeDir
└── static/              SakyaHuman 设计原型
    ├── index.html       入口: DesignCanvas + 10 个 artboards
    ├── colors_and_type.css   设计 token (颜色/字体/间距)
    ├── design-canvas.jsx     画布容器: zoom + section + artboard
    ├── core.jsx              共享组件
    ├── game-screen.jsx       主对局 (Normal / Command / Action modal)
    ├── action-modal.jsx      鸣牌/和了 modal
    ├── pregame.jsx           主菜单 + pre-game 配置
    ├── multiplayer.jsx       LAN 大厅 + 房间等待
    ├── results.jsx           和了结算 + 终局
    ├── zerotrust.jsx         零信任 mental poker 对局
    └── tiles/                40 个牌面 SVG (m/p/s/字, 含 dora)
```

## 设计系统

**SakyaHuman / Dorje** (单一深色主题,藏式唐卡矿物色板):
- 颜色 token 见 `static/colors_and_type.css` (`--sakya-p-*` 原色 +
  `--bg-*` / `--fg-*` / `--accent-*` 语义层)
- 字体: IBM Plex Sans / Serif / Mono (+ SC + Tibetan)
- 字号: Apple HIG 风格, 11px 起到 108px display

原型是 React + Babel inline (浏览器端编译),仅用于设计验证;不是
生产代码。

## Roadmap

### 当前 (此 commit)
- [x] axum + tower-http ServeDir
- [x] 静态 serve 设计原型,浏览器可看完整 10 屏

### 下一阶段: svelte 翻译
- [ ] 在 `crates/kagetsu-web/frontend/` 起 Vite + Svelte 工程
- [ ] 翻译 10 个 jsx 组件 (game-screen / action-modal / pregame /
      multiplayer / results / zerotrust) 到 svelte 组件
- [ ] 复用 `colors_and_type.css` 作 svelte global style
- [ ] vite build 产物落到 `static/`,替换原型
- [ ] tracking: 见 task list 中 "把 kagetsu-web 原型转 svelte"

### 再之后: 真业务
- [ ] WebSocket endpoint (`/ws`): browser ↔ backend 协议
- [ ] backend 内嵌 `kagetsu_core::net::session::NetSession`,
      跟 libp2p P2P 网通信
- [ ] HTTP REST: 大厅 / 房间列表 / 历史回放
- [ ] auth / session: 用 `kagetsu_core::config::LocalPrefs` 持久身份

## 安全

`ServeDir` 仅 serve `static/` 内的文件,不暴露文件系统其它位置。
但当前未做以下保护(后续要补):
- CORS 限制 (browser 跨域调 ws)
- TLS (`axum-server` + rustls)
- 速率限制
- CSP header

本机自部署场景 (`localhost`) 这些都不紧迫;开公网监听时再补。
