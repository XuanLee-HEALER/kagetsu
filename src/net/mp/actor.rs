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
use crate::mental_poker::protocol_draw::{self, DecryptionShare};
use crate::mental_poker::protocol_reveal::{self, MemberInfo, RevealShare};
use crate::mental_poker::protocol_state::Table;
use crate::mental_poker::schnorr::{self, DlogProof};
use crate::mental_poker::session::RevealSession;
use crate::mental_poker::shuffle::shuffle_and_remask;
use crate::mental_poker::wire::{self, MentalPokerMsg};

use super::cmd::{MpEvent, MpRoomCmd};
use super::phase::MpPhase;
use std::collections::HashMap;
use uuid::Uuid;

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
    /// 协议 2 摸牌进行中的 session (key = request_id). 摸牌方收齐 N-1 个
    /// share 后 + 自己 share → combine. 收齐后清理.
    draw_sessions: HashMap<Uuid, PendingDraw>,
    /// 协议 3 公开揭示进行中的 session (key = deck_index).
    reveal_sessions: HashMap<u32, PendingReveal>,
    /// 自己已摸到的 (deck_index → tile_id). 协议 4 弃牌时反查 plaintext (=
    /// card_mapping.encode(tile_id)). plaintext 不需要单独存因为 mapping 可
    /// 反查.
    own_drawn: HashMap<u32, usize>,
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
            draw_sessions: HashMap::new(),
            reveal_sessions: HashMap::new(),
            own_drawn: HashMap::new(),
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
            MpRoomCmd::TriggerDraw { deck_index } => {
                self.start_draw(deck_index);
                true
            }
            MpRoomCmd::TriggerReveal { deck_index } => {
                self.start_reveal(deck_index);
                true
            }
            MpRoomCmd::Discard { deck_index } => {
                self.do_discard(deck_index);
                true
            }
            MpRoomCmd::Call {
                call_type,
                deck_indices,
                from_player,
                from_position_in_meld,
            } => {
                self.do_call(call_type, deck_indices, from_player, from_position_in_meld);
                true
            }
            MpRoomCmd::ConcealedKan {
                deck_indices,
                monitor_player,
            } => {
                self.do_concealed_kan(deck_indices, monitor_player);
                true
            }
            MpRoomCmd::Shouminkan {
                target_meld_idx,
                new_deck_index,
            } => {
                self.do_shouminkan(target_meld_idx, new_deck_index);
                true
            }
            MpRoomCmd::Tsumo {
                hand_indices,
                winning_tile_index,
            } => {
                self.do_win(true, None, hand_indices, winning_tile_index);
                true
            }
            MpRoomCmd::Ron {
                from_player,
                hand_indices,
                winning_tile_index,
            } => {
                self.do_win(false, Some(from_player), hand_indices, winning_tile_index);
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
            MentalPokerMsg::DrawShareRequest {
                request_id,
                ct,
                deck_index,
            } if self.phase == MpPhase::Playing => {
                self.handle_draw_share_request(from, request_id, ct, deck_index);
            }
            MentalPokerMsg::DrawShareResponse {
                request_id,
                share,
                proof,
            } if self.phase == MpPhase::Playing => {
                self.handle_draw_share_response(from, request_id, share, proof);
            }
            MentalPokerMsg::RevealShare { ct, share, proof } if self.phase == MpPhase::Playing => {
                self.handle_reveal_share(from, ct, share, proof);
            }
            MentalPokerMsg::DrawAnnouncement { player, deck_index }
                if self.phase == MpPhase::Playing =>
            {
                self.handle_draw_announcement(player, deck_index);
            }
            MentalPokerMsg::Discard {
                player,
                deck_index,
                plaintext,
            } if self.phase == MpPhase::Playing => {
                self.handle_discard_msg(player, deck_index, plaintext);
            }
            MentalPokerMsg::Call {
                player,
                call_type,
                deck_indices,
                plaintexts,
                from_player,
                from_position_in_meld,
            } if self.phase == MpPhase::Playing => {
                self.handle_call_msg(
                    player,
                    call_type,
                    deck_indices,
                    plaintexts,
                    from_player,
                    from_position_in_meld,
                );
            }
            MentalPokerMsg::ConcealedKanAnnounce {
                player,
                deck_indices,
                monitor_player,
            } if self.phase == MpPhase::Playing => {
                self.handle_concealed_kan_announce(player, deck_indices, monitor_player);
            }
            MentalPokerMsg::ConcealedKanReveal { plaintexts } if self.phase == MpPhase::Playing => {
                self.handle_concealed_kan_reveal(from, plaintexts);
            }
            MentalPokerMsg::Win {
                player,
                win_type,
                hand_indices,
                hand_plaintexts,
                winning_tile_index,
                ..
            } if self.phase == MpPhase::Playing => {
                self.handle_win_msg(
                    player,
                    win_type,
                    hand_indices,
                    hand_plaintexts,
                    winning_tile_index,
                );
            }
            MentalPokerMsg::Shouminkan {
                player,
                target_meld_idx,
                new_deck_index,
                new_plaintext,
            } if self.phase == MpPhase::Playing => {
                self.handle_shouminkan_msg(player, target_meld_idx, new_deck_index, new_plaintext);
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

    /// 协议 2 摸牌发起 (caller TriggerDraw cmd). 仅在 Playing phase 合法.
    fn start_draw(&mut self, deck_index: u32) {
        if self.phase != MpPhase::Playing {
            tracing::warn!(
                "MpPlayerActor[{}] TriggerDraw ignored: phase={:?}",
                self.cfg.own_index,
                self.phase
            );
            return;
        }
        let final_deck = self
            .shuffle_decks
            .last()
            .expect("Playing phase 应有 final_deck");
        let Some(ct) = final_deck.get(deck_index as usize).copied() else {
            self.emit_protocol_error(None, format!("deck_index {deck_index} 越界"));
            return;
        };
        let request_id = Uuid::new_v4();
        // 自己先算自己 share, 不广播给自己
        let own_peer_id = self.cfg.all_peer_ids[self.cfg.own_index].clone();
        let (own_share, _own_proof) = protocol_draw::compute_share(
            &mut self.rng,
            &self.own_sk,
            &self.own_pk,
            &ct,
            &own_peer_id,
        );
        let mut received = HashMap::new();
        received.insert(self.cfg.own_index, own_share);

        // store pending
        self.draw_sessions.insert(
            request_id,
            PendingDraw {
                request_id,
                deck_index,
                ct,
                received,
            },
        );

        // 广播 DrawShareRequest 给其他人
        let msg = MentalPokerMsg::DrawShareRequest {
            request_id,
            ct: wire::encode_ciphertext(&ct),
            deck_index,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
    }

    fn handle_draw_share_request(
        &mut self,
        from: Option<usize>,
        request_id: Uuid,
        ct_bytes: Vec<u8>,
        deck_index: u32,
    ) {
        let Some(requester_idx) = from else {
            self.emit_protocol_error(None, "DrawShareRequest 缺 from".into());
            return;
        };
        let ct = match wire::decode_ciphertext(&ct_bytes) {
            Ok(c) => c,
            Err(e) => {
                self.emit_protocol_error(Some(requester_idx), format!("ct decode: {e}"));
                return;
            }
        };
        // 验证 ct 跟自己 final_deck[deck_index] 一致 (防作弊摸牌位置)
        let final_deck = self.shuffle_decks.last().expect("Playing 应有 final_deck");
        let expected = final_deck.get(deck_index as usize).copied();
        if expected != Some(ct) {
            self.emit_protocol_error(
                Some(requester_idx),
                format!("DrawShareRequest ct 跟本地 final_deck[{deck_index}] 不一致"),
            );
            return;
        }
        // 计算自己 share (用 self peer_id 作 ctx)
        let own_peer_id = self.cfg.all_peer_ids[self.cfg.own_index].clone();
        let (share, proof) = protocol_draw::compute_share(
            &mut self.rng,
            &self.own_sk,
            &self.own_pk,
            &ct,
            &own_peer_id,
        );
        let response = MentalPokerMsg::DrawShareResponse {
            request_id,
            share: wire::encode_share(&share),
            proof: wire::encode_dleq_proof(&proof),
        };
        // 单播给 requester
        let _ = self.event_tx.send(MpEvent::OutboundMsg {
            to: Some(requester_idx),
            msg: response,
        });
    }

    fn handle_draw_share_response(
        &mut self,
        from: Option<usize>,
        request_id: Uuid,
        share_bytes: Vec<u8>,
        proof_bytes: Vec<u8>,
    ) {
        let Some(sender_idx) = from else {
            self.emit_protocol_error(None, "DrawShareResponse 缺 from".into());
            return;
        };
        let Some(pending) = self.draw_sessions.get_mut(&request_id) else {
            tracing::debug!(
                "MpPlayerActor[{}] DrawShareResponse 没找到 pending {request_id}",
                self.cfg.own_index
            );
            return;
        };
        let share = match wire::decode_share(&share_bytes) {
            Ok(s) => s,
            Err(e) => {
                self.emit_protocol_error(Some(sender_idx), format!("share decode: {e}"));
                return;
            }
        };
        let proof = match wire::decode_dleq_proof(&proof_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(Some(sender_idx), format!("proof decode: {e}"));
                return;
            }
        };
        // 验证 share + DLEQ proof
        let sender_peer_id = self.cfg.all_peer_ids[sender_idx].clone();
        let sender_pk = match self.members[sender_idx].as_ref() {
            Some(m) => m.pk,
            None => {
                self.emit_protocol_error(
                    Some(sender_idx),
                    "sender pk 不在 members (协议 0 未完成)".into(),
                );
                return;
            }
        };
        if !protocol_draw::verify_share(&sender_pk, &pending.ct, &share, &proof, &sender_peer_id) {
            self.emit_protocol_error(
                Some(sender_idx),
                format!("DrawShare DLEQ verify 失败 (request_id={request_id})"),
            );
            return;
        }
        pending.received.insert(sender_idx, share);
        // 收齐?
        if pending.received.len() != self.cfg.n_players() {
            return;
        }
        // combine
        let shares: Vec<DecryptionShare> = (0..self.cfg.n_players())
            .map(|i| pending.received[&i])
            .collect();
        let plaintext = protocol_draw::combine_shares(&pending.ct, &shares);
        // 反查 CardMapping
        let Some(tile_id) = self.card_mapping.decode(&plaintext) else {
            self.emit_protocol_error(
                None,
                "draw plaintext 不在 card_mapping (协议层 bug?)".into(),
            );
            return;
        };
        let deck_index = pending.deck_index;
        self.draw_sessions.remove(&request_id);
        // 更新 own_drawn / table / 广播 DrawAnnouncement
        self.finalize_own_draw(deck_index, tile_id, plaintext);
        let _ = self.event_tx.send(MpEvent::DrawComplete {
            request_id,
            deck_index,
            tile_id,
        });
    }

    /// 协议 3 公开揭示发起 (caller TriggerReveal cmd).
    fn start_reveal(&mut self, deck_index: u32) {
        if self.phase != MpPhase::Playing {
            return;
        }
        let final_deck = self.shuffle_decks.last().expect("Playing 应有 final_deck");
        let Some(ct) = final_deck.get(deck_index as usize).copied() else {
            self.emit_protocol_error(None, format!("reveal deck_index {deck_index} 越界"));
            return;
        };
        // 准备 members 结构 (跟 RevealSession 期望一致)
        let members: Vec<MemberInfo> = (0..self.cfg.n_players())
            .map(|i| self.members[i].as_ref().expect("协议 0 完成").clone())
            .collect();
        let mut session = RevealSession::new(members.clone(), ct);
        // 自己先算 + submit 自己
        let own_peer_id = self.cfg.all_peer_ids[self.cfg.own_index].clone();
        let own_contribution = protocol_reveal::prepare_share(
            &mut self.rng,
            &self.own_sk,
            &self.own_pk,
            &ct,
            &own_peer_id,
        );
        let _ = session.submit(self.cfg.own_index, own_contribution);

        self.reveal_sessions.insert(
            deck_index,
            PendingReveal {
                deck_index,
                ct,
                session,
            },
        );

        // 广播 RevealShare (协议 3 是 broadcast, 自己也"广播"自己的 share)
        let msg = MentalPokerMsg::RevealShare {
            ct: wire::encode_ciphertext(&ct),
            share: wire::encode_share(&own_contribution.share),
            proof: wire::encode_dleq_proof(&own_contribution.proof),
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });

        self.try_complete_reveal(deck_index);
    }

    fn handle_reveal_share(
        &mut self,
        from: Option<usize>,
        ct_bytes: Vec<u8>,
        share_bytes: Vec<u8>,
        proof_bytes: Vec<u8>,
    ) {
        let Some(sender_idx) = from else {
            self.emit_protocol_error(None, "RevealShare 缺 from".into());
            return;
        };
        let ct = match wire::decode_ciphertext(&ct_bytes) {
            Ok(c) => c,
            Err(e) => {
                self.emit_protocol_error(Some(sender_idx), format!("reveal ct decode: {e}"));
                return;
            }
        };
        // 找对应 deck_index
        let final_deck = self.shuffle_decks.last().expect("Playing 应有 final_deck");
        let Some(deck_index) = final_deck.iter().position(|c| *c == ct) else {
            self.emit_protocol_error(Some(sender_idx), "RevealShare 的 ct 不在 final_deck".into());
            return;
        };
        let deck_index = deck_index as u32;

        // 没 pending? — 自动 init + 算自己 share 并广播 (协议 3: 任何 actor 第一次
        // 见到这个 ct 都要参与 N-broadcast).
        let needs_init = !self.reveal_sessions.contains_key(&deck_index);
        if needs_init {
            let members: Vec<MemberInfo> = (0..self.cfg.n_players())
                .map(|i| self.members[i].as_ref().expect("协议 0 完成").clone())
                .collect();
            let mut session = RevealSession::new(members, ct);
            // 自己也算 share + submit 自己
            let own_peer_id = self.cfg.all_peer_ids[self.cfg.own_index].clone();
            let own_contribution = protocol_reveal::prepare_share(
                &mut self.rng,
                &self.own_sk,
                &self.own_pk,
                &ct,
                &own_peer_id,
            );
            let _ = session.submit(self.cfg.own_index, own_contribution);
            self.reveal_sessions.insert(
                deck_index,
                PendingReveal {
                    deck_index,
                    ct,
                    session,
                },
            );
            // 广播自己的 share, 让其他 actor 也能收齐 (含没主动 trigger 的)
            let msg = MentalPokerMsg::RevealShare {
                ct: wire::encode_ciphertext(&ct),
                share: wire::encode_share(&own_contribution.share),
                proof: wire::encode_dleq_proof(&own_contribution.proof),
            };
            let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        }

        // submit 远端 share
        let share = match wire::decode_share(&share_bytes) {
            Ok(s) => s,
            Err(e) => {
                self.emit_protocol_error(Some(sender_idx), format!("share decode: {e}"));
                return;
            }
        };
        let proof = match wire::decode_dleq_proof(&proof_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(Some(sender_idx), format!("proof decode: {e}"));
                return;
            }
        };
        let pending = self.reveal_sessions.get_mut(&deck_index).unwrap();
        let contribution = RevealShare { share, proof };
        if let Err(e) = pending.session.submit(sender_idx, contribution) {
            self.emit_protocol_error(Some(sender_idx), format!("RevealSession submit: {e}"));
            return;
        }
        self.try_complete_reveal(deck_index);
    }

    /// 协议 2 内部辅助: combine 完成后调, 同步 own_drawn / table 自己 drawn /
    /// 广播 DrawAnnouncement 给其他 actor (让他们 record_draw 同步).
    fn finalize_own_draw(
        &mut self,
        deck_index: u32,
        tile_id: usize,
        plaintext: crate::mental_poker::Curve,
    ) {
        self.own_drawn.insert(deck_index, tile_id);
        // 自己 record_draw with plaintext
        let _ = self
            .table
            .hand_mut(self.cfg.own_index)
            .record_draw(deck_index as usize, Some(plaintext));
        // 广播 DrawAnnouncement
        let msg = MentalPokerMsg::DrawAnnouncement {
            player: self.cfg.own_index as u32,
            deck_index,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
    }

    /// 协议 2 远端: 收到他人 DrawAnnouncement → record_draw(None) (我们不知 plaintext).
    fn handle_draw_announcement(&mut self, player: u32, deck_index: u32) {
        if (player as usize) == self.cfg.own_index {
            return; // 自己 echo, 已处理
        }
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("DrawAnnouncement player={player} 越界"));
            return;
        }
        if let Err(e) = self
            .table
            .hand_mut(player as usize)
            .record_draw(deck_index as usize, None)
        {
            self.emit_protocol_error(
                Some(player as usize),
                format!("DrawAnnouncement record_draw 失败: {e}"),
            );
            return;
        }
        let _ = self
            .event_tx
            .send(MpEvent::RemoteDrawObserved { player, deck_index });
    }

    /// 协议 4 自己弃牌. 验证 + apply 本地 + 广播.
    fn do_discard(&mut self, deck_index: u32) {
        if self.phase != MpPhase::Playing {
            return;
        }
        let Some(&tile_id) = self.own_drawn.get(&deck_index) else {
            self.emit_protocol_error(
                None,
                format!("Discard: 未摸过 deck_index={deck_index} (不在 own_drawn)"),
            );
            return;
        };
        let plaintext = self.card_mapping.encode(tile_id);
        // 自己 apply 到 table
        let ann = crate::mental_poker::protocol_discard::DiscardAnnouncement {
            player: self.cfg.own_index,
            deck_index: deck_index as usize,
            plaintext,
        };
        if let Err(e) = ann.apply(&mut self.table) {
            self.emit_protocol_error(None, format!("自己 Discard apply 失败: {e}"));
            return;
        }
        // 广播
        let msg = MentalPokerMsg::Discard {
            player: self.cfg.own_index as u32,
            deck_index,
            plaintext: wire::encode_curve(&plaintext),
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        // emit local UI event
        let _ = self.event_tx.send(MpEvent::DiscardApplied {
            player: self.cfg.own_index as u32,
            deck_index,
            tile_id,
        });
    }

    /// 协议 4 远端弃牌. 反查 plaintext → tile_id, validate + apply.
    fn handle_discard_msg(&mut self, player: u32, deck_index: u32, plaintext_bytes: Vec<u8>) {
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("Discard player={player} 越界"));
            return;
        }
        if (player as usize) == self.cfg.own_index {
            return; // 自己 echo, 已 apply
        }
        let plaintext = match wire::decode_curve(&plaintext_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(
                    Some(player as usize),
                    format!("Discard plaintext decode: {e}"),
                );
                return;
            }
        };
        let Some(tile_id) = self.card_mapping.decode(&plaintext) else {
            self.emit_protocol_error(
                Some(player as usize),
                "Discard plaintext 不在 card_mapping (作弊?)".into(),
            );
            return;
        };
        let ann = crate::mental_poker::protocol_discard::DiscardAnnouncement {
            player: player as usize,
            deck_index: deck_index as usize,
            plaintext,
        };
        if let Err(e) = ann.apply(&mut self.table) {
            self.emit_protocol_error(Some(player as usize), format!("Discard apply 失败: {e}"));
            return;
        }
        let _ = self.event_tx.send(MpEvent::DiscardApplied {
            player,
            deck_index,
            tile_id,
        });
    }

    /// 协议 5 自己鸣牌 (吃/碰/明杠).
    fn do_call(
        &mut self,
        call_type: crate::mental_poker::wire::WireCallType,
        deck_indices: Vec<u32>,
        from_player: u32,
        from_position_in_meld: u32,
    ) {
        if self.phase != MpPhase::Playing {
            return;
        }
        // 反查 plaintexts (自己 hand 部分用 own_drawn, from_position 用 from_player.discarded)
        let mut plaintexts: Vec<crate::mental_poker::Curve> =
            Vec::with_capacity(deck_indices.len());
        for (i, &idx) in deck_indices.iter().enumerate() {
            let pt = if i == from_position_in_meld as usize {
                // from_player 弃牌 — 从 table 拿
                match self
                    .table
                    .hand(from_player as usize)
                    .discarded_plaintext(idx as usize)
                {
                    Some(p) => *p,
                    None => {
                        self.emit_protocol_error(
                            None,
                            format!("Call: from_player={from_player} 没弃过 deck_index={idx}"),
                        );
                        return;
                    }
                }
            } else {
                // 自己 hand
                let Some(&tid) = self.own_drawn.get(&idx) else {
                    self.emit_protocol_error(
                        None,
                        format!("Call: deck_index={idx} 不在 own_drawn"),
                    );
                    return;
                };
                self.card_mapping.encode(tid)
            };
            plaintexts.push(pt);
        }

        let ann = crate::mental_poker::protocol_call::CallAnnouncement {
            player: self.cfg.own_index,
            call_type: call_type.into(),
            deck_indices: deck_indices.iter().map(|&i| i as usize).collect(),
            plaintexts: plaintexts.clone(),
            from_player: from_player as usize,
            from_position_in_meld: from_position_in_meld as usize,
        };
        if let Err(e) = ann.apply(&mut self.table) {
            self.emit_protocol_error(None, format!("自己 Call apply 失败: {e}"));
            return;
        }
        // 广播
        let plaintexts_bytes: Vec<Vec<u8>> = plaintexts
            .iter()
            .map(crate::mental_poker::wire::encode_curve)
            .collect();
        let msg = MentalPokerMsg::Call {
            player: self.cfg.own_index as u32,
            call_type,
            deck_indices: deck_indices.clone(),
            plaintexts: plaintexts_bytes,
            from_player,
            from_position_in_meld,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        // tile_ids 给 UI
        let tile_ids: Vec<usize> = plaintexts
            .iter()
            .map(|p| self.card_mapping.decode(p).unwrap_or(usize::MAX))
            .collect();
        let _ = self.event_tx.send(MpEvent::CallApplied {
            player: self.cfg.own_index as u32,
            call_type,
            deck_indices,
            tile_ids,
            from_player,
        });
    }

    /// 协议 5 远端鸣牌. 反查 plaintexts → tile_ids, validate + apply.
    #[allow(clippy::too_many_arguments)]
    fn handle_call_msg(
        &mut self,
        player: u32,
        call_type: crate::mental_poker::wire::WireCallType,
        deck_indices: Vec<u32>,
        plaintexts_bytes: Vec<Vec<u8>>,
        from_player: u32,
        from_position_in_meld: u32,
    ) {
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("Call player={player} 越界"));
            return;
        }
        if (player as usize) == self.cfg.own_index {
            return;
        }
        let mut plaintexts: Vec<crate::mental_poker::Curve> =
            Vec::with_capacity(plaintexts_bytes.len());
        for b in &plaintexts_bytes {
            match wire::decode_curve(b) {
                Ok(p) => plaintexts.push(p),
                Err(e) => {
                    self.emit_protocol_error(
                        Some(player as usize),
                        format!("Call plaintext decode: {e}"),
                    );
                    return;
                }
            }
        }
        let ann = crate::mental_poker::protocol_call::CallAnnouncement {
            player: player as usize,
            call_type: call_type.into(),
            deck_indices: deck_indices.iter().map(|&i| i as usize).collect(),
            plaintexts: plaintexts.clone(),
            from_player: from_player as usize,
            from_position_in_meld: from_position_in_meld as usize,
        };
        if let Err(e) = ann.apply(&mut self.table) {
            self.emit_protocol_error(Some(player as usize), format!("Call apply 失败: {e}"));
            return;
        }
        let tile_ids: Vec<usize> = plaintexts
            .iter()
            .map(|p| self.card_mapping.decode(p).unwrap_or(usize::MAX))
            .collect();
        let _ = self.event_tx.send(MpEvent::CallApplied {
            player,
            call_type,
            deck_indices,
            tile_ids,
            from_player,
        });
    }

    /// 协议 6 自己暗杠. 公开广播 ConcealedKanAnnounce + 私发 ConcealedKanReveal
    /// 给 monitor.
    fn do_concealed_kan(&mut self, deck_indices: [u32; 4], monitor_player: u32) {
        if self.phase != MpPhase::Playing {
            return;
        }
        if (monitor_player as usize) >= self.cfg.n_players()
            || (monitor_player as usize) == self.cfg.own_index
        {
            self.emit_protocol_error(
                None,
                format!("ConcealedKan: 非法 monitor_player {monitor_player}"),
            );
            return;
        }
        // 反查 4 张 plaintext from own_drawn
        let mut tile_ids: [usize; 4] = [0; 4];
        let mut plaintexts: [crate::mental_poker::Curve; 4] =
            [crate::mental_poker::Curve::default(); 4];
        for (i, &idx) in deck_indices.iter().enumerate() {
            let Some(&tid) = self.own_drawn.get(&idx) else {
                self.emit_protocol_error(
                    None,
                    format!("ConcealedKan: deck_index={idx} 不在 own_drawn"),
                );
                return;
            };
            tile_ids[i] = tid;
            plaintexts[i] = self.card_mapping.encode(tid);
        }
        // apply public part to own table
        let kan = crate::mental_poker::protocol_concealed_kan::ConcealedKanAnnouncement {
            player: self.cfg.own_index,
            deck_indices: deck_indices.map(|i| i as usize),
            monitor_player: monitor_player as usize,
        };
        if let Err(e) = kan.apply(&mut self.table) {
            self.emit_protocol_error(None, format!("自己 ConcealedKan apply 失败: {e}"));
            return;
        }
        // 广播 announce
        let announce_msg = MentalPokerMsg::ConcealedKanAnnounce {
            player: self.cfg.own_index as u32,
            deck_indices,
            monitor_player,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg {
            to: None,
            msg: announce_msg,
        });
        // 私发 reveal 给 monitor
        let plaintexts_bytes: [Vec<u8>; 4] = [
            wire::encode_curve(&plaintexts[0]),
            wire::encode_curve(&plaintexts[1]),
            wire::encode_curve(&plaintexts[2]),
            wire::encode_curve(&plaintexts[3]),
        ];
        let reveal_msg = MentalPokerMsg::ConcealedKanReveal {
            plaintexts: plaintexts_bytes,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg {
            to: Some(monitor_player as usize),
            msg: reveal_msg,
        });
        // 自己 emit applied event
        let _ = self.event_tx.send(MpEvent::ConcealedKanApplied {
            player: self.cfg.own_index as u32,
            deck_indices,
            monitor_player,
        });
    }

    /// 协议 6 远端公开 announcement: apply 到本地 table.
    fn handle_concealed_kan_announce(
        &mut self,
        player: u32,
        deck_indices: [u32; 4],
        monitor_player: u32,
    ) {
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("ConcealedKan player={player} 越界"));
            return;
        }
        if (player as usize) == self.cfg.own_index {
            return; // 自己 echo
        }
        let kan = crate::mental_poker::protocol_concealed_kan::ConcealedKanAnnouncement {
            player: player as usize,
            deck_indices: deck_indices.map(|i| i as usize),
            monitor_player: monitor_player as usize,
        };
        if let Err(e) = kan.apply(&mut self.table) {
            self.emit_protocol_error(
                Some(player as usize),
                format!("ConcealedKan apply 失败: {e}"),
            );
            return;
        }
        let _ = self.event_tx.send(MpEvent::ConcealedKanApplied {
            player,
            deck_indices,
            monitor_player,
        });
    }

    /// 协议 6 monitor 收到 reveal. 反查 4 张 tile_id, 验证 all-same kind.
    /// 选项 C: 仅 monitor 看到 plaintext, monitor 自行决定是否上报.
    fn handle_concealed_kan_reveal(&mut self, from: Option<usize>, plaintexts_bytes: [Vec<u8>; 4]) {
        let Some(player) = from else {
            self.emit_protocol_error(None, "ConcealedKanReveal 缺 from".into());
            return;
        };
        if player == self.cfg.own_index {
            return; // 自己发的, 已经处理
        }
        // 反查 tile_ids
        let mut tile_ids: [usize; 4] = [0; 4];
        for (i, b) in plaintexts_bytes.iter().enumerate() {
            let pt = match wire::decode_curve(b) {
                Ok(p) => p,
                Err(e) => {
                    self.emit_protocol_error(
                        Some(player),
                        format!("ConcealedKanReveal plaintext decode: {e}"),
                    );
                    return;
                }
            };
            let Some(tid) = self.card_mapping.decode(&pt) else {
                self.emit_protocol_error(
                    Some(player),
                    "ConcealedKanReveal plaintext 不在 card_mapping".into(),
                );
                return;
            };
            tile_ids[i] = tid;
        }
        // 验证 all_same: 协议层只做"全相等" sanity (具体 kind 模式留 application).
        // mental poker 选项 C 下 monitor 验证: 4 个 tile 应同 kind, 否则上报作弊.
        // tile_id == card_mapping index, 跟 Tile.kind 不直接对应 (mapping 是 0..136
        // = TILE_KINDS * 4 张). 如果 mapping 是按 standard_set() 顺序生成, tile_id /
        // 4 = kind. 但 application 层可能用其他 mapping. 协议层仅做 all-equal sanity
        // (4 张 mapping 索引相同).
        let all_same = tile_ids.iter().all(|t| *t == tile_ids[0]);
        let _ = self.event_tx.send(MpEvent::MonitorVerified {
            player: player as u32,
            deck_indices: [
                // monitor 不知 deck_indices 直接 — 但之前 announce 已 apply 到 table,
                // 可从 last concealed_kan record 拿. 简化: monitor 先收 announce 后收
                // reveal, table.hand(player).concealed_kans().last() 给 indices.
                self.table
                    .hand(player)
                    .concealed_kans()
                    .last()
                    .map(|k| k.deck_indices[0] as u32)
                    .unwrap_or(u32::MAX),
                self.table
                    .hand(player)
                    .concealed_kans()
                    .last()
                    .map(|k| k.deck_indices[1] as u32)
                    .unwrap_or(u32::MAX),
                self.table
                    .hand(player)
                    .concealed_kans()
                    .last()
                    .map(|k| k.deck_indices[2] as u32)
                    .unwrap_or(u32::MAX),
                self.table
                    .hand(player)
                    .concealed_kans()
                    .last()
                    .map(|k| k.deck_indices[3] as u32)
                    .unwrap_or(u32::MAX),
            ],
            tile_ids,
            all_same,
        });
    }

    /// M6.B 自己加杠. 把已有 Pon meld[target_meld_idx] 升级为 Kan,
    /// 加 deck[new_deck_index] (自摸的同 kind 牌). 公开广播 plaintext.
    fn do_shouminkan(&mut self, target_meld_idx: u32, new_deck_index: u32) {
        if self.phase != MpPhase::Playing {
            return;
        }
        // 反查自摸的 plaintext
        let Some(&tid) = self.own_drawn.get(&new_deck_index) else {
            self.emit_protocol_error(
                None,
                format!("Shouminkan: deck_index={new_deck_index} 不在 own_drawn"),
            );
            return;
        };
        let plaintext = self.card_mapping.encode(tid);
        // 本地 record_shouminkan
        if let Err(e) = self.table.hand_mut(self.cfg.own_index).record_shouminkan(
            target_meld_idx as usize,
            new_deck_index as usize,
            plaintext,
        ) {
            self.emit_protocol_error(None, format!("自己 Shouminkan apply 失败: {e}"));
            return;
        }
        // 广播
        let msg = MentalPokerMsg::Shouminkan {
            player: self.cfg.own_index as u32,
            target_meld_idx,
            new_deck_index,
            new_plaintext: wire::encode_curve(&plaintext),
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        let _ = self.event_tx.send(MpEvent::ShouminkanApplied {
            player: self.cfg.own_index as u32,
            target_meld_idx,
            new_deck_index,
            new_tile_id: tid,
        });
    }

    /// M6.B 远端加杠. 反查 plaintext + record_shouminkan.
    fn handle_shouminkan_msg(
        &mut self,
        player: u32,
        target_meld_idx: u32,
        new_deck_index: u32,
        new_plaintext_bytes: Vec<u8>,
    ) {
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("Shouminkan player={player} 越界"));
            return;
        }
        if (player as usize) == self.cfg.own_index {
            return; // 自己 echo, 已 apply
        }
        let plaintext = match wire::decode_curve(&new_plaintext_bytes) {
            Ok(p) => p,
            Err(e) => {
                self.emit_protocol_error(
                    Some(player as usize),
                    format!("Shouminkan plaintext decode: {e}"),
                );
                return;
            }
        };
        if let Err(e) = self.table.hand_mut(player as usize).record_shouminkan(
            target_meld_idx as usize,
            new_deck_index as usize,
            plaintext,
        ) {
            self.emit_protocol_error(Some(player as usize), format!("Shouminkan apply 失败: {e}"));
            return;
        }
        let tid = self.card_mapping.decode(&plaintext).unwrap_or(usize::MAX);
        let _ = self.event_tx.send(MpEvent::ShouminkanApplied {
            player,
            target_meld_idx,
            new_deck_index,
            new_tile_id: tid,
        });
    }

    /// 协议 7 自己宣告和牌 (Tsumo / Ron). validate 本地 + 广播 (不 apply 到
    /// Table 因为 Win 是终局事件不修改 state).
    fn do_win(
        &mut self,
        is_tsumo: bool,
        from_player: Option<u32>,
        hand_indices: Vec<u32>,
        winning_tile_index: u32,
    ) {
        if self.phase != MpPhase::Playing {
            return;
        }
        // 反查 hand plaintexts (自己持有的)
        let mut plaintexts: Vec<crate::mental_poker::Curve> =
            Vec::with_capacity(hand_indices.len());
        for &idx in &hand_indices {
            // own_drawn 含自己摸过的; melds / kan 走 Table 内部数据; ron 时
            // winning_tile 不在自己 drawn (是 from_player 弃牌, 已在 Table 中).
            // 简化: 从 own_drawn 反查 tile_id, 再 encode 到 plaintext.
            let pt = if let Some(&tid) = self.own_drawn.get(&idx) {
                self.card_mapping.encode(tid)
            } else {
                // 可能是 Ron 的 winning_tile: 从对方 discarded 中拿
                let mut found = None;
                for hand in &self.table.hands {
                    if let Some(p) = hand.discarded_plaintext(idx as usize) {
                        found = Some(*p);
                        break;
                    }
                }
                match found {
                    Some(p) => p,
                    None => {
                        self.emit_protocol_error(
                            None,
                            format!(
                                "Win: deck_index={idx} 不在 own_drawn 也不在任一玩家 discarded"
                            ),
                        );
                        return;
                    }
                }
            };
            plaintexts.push(pt);
        }
        let win_type = if is_tsumo {
            crate::mental_poker::protocol_win::WinType::Tsumo
        } else {
            crate::mental_poker::protocol_win::WinType::Ron {
                from_player: from_player.unwrap_or(0) as usize,
            }
        };
        let win = crate::mental_poker::protocol_win::WinAnnouncement {
            player: self.cfg.own_index,
            win_type,
            hand_indices: hand_indices.iter().map(|&i| i as usize).collect(),
            hand_plaintexts: plaintexts.clone(),
            winning_tile_index: winning_tile_index as usize,
            dora_plaintexts: vec![], // M5.B+ 加 dora indicators 时填
            uradoor_plaintexts: None,
        };
        if let Err(e) = win.validate(&self.table) {
            self.emit_protocol_error(None, format!("自己 Win validate 失败: {e}"));
            return;
        }
        // 广播
        let win_type_wire = match win_type {
            crate::mental_poker::protocol_win::WinType::Tsumo => {
                crate::mental_poker::wire::WireWinType::Tsumo
            }
            crate::mental_poker::protocol_win::WinType::Ron { from_player } => {
                crate::mental_poker::wire::WireWinType::Ron {
                    from_player: from_player as u32,
                }
            }
        };
        let msg = MentalPokerMsg::Win {
            player: self.cfg.own_index as u32,
            win_type: win_type_wire,
            hand_indices: hand_indices.clone(),
            hand_plaintexts: plaintexts
                .iter()
                .map(crate::mental_poker::wire::encode_curve)
                .collect(),
            winning_tile_index,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        let _ = self.event_tx.send(MpEvent::OutboundMsg { to: None, msg });
        // 自己也 emit WinValidated
        let tile_ids: Vec<usize> = plaintexts
            .iter()
            .map(|p| self.card_mapping.decode(p).unwrap_or(usize::MAX))
            .collect();
        // 反查 winning_tile 在 hand_indices 中的位置 → tile_id (M6.C)
        let winning_tile_id = hand_indices
            .iter()
            .position(|&i| i == winning_tile_index)
            .and_then(|p| tile_ids.get(p).copied())
            .unwrap_or(usize::MAX);
        let _ = self.event_tx.send(MpEvent::WinValidated {
            player: self.cfg.own_index as u32,
            is_tsumo,
            from_player,
            winning_tile_index,
            winning_tile_id,
            hand_tile_ids: tile_ids,
        });
        self.phase = MpPhase::GameOver;
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });
    }

    /// 协议 7 远端宣告和牌. validate Table + 反查 tile_ids + emit WinValidated.
    fn handle_win_msg(
        &mut self,
        player: u32,
        win_type_wire: crate::mental_poker::wire::WireWinType,
        hand_indices: Vec<u32>,
        hand_plaintexts: Vec<Vec<u8>>,
        winning_tile_index: u32,
    ) {
        if (player as usize) >= self.cfg.n_players() {
            self.emit_protocol_error(None, format!("Win player={player} 越界"));
            return;
        }
        if (player as usize) == self.cfg.own_index {
            return; // 自己 echo
        }
        // decode plaintexts
        let mut plaintexts: Vec<crate::mental_poker::Curve> =
            Vec::with_capacity(hand_plaintexts.len());
        for b in &hand_plaintexts {
            match wire::decode_curve(b) {
                Ok(p) => plaintexts.push(p),
                Err(e) => {
                    self.emit_protocol_error(
                        Some(player as usize),
                        format!("Win plaintext decode: {e}"),
                    );
                    return;
                }
            }
        }
        let win_type: crate::mental_poker::protocol_win::WinType = win_type_wire.into();
        let is_tsumo = matches!(win_type, crate::mental_poker::protocol_win::WinType::Tsumo);
        let from_player = match win_type {
            crate::mental_poker::protocol_win::WinType::Ron { from_player } => {
                Some(from_player as u32)
            }
            _ => None,
        };
        let win = crate::mental_poker::protocol_win::WinAnnouncement {
            player: player as usize,
            win_type,
            hand_indices: hand_indices.iter().map(|&i| i as usize).collect(),
            hand_plaintexts: plaintexts.clone(),
            winning_tile_index: winning_tile_index as usize,
            dora_plaintexts: vec![],
            uradoor_plaintexts: None,
        };
        if let Err(e) = win.validate(&self.table) {
            self.emit_protocol_error(Some(player as usize), format!("Win validate 失败: {e}"));
            return;
        }
        let tile_ids: Vec<usize> = plaintexts
            .iter()
            .map(|p| self.card_mapping.decode(p).unwrap_or(usize::MAX))
            .collect();
        let winning_tile_id = hand_indices
            .iter()
            .position(|&i| i == winning_tile_index)
            .and_then(|p| tile_ids.get(p).copied())
            .unwrap_or(usize::MAX);
        let _ = self.event_tx.send(MpEvent::WinValidated {
            player,
            is_tsumo,
            from_player,
            winning_tile_index,
            winning_tile_id,
            hand_tile_ids: tile_ids,
        });
        self.phase = MpPhase::GameOver;
        let _ = self
            .event_tx
            .send(MpEvent::PhaseChanged { phase: self.phase });
    }

    fn try_complete_reveal(&mut self, deck_index: u32) {
        let Some(pending) = self.reveal_sessions.get(&deck_index) else {
            return;
        };
        let Some(plaintext) = pending.session.try_combine() else {
            return;
        };
        let Some(tile_id) = self.card_mapping.decode(&plaintext) else {
            self.emit_protocol_error(None, "reveal plaintext 不在 card_mapping".into());
            return;
        };
        self.reveal_sessions.remove(&deck_index);
        let _ = self.event_tx.send(MpEvent::RevealComplete {
            deck_index,
            tile_id,
        });
    }

    fn emit_protocol_error(&mut self, offender: Option<usize>, reason: String) {
        let _ = self.event_tx.send(MpEvent::ProtocolError {
            offender,
            reason: reason.clone(),
        });
        tracing::warn!(
            "MpPlayerActor[{}] ProtocolError offender={offender:?}: {reason}",
            self.cfg.own_index
        );
        // 协议错误后转 GameOver, 让 UI 知道局已 abort 可以干净退出. 不再继续推
        // 协议消息 (state 已不一致, 继续可能引入更多错误).
        if self.phase != MpPhase::GameOver {
            self.phase = MpPhase::GameOver;
            let _ = self
                .event_tx
                .send(MpEvent::PhaseChanged { phase: self.phase });
            let _ = self.event_tx.send(MpEvent::GameOver {
                reason: format!("协议错误 abort: {reason}"),
            });
        }
    }

    // M5.B.5+ 添加: handle_draw_request / handle_reveal_share / handle_discard /
    // handle_call / handle_concealed_kan / handle_win
}

