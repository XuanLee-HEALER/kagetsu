//! Chaum-Pedersen DLEQ (Discrete Log Equality) proof (M4.A.2).
//!
//! 证明: "我知道 x 使得 H1 = x · G1 **且** H2 = x · G2 (同一个 x)" — 不揭示 x.
//!
//! ## 在 mental poker 中的用途
//!
//! 1. **Remasking proof** (协议 1 shuffle 子组件): 重加密 (c1, c2) → (c1', c2')
//!    时, c1' - c1 = r · G **且** c2' - c2 = r · PK 必须用同一个 r.
//!    DLEQ 实例 = ((G, c1' - c1), (PK, c2' - c2)), x = r.
//!
//! 2. **Reveal token correctness** (协议 2 / 3 摸牌 / 揭示): 玩家 i 广播
//!    d_i = sk_i · c1 时要证明 sk_i 跟自己公开的 pk_i = sk_i · G 是同一个 sk.
//!    DLEQ 实例 = ((G, pk_i), (c1, d_i)), x = sk_i.
//!
//! ## Sigma 协议 (Chaum-Pedersen 1992)
//! 1. P 取随机 r, 计算 a1 = r · G1, a2 = r · G2, 广播 (a1, a2)
//! 2. challenge c = Hash(G1, H1, G2, H2, a1, a2, ctx)  (Fiat-Shamir)
//! 3. P 计算 z = r + c · x (mod q), 广播 z
//! 4. proof = (a1, a2, z)
//! 5. V 验证:
//!      z · G1 == a1 + c · H1
//!      z · G2 == a2 + c · H2

use ark_ff::UniformRand;
use ark_std::rand::Rng;

use super::transcript::Transcript;
use super::{Curve, Scalar};

/// DLEQ 证明.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DleqProof {
    pub a1: Curve,
    pub a2: Curve,
    pub z: Scalar,
}

const DOMAIN: &[u8] = b"tui-majo/mp/dleq/v1";

/// 生成证明: 我知道 x 使得 H1 = x · G1 **且** H2 = x · G2.
///
/// **调用方必须保证**: 传入的 x 真的同时满足两个等式 (此函数不验证 input).
/// 否则验证一定失败 (相当于 honest prover sanity, 不是 cheat).
pub fn prove<R: Rng + ?Sized>(
    rng: &mut R,
    x: &Scalar,
    g1: &Curve,
    _h1: &Curve,
    g2: &Curve,
    _h2: &Curve,
    ctx: &[u8],
) -> DleqProof {
    let r = Scalar::rand(rng);
    let a1 = *g1 * r;
    let a2 = *g2 * r;

    let h1 = *g1 * *x;
    let h2 = *g2 * *x;
    let c = build_challenge(g1, &h1, g2, &h2, &a1, &a2, ctx);
    let z = r + c * *x;
    DleqProof { a1, a2, z }
}

/// 验证: H1 = x · G1 ∧ H2 = x · G2 (同一未知 x).
pub fn verify(
    g1: &Curve,
    h1: &Curve,
    g2: &Curve,
    h2: &Curve,
    proof: &DleqProof,
    ctx: &[u8],
) -> bool {
    let c = build_challenge(g1, h1, g2, h2, &proof.a1, &proof.a2, ctx);
    *g1 * proof.z == proof.a1 + *h1 * c && *g2 * proof.z == proof.a2 + *h2 * c
}

