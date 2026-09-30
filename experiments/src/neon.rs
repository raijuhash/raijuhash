//! NEON compression-kernel prototypes for candidate screening.
//!
//! Every kernel consumes whole chunks of input with its own key-table layout
//! and XOR/add-accumulates into a scratch state. They are throughput probes:
//! only the chain kernels are later turned into a specified hash.

use core::arch::aarch64::*;

pub type V = uint64x2_t;

#[inline(always)]
pub unsafe fn ld(p: *const u8) -> V {
    unsafe { vreinterpretq_u64_u8(vld1q_u8(p)) }
}

#[inline(always)]
pub fn xor(a: V, b: V) -> V {
    unsafe { veorq_u64(a, b) }
}

#[inline(always)]
pub fn xor3(a: V, b: V, c: V) -> V {
    unsafe { veor3q_u64(a, b, c) }
}

/// Carryless product of the low 64-bit lanes.
#[inline(always)]
pub fn clmul_lo(a: V, b: V) -> V {
    unsafe {
        vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64(a, 0), vgetq_lane_u64(b, 0)))
    }
}

/// Carryless product of the high 64-bit lanes.
#[inline(always)]
pub fn clmul_hi(a: V, b: V) -> V {
    unsafe {
        vreinterpretq_u64_p128(vmull_high_p64(vreinterpretq_p64_u64(a), vreinterpretq_p64_u64(b)))
    }
}

pub fn zero() -> V {
    unsafe { vdupq_n_u64(0) }
}

/// Chain-coded carryless NH over a stripe of `n` 128-byte blocks.
///
/// Block `j` holds an X row (bytes 0..64) and a Y row (bytes 64..128); lane
/// `l` of both rows forms one symbol. Per lane, encoded symbol `j` is
/// `row[j-1] ^ row[j]` (with zero rows outside the stripe), so `n` blocks give
/// `n + 1` encoded symbols, each with its own 128 key bytes.
///
/// Expanded as straight-line code so every accumulator index is a constant.
#[macro_export]
macro_rules! chain_stripe {
    ($acc:ident, $data:expr, $key:expr, [$($j:literal)*], $n:literal) => {{
        use $crate::neon::*;
        let data: *const u8 = $data;
        let key: *const u8 = $key;
        let mut px = [zero(); 4];
        let mut py = [zero(); 4];
        for q in 0..4 {
            let x = ld(data.add(16 * q));
            let y = ld(data.add(64 + 16 * q));
            let t = xor(x, ld(key.add(16 * q)));
            let u = xor(y, ld(key.add(64 + 16 * q)));
            $acc[0] = xor3($acc[0], clmul_lo(t, u), clmul_hi(t, u));
            px[q] = x;
            py[q] = y;
        }
        $({
            let d = data.add(128 * $j);
            let k = key.add(128 * $j);
            for q in 0..4 {
                let x = ld(d.add(16 * q));
                let y = ld(d.add(64 + 16 * q));
                let t = xor3(px[q], x, ld(k.add(16 * q)));
                let u = xor3(py[q], y, ld(k.add(64 + 16 * q)));
                $acc[$j] = xor3($acc[$j], clmul_lo(t, u), clmul_hi(t, u));
                px[q] = x;
                py[q] = y;
            }
        })*
        let k = key.add(128 * $n);
        for q in 0..4 {
            let t = xor(px[q], ld(k.add(16 * q)));
            let u = xor(py[q], ld(k.add(64 + 16 * q)));
            $acc[$n] = xor3($acc[$n], clmul_lo(t, u), clmul_hi(t, u));
        }
    }};
}

/// Chain code with 32-byte units (X = 16 bytes, Y = 16 bytes), NEON-native.
#[macro_export]
macro_rules! chain32_stripe {
    ($acc:ident, $data:expr, $key:expr, [$($j:literal)*], $n:literal) => {{
        use $crate::neon::*;
        let data: *const u8 = $data;
        let key: *const u8 = $key;
        let x = ld(data);
        let y = ld(data.add(16));
        let t = xor(x, ld(key));
        let u = xor(y, ld(key.add(16)));
        $acc[0] = xor3($acc[0], clmul_lo(t, u), clmul_hi(t, u));
        let (mut px, mut py) = (x, y);
        $({
            let x = ld(data.add(32 * $j));
            let y = ld(data.add(32 * $j + 16));
            let t = xor3(px, x, ld(key.add(32 * $j)));
            let u = xor3(py, y, ld(key.add(32 * $j + 16)));
            $acc[$j] = xor3($acc[$j], clmul_lo(t, u), clmul_hi(t, u));
            px = x;
            py = y;
        })*
        let t = xor(px, ld(key.add(32 * $n)));
        let u = xor(py, ld(key.add(32 * $n + 16)));
        $acc[$n] = xor3($acc[$n], clmul_lo(t, u), clmul_hi(t, u));
    }};
}

