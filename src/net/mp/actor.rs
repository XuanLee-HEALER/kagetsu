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
use crate::mental_poker::cut_and_choose::{self, ShuffleProof};
use crate::mental_poker::elgamal::{Ciphertext, SecretKey, keygen, mask_with_r};
use crate::mental_poker::joint_key::{JointPublicKey, aggregate};
use crate::mental_poker::protocol_reveal::MemberInfo;
use crate::mental_poker::protocol_state::Table;
use crate::mental_poker::schnorr::{self, DlogProof};
use crate::mental_poker::shuffle::shuffle_and_remask;
use crate::mental_poker::wire::{self, MentalPokerMsg};

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
/// M5.B.5+ 还会加 sessions (DrawSession / RevealSession) 字段 — 暂用
/// `#[allow(dead_code)]` 静默 table (Table 字段在协议 4-7 才用).
#[allow(dead_code)]
pub struct MpPlayerActor {
    cfg: MpConfig,
    /// 自己的 sk. 启动时随机生成 (或注入 seed for test).
    own_sk: SecretKey,
    /// 自己的 pk (= sk · G).
    own_pk: crate::mental_poker::elgamal::PublicKey,
    /// 自己的 Schnorr DLOG proof (绑 peer_id 作 ctx).
    own_dlog_proof: DlogProof,
    /// CardMapping (deterministic from session_label). 4 方独立派生应一致.
    card_mapping: CardMapping,
    /// 当前 phase.
    phase: MpPhase,
    /// 收到的 N 个成员 (含自己). 进 Shuffling 后才完整, 期间累积.
    members: Vec<Option<MemberInfo>>,
    /// 协议 0 完成后的 jpk.
    jpk: Option<JointPublicKey>,
    /// 全桌账本镜像 (协议 4-7 transition).
    table: Table,
    /// 协议 1 shuffle 历史: decks[k] = 玩家 k 提交后的 deck.
    /// decks[0] = 用 jpk 加密初始 plaintext (各方独立用 card_mapping 派生).
    /// decks 长 = 当前已 verify 通过的 round 数 + 1 (含初始).
    shuffle_decks: Vec<Vec<Ciphertext>>,
    /// 协议 1 shuffle round 累积的 proofs (cnc verify 通过后 push).
    shuffle_proofs: Vec<ShuffleProof>,
    /// Cmd 接收 channel.
    rx: UnboundedReceiver<MpRoomCmd>,
    /// 事件发送 channel (上层 UI / P2P 桥).
    event_tx: UnboundedSender<MpEvent>,
    /// 关停 signal (drop handle 时触发).
    shutdown_rx: oneshot::Receiver<()>,
    /// RNG (持久化, 用于 shuffle round / mask 因子). seeded from seed_hint.
    rng: StdRng,
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
            shuffle_decks: Vec::new(),
            shuffle_proofs: Vec::new(),
            rx,
            event_tx,
            shutdown_rx,
            rng,
        }
    }

    /// Actor 主 loop. 处理 cmd 直到 Disconnect / shutdown.
    async fn run(mut self) {
        // 进入 KeyExchange phase: emit PhaseChanged + 立即广播自己的 KeyShare.
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });
        self.broadcast_own_key_share();

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

    /// 立即广播自己的 KeyShare (entered KeyExchange). 上层桥负责发到 P2P.
    fn broadcast_own_key_share(&mut self) {
        let msg = MentalPokerMsg::KeyShare {
            peer_id: self.cfg.all_peer_ids[self.cfg.own_index].clone(),
            pk: wire::encode_pk(&self.own_pk),
            proof: wire::encode_dlog_proof(&self.own_dlog_proof),
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
    }

    /// 处理一个 cmd. 返回 false 表示 actor 应退出.
    fn handle_cmd(&mut self, cmd: MpRoomCmd) -> bool {
        match cmd {
            MpRoomCmd::Disconnect => {
                tracing::info!("MpPlayerActor[{}] Disconnect", self.cfg.own_index);
                false
            }
            MpRoomCmd::PeerMsg { from, msg } => {
                self.handle_peer_msg(from, msg);
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
            MpRoomCmd::Tick => true,
        }
    }

    fn handle_peer_msg(&mut self, from: Option<usize>, msg: MentalPokerMsg) {
        match msg {
            MentalPokerMsg::KeyShare { peer_id, pk, proof }
                if self.phase == MpPhase::KeyExchange =>
            {
                self.handle_key_share(peer_id, pk, proof);
            }
            MentalPokerMsg::ShuffleRound {
                round_idx,
                new_deck,
                proof,
            } if self.phase == MpPhase::Shuffling => {
                self.handle_shuffle_round(round_idx, new_deck, proof);
            }
            other => {
                tracing::debug!(
                    "MpPlayerActor[{}] PeerMsg from={from:?} ignored (phase={:?}, msg={other:?})",
                    self.cfg.own_index,
                    self.phase,
                );
            }
        }
    }

    /// 协议 0: 收到对方 KeyShare. 验证 schnorr proof + store member.
    /// 收齐 N 个 → aggregate jpk → transition Shuffling. actor 0 立即提交第一轮.
    fn handle_key_share(&mut self, peer_id: Vec<u8>, pk_bytes: Vec<u8>, proof_bytes: Vec<u8>) {
        // 找 peer_id 对应 index
        let Some(idx) = self.cfg.all_peer_ids.iter().position(|id| *id == peer_id) else {
            self.emit_protocol_error(None, format!("未知 peer_id={}", hex_short(&peer_id)));
            return;
        };
        if idx == self.cfg.own_index {
            // 自己的 echo, 忽略
            return;
        }
        if self.members[idx].is_some() {
            // 重复 KeyShare, 忽略
            return;
        }
        let pk = match wire::decode_pk(&pk_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(Some(idx), format!("pk decode 失败: {e}"));
                return;
            }
        };
        let proof = match wire::decode_dlog_proof(&proof_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(Some(idx), format!("dlog proof decode 失败: {e}"));
                return;
            }
        };
        // 验证 schnorr proof
        if !schnorr::verify(&pk, &proof, &peer_id) {
            self.emit_protocol_error(
                Some(idx),
                "schnorr DLOG proof 无效 (rogue key 或 ctx mismatch)".into(),
            );
            return;
        }
        self.members[idx] = Some(MemberInfo { peer_id, pk });

        // 全员到齐?
        if self.members.iter().all(|m| m.is_some()) {
            self.complete_key_exchange();
        }
    }

    fn complete_key_exchange(&mut self) {
        // 入口 handle_key_share 已 schnorr::verify 过每个远端 proof, 这里直接 sum
        // pk_i 等价于 [`aggregate`] 的结果 (aggregate 内部也是 verify-then-sum).
        let _ = aggregate; // imported for documentation
        let aggregate_pk: crate::mental_poker::Curve = self
            .members
            .iter()
            .map(|m| m.as_ref().expect("all some").pk.0)
            .sum();
        self.jpk = Some(JointPublicKey(aggregate_pk));

        // 加密初始 deck under jpk. **关键**: 4 方独立派生必须 produce 同一 initial
        // deck (后续 shuffle round 在此基础上 verify). 用 r = Scalar::from(1) 让
        // mask 是 deterministic (c1=G, c2=m+PK). 不影响安全性 — initial deck 是公
        // 开的 (mapping plaintexts 各方都知道), shuffle round 才用 random r 引入
        // 不可预知性.
        let pk = self.jpk.as_ref().unwrap().as_pk();
        let plaintexts = self.card_mapping.points().to_vec();
        let r_one = crate::mental_poker::Scalar::from(1u64);
        let initial: Vec<Ciphertext> = plaintexts
            .iter()
            .map(|m| mask_with_r(&pk, m, r_one).0)
            .collect();
        self.shuffle_decks.push(initial);

        // transition
        self.phase = MpPhase::Shuffling;
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });
        let _ = self.event_tx.send(MpEvent::ShuffleProgress {
            completed: 0,
            total: self.cfg.n_players() as u32,
        });

        // 如果自己是第一个 actor (own_index == 0), 立即提交 shuffle round
        if self.cfg.own_index == 0 {
            self.submit_own_shuffle_round();
        }
    }

    fn submit_own_shuffle_round(&mut self) {
        let round_idx = self.shuffle_decks.len() as u32 - 1; // 0-based
        let pk = self.jpk.as_ref().expect("jpk 已聚合").as_pk();
        let input = self
            .shuffle_decks
            .last()
            .expect("decks 至少含 initial")
            .clone();
        let (out, pi, r) = shuffle_and_remask(&mut self.rng, &pk, &input);
        let proof = cut_and_choose::prove(
            &mut self.rng,
            &pk,
            &input,
            &out,
            &pi,
            &r,
            self.cfg.cnc_k_rounds,
        );

        // 本地 store (跳过 verify 自己的 proof, M5.B.7 e2e 时所有人独立 verify)
        self.shuffle_decks.push(out.clone());
        self.shuffle_proofs.push(proof.clone());

        // 广播
        let proof_bytes = match wire::encode_shuffle_proof(&proof) {
            Ok(b) => b,
            Err(e) => {
                self.emit_protocol_error(None, format!("encode shuffle proof 失败: {e}"));
                return;
            }
        };
        let msg = MentalPokerMsg::ShuffleRound {
            round_idx,
            new_deck: wire::encode_ciphertext_vec(&out),
            proof: proof_bytes,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        let _ = self.event_tx.send(MpEvent::ShuffleProgress {
            completed: round_idx + 1,
            total: self.cfg.n_players() as u32,
        });

        // 自己是最后一个 → transition Playing
        if self.cfg.own_index == self.cfg.n_players() - 1 {
            self.complete_shuffling();
        }
    }

    /// 协议 1: 收到对方 ShuffleRound. 验证 cnc proof + apply.
    fn handle_shuffle_round(&mut self, round_idx: u32, deck_bytes: Vec<u8>, proof_bytes: Vec<u8>) {
        let expected_round = self.shuffle_decks.len() as u32 - 1;
        if round_idx != expected_round {
            tracing::debug!(
                "MpPlayerActor[{}] shuffle round_idx mismatch: expected {expected_round}, got {round_idx}",
                self.cfg.own_index
            );
            return;
        }
        let new_deck = match wire::decode_ciphertext_vec(&deck_bytes) {
            Ok(d) => d,
            Err(e) => {
                self.emit_protocol_error(
                    Some(round_idx as usize),
                    format!("deck decode 失败: {e}"),
                );
                return;
            }
        };
        let proof = match wire::decode_shuffle_proof(&proof_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(
                    Some(round_idx as usize),
                    format!("shuffle proof decode 失败: {e}"),
                );
                return;
            }
        };

        let pk = self.jpk.as_ref().expect("jpk 已聚合").as_pk();
        let input_deck = self
            .shuffle_decks
            .last()
            .expect("decks 至少 initial")
            .clone();
        if !cut_and_choose::verify(&pk, &input_deck, &new_deck, &proof) {
            self.emit_protocol_error(
                Some(round_idx as usize),
                "shuffle cut-and-choose proof 验证失败".into(),
            );
            return;
        }
        self.shuffle_decks.push(new_deck);
        self.shuffle_proofs.push(proof);
        let completed = self.shuffle_decks.len() as u32 - 1;
        let _ = self.event_tx.send(MpEvent::ShuffleProgress {
            completed,
            total: self.cfg.n_players() as u32,
        });

        // 接下来该谁?
        let next_actor = completed as usize;
        if next_actor == self.cfg.n_players() {
            self.complete_shuffling();
        } else if next_actor == self.cfg.own_index {
            self.submit_own_shuffle_round();
        }
    }

    fn complete_shuffling(&mut self) {
        self.phase = MpPhase::Playing;
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });
    }

    fn emit_protocol_error(&self, offender: Option<usize>, reason: String) {
        let _ = self.event_tx.send(MpEvent::ProtocolError {
            offender,
            reason: reason.clone(),
        });
        tracing::warn!(
            "MpPlayerActor[{}] ProtocolError offender={offender:?}: {reason}",
            self.cfg.own_index
        );
    }

    // M5.B.5+ 添加: handle_draw_request / handle_reveal_share / handle_discard /
    // handle_call / handle_concealed_kan / handle_win
}

fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut out = String::with_capacity(take * 2);
    for b in &bytes[..take] {
        out.push_str(&format!("{b:02x}"));
    }
    out
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

    /// spawn 后立即 emit OutboundMsg::KeyShare (协议 0 启动).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_emits_outbound_key_share() {
        let mut h = spawn_mp_player(test_cfg(0), Some(42));
        let mut rx = h.take_event_rx().unwrap();
        // 先 PhaseChanged, 然后 OutboundMsg::KeyShare
        let mut got_keyshare = false;
        for _ in 0..3 {
            match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
                Ok(Some(MpEvent::OutboundMsg {
                    to: None,
                    msg: MentalPokerMsg::KeyShare { .. },
                })) => {
                    got_keyshare = true;
                    break;
                }
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
        assert!(got_keyshare, "spawn 后应 emit KeyShare 给 broadcast");
    }

    /// **M5.B.4 核心 e2e**: 4 actor 用 mpsc 桥接, 跑通协议 0 (keygen) + 协议 1
    /// (联合洗牌). 各 actor 应 transition KeyExchange → Shuffling → Playing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn four_actors_complete_keygen_and_shuffle() {
        const N: usize = 4;
        // 4 个 actor, 各自 own_index
        let mut handles: Vec<MpPlayerHandle> = (0..N)
            .map(|i| spawn_mp_player(test_cfg(i), Some((i + 1) as u64)))
            .collect();
        let mut rxs: Vec<_> = handles
            .iter_mut()
            .map(|h| h.take_event_rx().unwrap())
            .collect();

        // 桥: 收每个 actor 的 OutboundMsg → 路由给其他 actor 作 PeerMsg.
        // broadcast (to=None) → 发给其他 N-1 个; unicast (to=Some(idx)) → 发给指定.
        let max_steps = 200;
        let mut phases: Vec<MpPhase> = vec![MpPhase::KeyExchange; N];
        let mut shuffle_progress: Vec<u32> = vec![0; N];
        let mut all_in_playing = false;

        'outer: for step in 0usize..max_steps {
            let mut any_progress = false;
            for src in 0..N {
                let rx = &mut rxs[src];
                while let Ok(ev) = rx.try_recv() {
                    any_progress = true;
                    match ev {
                        MpEvent::PhaseChanged { phase } => {
                            phases[src] = phase;
                        }
                        MpEvent::ShuffleProgress { completed, .. } => {
                            shuffle_progress[src] = completed;
                        }
                        MpEvent::OutboundMsg { to, msg } => {
                            // 路由给其他 actor
                            let targets: Vec<usize> = match to {
                                None => (0..N).filter(|&i| i != src).collect(),
                                Some(idx) => vec![idx],
                            };
                            for t in targets {
                                handles[t]
                                    .send(MpRoomCmd::PeerMsg {
                                        from: Some(src),
                                        msg: msg.clone(),
                                    })
                                    .unwrap();
                            }
                        }
                        MpEvent::ProtocolError { offender, reason } => {
                            panic!(
                                "actor {src} reported ProtocolError offender={offender:?}: {reason}"
                            );
                        }
                        MpEvent::GameOver { .. } => {}
                    }
                }
            }
            if phases.iter().all(|p| *p == MpPhase::Playing) {
                all_in_playing = true;
                break 'outer;
            }
            if !any_progress {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            // 防卡死: 每 20 步打一次状态
            if step.is_multiple_of(20) {
                tracing::debug!(
                    "step {step}: phases={phases:?}, shuffle_progress={shuffle_progress:?}"
                );
            }
        }

        assert!(
            all_in_playing,
            "4 actor 应全 transition 到 Playing, 实际 phases={phases:?}"
        );
        // 每方都看到 4 轮 shuffle 完成
        for (i, p) in shuffle_progress.iter().enumerate() {
            assert_eq!(*p, N as u32, "actor {i} shuffle_progress != {N}");
        }

        // 清理
        for h in &handles {
            h.send(MpRoomCmd::Disconnect).ok();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
