//! Bulletproofs-style Inner Product Argument (M4.C.2).
//!
//! ## Statement
//! 给定 generators (G, H) (两组各 n 个独立曲线点), generator u, commitment
//! P ∈ Curve, 公开值 c ∈ Scalar, 证明知道向量 (a, b) ∈ F^n × F^n 使得:
//!
//!     P = ⟨a, G⟩ + ⟨b, H⟩ + ⟨a, b⟩ · u
//!
//! ## 协议结构 (递归 log_2(n) 轮)
//! 每轮 prover 把 a/b 折半, 用 challenge x 重新组合:
//! - L = ⟨a_L, G_R⟩ + ⟨b_R, H_L⟩ + ⟨a_L, b_R⟩ · u
//! - R = ⟨a_R, G_L⟩ + ⟨b_L, H_R⟩ + ⟨a_R, b_L⟩ · u
//! - 派生 challenge x = Hash(transcript, L, R)
//! - 更新 (Bulletproofs standard form):
//!     a' = a_L · x + a_R · x⁻¹
//!     b' = b_L · x⁻¹ + b_R · x
//!     G' = G_L · x⁻¹ + G_R · x
//!     H' = H_L · x + H_R · x⁻¹
//!     P' = x² · L + P + x⁻² · R
//!
//! Invariant: P' = ⟨a', G'⟩ + ⟨b', H'⟩ + ⟨a', b'⟩ · u  (展开后所有交叉项归零)
//!
//! 经过 log_2(n) 轮 a, b 缩到单 scalar, prover 发送 (a_final, b_final).
//! Verifier 检查: P_final == a_final · G_final + b_final · H_final
//!                          + (a_final · b_final) · u
//!
//! ## 大小
//! - 证明 = (L_1, ..., L_log_n, R_1, ..., R_log_n, a_final, b_final)
//! - 通信 = 2 · log_2(n) 个曲线点 + 2 个 scalar
//! - n=128: 14 个 G1 (~672 字节) + 2 个 Fr (~64 字节) ≈ 736 字节
//!
//! ## 当前限制
//! n 必须是 2 的幂. 调用方 padding 自处理.

use ark_ff::{Field, UniformRand};
use ark_std::rand::Rng;

use super::transcript::Transcript;
use super::{Curve, Scalar};

/// Inner product argument 证明.
#[derive(Debug, Clone)]
pub struct InnerProductProof {
    /// log_2(n) 个 L 点
    pub l_vec: Vec<Curve>,
    /// log_2(n) 个 R 点
    pub r_vec: Vec<Curve>,
    /// 折叠到底的最终 a (单 scalar)
    pub a_final: Scalar,
    /// 折叠到底的最终 b (单 scalar)
    pub b_final: Scalar,
}

const DOMAIN: &[u8] = b"tui-majo/mp/ipa/v1";

/// 计算 inner product ⟨a, b⟩ = sum_i a_i · b_i.
pub fn inner_product(a: &[Scalar], b: &[Scalar]) -> Scalar {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b.iter()).map(|(x, y)| *x * *y).sum()
}

/// 多标量乘法 ⟨scalars, points⟩ = sum_i scalars_i · points_i.
fn msm(scalars: &[Scalar], points: &[Curve]) -> Curve {
    debug_assert_eq!(scalars.len(), points.len());
    let mut acc = Curve::default();
    for (s, p) in scalars.iter().zip(points.iter()) {
        acc += *p * *s;
    }
    acc
}