/// Parity-coded carryless NH: `n` data blocks plus one independently keyed
/// parity block per lane group (U4/U8/U16 in the candidate catalogue).
#[macro_export]
macro_rules! parity_stripe {
    ($acc:ident, $data:expr, $key:expr, [$($j:literal)*], $n:literal) => {{
        use $crate::neon::*;
        let data: *const u8 = $data;
        let key: *const u8 = $key;
        let mut px = [zero(); 4];
        let mut py = [zero(); 4];
        $({
            let d = data.add(128 * $j);
            let k = key.add(128 * $j);
            for q in 0..4 {
                let x = ld(d.add(16 * q));
                let y = ld(d.add(64 + 16 * q));
                let t = xor(x, ld(k.add(16 * q)));
                let u = xor(y, ld(k.add(64 + 16 * q)));
                $acc[$j] = xor3($acc[$j], clmul_lo(t, u), clmul_hi(t, u));
                px[q] = xor(px[q], x);
                py[q] = xor(py[q], y);
            }
        })*
        let k = key.add(128 * $n);
        for q in 0..4 {
            let t = xor(px[q], ld(k.add(16 * q)));
            let u = xor(py[q], ld(k.add(64 + 16 * q)));
            $acc[$n] = xor3($acc[$n], clmul_lo(t, u), clmul_hi(t, u));
        }
    }};
}

/// Column-major chain: walk one 16-byte column through all blocks, so only
/// one previous X/Y register pair is live at a time.
#[macro_export]
macro_rules! chain_stripe_cm {
    ($acc:ident, $data:expr, $key:expr, [$($j:literal)*], $n:literal) => {{
        use $crate::neon::*;
        let data: *const u8 = $data;
        let key: *const u8 = $key;
        for q in 0..4 {
            let x = ld(data.add(16 * q));
            let y = ld(data.add(64 + 16 * q));
            let t = xor(x, ld(key.add(16 * q)));
            let u = xor(y, ld(key.add(64 + 16 * q)));
            $acc[0] = xor3($acc[0], clmul_lo(t, u), clmul_hi(t, u));
            let (mut px, mut py) = (x, y);
            $({
                let x = ld(data.add(128 * $j + 16 * q));
                let y = ld(data.add(128 * $j + 64 + 16 * q));
                let t = xor3(px, x, ld(key.add(128 * $j + 16 * q)));
                let u = xor3(py, y, ld(key.add(128 * $j + 64 + 16 * q)));
                $acc[$j] = xor3($acc[$j], clmul_lo(t, u), clmul_hi(t, u));
                px = x;
                py = y;
            })*
            let t = xor(px, ld(key.add(128 * $n + 16 * q)));
            let u = xor(py, ld(key.add(128 * $n + 64 + 16 * q)));
            $acc[$n] = xor3($acc[$n], clmul_lo(t, u), clmul_hi(t, u));
        }
    }};
}

/// Column-major parity code (U4/U8/U16) to limit register pressure.
#[macro_export]
macro_rules! parity_stripe_cm {
    ($acc:ident, $data:expr, $key:expr, [$($j:literal)*], $n:literal) => {{
        use $crate::neon::*;
        let data: *const u8 = $data;
        let key: *const u8 = $key;
        for q in 0..4 {
            let mut px = zero();
            let mut py = zero();
            $({
                let x = ld(data.add(128 * $j + 16 * q));
                let y = ld(data.add(128 * $j + 64 + 16 * q));
                let t = xor(x, ld(key.add(128 * $j + 16 * q)));
                let u = xor(y, ld(key.add(128 * $j + 64 + 16 * q)));
                $acc[$j] = xor3($acc[$j], clmul_lo(t, u), clmul_hi(t, u));
                px = xor(px, x);
                py = xor(py, y);
            })*
            let t = xor(px, ld(key.add(128 * $n + 16 * q)));
            let u = xor(py, ld(key.add(128 * $n + 64 + 16 * q)));
            $acc[$n] = xor3($acc[$n], clmul_lo(t, u), clmul_hi(t, u));
        }
    }};
}

/// Plain carryless NH (only 2^-64): the lower bound on work for this family.
#[inline(always)]
pub unsafe fn nh_block(acc: &mut [V], data: *const u8, key: *const u8) {
    unsafe {
        for q in 0..4 {
            let t = xor(ld(data.add(16 * q)), ld(key.add(16 * q)));
            let u = xor(ld(data.add(64 + 16 * q)), ld(key.add(64 + 16 * q)));
            acc[q] = xor3(acc[q], clmul_lo(t, u), clmul_hi(t, u));
        }
    }
}

/// D2: two independently keyed carryless NH streams over the same block.
#[inline(always)]
pub unsafe fn d2_block(acc: &mut [V], data: *const u8, key: *const u8) {
    unsafe {
        for q in 0..4 {
            let x = ld(data.add(16 * q));
            let y = ld(data.add(64 + 16 * q));
            let t = xor(x, ld(key.add(16 * q)));
            let u = xor(y, ld(key.add(64 + 16 * q)));
            acc[q] = xor3(acc[q], clmul_lo(t, u), clmul_hi(t, u));
            let t = xor(x, ld(key.add(128 + 16 * q)));
            let u = xor(y, ld(key.add(192 + 16 * q)));
            acc[4 + q] = xor3(acc[4 + q], clmul_lo(t, u), clmul_hi(t, u));
        }
    }
}

