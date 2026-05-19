//! Sako-Killian Cut-and-Choose Shuffle Proof (M4.C.3).
//!
//! ## 协议
//! 给定输入密文向量 C, 输出密文向量 C', prover 知道 (π, r) 使得
//! C'_i = ReEnc(C_{π(i)}, r_i, PK) (整体 shuffle + remask 关系). 该协议
//! 让 prover 证明此关系而不揭示 π 和 r.
//!
//! ## 思路 (cut-and-choose)
//! 重复 K 轮, 每轮 prover 把整体 shuffle 拆成两步:
//!   - σ_k 预 shuffle:  D^{(k)}_i = ReEnc(C_{σ_k(i)}, s_k[i])
//!   - τ_k 后 shuffle:  C'_i = ReEnc(D^{(k)}_{τ_k(i)}, t_k[i])
//! 其中 τ_k = σ_k^{-1} ∘ π, t_k[i] = r[i] - s_k[τ_k(i)] (composition 等于 π).
//!
//! Verifier 派生 challenge bit b_k:
//!   - b_k = 0: prover 揭示 (σ_k, s_k), verifier 检查 D^{(k)} = ReEnc(C ∘ σ_k, s_k)
//!   - b_k = 1: prover 揭示 (τ_k, t_k), verifier 检查 C' = ReEnc(D^{(k)} ∘ τ_k, t_k)
//!
//! Prover 只 commit 一侧的话另一侧揭不开. 任一轮欺骗以 1/2 概率被抓; K 轮
//! 总作弊成功率 2^{-K}. K=80 给 80-bit 安全性.
//!
//! ## 大小 vs 安全 tradeoff
//! 每轮 proof 含: D^{(k)} (N 个 ciphertext = 2N curve point) + 一侧 (σ, s) 或
//! (τ, t) (N 个 index + N 个 scalar). 对 N=136, K=80 ≈ 1.5MB. 较大但作开局
//! 一次性数据传输 OK.
//!
//! ## hide π 的 ZK 性质
//! 每轮揭一侧, 另一侧 σ_k / τ_k 完全随机, 互相 mask. 经过 K 轮, attacker 看
//! 到 K/2 个 σ_k (随机) 和 K/2 个 τ_k (= σ_k^{-1} ∘ π for those k 但 σ_k 没揭),
//! 不能从中恢复 π.

use ark_ff::UniformRand;
use ark_serialize::CanonicalSerialize;
use ark_std::rand::Rng;

use super::Scalar;
use super::elgamal::{Ciphertext, PublicKey, remask};
use super::shuffle::Permutation;
use super::transcript::Transcript;

/// 默认 cut-and-choose 重复轮数. 80 给 80-bit 安全.
/// 测试可用更小值加速 (e.g. 20 → 1M 分之 1 概率, 也足够调试).
pub const DEFAULT_K: usize = 80;

const DOMAIN: &[u8] = b"tui-majo/mp/cut-and-choose-shuffle/v1";

/// 证明.
#[derive(Debug, Clone)]
pub struct ShuffleProof {
    /// K 个 intermediate ciphertext 向量 D^{(k)}, 每个长度 N.
    pub intermediates: Vec<Vec<Ciphertext>>,
    /// K 个 opening (跟 challenge bit 对应).
    pub openings: Vec<Opening>,
}

/// 单轮揭示的内容. challenge bit 决定这一侧.
#[derive(Debug, Clone)]
pub enum Opening {
    /// b_k = 0: 揭示 σ_k 和 s_k.
    PreShuffle { sigma: Permutation, s: Vec<Scalar> },
    /// b_k = 1: 揭示 τ_k 和 t_k.
    PostShuffle { tau: Permutation, t: Vec<Scalar> },
}

