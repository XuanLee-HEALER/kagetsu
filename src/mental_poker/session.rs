//! Session 状态机 — 协议层封装 (M4.F).
//!
//! 把 协议 1/2/3 的多方协调状态机抽象出来, 给后续 RoomActor 集成提供干净
//! 的 API 边界. 每个 session 状态机:
//! - 维护 "等待谁提交 / 已收谁的提交" 状态
//! - submit 时验证 ZK proof / DLEQ proof
//! - 全部收齐后产出结果 (final_deck / plaintext)
//!
//! ## 当前阶段范围 (M4.F 第一版)
//! 提供 Session 状态机 API + 协议层 e2e 测试. **不做** RoomActor 实际集成 —
//! 集成涉及 RoomActor (1655 LOC) + GameState (1012 LOC) 大重构, 留给后续
//! phase (跟 M5 断线重洗一起设计).
//!
//! 后续 RoomActor 集成时 import 这些 Session struct, 在 lobby phase 启动
//! ShuffleSession, 每次摸牌 spawn DrawSession, dora 揭示 spawn RevealSession.

use std::collections::HashMap;
use thiserror::Error;

use super::Curve;
use super::cut_and_choose::{self, ShuffleProof};
use super::elgamal::{Ciphertext, mask};
use super::joint_key::JointPublicKey;
use super::protocol_reveal::{self, MemberInfo, RevealShare};

// ============================================================================
// ShuffleSession — 协议 1 (联合洗牌)
// ============================================================================

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShuffleError {
    #[error("成员数 {got} 不在合法范围 (≥2)")]
    InsufficientMembers { got: usize },
    #[error("当前等待玩家 {expected} 但收到玩家 {got} 的提交")]
    WrongActor { expected: usize, got: usize },
    #[error("玩家 {player} 提交的 deck 长度 {got} 不等于初始 deck 长度 {expected}")]
    DeckLengthMismatch {
        player: usize,
        got: usize,
        expected: usize,
    },
    #[error("玩家 {player} 的 shuffle proof 验证失败")]
    InvalidProof { player: usize },
    #[error("session 已完成, 不能再 submit")]
    AlreadyComplete,
}

/// 协议 1 (4 玩家联合洗牌) 状态机.
///
/// ## Lifecycle
/// 1. `ShuffleSession::start(members, plaintexts)` — 加密初始 deck under jpk
/// 2. 当前等待玩家 = `next_actor()` (从 0 开始)
/// 3. 该玩家计算 (out_deck, π, r) + cut-and-choose proof, 调用 `submit_round`
/// 4. session verify proof, 通过则进入下一轮; 失败返回 `ShuffleError`
/// 5. 全部 N 轮提交完后 `is_complete() == true`, `final_deck()` 给最终牌山
pub struct ShuffleSession {
    members: Vec<MemberInfo>,
    jpk: JointPublicKey,
    initial_deck: Vec<Ciphertext>,
    /// `decks[k]` = 玩家 k 提交后的牌山. `decks[0]` = initial. session 完成时
    /// `decks` 长度 = members.len() + 1.
    decks: Vec<Vec<Ciphertext>>,
    proofs: Vec<ShuffleProof>,
    next_actor: usize,
    cnc_k_rounds: usize,
}

impl ShuffleSession {
    /// 启动 session. `members` 必须是已 schnorr-verified + jpk-aggregated 的玩家
    /// 列表 (顺序 deterministic, 影响 shuffle 顺序).
    /// `plaintexts` 是初始牌的 Curve point representation (由 Card mapping
    /// 给出, application 层负责 Card → Curve 映射).
    pub fn start(
        members: Vec<MemberInfo>,
        jpk: JointPublicKey,
        plaintexts: Vec<Curve>,
        cnc_k_rounds: usize,
    ) -> Result<Self, ShuffleError> {
        if members.len() < 2 {
            return Err(ShuffleError::InsufficientMembers { got: members.len() });
        }
        // 加密初始 deck under jpk. 没有 randomness 保留 (初始加密不参与
        // shuffle re-encryption proof, 它只是公开 commit).
        let pk = jpk.as_pk();
        let mut rng = ark_std::test_rng();
        let initial_deck: Vec<Ciphertext> = plaintexts
            .iter()
            .map(|m| mask(&mut rng, &pk, m).0)
            .collect();
        let decks = vec![initial_deck.clone()];
        Ok(Self {
            members,
            jpk,
            initial_deck,
            decks,
            proofs: Vec::new(),
            next_actor: 0,
            cnc_k_rounds,
        })
    }

    pub fn n(&self) -> usize {
        self.initial_deck.len()
    }

    pub fn next_actor(&self) -> usize {
        self.next_actor
    }

    pub fn is_complete(&self) -> bool {
        self.next_actor >= self.members.len()
    }

