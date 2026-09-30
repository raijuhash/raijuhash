//! AES-128 encryption with AES-NI, and 512-bit VAES for counter mode when
//! the CPU has it, for seed expansion and MAC masking. Outputs equal the
//! `aes` crate's; this only avoids its per-call dispatch and keeps more
//! independent blocks in flight.

use core::arch::x86_64::*;

type B = __m128i;

#[inline(always)]
unsafe fn load(x: &u128) -> B {
    unsafe { _mm_loadu_si128((x as *const u128).cast()) }
}

#[inline(always)]
unsafe fn block(x: u128) -> B {
    unsafe { _mm_set_epi64x((x >> 64) as i64, x as i64) }
}

#[inline(always)]
unsafe fn value(b: B) -> u128 {
    let mut r = 0u128;
    unsafe { _mm_storeu_si128((&mut r as *mut u128).cast(), b) };
    r
}

/// Whether this CPU has AES-NI (and SSE4.1 for the schedule's moves).
pub(crate) fn supported() -> bool {
    let mut ok = cfg!(all(target_feature = "aes", target_feature = "sse4.1"));
    #[cfg(feature = "std")]
    if !ok {
        ok = std::arch::is_x86_feature_detected!("aes") && std::arch::is_x86_feature_detected!("sse4.1");
    }
    ok
}

/// Whether counter mode can use 512-bit VAES.
pub(crate) fn wide_supported() -> bool {
    let mut ok = cfg!(all(target_feature = "vaes", target_feature = "avx512f"));
    #[cfg(feature = "std")]
    if !ok {
        ok = std::arch::is_x86_feature_detected!("vaes") && std::arch::is_x86_feature_detected!("avx512f");
    }
    ok && supported()
}

/// The eleven round keys of AES-128 for `key` (little-endian bytes).
///
/// # Safety
/// The CPU must support `aes` and `sse4.1`.
#[target_feature(enable = "sse2,sse4.1,aes")]
pub unsafe fn schedule(key: u128) -> [u128; 11] {
    unsafe {
        // One round of the expansion: SubWord(RotWord(w3)) ^ rcon from
        // `aeskeygenassist`, plus the running XOR of the previous words,
        // computed alongside it rather than after (the rounds are serial).
        macro_rules! next {
            ($k:expr, $rcon:literal) => {{
                let k = $k;
                let t = _mm_shuffle_epi32::<0xff>(_mm_aeskeygenassist_si128::<$rcon>(k));
                let run = _mm_xor_si128(
                    _mm_xor_si128(k, _mm_slli_si128::<4>(k)),
                    _mm_xor_si128(_mm_slli_si128::<8>(k), _mm_slli_si128::<12>(k)),
                );
                _mm_xor_si128(run, t)
            }};
        }
        let k0 = block(key);
        let k1 = next!(k0, 0x01);
        let k2 = next!(k1, 0x02);
        let k3 = next!(k2, 0x04);
        let k4 = next!(k3, 0x08);
        let k5 = next!(k4, 0x10);
        let k6 = next!(k5, 0x20);
        let k7 = next!(k6, 0x40);
        let k8 = next!(k7, 0x80);
        let k9 = next!(k8, 0x1b);
        let k10 = next!(k9, 0x36);
        [k0, k1, k2, k3, k4, k5, k6, k7, k8, k9, k10].map(|k| value(k))
    }
}

/// `E(a) XOR E(b)` under the round keys `rk`, the two blocks interleaved.
///
/// # Safety
/// The CPU must support `aes` and `sse4.1`.
#[target_feature(enable = "sse2,sse4.1,aes")]
pub unsafe fn encrypt2_xor(rk: &[u128; 11], a: u128, b: u128) -> u128 {
    unsafe {
        let k0 = load(&rk[0]);
        let (mut x, mut y) = (_mm_xor_si128(block(a), k0), _mm_xor_si128(block(b), k0));
        for k in &rk[1..10] {
            let k = load(k);
            x = _mm_aesenc_si128(x, k);
            y = _mm_aesenc_si128(y, k);
        }
        // The final round keys cancel in the XOR of the two blocks.
        let z = _mm_setzero_si128();
        value(_mm_xor_si128(_mm_aesenclast_si128(x, z), _mm_aesenclast_si128(y, z)))
    }
}

/// Write `n` encrypted counter blocks `label << 64 | (first + i)` to `out`,
/// eight in flight at a time.
///
/// # Safety
/// The CPU must support `aes` and `sse4.1`; `out` must be valid for writes of
/// `16 n` bytes, which may be uninitialized.
#[target_feature(enable = "sse2,sse4.1,aes")]
pub unsafe fn ctr(rk: &[u128; 11], label: u64, first: u64, out: *mut u8, n: usize) {
    unsafe {
        const N: usize = 8;
        let k: [B; 11] = core::array::from_fn(|i| load(&rk[i]));
        let mut i = 0;
        while i < n {
            let m = N.min(n - i);
            let mut b: [B; N] =
                core::array::from_fn(|j| _mm_xor_si128(block((label as u128) << 64 | (first + (i + j) as u64) as u128), k[0]));
            for key in &k[1..10] {
                for x in b.iter_mut() {
                    *x = _mm_aesenc_si128(*x, *key);
                }
            }
            for (j, x) in b.iter().enumerate().take(m) {
                // SAFETY: block `i + j < n` lies within `out`.
                _mm_storeu_si128(out.add(16 * (i + j)).cast(), _mm_aesenclast_si128(*x, k[10]));
            }
            i += m;
        }
    }
}

