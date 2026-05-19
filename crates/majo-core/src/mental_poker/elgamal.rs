//! ElGamal 加密 / 解密原语 (M4.A.0).
//!
//! 协议每张牌都是一个 [`Ciphertext`] (= ElGamal 密文), 牌山 = 密文向量.
//! 单方私钥下的 mask/unmask 在这一层就是普通 ElGamal; 4 玩家联合公钥下的
//! threshold 解密走 [`crate::mental_poker::primitives::reveal`] (M4.A.2 后).
//!
//! ## ElGamal 定义 (加法群版本, 适合 ECC)
//! - 生成元 G ∈ E (椭圆曲线 group), 子群阶 q
//! - 私钥 sk ∈ Z_q, 公钥 PK = sk · G
//! - 加密 m (m 必须是 group 元素): 取随机 r,
//!     c1 = r · G
//!     c2 = m + r · PK
//!   密文 = (c1, c2)
//! - 解密 (单方): m = c2 - sk · c1
//!   (= c2 - sk · r · G = m + r · PK - sk · r · G = m + r · (sk·G) - sk · r · G = m)
//!
//! ## 重要: 明文 m 必须是 group 元素
//! 不能直接加密整数 i. 麻将协议里, 我们用 [`Card::random`] 给每张明牌
//! 分配一个**唯一**的 group 元素 (随机点), 然后建立 (Card → Tile) 映射表.
//! 加密时编码 Card 元素, 解密时查表反编码.

use ark_ec::PrimeGroup;
use ark_ff::UniformRand;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::Rng;

use super::{Curve, Scalar};

/// ElGamal 密文 (c1, c2). 在 mental poker 协议中代表"未揭示的牌".
///
/// `Curve` 加法群表达, c1/c2 都是曲线点.
#[derive(Debug, Clone, Copy, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct Ciphertext {
    pub c1: Curve,
    pub c2: Curve,
}

/// 单方私钥 (1 个标量).
///
/// 联合公钥模式下每个玩家持自己 1 份 sk_i, 联合公钥 PK = sum(pk_i).
/// 协议 2 / 3 摸牌 / 揭示时, 玩家用 sk_i 计算 reveal token = sk_i · c1.
#[derive(Debug, Clone, Copy, CanonicalSerialize, CanonicalDeserialize)]
pub struct SecretKey(pub Scalar);

/// 单方公钥 (1 个曲线点).
#[derive(Debug, Clone, Copy, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
pub struct PublicKey(pub Curve);

/// 生成 keypair: sk 随机标量, pk = sk · G.
pub fn keygen<R: Rng + ?Sized>(rng: &mut R) -> (SecretKey, PublicKey) {
    let sk = Scalar::rand(rng);
    let pk = Curve::generator() * sk;
    (SecretKey(sk), PublicKey(pk))
}

/// 用公钥 pk 加密明文 message (group 元素), 输出密文 + 用到的 mask 因子 r.
///
/// r 由调用方决定保留还是丢弃: shuffle / remask 协议需要保留 r 以便生成
/// re-encryption proof; 普通"加密一张随机牌"丢弃即可.
pub fn mask<R: Rng + ?Sized>(rng: &mut R, pk: &PublicKey, message: &Curve) -> (Ciphertext, Scalar) {
    let r = Scalar::rand(rng);
    mask_with_r(pk, message, r)
}

/// 用指定 r 加密 (用于 re-encryption / 测试确定性).
pub fn mask_with_r(pk: &PublicKey, message: &Curve, r: Scalar) -> (Ciphertext, Scalar) {
    let g = Curve::generator();
    let c1 = g * r;
    let c2 = *message + pk.0 * r;
    (Ciphertext { c1, c2 }, r)
}

/// 单方私钥下的解密: m = c2 - sk · c1.
///
/// **仅用于单方测试**. 生产协议下不能让任何单方解密 — 必须走 4 方
/// threshold (协议 2 / 3).
pub fn unmask_with_sk(sk: &SecretKey, ct: &Ciphertext) -> Curve {
    ct.c2 - ct.c1 * sk.0
}

/// 重加密 (re-encryption): 给已有密文加额外 mask 因子 r', 得到新密文,
/// 但解密结果不变. shuffle 协议每轮都做这一步.
///
/// (c1, c2) → (c1 + r' · G, c2 + r' · PK)
pub fn remask(pk: &PublicKey, ct: &Ciphertext, r_prime: Scalar) -> Ciphertext {
    let g = Curve::generator();
    Ciphertext {
        c1: ct.c1 + g * r_prime,
        c2: ct.c2 + pk.0 * r_prime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_std::test_rng;

    /// 基本健全性: keygen + mask + unmask 闭环, 明文恢复正确.
    #[test]
    fn elgamal_roundtrip() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct, _r) = mask(rng, &pk, &message);
        let recovered = unmask_with_sk(&sk, &ct);
        assert_eq!(recovered, message);
    }

    /// 不同 mask 因子产生不同密文, 但解密都得到同一明文.
    #[test]
    fn elgamal_different_r_same_plaintext() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct1, _) = mask(rng, &pk, &message);
        let (ct2, _) = mask(rng, &pk, &message);
        assert_ne!(ct1, ct2, "随机 mask 应产生不同密文");
        assert_eq!(unmask_with_sk(&sk, &ct1), message);
        assert_eq!(unmask_with_sk(&sk, &ct2), message);
    }

    /// remask 后密文变化但解密结果不变 (re-encryption 同态性).
    #[test]
    fn remask_preserves_plaintext() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk, &message);
        let r_prime = Scalar::rand(rng);
        let ct2 = remask(&pk, &ct, r_prime);
        assert_ne!(ct, ct2);
        assert_eq!(unmask_with_sk(&sk, &ct2), message);
    }

    /// 错的 sk 不能解密 (sanity).
    #[test]
    fn wrong_sk_fails_decryption() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let (other_sk, _) = keygen(rng);
        let message = Curve::rand(rng);
        let (ct, _) = mask(rng, &pk, &message);
        assert_eq!(unmask_with_sk(&sk, &ct), message);
        assert_ne!(unmask_with_sk(&other_sk, &ct), message);
    }

    /// 加密 N=8 张不同牌, 所有都能正确恢复.
    #[test]
    fn elgamal_batch_distinct_messages() {
        let rng = &mut test_rng();
        let (sk, pk) = keygen(rng);
        let messages: Vec<Curve> = (0..8).map(|_| Curve::rand(rng)).collect();
        let cts: Vec<Ciphertext> = messages.iter().map(|m| mask(rng, &pk, m).0).collect();
        for (m, ct) in messages.iter().zip(cts.iter()) {
            assert_eq!(unmask_with_sk(&sk, ct), *m);
        }
    }
}