    /// 当前轮的输入 deck (要 shuffle 的源 deck). 给当前 actor 看.
    pub fn current_input_deck(&self) -> &[Ciphertext] {
        self.decks.last().expect("decks 至少有 initial")
    }

    pub fn jpk(&self) -> &JointPublicKey {
        &self.jpk
    }

    pub fn members(&self) -> &[MemberInfo] {
        &self.members
    }

    /// 玩家 idx 提交自己的 shuffle 结果.
    pub fn submit_round(
        &mut self,
        player_idx: usize,
        new_deck: Vec<Ciphertext>,
        proof: ShuffleProof,
    ) -> Result<(), ShuffleError> {
        if self.is_complete() {
            return Err(ShuffleError::AlreadyComplete);
        }
        if player_idx != self.next_actor {
            return Err(ShuffleError::WrongActor {
                expected: self.next_actor,
                got: player_idx,
            });
        }
        let input_deck = self.current_input_deck();
        if new_deck.len() != input_deck.len() {
            return Err(ShuffleError::DeckLengthMismatch {
                player: player_idx,
                got: new_deck.len(),
                expected: input_deck.len(),
            });
        }
        if !cut_and_choose::verify(&self.jpk.as_pk(), input_deck, &new_deck, &proof) {
            return Err(ShuffleError::InvalidProof { player: player_idx });
        }
        self.decks.push(new_deck);
        self.proofs.push(proof);
        self.next_actor += 1;
        Ok(())
    }

    /// 完成后取最终牌山.
    pub fn final_deck(&self) -> Option<&[Ciphertext]> {
        if self.is_complete() {
            self.decks.last().map(|v| v.as_slice())
        } else {
            None
        }
    }

    /// cut-and-choose K 轮 (供 caller 跑 prove 时用同样参数).
    pub fn cnc_k_rounds(&self) -> usize {
        self.cnc_k_rounds
    }
}

// ============================================================================
// ThresholdDecryptSession — 协议 2 + 3 共用基础
// ============================================================================

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecryptError {
    #[error("玩家 {player} 不在 session members 列表内")]
    UnknownPlayer { player: usize },
    #[error("玩家 {player} 已提交过 share")]
    DuplicateSubmission { player: usize },
    #[error("玩家 {player} 的 share DLEQ 验证失败")]
    InvalidShare { player: usize },
}

/// 通用 threshold decryption session (协议 2 摸牌 / 协议 3 揭示共用).
///
/// 跟具体协议无关 — 仅维护 "已收齐谁的 share". 协议 2 需求是收齐 N 个 (含
/// 自己), 协议 3 同样是 N 个但所有人都参与 broadcast 而不仅 X.
pub struct ThresholdDecryptSession {
    members: Vec<MemberInfo>,
    ct: Ciphertext,
    received: HashMap<usize, RevealShare>,
}

impl ThresholdDecryptSession {
    pub fn new(members: Vec<MemberInfo>, ct: Ciphertext) -> Self {
        Self {
            members,
            ct,
            received: HashMap::new(),
        }
    }

    pub fn ciphertext(&self) -> &Ciphertext {
        &self.ct
    }

    pub fn members(&self) -> &[MemberInfo] {
        &self.members
    }

    /// 已收 share 的玩家集合.
    pub fn received_count(&self) -> usize {
        self.received.len()
    }

    /// 是否全部 N 个 share 收齐.
    pub fn is_ready(&self) -> bool {
        self.received.len() == self.members.len()
    }

    /// 玩家 idx 提交 share. verify_one 通过才接受.
    pub fn submit(
        &mut self,
        player_idx: usize,
        contribution: RevealShare,
    ) -> Result<(), DecryptError> {
        if player_idx >= self.members.len() {
            return Err(DecryptError::UnknownPlayer { player: player_idx });
        }
        if self.received.contains_key(&player_idx) {
            return Err(DecryptError::DuplicateSubmission { player: player_idx });
        }
        let m = &self.members[player_idx];
        if !protocol_reveal::verify_one(&m.pk, &self.ct, &contribution, &m.peer_id) {
            return Err(DecryptError::InvalidShare { player: player_idx });
        }
        self.received.insert(player_idx, contribution);
        Ok(())
    }

    /// 全部 share 收齐后 combine 恢复明文. 否则 None.
    pub fn try_combine(&self) -> Option<Curve> {
        if !self.is_ready() {
            return None;
        }
        let contributions: Vec<RevealShare> =
            (0..self.members.len()).map(|i| self.received[&i]).collect();
        // verify 已经 submit 时通过, 这里 Err 路径不应走到.
        protocol_reveal::public_reveal(&self.members, &self.ct, &contributions).ok()
    }
}