/// 生成证明.
///
/// `c_in`, `c_out`: 输入 / 输出密文向量, 长度同, prover 已计算好.
/// `pi`, `r`: prover 知道的 witness, c_out[i] = ReEnc(c_in[π(i)], r[i], PK).
/// `k_rounds`: cut-and-choose 重复轮数, 默认 [`DEFAULT_K`] = 80.
pub fn prove<R: Rng + ?Sized>(
    rng: &mut R,
    pk: &PublicKey,
    c_in: &[Ciphertext],
    c_out: &[Ciphertext],
    pi: &Permutation,
    r: &[Scalar],
    k_rounds: usize,
) -> ShuffleProof {
    let n = c_in.len();
    assert_eq!(c_out.len(), n);
    assert_eq!(pi.len(), n);
    assert_eq!(r.len(), n);
    assert!(pi.is_valid(), "π 必须是合法 permutation");

    // 预生成所有轮的 (σ_k, s_k), 计算 D^{(k)}.
    let mut sigmas: Vec<Permutation> = Vec::with_capacity(k_rounds);
    let mut s_vecs: Vec<Vec<Scalar>> = Vec::with_capacity(k_rounds);
    let mut intermediates: Vec<Vec<Ciphertext>> = Vec::with_capacity(k_rounds);

    for _ in 0..k_rounds {
        let sigma = Permutation::random(rng, n);
        let s: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        // D^{(k)}[i] = ReEnc(c_in[σ(i)], s[i])
        let d: Vec<Ciphertext> = (0..n)
            .map(|i| remask(pk, &c_in[sigma.as_slice()[i]], s[i]))
            .collect();
        sigmas.push(sigma);
        s_vecs.push(s);
        intermediates.push(d);
    }

    // FS 派生 K 个 challenge bits, 顺序 hash (C, C', D^{(1)}, ..., D^{(K)}).
    let bits = derive_bits(c_in, c_out, &intermediates, k_rounds);

    // 根据 bits 决定每轮揭示哪侧.
    let mut openings: Vec<Opening> = Vec::with_capacity(k_rounds);
    for (k, bit) in bits.iter().enumerate() {
        if !*bit {
            // b_k = 0: 揭 (σ_k, s_k)
            openings.push(Opening::PreShuffle {
                sigma: sigmas[k].clone(),
                s: s_vecs[k].clone(),
            });
        } else {
            // b_k = 1: 计算 τ_k = σ_k^{-1} ∘ π, t_k[i] = r[i] - s_k[τ_k(i)]
            let sigma_inv = sigmas[k].inverse();
            let tau_indices: Vec<usize> = (0..n)
                .map(|i| sigma_inv.as_slice()[pi.as_slice()[i]])
                .collect();
            let tau = Permutation::from_raw(tau_indices);
            let t: Vec<Scalar> = (0..n)
                .map(|i| r[i] - s_vecs[k][tau.as_slice()[i]])
                .collect();
            openings.push(Opening::PostShuffle { tau, t });
        }
    }

    ShuffleProof {
        intermediates,
        openings,
    }
}

/// 验证证明.
pub fn verify(
    pk: &PublicKey,
    c_in: &[Ciphertext],
    c_out: &[Ciphertext],
    proof: &ShuffleProof,
) -> bool {
    let n = c_in.len();
    if c_out.len() != n {
        return false;
    }
    let k_rounds = proof.intermediates.len();
    if proof.openings.len() != k_rounds {
        return false;
    }

    // 重新派生 bits
    let bits = derive_bits(c_in, c_out, &proof.intermediates, k_rounds);

    // 每轮检查
    for (k, bit) in bits.iter().enumerate() {
        let d = &proof.intermediates[k];
        if d.len() != n {
            return false;
        }
        match (&proof.openings[k], *bit) {
            (Opening::PreShuffle { sigma, s }, false) => {
                if sigma.len() != n || s.len() != n || !sigma.is_valid() {
                    return false;
                }
                // 检查 D^{(k)}[i] == ReEnc(c_in[σ(i)], s[i])
                for i in 0..n {
                    let expected = remask(pk, &c_in[sigma.as_slice()[i]], s[i]);
                    if d[i] != expected {
                        return false;
                    }
                }
            }
            (Opening::PostShuffle { tau, t }, true) => {
                if tau.len() != n || t.len() != n || !tau.is_valid() {
                    return false;
                }
                // 检查 c_out[i] == ReEnc(D^{(k)}[τ(i)], t[i])
                for i in 0..n {
                    let expected = remask(pk, &d[tau.as_slice()[i]], t[i]);
                    if c_out[i] != expected {
                        return false;
                    }
                }
            }
            _ => {
                // bit 跟 opening 类型不匹配 — 拒绝
                return false;
            }
        }
    }
    true
}

