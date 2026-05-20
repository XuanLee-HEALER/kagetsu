# kagetsu

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

<p align="center">
  <img src="hero.svg" alt="kagetsu — 純正九蓮宝燈" width="760">
</p>

> 仕事中のサボり用。

**kagetsu** は日本のリーチ麻雀の実装です。中核に純粋関数型の計算エンジンを置き、その外側を端末(TUI)とセルフホスト型のウェブという二つのフロントエンドで包んでいます。ローカルファースト —— オフラインの一人打ちも、LAN 上で、あるいはゼロトラストプロトコル経由でインターネット越しに実在のプレイヤーとの対局もできます。

> 📖 他の言語: [中文](README.md) · [English](README.en.md)

**ハイライト:**

- 🀄 **純粋関数型エンジン** —— ルール / 役 / 点数計算 / 状態機械はすべて純粋関数、隠れた可変状態なし
- 🖥️ **TUI ファースト** —— ratatui の端末 UI、全角の漢字牌、キーボード操作のみ、モダンな端末に自動適応
- 🌐 **セルフホスト WebUI** —— Docker でワンコマンド起動、または `cargo run`、中央サーバー不要
- 🔒 **ゼロトラスト対局** —— 4 人のメンタルポーカープロトコルによる P2P 対局、ホストを信頼する必要なし
- 📡 **LAN / インターネット** —— mDNS 自動検出 + 低遅延の QUIC トランスポート + NAT 越え

## スクリーンショット

![端末画面](screenshot.png)

## 詳細

### 純粋関数型の計算エンジン

一局の対局は、実のところイベント列に対する一度の fold です。初期状態から出発し、各イベントを純粋関数 `f(state, event) -> state` で畳み込んで進めていく —— どこにも隠れた可変状態はありません。直接的な利点が三つ:

- **テストしやすい** —— 任意の状態遷移を直接アサートできる。アルゴリズム層に 403 個のユニットテスト
- **リプレイ / セーブ** —— 任意時点の状態をシリアライズ可能。F5/F9 のクイックセーブと天鳳 mjlog のリプレイに対応
- **決定性** —— 同じ局のシード + 同じ操作なら必ず同じ結果。検討に便利

設計ドキュメントは [`docs/design/pure-functional-refactor.md`](docs/design/pure-functional-refactor.md) を参照。

### 完全なリーチルール

競技ルール(WRC 2022 を主な基準)に基づく:

- 半荘戦 / 東風戦、ウマ + オカの終局精算、頭ハネ / ダブロン / トリプルロンを設定可能
- すべての標準役(1〜6 翻)+ すべての役満 + 古役(既定はオフ、個別に有効化可能)
- 喰い断 / 赤ドラ / 一発 / 裏ドラ / 西入 / トビなどの細則を設定可能

**実戦の牌譜による検証**: 天鳳 mjlog 10 局を解析 → リプレイし、99 局分の符 / 翻 / 役がすべて mjx-project の基準と一致。

### ZeroTrust: ゼロトラストなメンタルポーカー対局

v2.0 以降、kagetsu は ZeroTrust モードに対応します —— 4 人の実プレイヤーが、ゼロトラストのメンタルポーカープロトコルで一手の麻雀を P2P で進めます。牌山は 4 者が共同でシャッフルし、誰も牌の並び全体を知りません。各牌は閾値 ElGamal で復号され、「見るべき人」だけに見えます。**ホストを信頼する必要はありません。**