fn build_challenge(
    g1: &Curve,
    h1: &Curve,
    g2: &Curve,
    h2: &Curve,
    a1: &Curve,
    a2: &Curve,
    ctx: &[u8],
) -> Scalar {
    let mut t = Transcript::new(DOMAIN);
    t.append_point(b"G1", g1);
    t.append_point(b"H1", h1);
    t.append_point(b"G2", g2);
    t.append_point(b"H2", h2);
    t.append_point(b"a1", a1);
    t.append_point(b"a2", a2);
    t.append_message(b"ctx", ctx);
    t.challenge_scalar(b"c")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ec::PrimeGroup;
    use ark_std::test_rng;

    /// 标准用例: 两个不同生成元下同一 sk 的 Schnorr-pair.
    #[test]
    fn honest_roundtrip() {
        let rng = &mut test_rng();
        let x = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng); // 模拟 c1 (随机点)
        let h1 = g1 * x;
        let h2 = g2 * x;
        let proof = prove(rng, &x, &g1, &h1, &g2, &h2, b"ctx");
        assert!(verify(&g1, &h1, &g2, &h2, &proof, b"ctx"));
    }

    /// 不同的 (G1, G2) 下不同的 x → 验证失败 (h1 / h2 不一致).
    #[test]
    fn mismatched_secrets_fail() {
        let rng = &mut test_rng();
        let x1 = Scalar::rand(rng);
        let x2 = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng);
        let h1 = g1 * x1;
        let h2 = g2 * x2; // 不同 sk

        // prove 用 x1, 但实际 h2 用了 x2 → verify 失败
        let proof = prove(rng, &x1, &g1, &h1, &g2, &h2, b"ctx");
        assert!(!verify(&g1, &h1, &g2, &h2, &proof, b"ctx"));
    }

    #[test]
    fn wrong_h1_fails() {
        let rng = &mut test_rng();
        let x = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng);
        let h1 = g1 * x;
        let h2 = g2 * x;
        let proof = prove(rng, &x, &g1, &h1, &g2, &h2, b"ctx");

        let wrong_h1 = h1 + Curve::generator();
        assert!(!verify(&g1, &wrong_h1, &g2, &h2, &proof, b"ctx"));
    }

    #[test]
    fn wrong_ctx_fails() {
        let rng = &mut test_rng();
        let x = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng);
        let h1 = g1 * x;
        let h2 = g2 * x;
        let proof = prove(rng, &x, &g1, &h1, &g2, &h2, b"alice");
        assert!(!verify(&g1, &h1, &g2, &h2, &proof, b"bob"));
    }

    #[test]
    fn tampered_proof_fails() {
        let rng = &mut test_rng();
        let x = Scalar::rand(rng);
        let g1 = Curve::generator();
        let g2 = Curve::rand(rng);
        let h1 = g1 * x;
        let h2 = g2 * x;
        let proof = prove(rng, &x, &g1, &h1, &g2, &h2, b"ctx");

        let mut bad = proof;
        bad.a1 += Curve::generator();
        assert!(!verify(&g1, &h1, &g2, &h2, &bad, b"ctx"));

        let mut bad = proof;
        bad.a2 += Curve::generator();
        assert!(!verify(&g1, &h1, &g2, &h2, &bad, b"ctx"));

        let mut bad = proof;
        bad.z += Scalar::from(1u64);
        assert!(!verify(&g1, &h1, &g2, &h2, &bad, b"ctx"));
    }

    /// 模拟 reveal token 用例: pk = sk · G, d = sk · c1, prove DLEQ.
    /// 这是协议 2 / 3 摸牌 / 揭示的核心.
    #[test]
    fn reveal_token_use_case() {
        let rng = &mut test_rng();
        let sk = Scalar::rand(rng);
        let g = Curve::generator();
        let pk = g * sk;
        let c1 = Curve::rand(rng); // 牌的密文 c1 部分
        let d = c1 * sk; // reveal token

        let proof = prove(rng, &sk, &g, &pk, &c1, &d, b"player-X");
        assert!(verify(&g, &pk, &c1, &d, &proof, b"player-X"));
    }

    /// 模拟 remask 用例: c1' - c1 = r · G ∧ c2' - c2 = r · PK.
    /// 这是协议 1 shuffle 中每个密文的重加密 proof.
    #[test]
    fn remask_use_case() {
        let rng = &mut test_rng();
        let sk = Scalar::rand(rng);
        let g = Curve::generator();
        let pk = g * sk;
        let r = Scalar::rand(rng);

        // 原密文 (随便构造一对合法 ElGamal)
        let r0 = Scalar::rand(rng);
        let m = Curve::rand(rng);
        let c1 = g * r0;
        let c2 = m + pk * r0;

        // 重加密: 加 r
        let c1_new = c1 + g * r;
        let c2_new = c2 + pk * r;

        // DLEQ 实例: G1=G, H1=c1_new-c1, G2=PK, H2=c2_new-c2
        let h1 = c1_new - c1;
        let h2 = c2_new - c2;
        let proof = prove(rng, &r, &g, &h1, &pk, &h2, b"shuffle-step-3");
        assert!(verify(&g, &h1, &pk, &h2, &proof, b"shuffle-step-3"));

        // 用错的 PK 应失败
        let (_other_sk, other_pk_pt) = (Scalar::rand(rng), Curve::rand(rng));
        let _ = _other_sk;
        assert!(!verify(
            &g,
            &h1,
            &other_pk_pt,
            &h2,
            &proof,
            b"shuffle-step-3"
        ));
    }
}