/// 协议 2 (摸牌). 语义化包装 [`ThresholdDecryptSession`]: 仅 requesting player
/// 拿到结果, 其他人的 share 都给 X.
pub type DrawSession = ThresholdDecryptSession;

/// 协议 3 (公开揭示). 同样底层, 但语义上所有人都收齐后 combine.
pub type RevealSession = ThresholdDecryptSession;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::cut_and_choose;
    use crate::mental_poker::elgamal::{PublicKey, SecretKey, keygen};
    use crate::mental_poker::joint_key::aggregate;
    use crate::mental_poker::protocol_reveal::prepare_share;
    use crate::mental_poker::schnorr;
    use crate::mental_poker::shuffle::shuffle_and_remask;
    use ark_ff::UniformRand;
    use ark_std::test_rng;
    use std::collections::HashMap;

    /// 构造 4 玩家场景, 返回 (sks, members, jpk).
    fn setup_4_players() -> (
        Vec<SecretKey>,
        Vec<PublicKey>,
        Vec<MemberInfo>,
        JointPublicKey,
    ) {
        let rng = &mut test_rng();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut members = Vec::new();
        let mut entries = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            members.push(MemberInfo {
                peer_id: peer_id.clone(),
                pk,
            });
            entries.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();
        (sks, pks, members, jpk)
    }

    /// ShuffleSession honest 流程: 4 玩家轮流 submit, session 完成后给出 final_deck.
    #[test]
    fn shuffle_session_honest_4_players() {
        let rng = &mut test_rng();
        let (_sks, _pks, members, jpk) = setup_4_players();
        let n = 16usize;
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let mut session = ShuffleSession::start(members, jpk, plaintexts.clone(), 20).unwrap();

        for player in 0..4 {
            assert_eq!(session.next_actor(), player);
            let input = session.current_input_deck().to_vec();
            let (out, pi, r) = shuffle_and_remask(rng, &session.jpk().as_pk(), &input);
            let proof = cut_and_choose::prove(
                rng,
                &session.jpk().as_pk(),
                &input,
                &out,
                &pi,
                &r,
                session.cnc_k_rounds(),
            );
            session.submit_round(player, out, proof).unwrap();
        }

        assert!(session.is_complete());
        let final_deck = session.final_deck().unwrap();
        assert_eq!(final_deck.len(), n);
    }

    /// 错误 actor (跳过 player 0, 直接交 player 1) → WrongActor.
    #[test]
    fn shuffle_session_wrong_actor_rejected() {
        let rng = &mut test_rng();
        let (_, _, members, jpk) = setup_4_players();
        let n = 8;
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let mut session = ShuffleSession::start(members, jpk, plaintexts, 20).unwrap();

        let input = session.current_input_deck().to_vec();
        let (out, pi, r) = shuffle_and_remask(rng, &session.jpk().as_pk(), &input);
        let proof = cut_and_choose::prove(rng, &session.jpk().as_pk(), &input, &out, &pi, &r, 20);
        let err = session.submit_round(1, out, proof).unwrap_err();
        assert_eq!(
            err,
            ShuffleError::WrongActor {
                expected: 0,
                got: 1
            }
        );
    }

    /// invalid proof → InvalidProof.
    #[test]
    fn shuffle_session_invalid_proof_rejected() {
        let rng = &mut test_rng();
        let (_, _, members, jpk) = setup_4_players();
        let n = 8;
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let mut session = ShuffleSession::start(members, jpk, plaintexts, 20).unwrap();

        let input = session.current_input_deck().to_vec();
        let (out, pi, r) = shuffle_and_remask(rng, &session.jpk().as_pk(), &input);
        let mut proof =
            cut_and_choose::prove(rng, &session.jpk().as_pk(), &input, &out, &pi, &r, 20);
        // 篡改 proof
        proof.intermediates[0][0].c1 += session.jpk().as_pk().0;
        let err = session.submit_round(0, out, proof).unwrap_err();
        assert_eq!(err, ShuffleError::InvalidProof { player: 0 });
    }

    /// DrawSession (协议 2) 摸牌: 玩家 0 收齐 4 个 share + combine 拿明文.
    #[test]
    fn draw_session_player_0_draws_card() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let tile = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &tile);

        let mut session = DrawSession::new(members.clone(), ct);
        for i in 0..4 {
            let contribution = prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id);
            session.submit(i, contribution).unwrap();
        }
        assert!(session.is_ready());
        let recovered = session.try_combine().unwrap();
        assert_eq!(recovered, tile);
    }

    /// DrawSession invalid share rejected.
    #[test]
    fn draw_session_invalid_share_rejected() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let tile = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &tile);

        let mut session = DrawSession::new(members.clone(), ct);
        let mut bad = prepare_share(rng, &sks[0], &pks[0], &ct, &members[0].peer_id);
        bad.share.0 += Curve::generator();
        let err = session.submit(0, bad).unwrap_err();
        assert_eq!(err, DecryptError::InvalidShare { player: 0 });
    }

    /// DrawSession duplicate submission rejected.
    #[test]
    fn draw_session_duplicate_submission_rejected() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let tile = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &tile);

        let mut session = DrawSession::new(members.clone(), ct);
        let c = prepare_share(rng, &sks[0], &pks[0], &ct, &members[0].peer_id);
        session.submit(0, c).unwrap();
        let c2 = prepare_share(rng, &sks[0], &pks[0], &ct, &members[0].peer_id);
        let err = session.submit(0, c2).unwrap_err();
        assert_eq!(err, DecryptError::DuplicateSubmission { player: 0 });
    }

    use ark_ec::PrimeGroup;

    /// **协议 1 + 2 + 3 完整 e2e**: 4 玩家联合洗牌 → 玩家 0 摸 8 张牌 → 揭示 1 张 dora.
    /// 验证摸到的 8 张 + dora 全部都来自 initial plaintexts 集合.
    #[test]
    fn protocol_1_2_3_full_e2e() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let n = 16usize; // 加速测试
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let plaintext_set: std::collections::HashSet<String> =
            plaintexts.iter().map(|p| format!("{p}")).collect();

        // ===== 协议 1: 联合洗牌 =====
        let mut shuffle_sess = ShuffleSession::start(members.clone(), jpk, plaintexts, 20).unwrap();
        for player in 0..4 {
            let input = shuffle_sess.current_input_deck().to_vec();
            let (out, pi, r) = shuffle_and_remask(rng, &shuffle_sess.jpk().as_pk(), &input);
            let proof = cut_and_choose::prove(
                rng,
                &shuffle_sess.jpk().as_pk(),
                &input,
                &out,
                &pi,
                &r,
                shuffle_sess.cnc_k_rounds(),
            );
            shuffle_sess.submit_round(player, out, proof).unwrap();
        }
        let final_deck = shuffle_sess.final_deck().unwrap().to_vec();

        // ===== 协议 2: 玩家 0 摸 8 张牌 =====
        let draw_n = 8;
        let mut drawn_tiles = Vec::new();
        for ct in final_deck.iter().take(draw_n).copied() {
            let mut draw_sess = DrawSession::new(members.clone(), ct);
            for i in 0..4 {
                let c = prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id);
                draw_sess.submit(i, c).unwrap();
            }
            let tile = draw_sess.try_combine().unwrap();
            // tile 应在 initial plaintext 集合内
            assert!(plaintext_set.contains(&format!("{tile}")));
            drawn_tiles.push(tile);
        }
        // 8 张全不同 (因为 deck 长度 16, 都是不同位置)
        let drawn_set: std::collections::HashSet<String> =
            drawn_tiles.iter().map(|t| format!("{t}")).collect();
        assert_eq!(drawn_set.len(), draw_n);

        // ===== 协议 3: 揭示牌山的 dora indicator (deck[draw_n]) =====
        let dora_ct = final_deck[draw_n];
        let mut reveal_sess = RevealSession::new(members.clone(), dora_ct);
        for i in 0..4 {
            let c = prepare_share(rng, &sks[i], &pks[i], &dora_ct, &members[i].peer_id);
            reveal_sess.submit(i, c).unwrap();
        }
        let dora = reveal_sess.try_combine().unwrap();
        assert!(plaintext_set.contains(&format!("{dora}")));
        // dora 不应在已摸过的 tiles 里 (deck[draw_n] != deck[0..draw_n])
        assert!(!drawn_set.contains(&format!("{dora}")));
    }

    /// 协议 2 / 3 同 ct 应得相同 plaintext (跨 session 一致性).
    #[test]
    fn draw_and_reveal_session_agree() {
        let rng = &mut test_rng();
        let (sks, pks, members, jpk) = setup_4_players();
        let m = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &m);

        let mut draw_sess = DrawSession::new(members.clone(), ct);
        let mut reveal_sess = RevealSession::new(members.clone(), ct);
        for i in 0..4 {
            let c = prepare_share(rng, &sks[i], &pks[i], &ct, &members[i].peer_id);
            draw_sess.submit(i, c).unwrap();
            reveal_sess.submit(i, c).unwrap();
        }
        let p1 = draw_sess.try_combine().unwrap();
        let p2 = reveal_sess.try_combine().unwrap();
        assert_eq!(p1, p2);
        assert_eq!(p1, m);
    }

    // 抑制 unused 警告
    #[allow(dead_code)]
    fn _hashmap_used() -> HashMap<usize, RevealShare> {
        HashMap::new()
    }
}
