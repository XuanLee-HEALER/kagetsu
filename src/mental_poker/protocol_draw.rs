//! 协议 2: 摸牌 (threshold ElGamal decryption) — M4.D.
//!
//! ## 协议
//! 玩家 X 要从牌山头摸一张 ciphertext c = (c1, c2) (= ElGamal under jpk):
//!
//! 1. X 向其他 N-1 玩家广播 "请求解密分片 (c)"
//! 2. 每个 i ≠ X:
//!    - 计算解密分片 d_i = sk_i · c.c1
//!    - 生成 DLEQ 证明: 同一个 sk_i 同时满足 pk_i = sk_i · G 和 d_i = sk_i · c.c1
//!    - 广播 (d_i, proof_i) 给 X
//! 3. X 验证每个 proof, 用自己 sk_X 算 d_X, 然后恢复明文:
//!    m = c.c2 - sum(d_i)
//! 4. X 把 m (Curve point) 反查 (Tile mapping) 还原成具体 Tile.
//!
//! ## 数学
//! jpk = sum(pk_i) = sum(sk_i · G) = (sum sk_i) · G
//! ct.c2 = m + r · jpk = m + r · sum(sk_i) · G = m + sum(sk_i · r · G) = m + sum(sk_i · c1)
//!       = m + sum(d_i)
//! 因此 m = c.c2 - sum(d_i). ✓
//!
//! ## 安全
//! - 任一方拒绝合作 / 发错分片 (但 DLEQ verify 失败) → X 摸不到牌, 该方
//!   暴露 → 触发断线重洗 (M5+ 实现).
//! - 任一方诚实 + 其他人都给真分片, X 能拿到正确明文.
//! - 单方私自解密任何 ct 不可能 (因为 sum 缺其它人 sk · c1).
//!
//! ## 跟协议 3 (公开揭示) 的区别
//! 协议 2: 只有 X 拿到明文, 因为只有 X 收齐 3 个 share 后用自己 sk_X 算第 4 个.
//! 协议 3: 所有人都拿到明文 — 全部 4 个 share 都广播.
//! 共享同一 [`compute_share`] / [`verify_share`] / [`combine_shares`] 原语.

use ark_ec::PrimeGroup;
use ark_std::rand::Rng;

use super::Curve;
use super::dleq::{self, DleqProof};
use super::elgamal::{Ciphertext, PublicKey, SecretKey};

/// 玩家 i 对一个 ciphertext 计算的 partial decryption.
///
/// 数学上 = sk_i · c.c1. 单独公开它不会泄露 sk_i (DLOG 假设),
/// 也不会泄露明文 (需要 sum 全 4 个).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecryptionShare(pub Curve);

/// 玩家 i 计算自己的解密分片 + DLEQ 证明.
///
/// `peer_id` 绑定 prover 身份 (类似 Schnorr proof 的 ctx), 防止 attacker
/// 把别人发的合法 share 重新打包成自己的.
pub fn compute_share<R: Rng + ?Sized>(
    rng: &mut R,
    sk: &SecretKey,
    pk: &PublicKey,
    ct: &Ciphertext,
    peer_id: &[u8],
) -> (DecryptionShare, DleqProof) {
    let g = Curve::generator();
    let d = ct.c1 * sk.0;
    // DLEQ instance: ((G, pk), (c1, d)), witness = sk
    let proof = dleq::prove(rng, &sk.0, &g, &pk.0, &ct.c1, &d, peer_id);
    (DecryptionShare(d), proof)
}

/// 验证一个解密分片的正确性 (用 DLEQ proof).
pub fn verify_share(
    pk: &PublicKey,
    ct: &Ciphertext,
    share: &DecryptionShare,
    proof: &DleqProof,
    peer_id: &[u8],
) -> bool {
    let g = Curve::generator();
    dleq::verify(&g, &pk.0, &ct.c1, &share.0, proof, peer_id)
}

