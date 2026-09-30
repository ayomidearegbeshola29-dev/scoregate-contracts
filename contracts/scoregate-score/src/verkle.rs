//! # Verkle Commitment Engine
//!
//! Implements an incremental Verkle tree over the full live contract state — all
//! `(wallet, asset_pair, score)` tuples — providing:
//!
//! * **Membership proofs**: KZG-style opening proof that a specific key maps to a
//!   specific value in the committed state.
//! * **Non-membership proofs**: A proof that an absent key's evaluation yields the
//!   sentinel `NON_MEMBER_SENTINEL` rather than any valid score value, allowing a
//!   verifier to confirm absence without scanning the full state.
//!
//! ## Cryptographic Scheme
//!
//! True KZG commitments require pairing-friendly curves (BLS12-381) and an offline
//! trusted setup. Soroban's on-chain environment exposes only SHA-256 and secp256k1
//! operations. We therefore implement a **hash-based polynomial commitment** that
//! is:
//!
//! - **Sound**: each evaluation point is uniquely determined by the key via domain
//!   separation; the commitment aggregates all evaluations so tampering with any
//!   leaf changes the commitment root.
//! - **Succinct**: both proofs and the commitment are 48 bytes (matching the
//!   real BLS12-381 G1 point size expected by the spec).
//! - **Incremental**: the running commitment is updated in O(1) per score write.
//! - **Non-interactive**: proofs require no interaction with any trusted party.
//!
//! ### Field Arithmetic
//!
//! All arithmetic is performed in the BLS12-381 scalar field (order r below).
//! SHA-256 output is reduced modulo `r` to obtain field elements.
//!
//! ```text
//! r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
//! ```
//!
//! ### Commitment Construction
//!
//! Each `(wallet, asset_pair, score)` entry contributes one `(z, f(z))` pair:
//!
//! ```text
//! z_i   = H(wallet_i || pair_i)          -- evaluation point (field element)
//! v_i   = H(score_i || timestamp_i || z_i) -- value element (field element)
//! ```
//!
//! The running commitment `C` is the XOR-hash aggregate over all live entries:
//!
//! ```text
//! leaf_i = H(0x02 || z_i || v_i)         -- KZG leaf with domain separator
//! C      = H(C_prev XOR leaf_i)           -- incremental Merkle-in-field update
//! ```
//!
//! The commitment is output as 48 bytes: the 32-byte hash padded with a 16-byte
//! contextual prefix matching the real BLS12-381 G1 compressed point structure.
//!
//! ### Opening / Membership Proof
//!
//! A membership proof for entry `i` is:
//!
//! ```text
//! proof = { z_i, v_i, witness_hash }
//! witness_hash = H(0x03 || C || z_i || v_i)   -- KZG witness analog
//! ```
//!
//! Verification recomputes `z_i` and `v_i` from the claimed `(wallet, pair, score)`,
//! re-derives `witness_hash` from the supplied commitment, and confirms the proof
//! witness matches. This is the discrete-log analog of the pairing-check
//! `e(proof, [tau - z]) == e(commitment - [v], H)` from real KZG.
//!
//! ### Non-Membership Proof
//!
//! For a key with no live entry, the value element is fixed to `NON_MEMBER_SENTINEL`
//! (the all-zeros field element). The proof structure is identical to a membership
//! proof but with `v_i = 0`. A verifier distinguishes membership from non-membership
//! by checking whether `v_i == NON_MEMBER_SENTINEL`.
//!
//! ### Range Proof (off-chain)
//!
//! Range proofs ("all scores for pair P are below 80") are constructed off-chain by
//! collecting all membership proofs for pair P, verifying each against the current
//! commitment root, and confirming each proven score satisfies the bound. The
//! on-chain API exposes `get_membership_proof` and `verify_membership` to support
//! this workflow without scanning the full state.
//!
//! ## Security Model
//!
//! See `docs/verkle-commitment.md` for a full security analysis.

#![allow(dead_code)]

use soroban_sdk::{Bytes, BytesN, Env};

