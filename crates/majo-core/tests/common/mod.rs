//! testkit: 协议级集成测试公共 helper.
//!
//! 跳过 UI 层, 直接通过 [`majo_core::net::room::RoomHandle`] 与
//! [`majo_core::net::session::NetSession`] 模拟多 client. 比起开多个真实
//! binary, 这种方式快 100×, 状态断言更直接.
//!
//! ## 设计
//!
//! - **TestRoomBuilder**: 配置 nick / config / seed, 构造好房间.
//! - **TestRoom**: spawn 出来的房间 + 各 TestClient.
//! - **TestClient**: 包装一个 NetSession (实际上直接走 RoomHandle channel),
//!   缓存历史消息 + latest_view.
//!
//! ## 决定性
//!
//! 测试默认用 [`DEFAULT_SEED`] 注入 [`spawn_room_with_seed`], 让牌山可
//! 复现. AI 决策本身无随机, 配合 seed 整局可复现.

#![allow(dead_code)] // 各 test file 用其中子集, 全允许

use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::sync::oneshot;
use uuid::Uuid;

use majo_core::engine::domain::meld::Seat;
use majo_core::engine::domain::tile::{Tile, TileIndex};
use majo_core::engine::event::GameEvent;
use majo_core::engine::phase::Phase;
use majo_core::engine::rules::GameRules;
use majo_core::net::protocol::{ClientMsg, GameStateView, NetAction, RoomView, ServerMsg};
use majo_core::net::room::{RoomCmd, RoomHandle, spawn_room_advanced};
use majo_core::net::protocol::TileSpec;

/// 默认测试 seed; 改这里影响所有未指定 seed 的用例.
pub const DEFAULT_SEED: u64 = 0x1234_5678_DEAD_BEEF;

/// drain 等待时长上限 (单次 test step 内).
pub const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

// ============================================================================
// Builder
// ============================================================================

pub struct TestRoomBuilder {
    nicks: Vec<String>,
    config: GameRules,
    seed: u64,
    /// drain 时检测到 AwaitCalls 鸣牌窗口的 ActionRequired 自动发 Pass.
    /// 默认 true. calls 场景需主动响应时设 false.
    auto_pass_calls: bool,
    /// 鸣牌窗口超时 ms. 默认 None = server 用 2500ms. 测试可缩短.
    call_window_ms: Option<u64>,
}

impl Default for TestRoomBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestRoomBuilder {
    pub fn new() -> Self {
        Self {
            nicks: vec!["Host".into()],
            config: GameRules::default(),
            seed: DEFAULT_SEED,
            auto_pass_calls: true,
            call_window_ms: None,
        }
    }

    pub fn auto_pass_calls(mut self, b: bool) -> Self {
        self.auto_pass_calls = b;
        self
    }

    pub fn call_window_ms(mut self, ms: u64) -> Self {
        self.call_window_ms = Some(ms);
        self
    }

    /// 真人玩家数 (1-4). 自动用 Host/Alice/Bob/Carol 命名.
    pub fn humans(mut self, count: usize) -> Self {
        let names = ["Host", "Alice", "Bob", "Carol"];
        assert!(
            (1..=4).contains(&count),
            "humans count must be 1..=4, got {count}"
        );
        self.nicks = names.iter().take(count).map(|s| s.to_string()).collect();
        self
    }

    pub fn nicks(mut self, nicks: Vec<String>) -> Self {
        self.nicks = nicks;
        self
    }

    pub fn config(mut self, c: GameRules) -> Self {
        self.config = c;
        self
    }

    pub fn seed(mut self, s: u64) -> Self {
        self.seed = s;
        self
    }

    /// spawn 房间 + 所有 humans 加入. 返回 lobby 状态的 TestRoom.
    pub async fn build_lobby(self) -> TestRoom {
        let handle = spawn_room_advanced(
            self.nicks[0].clone(),
            self.config.clone(),
            Some(self.seed),
            self.call_window_ms,
        );
        let mut clients = Vec::new();
        for nick in &self.nicks {
            let mut c = TestClient::join(handle.clone(), nick.clone()).await;
            c.auto_pass_calls = self.auto_pass_calls;
            clients.push(c);
        }
        let mut room = TestRoom {
            handle,
            clients,
            seed: self.seed,
            auto_pass_calls: self.auto_pass_calls,
        };
        room.drain_all().await;
        room
    }