/// 用所有持 sk 玩家的解密分片恢复明文 (Curve point).
///
/// caller 必须保证: shares 来自全部 N 个 sk_i 持有者 (i = 1..N), 且每个
/// share 已通过 [`verify_share`] (摸牌方自己也要算 + 加进 shares).
///
/// 返回 Curve point. 反查 Tile 是 application 层的职责 (Card mapping table).
pub fn combine_shares(ct: &Ciphertext, shares: &[DecryptionShare]) -> Curve {
    let sum_shares: Curve = shares.iter().map(|s| s.0).sum();
    ct.c2 - sum_shares
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::Scalar;
    use crate::mental_poker::elgamal::{keygen, mask, unmask_with_sk};
    use crate::mental_poker::joint_key::aggregate;
    use crate::mental_poker::schnorr;
    use ark_ff::UniformRand;
    use ark_std::test_rng;

    /// 标准用例: 4 玩家联合 PK 加密 plaintext, 4 个 share 全部 verify 通过,
    /// combine 恢复明文.
    #[test]
    fn threshold_decrypt_4_players_honest() {
        let rng = &mut test_rng();
        // 1. 4 玩家 keygen + Schnorr proof
        let mut entries = Vec::new();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut peer_ids = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            peer_ids.push(peer_id.clone());
            entries.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();

        // 2. 加密 message
        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &message);

        // 3. 每玩家计算 share + verify
        let mut shares = Vec::new();
        for i in 0..4 {
            let (share, proof) = compute_share(rng, &sks[i], &pks[i], &ct, &peer_ids[i]);
            assert!(verify_share(&pks[i], &ct, &share, &proof, &peer_ids[i]));
            shares.push(share);
        }

        // 4. combine 恢复
        let recovered = combine_shares(&ct, &shares);
        assert_eq!(recovered, message);
    }

    /// 玩家 X 摸牌场景: X 收齐其他 3 人 share + 自己算 share + combine.
    #[test]
    fn player_x_draws_card_via_threshold() {
        let rng = &mut test_rng();
        let mut entries = Vec::new();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut peer_ids = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            peer_ids.push(peer_id.clone());
            entries.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();

        let tile = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &tile);

        // X = 玩家 0
        // X 收其他 3 人 share (i = 1, 2, 3)
        let mut received_shares = Vec::new();
        for i in 1..4 {
            let (share, proof) = compute_share(rng, &sks[i], &pks[i], &ct, &peer_ids[i]);
            assert!(verify_share(&pks[i], &ct, &share, &proof, &peer_ids[i]));
            received_shares.push(share);
        }

        // X 自己算 share_0
        let (share_0, _) = compute_share(rng, &sks[0], &pks[0], &ct, &peer_ids[0]);
        received_shares.push(share_0);

        // combine
        let recovered = combine_shares(&ct, &received_shares);
        assert_eq!(recovered, tile);
    }

    /// 错的 sk 算出的 share → DLEQ proof verify 失败.
    #[test]
    fn cheating_with_wrong_sk_caught_by_dleq() {
        let rng = &mut test_rng();
        let (sk_real, pk_real) = keygen(rng);
        let (_, pk_other) = keygen(rng);

        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk_real, &message);

        // cheater 用真 sk 算 share, 但用别人 pk 提交
        let (share, proof) = compute_share(rng, &sk_real, &pk_real, &ct, b"alice");
        // 用 wrong pk 验证
        assert!(!verify_share(&pk_other, &ct, &share, &proof, b"alice"));
    }

    /// 篡改 share 数值 → DLEQ proof verify 失败 (proof 跟 share 绑死).
    #[test]
    fn tampered_share_caught() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk, &message);

        let (mut share, proof) = compute_share(rng, &sk, &pk, &ct, b"alice");
        // 篡改 share value
        share.0 += Curve::generator();
        assert!(!verify_share(&pk, &ct, &share, &proof, b"alice"));
    }

    /// peer_id 不匹配 (重放) → DLEQ proof verify 失败.
    #[test]
    fn mismatched_peer_id_fails() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk, &message);

        let (share, proof) = compute_share(rng, &sk, &pk, &ct, b"alice");
        assert!(verify_share(&pk, &ct, &share, &proof, b"alice"));
        assert!(!verify_share(&pk, &ct, &share, &proof, b"bob"));
    }

    /// 缺一个 share → combine 出错 (不等于 message).
    #[test]
    fn missing_share_yields_wrong_plaintext() {
        let rng = &mut test_rng();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut entries = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            entries.push((peer_id, pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();

        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk.as_pk(), &message);

        let mut shares = Vec::new();
        // 只取 3 个 share, 漏掉 i=3
        for i in 0..3 {
            let (share, _) = compute_share(rng, &sks[i], &pks[i], &ct, b"x");
            shares.push(share);
        }
        let recovered = combine_shares(&ct, &shares);
        assert_ne!(recovered, message, "缺 share 不应恢复原 message");
    }

    /// 单方私自解密失败 (核心零信任安全): 用单 sk 直接 unmask_with_sk
    /// 拿不到 message.
    #[test]
    fn single_sk_cannot_decrypt_joint_ciphertext() {
        let rng = &mut test_rng();
        let (sk_a, pk_a) = keygen(rng);
        let (sk_b, pk_b) = keygen(rng);
        // 联合 PK = pk_a + pk_b (2 玩家简化测试)
        let jpk = PublicKey(pk_a.0 + pk_b.0);

        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &jpk, &message);

        // 单方 sk 解密
        assert_ne!(unmask_with_sk(&sk_a, &ct), message);
        assert_ne!(unmask_with_sk(&sk_b, &ct), message);

        // 但 combine 2 个 share 可以恢复
        let (share_a, _) = compute_share(rng, &sk_a, &pk_a, &ct, b"a");
        let (share_b, _) = compute_share(rng, &sk_b, &pk_b, &ct, b"b");
        let recovered = combine_shares(&ct, &[share_a, share_b]);
        assert_eq!(recovered, message);
    }

    /// 多张牌批量 threshold decrypt (sanity: 摸 8 张牌都对).
    #[test]
    fn threshold_decrypt_batch_of_cards() {
        let rng = &mut test_rng();
        let mut sks = Vec::new();
        let mut pks = Vec::new();
        let mut entries = Vec::new();
        for i in 0..4 {
            let peer_id = format!("p{i}").into_bytes();
            let (sk, pk) = keygen(rng);
            let proof = schnorr::prove(rng, &sk, &pk, &peer_id);
            sks.push(sk);
            pks.push(pk);
            entries.push((peer_id.clone(), pk, proof));
        }
        let jpk = aggregate(&entries).unwrap();

        let n = 8;
        let messages: Vec<Curve> = (0..n).map(|_| Curve::rand(rng)).collect();
        let cts: Vec<Ciphertext> = messages
            .iter()
            .map(|m| mask(rng, &jpk.as_pk(), m).0)
            .collect();

        for (m, ct) in messages.iter().zip(cts.iter()) {
            let shares: Vec<DecryptionShare> = (0..4)
                .map(|i| {
                    let (s, _) = compute_share(rng, &sks[i], &pks[i], ct, b"x");
                    s
                })
                .collect();
            assert_eq!(combine_shares(ct, &shares), *m);
        }
    }

    /// 解密分片用错 ct (不是声明的 ct) → verify 失败.
    #[test]
    fn share_tied_to_specific_ciphertext() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let m1 = Curve::rand(rng);
        let m2 = Curve::rand(rng);
        let (ct1, _) = mask(rng, &pk, &m1);
        let (ct2, _) = mask(rng, &pk, &m2);

        let (share_1, proof_1) = compute_share(rng, &sk, &pk, &ct1, b"x");
        // 拿 ct1 的 share 用到 ct2 上 → fail
        assert!(!verify_share(&pk, &ct2, &share_1, &proof_1, b"x"));
        // share 自己算 ct2 的就对
        let (share_2, proof_2) = compute_share(rng, &sk, &pk, &ct2, b"x");
        assert!(verify_share(&pk, &ct2, &share_2, &proof_2, b"x"));
    }

    /// 防止用 0 sk 提交假 share (= identity point) 通过 DLEQ.
    /// 0 · c1 = identity, 0 · G = identity = pk?  pk 实际是 sk · G ≠ identity 对正常 sk.
    /// 但 cheater 可以宣称 sk = 0, pk = identity, share = identity. proof 仍然对吗?
    /// DLEQ((G, identity), (c1, identity)): x = 0 满足, proof 应该过, 但 pk = identity
    /// 应被 schnorr/aggregate 阶段已经拒绝 (Scalar 0 时 schnorr proof 退化).
    /// 这里只测 verify_share 自身 — pk=identity 的话 share 被验证为 identity 没意义.
    #[test]
    fn zero_sk_pathological_share_consistency() {
        let rng = &mut test_rng();
        let zero_sk = SecretKey(Scalar::from(0u64));
        let zero_pk = PublicKey(Curve::default()); // identity
        let m = Curve::rand(rng);
        let other_pk = PublicKey(Curve::generator());
        let (ct, _) = mask(rng, &other_pk, &m);

        // zero_sk produces share = 0 · c1 = identity, proof DLEQ(0)
        let (share, proof) = compute_share(rng, &zero_sk, &zero_pk, &ct, b"x");
        assert_eq!(share.0, Curve::default());
        // verify 在 zero_pk 下应过 (数学上 trivially, 0=0=0)
        assert!(verify_share(&zero_pk, &ct, &share, &proof, b"x"));
        // 但用 normal pk 验证就不过
        assert!(!verify_share(&other_pk, &ct, &share, &proof, b"x"));
    }
}
