//! Mental Poker — 零信任模式底层密码学 (M4).
//!
//! 自实现 Barnett-Smart 协议 + Bayer-Groth shuffle argument, 不依赖外部
//! mental-poker crate. 当前 embed 在主 crate, 后续稳定后抽离为独立开源 crate.
//!
//! ## 设计来源
//! - Barnett-Smart 2003 (eprint 2005/162): "On the Security of Discrete-Log
//!   Cards Schemes"
//! - Bayer-Groth 2012 (eprint 2011/646): "Efficient Zero-Knowledge Argument
//!   for Correctness of a Shuffle"
//! - Geometry mental-poker (Rust, ark 0.3) 作 API 参考, 但代码自写.
//!
//! ## Curve 选择
//! BLS12-381 G1 作普通 ECC 群 (不用 pairing). 选它的理由:
//! - arkworks 0.5 上游维护, Rust 生态最广
//! - 256-bit Fr 标量域, 安全 (不需要 pairing-friendly 也能用 G1)
//! - 后续若要加 ZK-SNARK 暗杠证明 (M6 协议 6 选项 B) 时可复用 BLS12-381 pairing
//!
//! ## 协议层次
//! ```text
//!  +------------------------------------------------+
//!  | M4.F  RoomActor 集成 (ZeroTrust 模式 GameState)|
//!  +------------------------------------------------+
//!  | M4.E  protocol::reveal      M4.D  protocol::draw|
//!  | M4.C  protocol::shuffle (Bayer-Groth)           |
//!  +------------------------------------------------+
//!  | M4.A  primitives: ElGamal, Schnorr DLOG, DLEQ,  |
//!  |       JointPublicKey aggregate                  |
//!  +------------------------------------------------+
//!  | arkworks: ark-bls12-381, ark-ec, ark-ff         |
//!  +------------------------------------------------+
//! ```
//!
//! 当前实现进度: M4.A.0 (ElGamal mask/unmask baseline).

pub mod cut_and_choose;
pub mod dleq;
pub mod elgamal;
pub mod ipa;
pub mod joint_key;
pub mod pedersen;
pub mod protocol_call;
pub mod protocol_concealed_kan;
pub mod protocol_discard;
pub mod protocol_draw;
pub mod protocol_reveal;
pub mod protocol_state;
pub mod protocol_win;
pub mod schnorr;
pub mod session;
pub mod shuffle;
pub mod transcript;

/// 椭圆曲线群类型: BLS12-381 G1 (作普通 ECC 群, 不用 pairing).
pub type Curve = ark_bls12_381::G1Projective;
/// 标量域 Fr (256-bit). 用于私钥, 随机 mask 因子, 排列等.
pub type Scalar = ark_bls12_381::Fr;