/// 生成证明.
///
/// prover 知道 a, b (长度 n, n = 2^k). 调用方负责把 P 构造成
/// `⟨a, G⟩ + ⟨b, H⟩ + ⟨a, b⟩ · u`. 该函数不重新计算 P.
///
/// `transcript` 进入函数前应已 append 必要的 statement 让 prover/verifier
/// 派生同序列的 challenge.
pub fn prove(
    transcript: &mut Transcript,
    g: &[Curve],
    h: &[Curve],
    u: &Curve,
    mut a: Vec<Scalar>,
    mut b: Vec<Scalar>,
) -> InnerProductProof {
    let n = a.len();
    assert_eq!(b.len(), n);
    assert_eq!(g.len(), n);
    assert_eq!(h.len(), n);
    assert!(n.is_power_of_two() && n > 0, "n must be power of 2 and > 0");

    let mut g = g.to_vec();
    let mut h = h.to_vec();
    let log_n = n.trailing_zeros() as usize;
    let mut l_vec: Vec<Curve> = Vec::with_capacity(log_n);
    let mut r_vec: Vec<Curve> = Vec::with_capacity(log_n);

    while a.len() > 1 {
        let half = a.len() / 2;
        let (a_l, a_r) = a.split_at(half);
        let (b_l, b_r) = b.split_at(half);
        let (g_l, g_r) = g.split_at(half);
        let (h_l, h_r) = h.split_at(half);

        // L = ⟨a_L, G_R⟩ + ⟨b_R, H_L⟩ + ⟨a_L, b_R⟩ · u
        let l_pt = msm(a_l, g_r) + msm(b_r, h_l) + *u * inner_product(a_l, b_r);
        // R = ⟨a_R, G_L⟩ + ⟨b_L, H_R⟩ + ⟨a_R, b_L⟩ · u
        let r_pt = msm(a_r, g_l) + msm(b_l, h_r) + *u * inner_product(a_r, b_l);

        transcript.append_point(b"L", &l_pt);
        transcript.append_point(b"R", &r_pt);
        let x = transcript.challenge_scalar(b"x");
        let x_inv = x.inverse().expect("x must be non-zero (FS challenge)");

        // a' = a_L · x + a_R · x⁻¹
        let new_a: Vec<Scalar> = a_l
            .iter()
            .zip(a_r.iter())
            .map(|(l, r)| *l * x + *r * x_inv)
            .collect();
        // b' = b_L · x⁻¹ + b_R · x
        let new_b: Vec<Scalar> = b_l
            .iter()
            .zip(b_r.iter())
            .map(|(l, r)| *l * x_inv + *r * x)
            .collect();
        // G' = G_L · x⁻¹ + G_R · x
        let new_g: Vec<Curve> = g_l
            .iter()
            .zip(g_r.iter())
            .map(|(l, r)| *l * x_inv + *r * x)
            .collect();
        // H' = H_L · x + H_R · x⁻¹
        let new_h: Vec<Curve> = h_l
            .iter()
            .zip(h_r.iter())
            .map(|(l, r)| *l * x + *r * x_inv)
            .collect();

        l_vec.push(l_pt);
        r_vec.push(r_pt);
        a = new_a;
        b = new_b;
        g = new_g;
        h = new_h;
    }

    InnerProductProof {
        l_vec,
        r_vec,
        a_final: a[0],
        b_final: b[0],
    }
}

/// 验证证明.
///
/// 调用方提供 (G, H, u, P), transcript 状态需跟 prove 调用前完全一致.
pub fn verify(
    transcript: &mut Transcript,
    g: &[Curve],
    h: &[Curve],
    u: &Curve,
    p: &Curve,
    proof: &InnerProductProof,
) -> bool {
    let n = g.len();
    if !(n.is_power_of_two() && n > 0) || h.len() != n {
        return false;
    }
    let log_n = n.trailing_zeros() as usize;
    if proof.l_vec.len() != log_n || proof.r_vec.len() != log_n {
        return false;
    }

    // 重放 transcript 派生所有 challenge
    let mut challenges: Vec<Scalar> = Vec::with_capacity(log_n);
    for k in 0..log_n {
        transcript.append_point(b"L", &proof.l_vec[k]);
        transcript.append_point(b"R", &proof.r_vec[k]);
        challenges.push(transcript.challenge_scalar(b"x"));
    }
    let mut challenges_inv: Vec<Scalar> = Vec::with_capacity(log_n);
    for x in &challenges {
        match x.inverse() {
            Some(xi) => challenges_inv.push(xi),
            None => return false,
        }
    }

    // 折叠系数 s:
    //   G' = G_L · x⁻¹ + G_R · x  ⇒  对原始 G_i, 第 j 轮高位 bit==0 (左半)
    //                                 系数乘 x⁻¹, bit==1 (右半) 系数乘 x.
    //   H 反向 (H' = H_L · x + H_R · x⁻¹), 系数 reverse.
    let s_g = compute_s_vec(&challenges, &challenges_inv, n, /* g_side */ true);
    let s_h = compute_s_vec(&challenges, &challenges_inv, n, /* g_side */ false);

    let g_final = msm(&s_g, g);
    let h_final = msm(&s_h, h);

    // 重建 P_final = sum(x² · L + x⁻² · R) + P
    let mut p_final = *p;
    for ((l, r), x) in proof
        .l_vec
        .iter()
        .zip(proof.r_vec.iter())
        .zip(challenges.iter())
    {
        let x_sq = *x * *x;
        let x_sq_inv = x_sq.inverse().expect("x² inverse");
        p_final += *l * x_sq + *r * x_sq_inv;
    }

    // 检查: P_final == a_final · G_final + b_final · H_final + (a · b) · u
    let expected = g_final * proof.a_final
        + h_final * proof.b_final
        + *u * (proof.a_final * proof.b_final);
    p_final == expected
}

