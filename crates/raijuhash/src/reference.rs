//! Direct, unoptimized transcription of the specification in `SPEC.md`.
//!
//! This is the oracle every optimized backend is tested against. It favours
//! obviousness over speed: products are computed one bit at a time and the
//! chunk combination multiplies by each position index explicitly.

use crate::params::{BLOCK, CHUNK, CHUNK_BLOCKS, Params};

/// Carryless (polynomial over GF(2)) product of two 64-bit words.
pub fn clmul64(a: u64, b: u64) -> u128 {
    let mut r = 0u128;
    for i in 0..64 {
        if (b >> i) & 1 == 1 {
            r ^= (a as u128) << i;
        }
    }
    r
}

/// Multiplication in `GF(2^128) = GF(2)[x] / (x^128 + x^7 + x^2 + x + 1)`,
/// where bit `i` of the integer is the coefficient of `x^i`.
pub fn gf_mul(a: u128, b: u128) -> u128 {
    let (a0, a1) = (a as u64, (a >> 64) as u64);
    let (b0, b1) = (b as u64, (b >> 64) as u64);
    let lo = clmul64(a0, b0);
    let hi = clmul64(a1, b1);
    let mid = clmul64(a0, b1) ^ clmul64(a1, b0);
    // 256-bit product as (low 128, high 128).
    let low = lo ^ (mid << 64);
    let high = hi ^ (mid >> 64);
    reduce256(low, high)
}

/// Reduce `low + high * x^128` modulo the field polynomial.
pub fn reduce256(low: u128, high: u128) -> u128 {
    // x^128 = x^7 + x^2 + x + 1. Folding `high` once leaves at most 7 bits
    // above x^128, which a second fold absorbs.
    let fold = |h: u128| -> (u128, u128) {
        let t = h ^ (h << 1) ^ (h << 2) ^ (h << 7);
        let over = (h >> 127) ^ (h >> 126) ^ (h >> 121);
        (t, over)
    };
    let (t, over) = fold(high);
    let (t2, over2) = fold(over);
    debug_assert_eq!(over2, 0);
    low ^ t ^ t2
}

fn word(bytes: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(bytes[8 * i..8 * i + 8].try_into().unwrap())
}

/// Hash of one chunk (1..=8192 bytes) to the pair `(h0, h1)`.
pub fn chunk_pair(p: &Params, chunk: &[u8]) -> (u128, u128) {
    assert!(!chunk.is_empty() && chunk.len() <= CHUNK);
    let nb = chunk.len().div_ceil(BLOCK);
    let mut blocks = [[0u8; BLOCK]; CHUNK_BLOCKS];
    for (b, src) in blocks.iter_mut().zip(chunk.chunks(BLOCK)) {
        b[..src.len()].copy_from_slice(src);
    }
    // Row word `l` of block `b`: X row is words 0..8, Y row is words 8..16.
    let w = |b: usize, l: usize| -> u64 {
        if b < nb { word(&blocks[b], l) } else { 0 }
    };

    let (mut h0, mut h1) = (0u128, 0u128);
    for j in 0..=nb {
        let mut pj = 0u128;
        for l in 0..8 {
            let prev = |i: usize| if j > 0 { w(j - 1, i) } else { 0 };
            let tx = prev(l) ^ w(j, l) ^ p.table[j][l];
            let ty = prev(8 + l) ^ w(j, 8 + l) ^ p.table[j][8 + l];
            pj ^= clmul64(tx, ty);
        }
        if j == CHUNK_BLOCKS {
            // Column (0, 1): only possible for a full chunk's end position.
            h1 ^= pj;
        } else {
            // Column (1, j), with j read as a polynomial of degree < 6.
            h0 ^= pj;
            h1 ^= gf_mul(j as u128, pj);
        }
    }
    (h0, h1)
}

/// The complete hash function.
pub fn hash(p: &Params, msg: &[u8]) -> u128 {
    let len = msg.len();
    if len < 16 {
        let mut x0 = [0u8; 16];
        x0[..len].copy_from_slice(msg);
        x0[15] = len as u8;
        return gf_mul(u128::from_le_bytes(x0), p.a) ^ p.s;
    }
    if len < 32 {
        let x0 = u128::from_le_bytes(msg[..16].try_into().unwrap());
        let mut x1 = [0u8; 16];
        x1[..len - 16].copy_from_slice(&msg[16..]);
        x1[15] = len as u8;
        return gf_mul(x0, p.a) ^ gf_mul(u128::from_le_bytes(x1), p.b) ^ p.s;
    }
    let mut acc = 0u128;
    for chunk in msg.chunks(CHUNK) {
        let (h0, h1) = chunk_pair(p, chunk);
        acc = gf_mul(acc ^ h0, p.r) ^ gf_mul(h1, p.r2);
    }
    acc ^ gf_mul(len as u64 as u128, p.t) ^ p.s
}

/// The fixed bijection of the avalanche finalizer: the tweak is added to
/// the low half, MurmurHash3's 64-bit finalizer (public domain) mixes each
/// half, then the halves are crossed. Every step is invertible, so for a
/// given tweak two inputs collide exactly when they are equal.
pub fn avalanche_mix(raw: u128, tweak: u64) -> u128 {
    fn fmix64(mut k: u64) -> u64 {
        k ^= k >> 33;
        k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
        k ^= k >> 33;
        k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        k ^ k >> 33
    }
    let lo = fmix64((raw as u64).wrapping_add(tweak));
    let hi = fmix64((raw >> 64) as u64);
    let lo = lo ^ hi.rotate_left(32);
    let hi = hi.wrapping_add(lo);
    (hi as u128) << 64 | lo as u128
}

/// The avalanche output: `avalanche_mix(hash, tweak) * V`.
pub fn hash_avalanche(p: &Params, msg: &[u8], tweak: u64) -> u128 {
    gf_mul(avalanche_mix(hash(p, msg), tweak), p.v)
}
