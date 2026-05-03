//! Schnorr DLOG knowledge proof (M4.A.1).
//!
//! 证明: "我知道 sk 使得 pk = sk · G" — 不揭示 sk.
//!
//! 用于 4 玩家联合公钥协议: 每个玩家广播 (pk_i, schnorr_proof_i), 其他人
//! 验证 → 联合公钥 PK = sum(pk_i) 才能保证每个 pk_i 都有合法 sk_i 对应
//! (否则 Wagner-Boneh "rogue key attack": 攻击者发布 pk_attacker - sum(其他 pk),
//! 这样 PK = pk_attacker, 攻击者掌握全部 sk).
//!
//! ## Sigma 协议
//! 1. P 取 r ∈ Z_q, 计算 a = r · G
//! 2. challenge c = Hash(G, pk, a, ctx)  (Fiat-Shamir)
//! 3. P 计算 z = r + c · sk (mod q)
//! 4. 证明 = (a, z)
//! 5. V 验证: z · G == a + c · pk
//!
//! `ctx` 是绑定 prover identity 的可选 byte slice (e.g. peer_id), 防止
//! 中间人重放别人的 proof.

use ark_ec::PrimeGroup;
use ark_ff::UniformRand;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::Rng;

use super::elgamal::{PublicKey, SecretKey};
use super::transcript::Transcript;
use super::{Curve, Scalar};

/// Schnorr 证明 = (commitment a, response z).
#[derive(Debug, Clone, Copy, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct DlogProof {
    pub a: Curve,
    pub z: Scalar,
}

const DOMAIN: &[u8] = b"tui-majo/mp/schnorr-dlog/v1";

/// 生成证明: 我知道 sk 使得 pk = sk · G.
/// `ctx` 应包含 prover 身份 (e.g. peer_id bytes), 否则可被重放.
pub fn prove<R: Rng + ?Sized>(
    rng: &mut R,
    sk: &SecretKey,
    pk: &PublicKey,
    ctx: &[u8],
) -> DlogProof {
    let g = Curve::generator();
    let r = Scalar::rand(rng);
    let a = g * r;

    let c = build_challenge(pk, &a, ctx);
    let z = r + c * sk.0;
    DlogProof { a, z }
}

/// 验证: pk 是否对应一个已知 sk 的 prover.
pub fn verify(pk: &PublicKey, proof: &DlogProof, ctx: &[u8]) -> bool {
    let g = Curve::generator();
    let c = build_challenge(pk, &proof.a, ctx);
    g * proof.z == proof.a + pk.0 * c
}

/// 构造 Fiat-Shamir challenge. 证明者和验证者必须用完全相同序列.
fn build_challenge(pk: &PublicKey, a: &Curve, ctx: &[u8]) -> Scalar {
    let mut t = Transcript::new(DOMAIN);
    t.append_point(b"G", &Curve::generator());
    t.append_point(b"pk", &pk.0);
    t.append_point(b"a", a);
    t.append_message(b"ctx", ctx);
    t.challenge_scalar(b"c")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_poker::elgamal::keygen;
    use ark_std::test_rng;

    #[test]
    fn honest_prove_verify_succeeds() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let proof = prove(rng, &sk, &pk, b"alice");
        assert!(verify(&pk, &proof, b"alice"));
    }

    /// 不同 ctx (e.g. 别的 peer_id) → 验证失败. 防重放.
    #[test]
    fn different_ctx_fails() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let proof = prove(rng, &sk, &pk, b"alice");
        assert!(!verify(&pk, &proof, b"bob"));
    }

    /// 用别人 pk 验证 → 失败 (proof 绑死 pk).
    #[test]
    fn wrong_pk_fails() {
        let rng = &mut test_rng();
        let (sk_a, pk_a) = keygen(rng);
        let (_, pk_b) = keygen(rng);
        let proof = prove(rng, &sk_a, &pk_a, b"alice");
        assert!(!verify(&pk_b, &proof, b"alice"));
    }

    /// 篡改 a 或 z 都失败.
    #[test]
    fn tampered_proof_fails() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let proof = prove(rng, &sk, &pk, b"alice");

        let mut bad = proof;
        bad.a += Curve::generator();
        assert!(!verify(&pk, &bad, b"alice"));

        let mut bad = proof;
        bad.z += Scalar::from(1u64);
        assert!(!verify(&pk, &bad, b"alice"));
    }

    /// 同一 (sk, pk, ctx) 多次 prove 应得不同 a (因为 r 随机), 但都可验证.
    #[test]
    fn proofs_are_randomized() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let p1 = prove(rng, &sk, &pk, b"alice");
        let p2 = prove(rng, &sk, &pk, b"alice");
        assert_ne!(p1.a, p2.a);
        assert!(verify(&pk, &p1, b"alice"));
        assert!(verify(&pk, &p2, b"alice"));
    }

    /// rogue key 防御 sanity: 如果攻击者公布 pk_a 但不知道对应 sk_a, prove
    /// 不可能成功 (因为他需要 sk_a 算 z).
    /// 这条 test 不能直接验证密码学 hardness, 只验证 API 行为.
    #[test]
    fn proof_requires_knowing_sk() {
        let rng = &mut test_rng();
        let (sk_real, pk_real) = keygen(rng);
        let (_sk_other, _) = keygen(rng);

        // 用错的 sk + 真的 pk 不可能产生有效 proof
        let bad_proof = prove(rng, &SecretKey(Scalar::from(0u64)), &pk_real, b"x");
        assert!(!verify(&pk_real, &bad_proof, b"x"));

        // honest 的就成功
        let good = prove(rng, &sk_real, &pk_real, b"x");
        assert!(verify(&pk_real, &good, b"x"));
    }
}