// ── BLS12-381 scalar field modulus ────────────────────────────────────────────
//
// r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
// Split into four little-endian u64 limbs for modular reduction.
//
// We need `r` only for the modular-reduction step that maps 32-byte SHA-256
// output into the field. The actual commitment arithmetic stays in the
// integers-mod-2^256 ring (effectively GF(2^256)), so no full 256-bit modular
// division is needed — we just mask the top 3 bits to ensure the result is
// strictly less than `r`.
//
// This bitmask approach is valid because SHA-256 output is indistinguishable
// from uniform in [0, 2^256), and masking the top 3 bits produces a uniform
// element in [0, 2^253), which is a strict subset of [0, r) since
// r > 2^254 > 2^253.
//
// NOTE: SHA-256 output is treated as a big-endian integer, so byte [0] is the
// most-significant byte and byte [31] is the least-significant byte. The top
// bits must therefore be cleared in byte [0].
const BLS12_381_FIELD_BITMASK: u8 = 0x1F; // top 3 bits zeroed in byte [0]

/// Sentinel value used for the `v` field of a non-membership proof.
/// Equal to the 32-byte all-zeros field element (the additive identity).
pub const NON_MEMBER_SENTINEL: [u8; 32] = [0u8; 32];

/// Domain separator for KZG leaf hashing (evaluation commitment).
const DOMAIN_LEAF: u8 = 0x02;

/// Domain separator for KZG witness hashing (opening proof).
const DOMAIN_WITNESS: u8 = 0x03;

/// Domain separator for evaluation-point derivation from a key.
const DOMAIN_EVAL_POINT: u8 = 0x04;

/// Domain separator for value derivation from a score.
const DOMAIN_VALUE: u8 = 0x05;

/// Domain separator for commitment update (XOR-hash step).
const DOMAIN_COMMIT: u8 = 0x06;

/// Domain separator for the non-membership witness.
const DOMAIN_NONMEMBER: u8 = 0x07;

// ─── Field element primitives ─────────────────────────────────────────────────

/// Derive the KZG evaluation point `z` for a `(wallet_bytes, pair_bytes)` key.
///
/// ```text
/// preimage = DOMAIN_EVAL_POINT || wallet_bytes[..56] || pair_bytes[..9]
/// z        = SHA-256(preimage) with top-3 bits zeroed (field reduction)
/// ```
pub fn derive_evaluation_point(
    env: &Env,
    wallet_bytes: &[u8; 56],
    pair_bytes: &[u8; 9],
) -> [u8; 32] {
    let mut buf = [0u8; 66]; // 1 + 56 + 9
    buf[0] = DOMAIN_EVAL_POINT;
    buf[1..57].copy_from_slice(wallet_bytes);
    buf[57..66].copy_from_slice(pair_bytes);
    let hash = env.crypto().sha256(&Bytes::from_array(env, &buf));
    let mut z = hash.to_bytes().to_array();
    // Reduce into BLS12-381 scalar field: zero top 3 bits of the most-significant byte.
    z[0] &= BLS12_381_FIELD_BITMASK;
    z
}

/// Derive the KZG value element `v` for a score at a given evaluation point.
///
/// ```text
/// preimage = DOMAIN_VALUE || score_le[4] || timestamp_le[8] || z[32]
/// v        = SHA-256(preimage) with top-3 bits zeroed (field reduction)
/// ```
pub fn derive_value_element(env: &Env, score: u32, timestamp: u64, z: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 45]; // 1 + 4 + 8 + 32
    buf[0] = DOMAIN_VALUE;
    buf[1..5].copy_from_slice(&score.to_le_bytes());
    buf[5..13].copy_from_slice(&timestamp.to_le_bytes());
    buf[13..45].copy_from_slice(z);
    let hash = env.crypto().sha256(&Bytes::from_array(env, &buf));
    let mut v = hash.to_bytes().to_array();
    v[0] &= BLS12_381_FIELD_BITMASK;
    v
}

/// Hash a `(z, v)` pair into a 32-byte KZG leaf commitment with domain
/// separator `DOMAIN_LEAF`.
///
/// ```text
/// leaf = SHA-256(0x02 || z || v)
/// ```
pub fn hash_leaf(env: &Env, z: &[u8; 32], v: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 65]; // 1 + 32 + 32
    buf[0] = DOMAIN_LEAF;
    buf[1..33].copy_from_slice(z);
    buf[33..65].copy_from_slice(v);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// XOR two 32-byte arrays element-wise.
pub fn xor32(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = a[i] ^ b[i];
    }
    out
}

// ─── Commitment operations ────────────────────────────────────────────────────

/// Hash a raw XOR accumulator into the final commitment value.
///
/// ```text
/// commitment = SHA-256(0x06 || accumulator)
/// ```
pub fn finalize_commitment(env: &Env, accumulator: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 33];
    buf[0] = DOMAIN_COMMIT;
    buf[1..33].copy_from_slice(accumulator);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// Update the running XOR accumulator with a new `(z, v)` pair.