/// FS 派生 K 个 challenge bits, hash (c_in, c_out, intermediates).
fn derive_bits(
    c_in: &[Ciphertext],
    c_out: &[Ciphertext],
    intermediates: &[Vec<Ciphertext>],
    k_rounds: usize,
) -> Vec<bool> {
    let mut t = Transcript::new(DOMAIN);
    append_ciphertext_vec(&mut t, b"c_in", c_in);
    append_ciphertext_vec(&mut t, b"c_out", c_out);
    for (k, d) in intermediates.iter().enumerate() {
        let label = format!("D_{k}");
        append_ciphertext_vec(&mut t, label.as_bytes(), d);
    }
    t.challenge_bits(b"bits", k_rounds)
}

fn append_ciphertext_vec(t: &mut Transcript, label: &[u8], v: &[Ciphertext]) {
    let mut buf = Vec::with_capacity(v.len() * 96);
    for ct in v {
        ct.c1.serialize_compressed(&mut buf).expect("serialize c1");
        ct.c2.serialize_compressed(&mut buf).expect("serialize c2");
    }
    t.append_message(label, &buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::Curve;
    use crate::mental_poker::elgamal::{keygen, mask};
    use crate::mental_poker::shuffle::shuffle_and_remask;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    /// 工具: 构造 (pk, c_in, c_out, π, r) 4 元组用于测试.
    fn setup_shuffle(
        n: usize,
    ) -> (
        PublicKey,
        Vec<Ciphertext>,
        Vec<Ciphertext>,
        Permutation,
        Vec<Scalar>,
    ) {
        let rng = &mut test_rng();
        let (_, pk) = keygen(rng);
        let messages: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let c_in: Vec<Ciphertext> = messages.iter().map(|m| mask(rng, &pk, m).0).collect();
        let (c_out, pi, r) = shuffle_and_remask(rng, &pk, &c_in);
        (pk, c_in, c_out, pi, r)
    }

    /// honest prove + verify 通过 (K=20, N=8 — 加速测试).
    #[test]
    fn cnc_honest_roundtrip_small() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(8);
        let proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, 20);
        assert!(verify(&pk, &c_in, &c_out, &proof));
    }

    /// honest prove + verify, K=80 默认值, N=16.
    #[test]
    fn cnc_honest_roundtrip_default_k() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(16);
        let proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, DEFAULT_K);
        assert!(verify(&pk, &c_in, &c_out, &proof));
        assert_eq!(proof.intermediates.len(), DEFAULT_K);
        assert_eq!(proof.openings.len(), DEFAULT_K);
    }

    /// 篡改一个 intermediate 中间态 → fail.
    #[test]
    fn cnc_tampered_intermediate_fails() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(8);
        let mut proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, 20);
        // 篡改第 0 轮的第 0 个密文的 c1
        proof.intermediates[0][0].c1 += Curve::default(); // identity, no-op
        proof.intermediates[0][0].c1 += pk.0;
        assert!(!verify(&pk, &c_in, &c_out, &proof));
    }

    /// 篡改一个 opening 的 σ → fail.
    #[test]
    fn cnc_tampered_opening_sigma_fails() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(8);
        // 用 K=20 跑直到至少有一个 PreShuffle opening (b_k = 0)
        let mut proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, 20);
        let mut tampered = false;
        for opening in proof.openings.iter_mut() {
            if let Opening::PreShuffle { sigma, .. } = opening {
                // 交换 sigma[0] 和 sigma[1] (仍是 valid permutation 但内容不同)
                let raw = sigma.as_slice().to_vec();
                let mut swapped = raw;
                swapped.swap(0, 1);
                *sigma = Permutation::from_raw(swapped);
                tampered = true;
                break;
            }
        }
        assert!(tampered, "20 轮里至少有一个 PreShuffle (概率 ~1)");
        assert!(!verify(&pk, &c_in, &c_out, &proof));
    }

    /// bogus c_out (随机替换) → fail.
    #[test]
    fn cnc_bogus_c_out_fails() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(8);
        let mut bogus = c_out.clone();
        // 让某一项变成完全无关的密文
        let m = Curve::rand(rng);
        bogus[3] = mask(rng, &pk, &m).0;

        // 用真 (π, r) 但 c_out=bogus prove — 协议本身不检 (π, r) 跟 c_out 一致,
        // 只 verify 关心 D^{(k)} 与 c_in/c_out 的一致性. 所以 prove 不会 panic
        // 但 verify 会失败.
        let proof = prove(rng, &pk, &c_in, &bogus, &pi, &r, 20);
        assert!(!verify(&pk, &c_in, &bogus, &proof));
    }

    /// proof 大小 sanity: 给定 K=80, N=16 看实际 size.
    #[test]
    fn cnc_proof_size_check() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(16);
        let proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, DEFAULT_K);
        assert_eq!(proof.intermediates.len(), DEFAULT_K);
        for d in &proof.intermediates {
            assert_eq!(d.len(), 16);
        }
        let mut pre_count = 0;
        let mut post_count = 0;
        for o in &proof.openings {
            match o {
                Opening::PreShuffle { sigma, s } => {
                    assert_eq!(sigma.len(), 16);
                    assert_eq!(s.len(), 16);
                    pre_count += 1;
                }
                Opening::PostShuffle { tau, t } => {
                    assert_eq!(tau.len(), 16);
                    assert_eq!(t.len(), 16);
                    post_count += 1;
                }
            }
        }
        // 期望 pre + post = K, 各约 K/2 (二项分布)
        assert_eq!(pre_count + post_count, DEFAULT_K);
        // 各 ~ K/2 (允许 ±20 偏差, 防 flaky)
        assert!((20..=60).contains(&pre_count), "pre_count={pre_count}");
    }

    /// 麻将 136 张 cut-and-choose, K=20 (加速测试).
    /// 仅作可行性 sanity, 不强制 K=80 (那是 cargo test 慢).
    #[test]
    fn cnc_136_cards_k_20() {
        let rng = &mut test_rng();
        let (pk, c_in, c_out, pi, r) = setup_shuffle(136);
        let t0 = std::time::Instant::now();
        let proof = prove(rng, &pk, &c_in, &c_out, &pi, &r, 20);
        let dt_prove = t0.elapsed();
        let t0 = std::time::Instant::now();
        assert!(verify(&pk, &c_in, &c_out, &proof));
        let dt_verify = t0.elapsed();
        println!("[cnc-136-K20] prove: {dt_prove:?}, verify: {dt_verify:?}");
        // sanity: < 30 秒
        assert!(dt_prove.as_secs() < 30);
        assert!(dt_verify.as_secs() < 30);
    }

    // ====== M4.C.4 4 玩家 e2e 测试 (协议 1 端到端) ======

    use crate::mental_poker::elgamal::{SecretKey, unmask_with_sk};
    use crate::mental_poker::joint_key::{JointPublicKey, aggregate};
    use crate::mental_poker::schnorr;
    use std::collections::HashSet;

    /// 构造 4 玩家 + 联合公钥 setup, 返回 (sk_vec, jpk, peer_ids).
    fn setup_4_players() -> (Vec<SecretKey>, JointPublicKey, Vec<Vec<u8>>) {
        let rng = &mut test_rng();
        let mut sks = Vec::with_capacity(4);
        let mut peer_ids = Vec::with_capacity(4);
        let mut members = Vec::with_capacity(4);
        for i in 0..4 {
            let peer_id = format!("player_{i}").into_bytes();
            let (sk, pk) = crate::mental_poker::elgamal::keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            peer_ids.push(peer_id.clone());
            members.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&members).expect("4 honest aggregate");
        (sks, jpk, peer_ids)
    }

    /// 加密 N 张随机牌, 返回 (plaintexts, ciphertexts).
    fn encrypt_initial_deck(pk: &PublicKey, n: usize) -> (Vec<Curve>, Vec<Ciphertext>) {
        let rng = &mut test_rng();
        let plaintexts: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let cts: Vec<Ciphertext> = plaintexts.iter().map(|m| mask(rng, pk, m).0).collect();
        (plaintexts, cts)
    }

    /// 协议 1 端到端: 4 玩家 keygen + 联合 PK + 加密初始 deck + 每方 shuffle 带
    /// cut-and-choose proof + 其他人 verify + 最终用 sum(sk) 解密 (注: 实际
    /// 协议 2 走 threshold ElGamal, 这里用 sum 简化测试).
    #[test]
    fn protocol_1_four_player_shuffle_e2e() {
        let rng = &mut test_rng();
        let (sks, jpk, _peer_ids) = setup_4_players();
        let pk = jpk.as_pk();

        let n = 16usize;
        let k_rounds = 20usize; // 加速测试
        let (plaintexts, mut deck) = encrypt_initial_deck(&pk, n);

        // 4 玩家轮流 shuffle, 每方生成 proof, 其他 3 方 verify.
        for player_idx in 0..4 {
            let (out_deck, pi, r) = shuffle_and_remask(rng, &pk, &deck);
            let proof = prove(rng, &pk, &deck, &out_deck, &pi, &r, k_rounds);
            // 其他 3 玩家验证
            assert!(
                verify(&pk, &deck, &out_deck, &proof),
                "玩家 {player_idx} 的 shuffle proof verify 失败"
            );
            deck = out_deck;
        }

        // 解密 (用 sum(sk) 简化, 实际协议 2 是 threshold).
        let total_sk = SecretKey(sks.iter().map(|sk| sk.0).sum());
        let recovered: HashSet<String> = deck
            .iter()
            .map(|c| format!("{}", unmask_with_sk(&total_sk, c)))
            .collect();
        let original: HashSet<String> = plaintexts.iter().map(|p| format!("{p}")).collect();
        assert_eq!(recovered, original, "4 轮 shuffle 后 plaintext 集合应保持");
        assert_eq!(recovered.len(), n);
    }

    /// 作弊玩家用 bogus 输出 (跟真 shuffle 无关) → 其他人 verify 失败.
    #[test]
    fn protocol_1_cheating_player_detected() {
        let rng = &mut test_rng();
        let (_sks, jpk, _) = setup_4_players();
        let pk = jpk.as_pk();
        let n = 8usize;
        let k_rounds = 20;
        let (_, deck) = encrypt_initial_deck(&pk, n);

        // 作弊: 完全跟 deck 无关的 bogus 输出
        let bogus_out: Vec<Ciphertext> = (0..n)
            .map(|_| {
                let m = Curve::rand(rng);
                mask(rng, &pk, &m).0
            })
            .collect();

        // 用真 (π, r) 但 c_out=bogus 跑 prove, verify 应失败 (cut-and-choose
        // 概率 1 - 2^{-K} 检测到不一致, K=20 时 ~99.9999%)
        let pi = Permutation::random(rng, n);
        let r: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        let proof = prove(rng, &pk, &deck, &bogus_out, &pi, &r, k_rounds);
        assert!(
            !verify(&pk, &deck, &bogus_out, &proof),
            "bogus 输出应被检测到"
        );
    }

    /// 4 轮 shuffle 后 deck 顺序变 (不等于初始 — sanity, 不是密码学不可预知性).
    #[test]
    fn protocol_1_four_player_shuffle_changes_order() {
        let rng = &mut test_rng();
        let (_, jpk, _) = setup_4_players();
        let pk = jpk.as_pk();
        let n = 16;
        let k_rounds = 20;
        let (_, initial_deck) = encrypt_initial_deck(&pk, n);
        let mut deck = initial_deck.clone();

        for _ in 0..4 {
            let (out_deck, pi, r) = shuffle_and_remask(rng, &pk, &deck);
            let proof = prove(rng, &pk, &deck, &out_deck, &pi, &r, k_rounds);
            assert!(verify(&pk, &deck, &out_deck, &proof));
            deck = out_deck;
        }

        // 至少有一个位置的密文跟初始不同 (实际上几乎 100% 都不同, 因为 remask 都换 r).
        let any_different = initial_deck.iter().zip(deck.iter()).any(|(a, b)| a != b);
        assert!(any_different, "4 轮 shuffle 后 deck 应跟初始不同");
    }

    /// 单方 sk 不足以解密 4 轮 shuffle 后的 deck (核心安全性: 联合 PK 加密).
    #[test]
    fn protocol_1_single_sk_cannot_decrypt_after_shuffle() {
        let rng = &mut test_rng();
        let (sks, jpk, _) = setup_4_players();
        let pk = jpk.as_pk();
        let n = 8;
        let k_rounds = 20;
        let (plaintexts, mut deck) = encrypt_initial_deck(&pk, n);

        for _ in 0..4 {
            let (out_deck, pi, r) = shuffle_and_remask(rng, &pk, &deck);
            let proof = prove(rng, &pk, &deck, &out_deck, &pi, &r, k_rounds);
            assert!(verify(&pk, &deck, &out_deck, &proof));
            deck = out_deck;
        }

        // 单方 sk_0 解密不出来 plaintext 集合.
        let recovered_single: HashSet<String> = deck
            .iter()
            .map(|c| format!("{}", unmask_with_sk(&sks[0], c)))
            .collect();
        let original: HashSet<String> = plaintexts.iter().map(|p| format!("{p}")).collect();
        assert_ne!(
            recovered_single, original,
            "单方 sk 不应能解密联合 PK 加密的 deck"
        );
    }
}