/// L128: XOR of full-field products `K[i] * M[i]`, reduction deferred. Key
/// entries hold `k` and its half-swapped copy (32 bytes per 16 data bytes).
#[inline(always)]
pub unsafe fn l128_block(acc: &mut [V], data: *const u8, key: *const u8) {
    unsafe {
        for i in 0..8 {
            let m = ld(data.add(16 * i));
            let k = ld(key.add(32 * i));
            let ks = ld(key.add(32 * i + 16));
            let s = i & 1;
            acc[3 * s] = xor(acc[3 * s], clmul_lo(m, k));
            acc[3 * s + 1] = xor(acc[3 * s + 1], clmul_hi(m, k));
            acc[3 * s + 2] = xor3(acc[3 * s + 2], clmul_lo(m, ks), clmul_hi(m, ks));
        }
    }
}

/// POLYVAL-style Horner with 8-block aggregation: compact key, no table.
/// `pw` holds R^8..R^1 and their half-swapped copies.
#[inline(always)]
pub unsafe fn poly_block(h: &mut V, data: *const u8, pw: &[V; 16]) {
    unsafe {
        let (mut lo, mut hi, mut mid) = (zero(), zero(), zero());
        for i in 0..8 {
            let mut m = ld(data.add(16 * i));
            if i == 0 {
                m = xor(m, *h);
            }
            lo = xor(lo, clmul_lo(m, pw[i]));
            hi = xor(hi, clmul_hi(m, pw[i]));
            mid = xor3(mid, clmul_lo(m, pw[8 + i]), clmul_hi(m, pw[8 + i]));
        }
        *h = reduce(lo, hi, mid);
    }
}

/// Reduce `lo ^ mid*x^64 ^ hi*x^128` modulo x^128 + x^7 + x^2 + x + 1.
#[inline(always)]
pub fn reduce(lo: V, hi: V, mid: V) -> V {
    unsafe {
        let z = zero();
        let lo = xor(lo, vextq_u64(z, mid, 1));
        let hi = xor(hi, vextq_u64(mid, z, 1));
        let poly = vdupq_n_u64(0x87);
        // Fold the top word into words 1..2, then word 2 into words 0..1.
        let t = clmul_hi(hi, poly);
        let hi = xor(hi, vextq_u64(t, z, 1));
        let lo = xor(lo, vextq_u64(z, t, 1));
        xor(lo, clmul_lo(hi, poly))
    }
}

/// I4: four independent 32-bit integer NH streams (UMAC NH style).
#[inline(always)]
pub unsafe fn i4_block(acc: &mut [V], data: *const u8, key: *const u8) {
    unsafe {
        let mut x = [vdupq_n_u32(0); 4];
        let mut y = [vdupq_n_u32(0); 4];
        for q in 0..4 {
            x[q] = vld1q_u32(data.add(16 * q) as *const u32);
            y[q] = vld1q_u32(data.add(64 + 16 * q) as *const u32);
        }
        for s in 0..4 {
            let k = key.add(128 * s);
            let mut a = acc[s];
            for q in 0..4 {
                let t = vaddq_u32(x[q], vld1q_u32(k.add(16 * q) as *const u32));
                let u = vaddq_u32(y[q], vld1q_u32(k.add(64 + 16 * q) as *const u32));
                a = vmlal_u32(a, vget_low_u32(t), vget_low_u32(u));
                a = vmlal_high_u32(a, t, u);
            }
            acc[s] = a;
        }
    }
}

/// M32: Multimixer-style block (key add, two circulant mixes, eight widening
/// 32x32 products per 32 bytes), sums kept in 64-bit lanes. Constants follow
/// the published shape for cost estimation only; not a verified port.
#[inline(always)]
pub unsafe fn m32_block(acc: &mut [V], data: *const u8, key: *const u8) {
    unsafe {
        for q in 0..4 {
            let x = vaddq_u32(
                vld1q_u32(data.add(32 * q) as *const u32),
                vld1q_u32(key.add(32 * q) as *const u32),
            );
            let y = vaddq_u32(
                vld1q_u32(data.add(32 * q + 16) as *const u32),
                vld1q_u32(key.add(32 * q + 16) as *const u32),
            );
            let x2 = vaddq_u32(vaddq_u32(x, vextq_u32(x, x, 1)), vextq_u32(x, x, 2));
            let y2 = vaddq_u32(vaddq_u32(y, vextq_u32(y, y, 1)), vextq_u32(y, y, 3));
            acc[0] = vmlal_u32(acc[0], vget_low_u32(x), vget_low_u32(y));
            acc[1] = vmlal_high_u32(acc[1], x, y);
            acc[2] = vmlal_u32(acc[2], vget_low_u32(x2), vget_low_u32(y2));
            acc[3] = vmlal_high_u32(acc[3], x2, y2);
        }
    }
}
