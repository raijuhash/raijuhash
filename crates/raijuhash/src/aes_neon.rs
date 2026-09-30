//! AES-128 encryption with the ARMv8 AES instructions, for seed expansion
//! and MAC masking. Outputs equal the `aes` crate's; this only avoids its
//! per-call dispatch and interleaves independent blocks.
//!
//! The key schedule computes SubWord with `AESE` on a word broadcast to all
//! four columns (ShiftRows then only permutes equal columns), so it has no
//! table lookups.

use core::arch::aarch64::*;

type B = uint8x16_t;

/// The eleven round keys of AES-128 for `key` (little-endian bytes).
///
/// # Safety
/// The CPU must support `neon` and `aes`.
#[target_feature(enable = "neon,aes")]
pub unsafe fn schedule(key: u128) -> [u128; 11] {
    const RCON: [u32; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
    let mut w = [key as u32, (key >> 32) as u32, (key >> 64) as u32, (key >> 96) as u32];
    let pack = |w: &[u32; 4]| w[0] as u128 | (w[1] as u128) << 32 | (w[2] as u128) << 64 | (w[3] as u128) << 96;
    let mut rk = [0u128; 11];
    rk[0] = pack(&w);
    for (r, c) in RCON.iter().enumerate() {
        let sub = vaeseq_u8(vreinterpretq_u8_u32(vdupq_n_u32(w[3])), vdupq_n_u8(0));
        let t = vgetq_lane_u32(vreinterpretq_u32_u8(sub), 0).rotate_right(8) ^ c;
        w[0] ^= t;
        w[1] ^= w[0];
        w[2] ^= w[1];
        w[3] ^= w[2];
        rk[r + 1] = pack(&w);
    }
    rk
}

#[inline(always)]
fn load(x: &u128) -> B {
    // SAFETY: a `&u128` is valid for a 16-byte load.
    unsafe { vld1q_u8(x as *const u128 as *const u8) }
}

#[inline(always)]
fn block(x: u128) -> B {
    // SAFETY: plain register moves.
    unsafe { vreinterpretq_u8_u64(vcombine_u64(vcreate_u64(x as u64), vcreate_u64((x >> 64) as u64))) }
}

#[inline(always)]
fn value(b: B) -> u128 {
    // SAFETY: plain register moves.
    unsafe {
        let v = vreinterpretq_u64_u8(b);
        vgetq_lane_u64(v, 0) as u128 | (vgetq_lane_u64(v, 1) as u128) << 64
    }
}

/// `E(a) XOR E(b)` under the round keys `rk`, the two blocks interleaved.
///
/// # Safety
/// The CPU must support `neon` and `aes`.
#[target_feature(enable = "neon,aes")]
pub unsafe fn encrypt2_xor(rk: &[u128; 11], a: u128, b: u128) -> u128 {
    let (mut x, mut y) = (block(a), block(b));
    for k in &rk[..9] {
        let k = load(k);
        x = vaesmcq_u8(vaeseq_u8(x, k));
        y = vaesmcq_u8(vaeseq_u8(y, k));
    }
    let k9 = load(&rk[9]);
    // The final round keys cancel in the XOR of the two blocks.
    value(veorq_u8(vaeseq_u8(x, k9), vaeseq_u8(y, k9)))
}

/// Write `n` encrypted counter blocks `label << 64 | (first + i)` to `out`,
/// twelve in flight at a time.
///
/// # Safety
/// The CPU must support `neon` and `aes`; `out` must be valid for writes of
/// `16 n` bytes, which may be uninitialized.
#[target_feature(enable = "neon,aes")]
pub unsafe fn ctr(rk: &[u128; 11], label: u64, first: u64, out: *mut u8, n: usize) {
    const N: usize = 12;
    let k: [B; 11] = core::array::from_fn(|i| load(&rk[i]));
    let counter = |c: u64| block((label as u128) << 64 | c as u128);
    let mut i = 0;
    while i < n {
        let m = N.min(n - i);
        let mut b: [B; N] = core::array::from_fn(|j| counter(first + (i + j) as u64));
        for key in &k[..9] {
            for x in b.iter_mut() {
                *x = vaesmcq_u8(vaeseq_u8(*x, *key));
            }
        }
        for (j, x) in b.iter().enumerate().take(m) {
            // SAFETY: block `i + j < n` lies within `out`.
            unsafe { vst1q_u8(out.add(16 * (i + j)), veorq_u8(vaeseq_u8(*x, k[9]), k[10])) };
        }
        i += m;
    }
}
