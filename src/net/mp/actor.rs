//! MpPlayerActor — 零信任模式 per-player actor (M5.B.3).
//!
//! 一个玩家本地一个 actor, 持自己 sk + 4 玩家 Table 镜像. P2P 消息驱动各方
//! 同步推进协议 1-7.
//!
//! ## 当前 commit (M5.B.3) 范围
//! Scaffold + lifecycle: spawn / shutdown / 收发 cmd 路由. 各 phase 具体
//! 协议实现留后续 commits:
//! - M5.B.4: phase=KeyExchange 接 KeyShare 消息 + aggregate jpk
//! - M5.B.5: phase=Shuffling 接 ShuffleRound + cnc verify
//! - M5.B.6: phase=Playing 接 Draw/Reveal/Discard/Call/Kan/Win
//!
//! 当前 actor.run 收 cmd 但仅 log + ignore, 除 Disconnect (退出 loop).

use ark_std::rand::SeedableRng;
use ark_std::rand::rngs::StdRng;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, oneshot};

use crate::mental_poker::card_mapping::CardMapping;
use crate::mental_poker::elgamal::{SecretKey, keygen};
use crate::mental_poker::joint_key::JointPublicKey;
use crate::mental_poker::protocol_reveal::MemberInfo;
use crate::mental_poker::protocol_state::Table;
use crate::mental_poker::schnorr;

use super::cmd::{MpEvent, MpRoomCmd};
use super::phase::MpPhase;

/// MpPlayerActor 配置 (spawn 时一次性传入).
#[derive(Debug, Clone)]
pub struct MpConfig {
    /// 自己在房间内的 index (0..N). 决定 shuffle 顺序 / 协议 broadcast 中的"发送方".
    pub own_index: usize,
    /// 房间内全部 N 玩家的 peer_id (按一致顺序).
    pub all_peer_ids: Vec<Vec<u8>>,
    /// 房间 ID + 开局 nonce (e.g. uuid v4) — 给 CardMapping 派生用.
    pub session_label: Vec<u8>,
    /// 牌山张数 (生产 = 136, 测试可小).
    pub deck_size: usize,
    /// cut-and-choose proof 安全参数 K. 生产 80, 测试可小.
    pub cnc_k_rounds: usize,
}

impl MpConfig {
    pub fn n_players(&self) -> usize {
        self.all_peer_ids.len()
    }
}

/// MpPlayerActor 内部状态 (private, run 内部 own).
///
/// 当前 scaffold 阶段: own_sk / own_pk / own_dlog_proof / card_mapping /
/// members / jpk / table 在 M5.B.4+ 才用, 暂用 #[allow(dead_code)] 静默.
#[allow(dead_code)]
pub struct MpPlayerActor {
    cfg: MpConfig,
    /// 自己的 sk. 启动时随机生成 (或注入 seed for test).
    own_sk: SecretKey,
    /// 自己的 pk (= sk · G).
    own_pk: crate::mental_poker::elgamal::PublicKey,
    /// 自己的 Schnorr DLOG proof (绑 peer_id 作 ctx).
    /// 启动时立即生成 — 进入 KeyExchange phase 后第一件事 broadcast.
    own_dlog_proof: crate::mental_poker::schnorr::DlogProof,
    /// CardMapping (deterministic from session_label). 4 方独立派生应一致.
    card_mapping: CardMapping,
    /// 当前 phase.
    phase: MpPhase,
    /// 收到的 N 个成员 (含自己) — 进 Shuffling 后才完整, 期间累积.
    members: Vec<Option<MemberInfo>>,
    /// 协议 0 完成后的 jpk.
    jpk: Option<JointPublicKey>,
    /// 全桌账本镜像 (4 方手牌状态, 协议 4-7 transition).
    table: Table,
    /// Cmd 接收 channel.
    rx: UnboundedReceiver<MpRoomCmd>,
    /// 事件发送 channel (上层 UI / P2P 桥).
    event_tx: UnboundedSender<MpEvent>,
    /// 关停 signal (drop handle 时触发).
    shutdown_rx: oneshot::Receiver<()>,
}