プロトコル 0〜7 が、鍵生成 / 共同シャッフル / ツモ / 公開 / 打牌 / 鳴き / 暗槓 / 和了 の全工程をカバー。基盤は [ark-bls12-381](https://github.com/arkworks-rs/algebra) 楕円曲線 + ChaCha20 RNG。すべての ZK 証明(DLEQ / Schnorr / cut-and-choose シャッフル)は Fiat-Shamir により非対話型です。

> 制約: ZeroTrust モードは 4 人の実プレイヤーが必須 —— AI は秘密鍵を持たず、プロトコルに参加できません。

### ネットワーク層

- **トランスポート** —— QUIC + TCP のデュアルスタック、低遅延の QUIC を優先
- **検出** —— 同一 LAN 上では mDNS + gossipsub で部屋を自動検出、5 秒ごとに更新
- **NAT 越え** —— autonat で公開到達性を探り、relay-server / dcutr で直接接続へ昇格。ゼロトラスト対局をインターネット越しに可能にする
- **耐障害性** —— 切断後 30 秒以内ならトークンで再接続して席へ復帰。鳴きは頭ハネ優先で Ron > Pon = Kan > Chi により裁定

Standard モードはさらに、ホスト権威型のアーキテクチャ + 空席を AI で補充する仕組みを提供します。

## プロジェクト構成

3 つの crate からなる cargo workspace:

```text
kagetsu/
├── crates/
│   ├── kagetsu-core/   エンジン —— ルール / 役 / 点数計算 / メンタルポーカー / ネットワーク / AI / リプレイ
│   ├── kagetsu/        端末フロントエンド (ratatui)
│   └── kagetsu-web/    ウェブフロントエンド (axum + svelte)
├── docs/               ルール spec / 設計ドキュメント
└── compose.yaml        ウェブのセルフホスト
```

| crate | 説明 | ドキュメント |
|---|---|---|
| [`kagetsu-core`](crates/kagetsu-core/README.md) | 純粋関数型エンジン。UI に依存せず、単体のライブラリとして利用可 | モジュール構成 / テスト階層 |
| [`kagetsu`](crates/kagetsu/README.md) | 端末版。`cargo install kagetsu` で導入 | キー操作 / フォント / 設定 |
| [`kagetsu-web`](crates/kagetsu-web/README.md) | セルフホスト型ウェブノード | デプロイ / デザインシステム |

## インストール / デプロイ

### 端末版

```sh
cargo install kagetsu
```

または [Releases](https://github.com/XuanLee-HEALER/kagetsu/releases) からお使いのプラットフォーム向けのバイナリアーカイブをダウンロードし、展開してください。

モダンな端末(WezTerm / kitty / Alacritty)を推奨します。端末フォントは **CJK 等幅**に対応している必要があります。さもないと全角牌が正しく表示されません。キー操作・設定項目・フォント一覧は [kagetsu crate の README](crates/kagetsu/README.md) を参照。

### ウェブ版

セルフホストで、中央サーバーは不要です。

**Docker(推奨)** —— リポジトリのルートで:

```sh
docker compose up
```

ブラウザで <http://localhost:8080/> を開きます。または手動でビルド:

```sh
docker build -f crates/kagetsu-web/Dockerfile -t kagetsu-web .
docker run --rm -p 8080:8080 kagetsu-web
```

**cargo(開発)**:

```sh
cargo run -p kagetsu-web
```

> ウェブフロントエンドは現在、SakyaHuman デザインのプロトタイプを配信しています。ブラウザ ↔ バックエンドの WebSocket 対局機能はまだ開発中です。進捗は [kagetsu-web の README](crates/kagetsu-web/README.md) を参照。

## 開発に参加する

issue と PR を歓迎します —— バグ報告、ルール細部の修正、新しい役、AI の改善、UI の調整、どれも歓迎です。

特に**新しい役**まわりはまだ土台づくりの段階です。役を一つ追加するには定義 / 点数計算 / 妥当性検証 / テストの四工程を通す必要があり、それらを安定した連携インターフェースへまとめること自体が進行中の作業です。このパターンづくりへの参加を特に歓迎します。

```sh
just test    # 全テストを実行
just ci      # fmt + clippy + test
```

## ライセンス

本プロジェクトは [GPL-3.0-or-later](LICENSE) で配布されています。

依存関係について: すべての依存は寛容なライセンスで GPL-3 と互換性があり、[`deny.toml`](deny.toml) によって継続的に検査されます。リリースバイナリには `cargo-about` が生成する第三者ライセンス一覧 `THIRD-PARTY-LICENSES.html` が同梱されます。