/// 计算 G 或 H 的折叠系数向量 s.
///
/// `g_side = true`:  G_i 系数 = prod_j (x_j if bit_msb==1 else x_j⁻¹)
/// `g_side = false`: H_i 系数 = prod_j (x_j⁻¹ if bit_msb==1 else x_j)
///
/// bit_msb = 第 j 轮的 high bit, 即 i 的第 (log_n - 1 - j) bit.
fn compute_s_vec(
    challenges: &[Scalar],
    challenges_inv: &[Scalar],
    n: usize,
    g_side: bool,
) -> Vec<Scalar> {
    let log_n = n.trailing_zeros() as usize;
    debug_assert_eq!(challenges.len(), log_n);
    debug_assert_eq!(challenges_inv.len(), log_n);
    let mut s = vec![Scalar::from(1u64); n];
    for (i, item) in s.iter_mut().enumerate() {
        for j in 0..log_n {
            let bit_pos = log_n - 1 - j;
            let bit_one = (i >> bit_pos) & 1 == 1;
            let factor = match (g_side, bit_one) {
                (true, true) => challenges[j],
                (true, false) => challenges_inv[j],
                (false, true) => challenges_inv[j],
                (false, false) => challenges[j],
            };
            *item *= factor;
        }
    }
    s
}