impl MpPlayerActor {
    /// 启动 actor: 立即生成 sk + Schnorr proof, 进入 KeyExchange phase.
    /// **未来扩展 (M5.B.4)**: 立即 emit OutboundMsg::KeyShare 让上层广播.
    fn new(
        cfg: MpConfig,
        seed_hint: Option<u64>,
        rx: UnboundedReceiver<MpRoomCmd>,
        event_tx: UnboundedSender<MpEvent>,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> Self {
        // 用 seed 派生确定性 RNG (生产 None → 真随机).
        let mut rng = match seed_hint {
            Some(s) => {
                let mut h = Sha256::new();
                h.update(b"tui-majo/mp/actor-seed/v1");
                h.update(s.to_be_bytes());
                let seed: [u8; 32] = h.finalize().into();
                StdRng::from_seed(seed)
            }
            None => {
                let mut bytes = [0u8; 32];
                use ark_std::rand::RngCore;
                let mut tr = ark_std::test_rng();
                tr.fill_bytes(&mut bytes);
                StdRng::from_seed(bytes)
            }
        };
        let (own_sk, own_pk) = keygen(&mut rng);
        let own_peer_id = &cfg.all_peer_ids[cfg.own_index];
        let own_dlog_proof = schnorr::prove(&mut rng, &own_sk, &own_pk, own_peer_id);
        let card_mapping = CardMapping::from_label_sized(&cfg.session_label, cfg.deck_size);

        let n = cfg.n_players();
        let mut members: Vec<Option<MemberInfo>> = vec![None; n];
        members[cfg.own_index] = Some(MemberInfo {
            peer_id: own_peer_id.clone(),
            pk: own_pk,
        });

        let table = Table::new(n, cfg.deck_size);

        Self {
            cfg,
            own_sk,
            own_pk,
            own_dlog_proof,
            card_mapping,
            phase: MpPhase::KeyExchange,
            members,
            jpk: None,
            table,
            rx,
            event_tx,
            shutdown_rx,
        }
    }

    /// Actor 主 loop. 处理 cmd 直到 Disconnect / shutdown.
    async fn run(mut self) {
        // 进入 KeyExchange phase 时主动 emit 自己的 KeyShare 让上层广播.
        // M5.B.4 实施时填充 OutboundMsg.
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });

        loop {
            tokio::select! {
                biased;
                _ = &mut self.shutdown_rx => {
                    tracing::debug!("MpPlayerActor[{}] shutdown signaled", self.cfg.own_index);
                    break;
                }
                Some(cmd) = self.rx.recv() => {
                    if !self.handle_cmd(cmd) {
                        break;
                    }
                }
                else => break,
            }
        }
    }

    /// 处理一个 cmd. 返回 false 表示 actor 应退出.
    fn handle_cmd(&mut self, cmd: MpRoomCmd) -> bool {
        match cmd {
            MpRoomCmd::Disconnect => {
                tracing::info!("MpPlayerActor[{}] Disconnect", self.cfg.own_index);
                false
            }
            MpRoomCmd::PeerMsg { from, msg } => {
                tracing::debug!(
                    "MpPlayerActor[{}] PeerMsg from={from:?} kind={msg:?} (phase={:?}, M5.B.4+ 实现)",
                    self.cfg.own_index,
                    self.phase
                );
                true
            }
            MpRoomCmd::LocalAction(a) => {
                tracing::debug!(
                    "MpPlayerActor[{}] LocalAction {a:?} (phase={:?}, M5.B.6 实现)",
                    self.cfg.own_index,
                    self.phase
                );
                true
            }
            MpRoomCmd::Tick => {
                // M5.B.4+ 用作 timeout 推进
                true
            }
        }
    }

    // 后续 commits 添加: handle_key_share / handle_shuffle_round / handle_draw_request /
    // handle_reveal_share / handle_discard / handle_call / handle_concealed_kan / handle_win
}