    /// build_lobby + 让所有非 host 真人 ready + host 发 StartGame.
    /// 等到所有 client 至少看到一个 GameStateView 才返回.
    pub async fn start_game(self) -> TestRoom {
        let mut room = self.build_lobby().await;
        for c in room.clients.iter_mut().skip(1) {
            c.send(ClientMsg::Ready { ready: true });
        }
        room.drain_all().await;
        room.host().send(ClientMsg::StartGame);
        // 等所有 clients 都拿到 GameStateView
        let _ = room
            .wait_for_all(|c| c.latest_view.is_some(), Duration::from_millis(500))
            .await;
        room
    }
}

// ============================================================================
// TestRoom
// ============================================================================

pub struct TestRoom {
    pub handle: RoomHandle,
    pub clients: Vec<TestClient>,
    pub seed: u64,
    pub auto_pass_calls: bool,
}

impl TestRoom {
    pub fn builder() -> TestRoomBuilder {
        TestRoomBuilder::new()
    }

    pub fn host(&mut self) -> &mut TestClient {
        &mut self.clients[0]
    }

    pub fn client(&mut self, idx: usize) -> &mut TestClient {
        &mut self.clients[idx]
    }

    /// 拉所有 clients 的 inbox, 反复 yield + sleep 直到没有新消息.
    pub async fn drain_all(&mut self) {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        let mut idle_rounds = 0;
        while Instant::now() < deadline {
            let mut got = 0usize;
            for c in &mut self.clients {
                got += c.drain();
            }
            if got == 0 {
                idle_rounds += 1;
                if idle_rounds >= 3 {
                    return;
                }
            } else {
                idle_rounds = 0;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// 等所有 clients 满足 cond 或超时.
    pub async fn wait_for_all<F: Fn(&TestClient) -> bool>(
        &mut self,
        cond: F,
        timeout: Duration,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            for c in &mut self.clients {
                c.drain();
            }
            if self.clients.iter().all(&cond) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// 找到 turn=East/AwaitDiscard 时执行 action 然后 drain. 等价 host 切牌.
    pub async fn host_discard_first(&mut self) {
        // 等 host 看到自己回合
        let _ = self
            .host()
            .await_view(
                |v| v.turn == v.my_seat && v.phase == Phase::AwaitDiscard,
                Duration::from_secs(1),
            )
            .await
            .expect("host should be on turn");
        let view = self.host().latest_view.clone().unwrap();
        let (sel, _) = split_hand_sorted(&view);
        let kind = sel.first().or(view.my_last_drawn.as_ref()).unwrap().kind;
        self.host()
            .send(ClientMsg::Action(NetAction::Discard(TileSpec { kind })));
        self.drain_all().await;
    }
}

// ============================================================================
// TestClient
// ============================================================================

pub struct TestClient {
    pub player_id: u32,
    pub token: Uuid,
    pub nickname: String,
    rx: UnboundedReceiver<ServerMsg>,
    handle: RoomHandle,

    pub history: Vec<ServerMsg>,
    pub latest_view: Option<GameStateView>,
    pub latest_room: Option<RoomView>,
    pub last_action_required: Option<(Vec<NetAction>, i64)>,
    /// drain 时如果当前 latest_view.phase == AwaitCalls 且收到 ActionRequired,
    /// 自动发 Pass. 大多数测试只关心切牌流, 不响应鸣牌.
    pub auto_pass_calls: bool,
}

impl TestClient {
    /// 直接走 RoomHandle.tx 发 Join cmd, 等 ack. 不经 ws.
    pub async fn join(handle: RoomHandle, nickname: String) -> Self {
        Self::join_with_token(handle, nickname, None).await
    }

    pub async fn join_with_token(
        handle: RoomHandle,
        nickname: String,
        reconnect_token: Option<Uuid>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<ServerMsg>();
        let (ack_tx, ack_rx) = oneshot::channel();
        handle
            .tx
            .send(RoomCmd::Join {
                nickname: nickname.clone(),
                reconnect_token,
                sender: tx,
                ack: ack_tx,
            })
            .expect("send Join");
        let result = ack_rx.await.expect("ack").expect("join ok");
        Self {
            player_id: result.player_id,
            token: result.reconnect_token,
            nickname,
            handle,
            rx,
            history: Vec::new(),
            latest_view: None,
            latest_room: Some(result.room),
            last_action_required: None,
            auto_pass_calls: true,
        }
    }

    pub fn send(&self, msg: ClientMsg) {
        let _ = self.handle.tx.send(RoomCmd::PlayerMsg {
            player_id: self.player_id,
            msg,
        });
    }

    /// 拉所有可读消息进 history, 返回新增数量. 如果 [`auto_pass_calls`] 开启,
    /// 在 phase=AwaitCalls 收到 ActionRequired 时自动 send Pass.
    pub fn drain(&mut self) -> usize {
        let mut n = 0;
        let mut should_auto_pass = false;
        while let Ok(msg) = self.rx.try_recv() {
            match &msg {
                ServerMsg::GameStateView(v) => {
                    self.latest_view = Some((**v).clone());
                }
                ServerMsg::Welcome { room, .. } => {
                    self.latest_room = Some((**room).clone());
                }
                ServerMsg::RoomUpdate(room) => {
                    self.latest_room = Some((**room).clone());
                }
                ServerMsg::ActionRequired {
                    hints,
                    deadline_unix_ms,
                } => {
                    self.last_action_required = Some((hints.clone(), *deadline_unix_ms));
                    // 如果 hints 包含 Pass (= 这是鸣牌窗口) 且 auto_pass 开启, 标记
                    if self.auto_pass_calls && hints.iter().any(|h| matches!(h, NetAction::Pass)) {
                        should_auto_pass = true;
                    }
                }
                _ => {}
            }
            self.history.push(msg);
            n += 1;
        }
        if should_auto_pass {
            self.send(ClientMsg::Action(NetAction::Pass));
        }
        n
    }

    /// 等 latest_view 满足 cond.
    pub async fn await_view<F: Fn(&GameStateView) -> bool>(
        &mut self,
        cond: F,
        timeout: Duration,
    ) -> Option<GameStateView> {
        let deadline = Instant::now() + timeout;
        loop {
            self.drain();
            if let Some(v) = &self.latest_view
                && cond(v)
            {
                return Some(v.clone());
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// 等 latest_room 满足 cond.
    pub async fn await_room<F: Fn(&RoomView) -> bool>(
        &mut self,
        cond: F,
        timeout: Duration,
    ) -> Option<RoomView> {
        let deadline = Instant::now() + timeout;
        loop {
            self.drain();
            if let Some(r) = &self.latest_room
                && cond(r)
            {
                return Some(r.clone());
            }
            if Instant::now() >= deadline {
                return None;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    pub async fn await_my_turn(&mut self, timeout: Duration) -> GameStateView {
        self.await_view(
            |v| v.turn == v.my_seat && v.phase == Phase::AwaitDiscard,
            timeout,
        )
        .await
        .unwrap_or_else(|| {
            panic!(
                "[{}] await_my_turn timeout. last view: turn={:?} phase={:?}",
                self.nickname,
                self.latest_view.as_ref().map(|v| v.turn),
                self.latest_view.as_ref().map(|v| v.phase),
            )
        })
    }

    pub fn discard(&self, kind: TileIndex) {
        self.send(ClientMsg::Action(NetAction::Discard(TileSpec { kind })));
    }

    pub fn discard_idx(&self, idx: usize) {
        let view = self.latest_view.as_ref().expect("有 view");
        let (sel, drawn) = split_hand_sorted(view);
        let t = sel
            .get(idx)
            .copied()
            .or(drawn)
            .expect("discard idx out of range");
        self.send(ClientMsg::Action(NetAction::Discard(TileSpec {
            kind: t.kind,
        })));
    }

    pub fn tsumogiri(&self) {
        let view = self.latest_view.as_ref().expect("有 view");
        let drawn = view.my_last_drawn.expect("无 last_drawn");
        self.send(ClientMsg::Action(NetAction::Discard(TileSpec {
            kind: drawn.kind,
        })));
    }

    pub fn riichi(&self, kind: TileIndex) {
        self.send(ClientMsg::Action(NetAction::Riichi(TileSpec { kind })));
    }

    pub fn tsumo(&self) {
        self.send(ClientMsg::Action(NetAction::Tsumo));
    }

    pub fn pon(&self) {
        self.send(ClientMsg::Action(NetAction::Pon));
    }

    pub fn chi(&self, idx: usize) {
        self.send(ClientMsg::Action(NetAction::Chi(idx)));
    }

    pub fn minkan(&self) {
        self.send(ClientMsg::Action(NetAction::Minkan));
    }

    pub fn ankan(&self, kind: TileIndex) {
        self.send(ClientMsg::Action(NetAction::Ankan(kind)));
    }

    pub fn pass(&self) {
        self.send(ClientMsg::Action(NetAction::Pass));
    }

    pub fn next_round(&self) {
        self.send(ClientMsg::Action(NetAction::NextRound));
    }

    pub fn leave(&self) {
        self.send(ClientMsg::Leave);
    }

    pub fn ready(&self, b: bool) {
        self.send(ClientMsg::Ready { ready: b });
    }

    /// 当前 latest_view 中的 events (server 推过来的).
    pub fn events(&self) -> Vec<GameEvent> {
        self.latest_view
            .as_ref()
            .map(|v| v.events.clone())
            .unwrap_or_default()
    }

    /// 我自己的座位 (start_game 后才有效).
    pub fn my_seat(&self) -> Option<Seat> {
        self.latest_view.as_ref().map(|v| v.my_seat)
    }

    /// 历史中存在某 ServerMsg variant.
    pub fn has_msg<F: Fn(&ServerMsg) -> bool>(&self, pred: F) -> bool {
        self.history.iter().any(pred)
    }

    /// 强制断开 sender (drop rx 等价).
    /// 通过让 server 端 try_send 失败间接通知 disconnect.
    /// 注意: server 可能不会立即检测, 测试可能要 send Disconnect cmd.
    pub fn force_disconnect(&self) {
        let _ = self.handle.tx.send(RoomCmd::Disconnect {
            player_id: self.player_id,
        });
    }
}

// ============================================================================
// 工具
// ============================================================================

pub fn split_hand_sorted(view: &GameStateView) -> (Vec<Tile>, Option<Tile>) {
    if let Some(d) = view.my_last_drawn {
        let mut sel = Vec::with_capacity(view.my_hand.len());
        let mut extracted = false;
        for t in &view.my_hand {
            if !extracted && t.kind == d.kind && t.red == d.red {
                extracted = true;
                continue;
            }
            sel.push(*t);
        }
        sel.sort_by_key(|t| t.kind.0);
        (sel, Some(d))
    } else {
        let mut all = view.my_hand.clone();
        all.sort_by_key(|t| t.kind.0);
        (all, None)
    }
}

// ============================================================================
// 断言宏
// ============================================================================

/// 断言 client 历史中含给定 ServerMsg 模式 (按出现顺序, 中间可穿插其他消息).
#[macro_export]
macro_rules! expect_msgs {
    ($client:expr, [$($pat:pat),* $(,)?]) => {{
        let history = &$client.history;
        let mut iter = history.iter();
        $(
            let found = iter.any(|m| matches!(m, $pat));
            assert!(
                found,
                "[{}] expected msg {} not found in history.\n实际历史 ({} 条):\n{:#?}",
                $client.nickname,
                stringify!($pat),
                history.len(),
                history
            );
        )*
    }};
}

/// 断言 client 当前 latest_view.events 中含给定 GameEvent 模式 (按顺序).
#[macro_export]
macro_rules! expect_events {
    ($client:expr, [$($pat:pat),* $(,)?]) => {{
        let events = $client.events();
        let mut iter = events.iter();
        $(
            let found = iter.any(|e| matches!(e, $pat));
            assert!(
                found,
                "[{}] expected event {} not found in events.\n实际 ({} 条):\n{:#?}",
                $client.nickname,
                stringify!($pat),
                events.len(),
                events
            );
        )*
    }};
}

/// 断言 client.latest_view 字段 == expected.
#[macro_export]
macro_rules! assert_view_eq {
    ($client:expr, $field:ident, $expected:expr) => {{
        let view = $client
            .latest_view
            .as_ref()
            .unwrap_or_else(|| panic!("[{}] no latest_view", $client.nickname));
        assert_eq!(
            view.$field,
            $expected,
            "[{}] view.{} mismatch",
            $client.nickname,
            stringify!($field),
        );
    }};
}