/// 工具: 生成 n 个独立 generators 给 IPA 单测用 (生产代码用 Pedersen ck).
pub fn random_generators<R: Rng + ?Sized>(rng: &mut R, n: usize) -> Vec<Curve> {
    (0..n).map(|_| Curve::rand(rng)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_std::test_rng;

    fn setup_and_prove(
        n: usize,
    ) -> (
        Vec<Curve>,
        Vec<Curve>,
        Curve,
        Curve,
        Scalar,
        InnerProductProof,
    ) {
        let rng = &mut test_rng();
        let g = random_generators(rng, n);
        let h = random_generators(rng, n);
        let u = Curve::rand(rng);
        let a: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        let b: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        let c = inner_product(&a, &b);
        let p = msm(&a, &g) + msm(&b, &h) + u * c;

        let mut t = Transcript::new(DOMAIN);
        t.append_point(b"u", &u);
        t.append_point(b"P", &p);
        let proof = prove(&mut t, &g, &h, &u, a, b);
        (g, h, u, p, c, proof)
    }

    fn verify_with_setup(
        g: &[Curve],
        h: &[Curve],
        u: &Curve,
        p: &Curve,
        proof: &InnerProductProof,
    ) -> bool {
        let mut t = Transcript::new(DOMAIN);
        t.append_point(b"u", u);
        t.append_point(b"P", p);
        verify(&mut t, g, h, u, p, proof)
    }

    #[test]
    fn ipa_honest_roundtrip_n_2() {
        let (g, h, u, p, _c, proof) = setup_and_prove(2);
        assert!(verify_with_setup(&g, &h, &u, &p, &proof));
    }

    #[test]
    fn ipa_honest_roundtrip_n_4() {
        let (g, h, u, p, _c, proof) = setup_and_prove(4);
        assert!(verify_with_setup(&g, &h, &u, &p, &proof));
    }

    #[test]
    fn ipa_honest_roundtrip_n_8() {
        let (g, h, u, p, _c, proof) = setup_and_prove(8);
        assert!(verify_with_setup(&g, &h, &u, &p, &proof));
    }

    #[test]
    fn ipa_honest_roundtrip_n_16() {
        let (g, h, u, p, _c, proof) = setup_and_prove(16);
        assert!(verify_with_setup(&g, &h, &u, &p, &proof));
    }

    #[test]
    fn ipa_honest_roundtrip_n_128() {
        let (g, h, u, p, _c, proof) = setup_and_prove(128);
        assert!(verify_with_setup(&g, &h, &u, &p, &proof));
        assert_eq!(proof.l_vec.len(), 7);
        assert_eq!(proof.r_vec.len(), 7);
    }

    /// 篡改 a_final → fail.
    #[test]
    fn ipa_tampered_a_final_fails() {
        let (g, h, u, p, _c, proof) = setup_and_prove(8);
        let mut bad = proof;
        bad.a_final += Scalar::from(1u64);
        assert!(!verify_with_setup(&g, &h, &u, &p, &bad));
    }

    /// 篡改 b_final → fail.
    #[test]
    fn ipa_tampered_b_final_fails() {
        let (g, h, u, p, _c, proof) = setup_and_prove(8);
        let mut bad = proof;
        bad.b_final += Scalar::from(1u64);
        assert!(!verify_with_setup(&g, &h, &u, &p, &bad));
    }

    /// 篡改 L_0 → fail.
    #[test]
    fn ipa_tampered_l_fails() {
        let (g, h, u, p, _c, proof) = setup_and_prove(8);
        let mut bad = proof;
        bad.l_vec[0] += g[0];
        assert!(!verify_with_setup(&g, &h, &u, &p, &bad));
    }

    /// 错的 c 通过错的 P → 也失败.
    #[test]
    fn ipa_wrong_c_fails() {
        let rng = &mut test_rng();
        let n = 8;
        let g = random_generators(rng, n);
        let h = random_generators(rng, n);
        let u = Curve::rand(rng);
        let a: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        let b: Vec<Scalar> = (0..n).map(|_| Scalar::rand(rng)).collect();
        let real_c = inner_product(&a, &b);
        let wrong_c = real_c + Scalar::from(1u64);
        let p_wrong = msm(&a, &g) + msm(&b, &h) + u * wrong_c;

        let mut t = Transcript::new(DOMAIN);
        t.append_point(b"u", &u);
        t.append_point(b"P", &p_wrong);
        let proof = prove(&mut t, &g, &h, &u, a, b);

        // verify 用同 P_wrong 应该失败 (因为 P_wrong 跟 ⟨a,b⟩ 不一致)
        assert!(!verify_with_setup(&g, &h, &u, &p_wrong, &proof));
    }

    /// transcript domain 不一致 → fail.
    #[test]
    fn ipa_transcript_mismatch_fails() {
        let (g, h, u, p, _c, proof) = setup_and_prove(8);
        let mut t = Transcript::new(b"different-domain");
        t.append_point(b"u", &u);
        t.append_point(b"P", &p);
        assert!(!verify(&mut t, &g, &h, &u, &p, &proof));
    }

    /// inner_product 帮手正确.
    #[test]
    fn inner_product_correctness() {
        let a = vec![Scalar::from(2u64), Scalar::from(3u64), Scalar::from(5u64)];
        let b = vec![Scalar::from(7u64), Scalar::from(11u64), Scalar::from(13u64)];
        let c = inner_product(&a, &b);
        assert_eq!(c, Scalar::from(2u64 * 7 + 3 * 11 + 5 * 13));
    }
}