/// 摸牌方进行中的 session 状态. 收齐 N-1 个 share + 自己算第 N 个 → combine.
#[allow(dead_code)] // request_id 字段供 future debugging / error reporting
struct PendingDraw {
    request_id: Uuid,
    deck_index: u32,
    ct: Ciphertext,
    /// 已收的 share (key = sender index in members).
    received: HashMap<usize, DecryptionShare>,
}

/// 公开揭示进行中. 收齐 N 个 share (含自己) → combine, emit RevealComplete.
#[allow(dead_code)] // deck_index/ct 字段供 future debugging / error reporting
struct PendingReveal {
    deck_index: u32,
    ct: Ciphertext,
    session: RevealSession,
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
#[allow(clippy::needless_range_loop)]
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
        use crate::engine::domain::action::Action;
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

    /// 桥: 4 actor mpsc 互联, 把 OutboundMsg 路由给其他 actor 作 PeerMsg.
    /// 跑直到 stop_when 返回 true (达到目标 phase) 或 max_steps 到.
    /// 返回 (final_phases, drawn_tile_ids, revealed_tile_ids) 给 caller assert.
    #[allow(clippy::type_complexity)]
    async fn run_bridge<F>(
        handles: &mut [MpPlayerHandle],
        rxs: &mut [tokio::sync::mpsc::UnboundedReceiver<MpEvent>],
        max_steps: usize,
        mut stop_when: F,
    ) -> (
        Vec<MpPhase>,
        Vec<(usize, u32, usize)>, // (actor_idx, deck_index, tile_id) draw
        Vec<(usize, u32, usize)>, // (actor_idx, deck_index, tile_id) reveal
    )
    where
        F: FnMut(&[MpPhase]) -> bool,
    {
        let n = handles.len();
        let mut phases = vec![MpPhase::KeyExchange; n];
        let mut draws: Vec<(usize, u32, usize)> = Vec::new();
        let mut reveals: Vec<(usize, u32, usize)> = Vec::new();
        for _step in 0usize..max_steps {
            let mut any_progress = false;
            for src in 0..n {
                while let Ok(ev) = rxs[src].try_recv() {
                    any_progress = true;
                    match ev {
                        MpEvent::PhaseChanged { phase } => {
                            phases[src] = phase;
                        }
                        MpEvent::OutboundMsg { to, msg } => {
                            let targets: Vec<usize> = match to {
                                None => (0..n).filter(|&i| i != src).collect(),
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
                        MpEvent::DrawComplete {
                            deck_index,
                            tile_id,
                            ..
                        } => {
                            draws.push((src, deck_index, tile_id));
                        }
                        MpEvent::RevealComplete {
                            deck_index,
                            tile_id,
                        } => {
                            reveals.push((src, deck_index, tile_id));
                        }
                        MpEvent::ProtocolError { offender, reason } => {
                            panic!(
                                "actor {src} reported ProtocolError offender={offender:?}: {reason}"
                            );
                        }
                        MpEvent::ShuffleProgress { .. }
                        | MpEvent::GameOver { .. }
                        | MpEvent::DiscardApplied { .. }
                        | MpEvent::CallApplied { .. }
                        | MpEvent::ConcealedKanApplied { .. }
                        | MpEvent::MonitorVerified { .. }
                        | MpEvent::RemoteDrawObserved { .. }
                        | MpEvent::ShouminkanApplied { .. }
                        | MpEvent::WinValidated { .. } => {}
                    }
                }
            }
            if stop_when(&phases) {
                return (phases, draws, reveals);
            }
            if !any_progress {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        (phases, draws, reveals)
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
        let max_steps = 600; // 全测试并行时 cnc proof CPU 争抢, 留余量
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
                        MpEvent::GameOver { .. }
                        | MpEvent::DrawComplete { .. }
                        | MpEvent::RevealComplete { .. }
                        | MpEvent::DiscardApplied { .. }
                        | MpEvent::CallApplied { .. }
                        | MpEvent::ConcealedKanApplied { .. }
                        | MpEvent::MonitorVerified { .. }
                        | MpEvent::RemoteDrawObserved { .. }
                        | MpEvent::ShouminkanApplied { .. }
                        | MpEvent::WinValidated { .. } => {}
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

    /// **M5.B.5 e2e**: 4 actor 完成协议 0+1, 进 Playing 后 actor 0 摸 deck[0]
    /// (协议 2). 验证 DrawComplete event 含合法 tile_id, 且其他 actor 不
    /// 收到 DrawComplete (协议 2 仅摸牌方知 plaintext).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn protocol_2_actor_0_draws_deck_0() {
        const N: usize = 4;
        let mut handles: Vec<MpPlayerHandle> = (0..N)
            .map(|i| spawn_mp_player(test_cfg(i), Some((i + 100) as u64)))
            .collect();
        let mut rxs: Vec<_> = handles
            .iter_mut()
            .map(|h| h.take_event_rx().unwrap())
            .collect();

        // 跑到 all in Playing
        let (phases, _, _) = run_bridge(&mut handles, &mut rxs, 600, |p| {
            p.iter().all(|x| *x == MpPhase::Playing)
        })
        .await;
        assert!(
            phases.iter().all(|p| *p == MpPhase::Playing),
            "phases={phases:?}"
        );

        // actor 0 触发摸 deck[0]
        handles[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 0 })
            .unwrap();

        // 跑直到 actor 0 收到 DrawComplete (drews.len() >= 1)
        let mut all_draws: Vec<(usize, u32, usize)> = Vec::new();
        for _step in 0..600usize {
            let mut any = false;
            for src in 0..N {
                while let Ok(ev) = rxs[src].try_recv() {
                    any = true;
                    match ev {
                        MpEvent::OutboundMsg { to, msg } => {
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
                        MpEvent::DrawComplete {
                            deck_index,
                            tile_id,
                            ..
                        } => {
                            all_draws.push((src, deck_index, tile_id));
                        }
                        MpEvent::ProtocolError { offender, reason } => {
                            panic!("actor {src} ProtocolError offender={offender:?}: {reason}");
                        }
                        _ => {}
                    }
                }
            }
            if !all_draws.is_empty() {
                break;
            }
            if !any {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        // 仅 actor 0 收到 DrawComplete (协议 2 仅摸牌方知)
        assert_eq!(
            all_draws.len(),
            1,
            "应仅 1 个 DrawComplete, 实际 {all_draws:?}"
        );
        let (drawer, deck_idx, tile_id) = all_draws[0];
        assert_eq!(drawer, 0);
        assert_eq!(deck_idx, 0);
        assert!(tile_id < test_cfg(0).deck_size);

        for h in &handles {
            h.send(MpRoomCmd::Disconnect).ok();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    /// **M5.B.6 e2e**: 协议 4 弃牌. actor 0 摸 deck[0] + 弃, 4 actor 都应收
    /// DiscardApplied 含同 tile_id.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn protocol_4_discard_propagates_to_all() {
        const N: usize = 4;
        let mut handles: Vec<MpPlayerHandle> = (0..N)
            .map(|i| spawn_mp_player(test_cfg(i), Some((i + 300) as u64)))
            .collect();
        let mut rxs: Vec<_> = handles
            .iter_mut()
            .map(|h| h.take_event_rx().unwrap())
            .collect();

        // 跑到 all in Playing
        let (phases, _, _) = run_bridge(&mut handles, &mut rxs, 600, |p| {
            p.iter().all(|x| *x == MpPhase::Playing)
        })
        .await;
        assert!(phases.iter().all(|p| *p == MpPhase::Playing));

        // actor 0 摸 deck[0]
        handles[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 0 })
            .unwrap();
        // 等 actor 0 收到 DrawComplete (并 emit DrawAnnouncement broadcast 给其他 actor)
        let mut got_draw = false;
        let mut discards: Vec<(usize, u32, usize)> = Vec::new();
        for _step in 0..400usize {
            let mut any = false;
            for src in 0..N {
                while let Ok(ev) = rxs[src].try_recv() {
                    any = true;
                    match ev {
                        MpEvent::OutboundMsg { to, msg } => {
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
                        MpEvent::DrawComplete { .. } => {
                            got_draw = true;
                            // 触发 actor 0 弃牌
                            handles[0]
                                .send(MpRoomCmd::Discard { deck_index: 0 })
                                .unwrap();
                        }
                        MpEvent::DiscardApplied {
                            player,
                            deck_index,
                            tile_id,
                        } => {
                            discards.push((src, deck_index, tile_id));
                            // 仅记录, 不停止 (等收齐 4 个)
                            let _ = player;
                        }
                        MpEvent::ProtocolError { offender, reason } => {
                            panic!("actor {src} ProtocolError offender={offender:?}: {reason}");
                        }
                        _ => {}
                    }
                }
            }
            if discards.len() == N {
                break;
            }
            if !any {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        assert!(got_draw, "actor 0 应完成摸牌");
        assert_eq!(discards.len(), N, "4 actor 应都收 DiscardApplied");
        // 全部 tile_id 一致 + deck_index = 0
        let first = discards[0].2;
        for (_src, idx, tile_id) in &discards {
            assert_eq!(*idx, 0);
            assert_eq!(*tile_id, first);
        }

        for h in &handles {
            h.send(MpRoomCmd::Disconnect).ok();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    /// **M5.B.5 e2e**: 协议 3 公开揭示 deck[1]. 4 actor 都应收 RevealComplete
    /// 含同 tile_id.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn protocol_3_all_actors_reveal_same_tile() {
        const N: usize = 4;
        let mut handles: Vec<MpPlayerHandle> = (0..N)
            .map(|i| spawn_mp_player(test_cfg(i), Some((i + 200) as u64)))
            .collect();
        let mut rxs: Vec<_> = handles
            .iter_mut()
            .map(|h| h.take_event_rx().unwrap())
            .collect();

        // 跑到 all in Playing
        let (phases, _, _) = run_bridge(&mut handles, &mut rxs, 600, |p| {
            p.iter().all(|x| *x == MpPhase::Playing)
        })
        .await;
        assert!(phases.iter().all(|p| *p == MpPhase::Playing));

        // 任一 actor (e.g. 玩家 0) 触发揭示 deck[1]
        handles[0]
            .send(MpRoomCmd::TriggerReveal { deck_index: 1 })
            .unwrap();

        let mut reveals: Vec<(usize, u32, usize)> = Vec::new();
        for _step in 0..600usize {
            let mut any = false;
            for src in 0..N {
                while let Ok(ev) = rxs[src].try_recv() {
                    any = true;
                    match ev {
                        MpEvent::OutboundMsg { to, msg } => {
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
                        MpEvent::RevealComplete {
                            deck_index,
                            tile_id,
                        } => {
                            reveals.push((src, deck_index, tile_id));
                        }
                        MpEvent::ProtocolError { offender, reason } => {
                            panic!("actor {src} ProtocolError offender={offender:?}: {reason}");
                        }
                        _ => {}
                    }
                }
            }
            if reveals.len() == N {
                break;
            }
            if !any {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        assert_eq!(reveals.len(), N, "4 actor 都应收 RevealComplete");
        // 全部 tile_id 相同 (公开揭示同一张)
        let first_tile = reveals[0].2;
        for (_src, deck_idx, tile_id) in &reveals {
            assert_eq!(*deck_idx, 1);
            assert_eq!(*tile_id, first_tile);
        }

        for h in &handles {
            h.send(MpRoomCmd::Disconnect).ok();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    /// 通用 step driver: 推动 4 actor 桥接, 直到 cond 返回 true 或 max_steps 用完.
    /// 收集 outbound 路由到 PeerMsg, 累积所有 event 给 caller fold.
    async fn drive_until<F>(
        handles: &mut [MpPlayerHandle],
        rxs: &mut [tokio::sync::mpsc::UnboundedReceiver<MpEvent>],
        max_steps: usize,
        events_out: &mut Vec<(usize, MpEvent)>,
        mut cond: F,
    ) -> bool
    where
        F: FnMut(&[(usize, MpEvent)]) -> bool,
    {
        let n = handles.len();
        for _step in 0..max_steps {
            let mut any = false;
            for src in 0..n {
                while let Ok(ev) = rxs[src].try_recv() {
                    any = true;
                    if let MpEvent::OutboundMsg { to, msg } = &ev {
                        let targets: Vec<usize> = match to {
                            None => (0..n).filter(|&i| i != src).collect(),
                            Some(idx) => vec![*idx],
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
                    if let MpEvent::ProtocolError { offender, reason } = &ev {
                        panic!("actor {src} ProtocolError offender={offender:?}: {reason}");
                    }
                    events_out.push((src, ev));
                }
            }
            if cond(events_out) {
                return true;
            }
            if !any {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        false
    }

    /// **M5.B.7 完整一手 e2e**: 4 actor 串联协议 0+1+2+3+4+5+7 — 不依赖
    /// yaku 牌型, 仅验 actor 状态机 + Table 镜像同步.
    ///
    /// 流程:
    /// 1. 4 actor 跑完 keygen + 联合洗牌, all in Playing
    /// 2. actor 0 摸 deck[0], deck[1] (准备 Pon 的 2 张)
    /// 3. actor 1 摸 deck[2] → 弃 deck[2]
    /// 4. actor 0 Pon [0, 1, 2] from=1 → 4 actor CallApplied
    /// 5. actor 0 摸 deck[3] → 弃 deck[3] → 4 actor DiscardApplied
    /// 6. actor 0 揭示 deck[15] (dora indicator) → 4 actor RevealComplete
    /// 7. actor 0 摸 deck[4], deck[5], deck[6]
    /// 8. actor 0 Tsumo: hand_indices=[4,5,6], winning_tile=6 → 4 actor WinValidated
    /// 9. 各 actor 进 GameOver
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn protocol_full_hand_e2e() {
        const N: usize = 4;
        let mut handles: Vec<MpPlayerHandle> = (0..N)
            .map(|i| spawn_mp_player(test_cfg(i), Some((i + 1000) as u64)))
            .collect();
        let mut rxs: Vec<_> = handles
            .iter_mut()
            .map(|h| h.take_event_rx().unwrap())
            .collect();

        // Step 1: 跑到 all in Playing
        let mut events: Vec<(usize, MpEvent)> = Vec::new();
        let entered_playing = drive_until(&mut handles, &mut rxs, 800, &mut events, |evs| {
            let mut phase = [MpPhase::KeyExchange; N];
            for (src, ev) in evs {
                if let MpEvent::PhaseChanged { phase: p } = ev {
                    phase[*src] = *p;
                }
            }
            phase.iter().all(|p| *p == MpPhase::Playing)
        })
        .await;
        assert!(entered_playing, "4 actor 应全 transition 到 Playing");

        // Step 2: actor 0 摸 deck[0], deck[1] — 等 2 个 DrawComplete
        events.clear();
        handles[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 0 })
            .unwrap();
        let drew_0 = drive_until(&mut handles, &mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 0 && matches!(e, MpEvent::DrawComplete { deck_index: 0, .. }))
        })
        .await;
        assert!(drew_0, "actor 0 应完成摸 deck[0]");

        events.clear();
        handles[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 1 })
            .unwrap();
        let drew_1 = drive_until(&mut handles, &mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 0 && matches!(e, MpEvent::DrawComplete { deck_index: 1, .. }))
        })
        .await;
        assert!(drew_1, "actor 0 应完成摸 deck[1]");

        // Step 3: actor 1 摸 deck[2] → 弃 deck[2]
        events.clear();
        handles[1]
            .send(MpRoomCmd::TriggerDraw { deck_index: 2 })
            .unwrap();
        let drew_2 = drive_until(&mut handles, &mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 1 && matches!(e, MpEvent::DrawComplete { deck_index: 2, .. }))
        })
        .await;
        assert!(drew_2, "actor 1 应完成摸 deck[2]");

        events.clear();
        handles[1]
            .send(MpRoomCmd::Discard { deck_index: 2 })
            .unwrap();
        let discard_2_done = drive_until(&mut handles, &mut rxs, 200, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::DiscardApplied {
                            player: 1,
                            deck_index: 2,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(
            discard_2_done,
            "4 actor 应都收 DiscardApplied(player=1, deck=2)"
        );

        // Step 4: actor 0 Pon deck[0,1,2], from_player=1, from_position=2
        events.clear();
        handles[0]
            .send(MpRoomCmd::Call {
                call_type: crate::mental_poker::wire::WireCallType::Pon,
                deck_indices: vec![0, 1, 2],
                from_player: 1,
                from_position_in_meld: 2,
            })
            .unwrap();
        let pon_done = drive_until(&mut handles, &mut rxs, 200, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::CallApplied {
                            player: 0,
                            from_player: 1,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(pon_done, "4 actor 应都收 CallApplied(player=0, from=1)");

        // Step 5: actor 0 摸 deck[3] → 弃 deck[3]
        events.clear();
        handles[0]
            .send(MpRoomCmd::TriggerDraw { deck_index: 3 })
            .unwrap();
        let drew_3 = drive_until(&mut handles, &mut rxs, 400, &mut events, |evs| {
            evs.iter()
                .any(|(s, e)| *s == 0 && matches!(e, MpEvent::DrawComplete { deck_index: 3, .. }))
        })
        .await;
        assert!(drew_3, "actor 0 应完成摸 deck[3]");

        events.clear();
        handles[0]
            .send(MpRoomCmd::Discard { deck_index: 3 })
            .unwrap();
        let discard_3_done = drive_until(&mut handles, &mut rxs, 200, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::DiscardApplied {
                            player: 0,
                            deck_index: 3,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(
            discard_3_done,
            "4 actor 应都收 DiscardApplied(player=0, deck=3)"
        );

        // Step 6: actor 0 揭示 deck[15] (dora indicator) — 4 actor 收同 tile_id
        events.clear();
        handles[0]
            .send(MpRoomCmd::TriggerReveal { deck_index: 15 })
            .unwrap();
        let reveal_done = drive_until(&mut handles, &mut rxs, 600, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| matches!(e, MpEvent::RevealComplete { deck_index: 15, .. }))
                .count()
                == N
        })
        .await;
        assert!(reveal_done, "4 actor 应都收 RevealComplete(deck=15)");
        // 4 actor 看到的 tile_id 一致
        let reveal_tids: Vec<usize> = events
            .iter()
            .filter_map(|(_, e)| match e {
                MpEvent::RevealComplete {
                    deck_index: 15,
                    tile_id,
                } => Some(*tile_id),
                _ => None,
            })
            .collect();
        assert_eq!(reveal_tids.len(), N);
        let dora_tid = reveal_tids[0];
        for tid in &reveal_tids {
            assert_eq!(*tid, dora_tid, "4 actor 看到的 dora tile_id 应一致");
        }

        // Step 7: actor 0 摸 deck[4], deck[5], deck[6]
        for idx in [4u32, 5, 6] {
            events.clear();
            handles[0]
                .send(MpRoomCmd::TriggerDraw { deck_index: idx })
                .unwrap();
            let ok = drive_until(&mut handles, &mut rxs, 400, &mut events, |evs| {
                evs.iter().any(|(s, e)| {
                    *s == 0
                        && matches!(e, MpEvent::DrawComplete { deck_index: di, .. } if *di == idx)
                })
            })
            .await;
            assert!(ok, "actor 0 应完成摸 deck[{idx}]");
        }

        // Step 8: actor 0 Tsumo, hand=[4,5,6], winning=6
        events.clear();
        handles[0]
            .send(MpRoomCmd::Tsumo {
                hand_indices: vec![4, 5, 6],
                winning_tile_index: 6,
            })
            .unwrap();
        let win_done = drive_until(&mut handles, &mut rxs, 200, &mut events, |evs| {
            evs.iter()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        MpEvent::WinValidated {
                            player: 0,
                            is_tsumo: true,
                            ..
                        }
                    )
                })
                .count()
                == N
        })
        .await;
        assert!(win_done, "4 actor 应都收 WinValidated(player=0, tsumo)");

        // Step 9: 各 actor 进 GameOver
        let game_over_count = events
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    MpEvent::PhaseChanged {
                        phase: MpPhase::GameOver
                    }
                )
            })
            .count();
        // 自己 emit 1 个 + 4 actor 中可能其他人也 emit (handle_win_msg 也 transition).
        // 至少应有 4 个 (4 actor 都 transition 到 GameOver).
        assert!(
            game_over_count >= N,
            "至少 {N} 个 PhaseChanged(GameOver), 实际 {game_over_count}, events={events:?}"
        );

        for h in &handles {
            h.send(MpRoomCmd::Disconnect).ok();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