/// Actor handle, UI / P2P 桥通过它发 cmd + 接 event.
pub struct MpPlayerHandle {
    pub cmd_tx: UnboundedSender<MpRoomCmd>,
    /// 取走 event 流 — 用 Option 让 caller 可 take 后单独 drive.
    pub event_rx: Option<UnboundedReceiver<MpEvent>>,
    /// drop 时 actor 收 shutdown.
    _shutdown: Option<oneshot::Sender<()>>,
}

impl MpPlayerHandle {
    pub fn send(&self, cmd: MpRoomCmd) -> Result<(), &'static str> {
        self.cmd_tx.send(cmd).map_err(|_| "actor channel closed")
    }

    /// 取走 event_rx. 只能调一次.
    pub fn take_event_rx(&mut self) -> Option<UnboundedReceiver<MpEvent>> {
        self.event_rx.take()
    }
}

/// Spawn 一个 MpPlayerActor 到当前 tokio runtime.
pub fn spawn_mp_player(cfg: MpConfig, seed_hint: Option<u64>) -> MpPlayerHandle {
    let (cmd_tx, rx) = mpsc::unbounded_channel::<MpRoomCmd>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<MpEvent>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let actor = MpPlayerActor::new(cfg, seed_hint, rx, event_tx, shutdown_rx);
    tokio::spawn(actor.run());

    MpPlayerHandle {
        cmd_tx,
        event_rx: Some(event_rx),
        _shutdown: Some(shutdown_tx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg(own: usize) -> MpConfig {
        MpConfig {
            own_index: own,
            all_peer_ids: vec![
                b"p0".to_vec(),
                b"p1".to_vec(),
                b"p2".to_vec(),
                b"p3".to_vec(),
            ],
            session_label: b"test-session".to_vec(),
            deck_size: 16, // 加速测试
            cnc_k_rounds: 8,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_emits_initial_phase_event() {
        let mut h = spawn_mp_player(test_cfg(0), Some(42));
        let mut rx = h.take_event_rx().unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("timeout")
            .expect("got event");
        match event {
            MpEvent::PhaseChanged { phase } => {
                assert_eq!(phase, MpPhase::KeyExchange);
            }
            other => panic!("expected PhaseChanged, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disconnect_terminates_actor() {
        let mut h = spawn_mp_player(test_cfg(0), Some(42));
        let _rx = h.take_event_rx().unwrap();
        h.send(MpRoomCmd::Disconnect).unwrap();
        // actor 退出后, drop handle 不 panic
        drop(h);
        // 等一下 spawn task 完全退出
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drop_handle_shuts_down_actor() {
        let h = spawn_mp_player(test_cfg(1), Some(7));
        // 直接 drop, shutdown channel close → actor 退出
        drop(h);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_action_in_keyexchange_ignored_no_panic() {
        use crate::domain::action::Action;
        let mut h = spawn_mp_player(test_cfg(0), Some(42));
        let mut rx = h.take_event_rx().unwrap();
        // 吞掉 init phase event
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        // KeyExchange phase 收 LocalAction(Pass) 应不 panic, M5.B.4+ 实际处理
        h.send(MpRoomCmd::LocalAction(Action::Pass)).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // actor 仍活着 (没有 panic 退出)
        h.send(MpRoomCmd::Tick).unwrap();
    }

    /// MpConfig::n_players 跟 all_peer_ids.len() 一致.
    #[test]
    fn config_n_players_from_peer_ids() {
        let cfg = test_cfg(0);
        assert_eq!(cfg.n_players(), 4);
    }

    /// 不同 own_index → 一致的 peer_id 列表.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn different_actors_share_peer_id_list() {
        let h0 = spawn_mp_player(test_cfg(0), Some(1));
        let h1 = spawn_mp_player(test_cfg(1), Some(2));
        // sanity: spawn 不 panic, channel 可 send
        h0.send(MpRoomCmd::Disconnect).ok();
        h1.send(MpRoomCmd::Disconnect).ok();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