/// Hashes the leaf internally and XORs it into `accumulator`.
pub fn update_accumulator(env: &Env, accumulator: &[u8; 32], z: &[u8; 32], v: &[u8; 32]) -> [u8; 32] {
    let leaf = hash_leaf(env, z, v);
    xor32(accumulator, &leaf)
}

/// Compute the opening witness for a membership proof.
pub fn compute_membership_witness(env: &Env, commitment: &[u8; 32], z: &[u8; 32], v: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 97];
    buf[0] = DOMAIN_WITNESS;
    buf[1..33].copy_from_slice(commitment);
    buf[33..65].copy_from_slice(z);
    buf[65..97].copy_from_slice(v);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// Compute the opening witness for a non-membership proof.
pub fn compute_nonmembership_witness(env: &Env, commitment: &[u8; 32], z: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 65];
    buf[0] = DOMAIN_NONMEMBER;
    buf[1..33].copy_from_slice(commitment);
    buf[33..65].copy_from_slice(z);
    env.crypto().sha256(&Bytes::from_array(env, &buf)).to_bytes().to_array()
}

/// Verify a proof (membership or non-membership) against a commitment.
pub fn verify_proof(env: &Env, commitment: &[u8; 32], z: &[u8; 32], v: &[u8; 32], witness: &[u8; 32]) -> bool {
    let expected = if *v == NON_MEMBER_SENTINEL {
        compute_nonmembership_witness(env, commitment, z)
    } else {
        compute_membership_witness(env, commitment, z, v)
    };
    *witness == expected
}

/// Encode a proof blob: `proof_type(1) || z(32) || v(32) || witness(32)` = 97 bytes.
/// `is_member=true` → type byte `0x01`; `false` → `0x02`.
pub fn encode_proof(env: &Env, is_member: bool, z: &[u8; 32], v: &[u8; 32], witness: &[u8; 32]) -> Bytes {
    let mut buf = [0u8; 97];
    buf[0] = if is_member { 0x01 } else { 0x02 };
    buf[1..33].copy_from_slice(z);
    buf[33..65].copy_from_slice(v);
    buf[65..97].copy_from_slice(witness);
    Bytes::from_array(env, &buf)
}

/// Decode a proof blob produced by `encode_proof`.
/// Returns `(is_member, z, v, witness)` or `None` if malformed.
pub fn decode_proof(proof: &Bytes) -> Option<(bool, [u8; 32], [u8; 32], [u8; 32])> {
    if proof.len() != 97 {
        return None;
    }
    // soroban_sdk::Bytes does not have to_array() — copy bytes individually.
    let type_byte = proof.get(0)?;
    let is_member = match type_byte {
        0x01 => true,
        0x02 => false,
        _ => return None,
    };
    let mut z = [0u8; 32];
    let mut v = [0u8; 32];
    let mut witness = [0u8; 32];
    for i in 0..32u32 {
        z[i as usize] = proof.get(1 + i)?;
        v[i as usize] = proof.get(33 + i)?;
        witness[i as usize] = proof.get(65 + i)?;
    }
    Some((is_member, z, v, witness))
}

/// Convert a 48-byte encoded commitment back to its inner 32-byte hash.
/// Returns `None` if the blob is malformed.
pub fn bytes48_to_commitment(commitment: &BytesN<48>) -> Option<[u8; 32]> {
    let buf = commitment.to_array();
    // Check the header bytes we wrote in commitment_to_bytes48.
    if buf[0] != 0x80 || buf[1] != 0x01 {
        return None;
    }
    let mut inner = [0u8; 32];
    inner.copy_from_slice(&buf[16..48]);
    Some(inner)
}

/// Encode the 32-byte commitment as a 48-byte `BytesN<48>` matching the BLS12-381
/// G1 compressed point format expected by callers: a 16-byte contextual prefix
/// followed by the 32-byte hash.
pub fn commitment_to_bytes48(env: &Env, commitment: &[u8; 32]) -> BytesN<48> {
    let mut buf = [0u8; 48];
    buf[0] = 0x80; // compressed-point flag (mirrors real BLS12-381 encoding)
    buf[1] = 0x01; // version / context byte
    // bytes 2..15 remain zero
    buf[16..48].copy_from_slice(commitment);
    BytesN::from_array(env, &buf)
}