/// `ctr` with 512-bit VAES, four blocks per vector: batches of 72 blocks
/// (a seeded key's eager rows), then 8 and 4. Zen 5 issues two vector rounds
/// per cycle with a latency of four, so a batch needs at least eight vectors
/// to keep it busy.
///
/// # Safety
/// The CPU must support `aes`, `sse4.1`, `vaes` and `avx512f`; `out` must be
/// valid for writes of `16 n` bytes, which may be uninitialized.
#[target_feature(enable = "sse2,sse4.1,aes,vaes,avx512f")]
pub unsafe fn ctr_wide(rk: &[u128; 11], label: u64, first: u64, out: *mut u8, n: usize) {
    unsafe {
        let k: [__m512i; 11] = core::array::from_fn(|i| _mm512_broadcast_i32x4(load(&rk[i])));
        // Lane `j` holds counter `first + j`; the label is the high word.
        let base = _mm512_add_epi64(
            _mm512_set_epi64(label as i64, 3, label as i64, 2, label as i64, 1, label as i64, 0),
            _mm512_maskz_set1_epi64(0x55, first as i64),
        );
        let mut i = 0;
        while n - i >= 72 {
            batch::<18>(&k, base, i, out);
            i += 72;
        }
        while n - i >= 8 {
            batch::<2>(&k, base, i, out);
            i += 8;
        }
        if n - i >= 4 {
            batch::<1>(&k, base, i, out);
            i += 4;
        }
        if i < n {
            // SAFETY: the remaining blocks lie within `out`.
            ctr(rk, label, first + i as u64, out.add(16 * i), n - i);
        }
    }
}

/// A seeded key's eager blocks in one batch: 7 field blocks (counters from
/// `fields_first`, two vectors) to `fields` and `4 V` table blocks (counters
/// from 0) to `table`, every round in parallel. Two `ctr_wide` calls would
/// wait out the ten-round latency once more for the field blocks.
///
/// # Safety
/// As for `ctr_wide`; `table` must be valid for writes of `64 V` bytes and
/// `fields` for 112.
#[target_feature(enable = "sse2,sse4.1,aes,vaes,avx512f")]
pub unsafe fn eager_wide<const V: usize>(rk: &[u128; 11], label: u64, table: *mut u8, fields_first: u64, fields: *mut u8) {
    unsafe {
        let k: [__m512i; 11] = core::array::from_fn(|i| _mm512_broadcast_i32x4(load(&rk[i])));
        let lanes = _mm512_set_epi64(label as i64, 3, label as i64, 2, label as i64, 1, label as i64, 0);
        let at = |c: u64| _mm512_xor_si512(_mm512_add_epi64(lanes, _mm512_maskz_set1_epi64(0x55, c as i64)), k[0]);
        // The field vectors' rounds first in program order: the scheduler
        // issues the oldest ready instruction first, so their chain runs at
        // its latency beside the rows' and the key's setup from the field
        // elements can start before the rows are done (3-4% faster than
        // interleaving all rounds).
        let mut f: [__m512i; 2] = core::array::from_fn(|j| at(fields_first + 4 * j as u64));
        for key in &k[1..10] {
            for x in f.iter_mut() {
                *x = _mm512_aesenc_epi128(*x, *key);
            }
        }
        _mm512_storeu_si512(fields.cast(), _mm512_aesenclast_epi128(f[0], k[10]));
        // Blocks 4..7 of the fields: three of them.
        _mm512_mask_storeu_epi64(fields.add(64).cast(), 0x3f, _mm512_aesenclast_epi128(f[1], k[10]));
        let mut b: [__m512i; V] = core::array::from_fn(|j| at(4 * j as u64));
        for key in &k[1..10] {
            for x in b.iter_mut() {
                *x = _mm512_aesenc_epi128(*x, *key);
            }
        }
        for (j, x) in b.iter().enumerate() {
            _mm512_storeu_si512(table.add(64 * j).cast(), _mm512_aesenclast_epi128(*x, k[10]));
        }
    }
}

/// `4 V` counter blocks from block `i` of `ctr_wide`.
#[inline(always)]
unsafe fn batch<const V: usize>(k: &[__m512i; 11], base: __m512i, i: usize, out: *mut u8) {
    unsafe {
        let mut b: [__m512i; V] = core::array::from_fn(|j| {
            _mm512_xor_si512(_mm512_add_epi64(base, _mm512_maskz_set1_epi64(0x55, (i + 4 * j) as i64)), k[0])
        });
        for key in &k[1..10] {
            for x in b.iter_mut() {
                *x = _mm512_aesenc_epi128(*x, *key);
            }
        }
        for (j, x) in b.iter().enumerate() {
            // SAFETY: the caller has `4 V` blocks from block `i` in `out`.
            _mm512_storeu_si512(out.add(16 * (i + 4 * j)).cast(), _mm512_aesenclast_epi128(*x, k[10]));
        }
    }
}
