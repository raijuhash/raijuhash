//! x86-64 backends: PCLMULQDQ with 128-bit vectors, VPCLMULQDQ with 256-bit
//! (AVX2) vectors, and the separate AVX-512 backend in `avx512`.
//!
//! One generic kernel is instantiated per vector width here: a 64-byte row
//! is `Q` vectors, each 128-bit lane of a vector holds one 16-byte column,
//! and the per-lane products `lo * lo` / `hi * hi` of a carryless multiply
//! are the lane products of the specification. Sums are kept per 128-bit
//! lane and folded together only when a chunk is combined, which is linear.
//! The chunk fold, outer step and output use 128-bit PCLMULQDQ arithmetic
//! shared by all widths. Semantics match `neon.rs` and `reference.rs`
//! exactly.

// Whether an intrinsic needs `unsafe` inside a matching `target_feature`
// function depends on the Rust version; keep the blocks and silence the lint.
#![allow(unused_unsafe)]

use core::arch::x86_64::*;

use crate::params::{BLOCK, CHUNK, CHUNK_BLOCKS};
use crate::state::{Core, GROUP_BLOCKS, padded_block, partial16};
use crate::{FieldKey, Key, S2Coeffs};

#[cfg(feature = "aes")]
pub mod aes;
pub mod avx512;

type X = __m128i;

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn xzero() -> X {
    unsafe { _mm_setzero_si128() }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn xxor(a: X, b: X) -> X {
    unsafe { _mm_xor_si128(a, b) }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn ldu(x: &u128) -> X {
    unsafe { _mm_loadu_si128(x as *const u128 as *const X) }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn stu(x: &mut u128, v: X) {
    unsafe { _mm_storeu_si128(x as *mut u128 as *mut X, v) }
}

/// Store to a word that may be uninitialized, which a `&mut u128` must not
/// point to.
///
/// # Safety
/// `p` must be valid for writes.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
unsafe fn stu_raw(p: *mut u128, v: X) {
    unsafe { _mm_storeu_si128(p as *mut X, v) }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn from_x(v: X) -> u128 {
    let mut r = 0u128;
    stu(&mut r, v);
    r
}

/// `(0, a.lo)`: move the low word up.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn up(a: X) -> X {
    unsafe { _mm_slli_si128::<8>(a) }
}

/// `(a.hi, 0)`: move the high word down.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn down(a: X) -> X {
    unsafe { _mm_srli_si128::<8>(a) }
}

/// Unreduced 256-bit sum of products: `lo + mid x^64 + hi x^128`.
pub(crate) struct Wide {
    lo: X,
    mid: X,
    hi: X,
}

impl Wide {
    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) fn new() -> Self {
        Wide { lo: xzero(), mid: xzero(), hi: xzero() }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul(&mut self, a: X, k: &FieldKey) {
        unsafe {
            let (kv, ks) = (ldu(&k.k), ldu(&k.swapped));
            self.lo = xxor(self.lo, _mm_clmulepi64_si128::<0x00>(a, kv));
            self.hi = xxor(self.hi, _mm_clmulepi64_si128::<0x11>(a, kv));
            self.mid = xxor(self.mid, xxor(_mm_clmulepi64_si128::<0x00>(a, ks), _mm_clmulepi64_si128::<0x11>(a, ks)));
        }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul64(&mut self, a: X, k: &FieldKey) {
        unsafe {
            let (kv, ks) = (ldu(&k.k), ldu(&k.swapped));
            self.lo = xxor(self.lo, _mm_clmulepi64_si128::<0x00>(a, kv));
            self.mid = xxor(self.mid, _mm_clmulepi64_si128::<0x00>(a, ks));
        }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn reduce(self) -> X {
        unsafe {
            let poly = _mm_set_epi64x(0x87, 0x87);
            let t = _mm_clmulepi64_si128::<0x11>(self.hi, poly);
            let w2 = xxor(xxor(self.hi, down(self.mid)), down(t));
            let lo = xxor(xxor(self.lo, up(self.mid)), up(t));
            xxor(lo, _mm_clmulepi64_si128::<0x00>(w2, poly))
        }
    }
}

/// Unreduced sum of products `lo + mid x^64` with keys `K` whose `K' = x^64
/// K` is prepared (`Key::pk`, `Key::sk`): `a K = (a.lo K.lo + a.hi K'.lo) +
/// (a.lo K.hi + a.hi K'.hi) x^64`, so the reduction folds back one word.
pub(crate) struct Wide64 {
    lo: X,
    mid: X,
}

impl Wide64 {
    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) fn new() -> Self {
        Wide64 { lo: xzero(), mid: xzero() }
    }

    /// `+= a K`, with `k64 = x^64 K`.
    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul(&mut self, a: X, k: &u128, k64: &u128) {
        unsafe {
            let (kv, kx) = (ldu(k), ldu(k64));
            self.lo = xxor(self.lo, xxor(_mm_clmulepi64_si128::<0x00>(a, kv), _mm_clmulepi64_si128::<0x01>(a, kx)));
            self.mid = xxor(self.mid, xxor(_mm_clmulepi64_si128::<0x10>(a, kv), _mm_clmulepi64_si128::<0x11>(a, kx)));
        }
    }

    /// `+= a K` for `a` in the low word.
    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul64(&mut self, a: X, k: &u128) {
        unsafe {
            let kv = ldu(k);
            self.lo = xxor(self.lo, _mm_clmulepi64_si128::<0x00>(a, kv));
            self.mid = xxor(self.mid, _mm_clmulepi64_si128::<0x10>(a, kv));
        }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn reduce(self) -> X {
        unsafe {
            let fold = _mm_clmulepi64_si128::<0x01>(self.mid, _mm_set_epi64x(0, 0x87));
            xxor(xxor(self.lo, up(self.mid)), fold)
        }
    }

    /// `+= a R`, `a R2` and `L T` with the keys of `Key::pk`.
    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul_r(&mut self, key: &Key, a: X) {
        unsafe { self.mul(a, &key.pk[0], &key.pk[4]) }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul_r2(&mut self, key: &Key, a: X) {
        unsafe { self.mul(a, &key.pk[2], &key.pk[6]) }
    }

    #[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
    #[inline]
    pub(crate) unsafe fn mul_len(&mut self, key: &Key, len: u64) {
        unsafe { self.mul64(len_x(len), &key.pk[1]) }
    }
}

/// `sum_k x^k * z_k` for the six small shifts of the chunk fold.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
unsafe fn shifted_sum(z: [X; 6]) -> X {
    unsafe {
        let sl = xxor(
            xxor(xxor(z[0], _mm_slli_epi64::<1>(z[1])), xxor(_mm_slli_epi64::<2>(z[2]), _mm_slli_epi64::<3>(z[3]))),
            xxor(_mm_slli_epi64::<4>(z[4]), _mm_slli_epi64::<5>(z[5])),
        );
        let sr = xxor(
            xxor(_mm_srli_epi64::<63>(z[1]), _mm_srli_epi64::<62>(z[2])),
            xxor(_mm_srli_epi64::<61>(z[3]), xxor(_mm_srli_epi64::<60>(z[4]), _mm_srli_epi64::<59>(z[5]))),
        );
        let low = xxor(sl, up(sr));
        // The at most five bits above x^128 fold back as
        // `o (x^7 + x^2 + x + 1)`, with shifts (no carries between words).
        let o = down(sr);
        let t = xxor(xxor(o, _mm_slli_epi64::<1>(o)), xxor(_mm_slli_epi64::<2>(o), _mm_slli_epi64::<7>(o)));
        xxor(low, t)
    }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn x4(a: X, b: X, c: X, d: X) -> X {
    xxor(xxor(a, b), xxor(c, d))
}

/// `h0` and the planes `z` with `h1 = sum_k x^k z_k` (see `neon::planes`).
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn planes(b: &[X; 8], f: &[X; 3], e: X) -> (X, [X; 6]) {
    let h0 = xxor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
    let z = [
        xxor(e, x4(b[1], b[3], b[5], b[7])),
        x4(b[2], b[3], b[6], b[7]),
        x4(b[4], b[5], b[6], b[7]),
        f[0],
        f[1],
        f[2],
    ];
    (h0, z)
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
unsafe fn outer_step(key: &Key, outer: X, b: &[X; 8], f: &[X; 3], e: X) -> X {
    unsafe {
        let (h0, z) = planes(b, f, e);
        let mut w = Wide64::new();
        w.mul_r(key, xxor(outer, h0));
        w.mul_r2(key, shifted_sum(z));
        w.reduce()
    }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn add_planes(f: &mut [X; 3], u: usize, x: X) {
    for (b, fb) in f.iter_mut().enumerate() {
        if u >> b & 1 == 1 {
            *fb = xxor(*fb, x);
        }
    }
}

#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
fn len_x(len: u64) -> X {
    unsafe { _mm_set_epi64x(0, len as i64) }
}

/// The short path for messages of fewer than 32 bytes, for every width:
/// `x0 A + x1 B + S`. With `A' = x^64 A` prepared (`Key::sk`),
/// `x A = (x.lo A.lo + x.hi A'.lo) + (x.lo A.hi + x.hi A'.hi) x^64`: four
/// independent products whose sum needs one fold of its top word, instead
/// of a two-step reduction.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
#[inline]
pub(crate) unsafe fn short(key: &Key, x0: u128, x1: Option<u128>) -> u128 {
    unsafe {
        let (mut lo, mut mid) = (xzero(), xzero());
        let mut term = |x: X, k: X, k64: X| {
            lo = xxor(lo, xxor(_mm_clmulepi64_si128::<0x00>(x, k), _mm_clmulepi64_si128::<0x01>(x, k64)));
            mid = xxor(mid, xxor(_mm_clmulepi64_si128::<0x10>(x, k), _mm_clmulepi64_si128::<0x11>(x, k64)));
        };
        term(ldu(&x0), ldu(&key.sk[0]), ldu(&key.sk[2]));
        if let Some(x1) = x1 {
            term(ldu(&x1), ldu(&key.sk[1]), ldu(&key.sk[3]));
        }
        let fold = _mm_clmulepi64_si128::<0x01>(mid, _mm_set_epi64x(0, 0x87));
        from_x(xxor(xxor(lo, up(mid)), xxor(fold, ldu(&key.s))))
    }
}

/// `[x0 | x1]` of a message of `1..SHORT_MAX` bytes (see `short`),
/// without byte-masked loads or length branches: whole dwords come from a
/// dword-masked load (masked-off memory is not accessed) and the last
/// `len % 4` bytes from three byte loads within the message.
///
/// # Safety
/// The CPU must support AVX2, and `1 <= msg.len() < SHORT_MAX`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2")]
#[inline]
unsafe fn short_row_avx2(msg: &[u8]) -> __m256i {
    unsafe {
        let len = msg.len();
        debug_assert!((1..crate::SHORT_MAX).contains(&len));
        let p = msg.as_ptr();
        let (q, t) = (len / 4, len % 4);
        let iota = _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7);
        let qv = _mm256_set1_epi32(q as i32);
        let x = _mm256_maskload_epi32(p.cast(), _mm256_cmpgt_epi32(qv, iota));
        // Bytes `4q..len` as first, middle and last byte (see `partial16`),
        // indices clamped into the message; none when `t == 0`.
        let at = |i: usize| *p.add(i.min(len - 1)) as u32;
        let tail = (at(4 * q) | at(4 * q + t / 2) << (8 * (t / 2)) | at(len - 1) << (8 * ((t + 3) % 4)))
            & 0u32.wrapping_sub((t != 0) as u32);
        let tail = _mm256_and_si256(_mm256_set1_epi32(tail as i32), _mm256_cmpeq_epi32(qv, iota));
        // The length byte: byte 15 of the last element.
        let at_len = _mm256_set1_epi32((3 | (len & 16) >> 2) as i32);
        let marker = _mm256_and_si256(_mm256_set1_epi32((len << 24) as i32), _mm256_cmpeq_epi32(at_len, iota));
        _mm256_or_si256(x, _mm256_or_si256(tail, marker))
    }
}

/// A whole message of `1..SHORT_MAX` bytes for AVX2: the sum of `short`
/// with `[x0 | x1]` from `short_row_avx2` in one vector, as in
/// `avx512::short_msg`.
///
/// # Safety
/// The CPU must support AVX2 and VPCLMULQDQ, and `1 <= msg.len() < SHORT_MAX`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2,vpclmulqdq")]
pub(crate) unsafe fn short_avx2(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let x = short_row_avx2(msg);
        let ks = key.sk.as_ptr().cast::<__m256i>();
        let (k, k64) = (_mm256_loadu_si256(ks), _mm256_loadu_si256(ks.add(1)));
        let lo = _mm256_xor_si256(_mm256_clmulepi64_epi128::<0x00>(x, k), _mm256_clmulepi64_epi128::<0x01>(x, k64));
        let mid = _mm256_xor_si256(_mm256_clmulepi64_epi128::<0x10>(x, k), _mm256_clmulepi64_epi128::<0x11>(x, k64));
        let fold = _mm256_clmulepi64_epi128::<0x01>(mid, _mm256_set1_epi64x(0x87));
        let r = _mm256_xor_si256(_mm256_xor_si256(lo, _mm256_bslli_epi128::<8>(mid)), fold);
        let r = _mm_xor_si128(_mm256_castsi256_si128(r), _mm256_extracti128_si256::<1>(r));
        from_x(_mm_xor_si128(r, ldu(&key.s)))
    }
}

/// `short_avx2` for CPUs with AVX2 but 128-bit carryless multiplies only
/// (the SSE backend on Haswell to Coffee Lake, Zen 1 and 2): the same
/// branchless loads, then `short`'s products.
///
/// # Safety
/// The CPU must support AVX2 and PCLMULQDQ, and `1 <= msg.len() < SHORT_MAX`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2")]
pub(crate) unsafe fn short_avx2_x(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let x = short_row_avx2(msg);
        let mut w = Wide64::new();
        w.mul(_mm256_castsi256_si128(x), &key.sk[0], &key.sk[2]);
        w.mul(_mm256_extracti128_si256::<1>(x), &key.sk[1], &key.sk[3]);
        from_x(xxor(w.reduce(), ldu(&key.s)))
    }
}

/// Byte indices 0..32, for moving a message tail with `pshufb`.
static IOTA: [u8; 32] = {
    let mut a = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        a[i] = i as u8;
        i += 1;
    }
    a
};

/// The X row of a `SHORT_MAX..=S2_MAX`-byte message: `msg` zero-padded to
/// 64 bytes, as four vectors, reading only `msg`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq")]
#[inline]
unsafe fn s2_row(msg: &[u8]) -> [X; 4] {
    unsafe {
        let len = msg.len();
        debug_assert!((crate::SHORT_MAX..=crate::S2_MAX).contains(&len));
        let p = msg.as_ptr();
        // X words 0..4 are always present. Words 4..8 are `msg[32..len]`
        // zero-padded: the last 32 bytes moved down by `64 - len`. Byte `i`
        // of the result is tail byte `k = i + 64 - len`, taken from the
        // first tail vector when `k < 16` and the second when `k < 32`;
        // `pshufb` zeroes a byte whose index has the top bit set.
        let (a, b) = (_mm_loadu_si128(p.add(len - 32).cast()), _mm_loadu_si128(p.add(len - 16).cast()));
        let shift = _mm_set1_epi8((crate::S2_MAX - len) as i8);
        let pick = |k: X| _mm_or_si128(k, _mm_cmpgt_epi8(k, _mm_set1_epi8(15)));
        let moved = |o: usize| {
            let k = _mm_add_epi8(_mm_loadu_si128(IOTA.as_ptr().add(o).cast()), shift);
            _mm_or_si128(_mm_shuffle_epi8(a, pick(k)), _mm_shuffle_epi8(b, pick(_mm_sub_epi8(k, _mm_set1_epi8(16)))))
        };
        [_mm_loadu_si128(p.cast()), _mm_loadu_si128(p.add(16).cast()), moved(0), moved(16)]
    }
}

/// `lo + mid x^64 + L T + C` of the affine form, reduced: `L T` adds its low
/// half to `lo` and its high half to `mid`, and the top word of `mid` folds
/// back through `x^128 = x^7 + x^2 + x + 1`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq")]
#[inline]
unsafe fn s2_close(key: &Key, len: usize, lo: X, mid: X) -> u128 {
    unsafe {
        let (lv, t) = (len_x(len as u64), ldu(&key.t.k));
        let lo = xxor(lo, _mm_clmulepi64_si128::<0x00>(lv, t));
        let mid = xxor(mid, _mm_clmulepi64_si128::<0x10>(lv, t));
        let fold = _mm_clmulepi64_si128::<0x01>(mid, _mm_set_epi64x(0, 0x87));
        from_x(xxor(xxor(xxor(lo, up(mid)), fold), ldu(&key.s2.c)))
    }
}

/// A message of `SHORT_MAX..=S2_MAX` bytes through the affine form of
/// `S2Coeffs` (see `neon`'s `medium`), with 128-bit vectors: 18 carryless
/// products and no length branches.
///
/// # Safety
/// The CPU must support `ssse3`, `sse4.1` and `pclmulqdq`; `msg.len()` must
/// be in `SHORT_MAX..=S2_MAX`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq")]
#[inline]
pub(crate) unsafe fn medium(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let x = s2_row(msg);
        let k = &key.s2;
        let (mut lo, mut mid) = (xzero(), xzero());
        for ((x, ka), kb) in x.iter().zip(&k.ka).zip(&k.kb) {
            let (ka, kb) = (ldu(ka), ldu(kb));
            lo = xxor(lo, xxor(_mm_clmulepi64_si128::<0x00>(*x, ka), _mm_clmulepi64_si128::<0x11>(*x, kb)));
            mid = xxor(mid, xxor(_mm_clmulepi64_si128::<0x11>(*x, ka), _mm_clmulepi64_si128::<0x00>(*x, kb)));
        }
        s2_close(key, msg.len(), lo, mid)
    }
}

/// `medium` with 256-bit multiplies: two X word pairs per vector, eight
/// multiplies instead of sixteen for the row.
///
/// # Safety
/// The CPU must support AVX2 and VPCLMULQDQ; `msg.len()` must be in
/// `SHORT_MAX..=S2_MAX`.
#[target_feature(enable = "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2,vpclmulqdq")]
pub(crate) unsafe fn medium_avx2(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let x = s2_row(msg);
        let (x01, x23) = (_mm256_set_m128i(x[1], x[0]), _mm256_set_m128i(x[3], x[2]));
        let k = &key.s2;
        let ld2 = |a: &[u128]| _mm256_loadu_si256(a.as_ptr().cast());
        let (ka0, ka1, kb0, kb1) = (ld2(&k.ka[..2]), ld2(&k.ka[2..]), ld2(&k.kb[..2]), ld2(&k.kb[2..]));
        let x4 = |a, b, c, d| _mm256_xor_si256(_mm256_xor_si256(a, b), _mm256_xor_si256(c, d));
        let lo = x4(
            _mm256_clmulepi64_epi128::<0x00>(x01, ka0),
            _mm256_clmulepi64_epi128::<0x11>(x01, kb0),
            _mm256_clmulepi64_epi128::<0x00>(x23, ka1),
            _mm256_clmulepi64_epi128::<0x11>(x23, kb1),
        );
        let mid = x4(
            _mm256_clmulepi64_epi128::<0x11>(x01, ka0),
            _mm256_clmulepi64_epi128::<0x00>(x01, kb0),
            _mm256_clmulepi64_epi128::<0x11>(x23, ka1),
            _mm256_clmulepi64_epi128::<0x00>(x23, kb1),
        );
        let lanes = |v: __m256i| _mm_xor_si128(_mm256_castsi256_si128(v), _mm256_extracti128_si256::<1>(v));
        s2_close(key, msg.len(), lanes(lo), lanes(mid))
    }
}

/// `x * k` in GF(2^128).
///
/// # Safety
/// The CPU must support `sse4.1` and `pclmulqdq`.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
pub(crate) unsafe fn mul_key(x: u128, k: &FieldKey) -> u128 {
    unsafe {
        let mut w = Wide::new();
        w.mul(ldu(&x), k);
        from_x(w.reduce())
    }
}

/// `x K` in GF(2^128) for `k = (K, x^64 K)`: one fold instead of a
/// two-step reduction (see `Wide64`).
///
/// # Safety
/// The CPU must support `sse4.1` and `pclmulqdq`.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
pub(crate) unsafe fn mul_x64(x: u128, k: &[u128; 2]) -> u128 {
    unsafe {
        let mut w = Wide64::new();
        w.mul(ldu(&x), &k[0], &k[1]);
        from_x(w.reduce())
    }
}

/// The `S2Coeffs` of a key, from its first two table rows and `R`, `R2`, `S`
/// (see `neon::s2_prepare`).
///
/// # Safety
/// The CPU must support `sse4.1` and `pclmulqdq`; `table` must be readable
/// for two rows.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq")]
pub(crate) unsafe fn s2_prepare(table: *const u8, r: u128, r2: u128, s: u128) -> S2Coeffs {
    // SAFETY: the reads stay within the first two rows.
    let word = |j: usize, i: usize| unsafe { core::ptr::read_unaligned(table.add(BLOCK * j + 8 * i) as *const u64) };
    let lane = |x: u64| unsafe { _mm_set_epi64x(0, x as i64) };
    let (w0, w1) = (FieldKey::new(r), FieldKey::new(r ^ r2));
    unsafe {
        let e: [u128; 8] = core::array::from_fn(|l| {
            let mut w = Wide::new();
            w.mul64(lane(word(0, 8 + l)), &w0);
            w.mul64(lane(word(1, 8 + l)), &w1);
            from_x(w.reduce())
        });
        let (mut n0, mut n1) = (xzero(), xzero());
        for l in 0..8 {
            n0 = xxor(n0, _mm_clmulepi64_si128::<0x00>(lane(word(0, l)), lane(word(0, 8 + l))));
            n1 = xxor(n1, _mm_clmulepi64_si128::<0x00>(lane(word(1, l)), lane(word(1, 8 + l))));
        }
        let mut w = Wide::new();
        w.mul(n0, &w0);
        w.mul(n1, &w1);
        let (lo, hi) = (|x: u128| x as u64 as u128, |x: u128| x >> 64);
        S2Coeffs {
            ka: core::array::from_fn(|q| lo(e[2 * q]) | hi(e[2 * q + 1]) << 64),
            kb: core::array::from_fn(|q| hi(e[2 * q]) | lo(e[2 * q + 1]) << 64),
            c: from_x(w.reduce()) ^ s,
        }
    }
}

/// `padded_block` in whole 32-byte stores, so that the AVX2 kernels'
/// 32-byte loads of it forward (a load spanning two 16-byte stores stalls
/// until they retire: about 14 ns for a 65-byte message on Zen 5).
#[target_feature(enable = "sse2,sse4.1,pclmulqdq,avx,avx2")]
#[inline]
unsafe fn padded_block32(src: &[u8]) -> [u8; BLOCK] {
    unsafe {
        let n = src.len();
        debug_assert!(n < BLOCK);
        let mut dst = core::mem::MaybeUninit::<[u8; BLOCK]>::uninit();
        let d = dst.as_mut_ptr().cast::<__m256i>();
        let (whole, r) = (n / 32, n % 32);
        for i in 0..BLOCK / 32 {
            let v = if i < whole {
                _mm256_loadu_si256(src.as_ptr().add(32 * i).cast())
            } else if i == whole && r != 0 {
                piece32(&src[32 * i..])
            } else {
                _mm256_setzero_si256()
            };
            _mm256_storeu_si256(d.add(i), v);
        }
        dst.assume_init()
    }
}

/// `src` (1 to 31 bytes) zero-padded to a 256-bit vector, reading only `src`.
#[target_feature(enable = "sse2,sse4.1,pclmulqdq,avx,avx2")]
#[inline]
unsafe fn piece32(src: &[u8]) -> __m256i {
    unsafe {
        let (lo, hi) = if src.len() > 16 {
            (_mm_loadu_si128(src.as_ptr().cast()), ldu(&partial16(src, 16)))
        } else {
            (ldu(&partial16(src, 0)), xzero())
        };
        _mm256_set_m128i(hi, lo)
    }
}

macro_rules! backend {
    (
        $name:ident, $features:literal, $v:ty, $q:literal,
        load = $load:expr, zero = $zero:expr, xor = $xor:expr, xor3 = $xor3:expr,
        clmul_lo = $clo:expr, clmul_hi = $chi:expr, fold = $fold:expr, widen = $widen:expr,
        pad = $pad:path $(,)?
    ) => {
        pub mod $name {
            use super::*;

            type V = $v;
            /// Vectors per 64-byte row.
            const Q: usize = $q;
            /// Bytes per vector.
            const W: usize = 64 / Q;

            #[target_feature(enable = $features)]
            #[inline]
            unsafe fn ld(p: *const u8) -> V {
                unsafe { ($load)(p) }
            }
            #[target_feature(enable = $features)]
            #[inline]
            fn vzero() -> V {
                unsafe { ($zero)() }
            }
            #[target_feature(enable = $features)]
            #[inline]
            fn vxor(a: V, b: V) -> V {
                unsafe { ($xor)(a, b) }
            }
            #[target_feature(enable = $features)]
            #[inline]
            fn vxor3(a: V, b: V, c: V) -> V {
                unsafe { ($xor3)(a, b, c) }
            }
            #[target_feature(enable = $features)]
            #[inline]
            fn fold(a: V) -> X {
                unsafe { ($fold)(a) }
            }
            #[target_feature(enable = $features)]
            #[inline]
            fn widen(a: X) -> V {
                unsafe { ($widen)(a) }
            }

            type Row = [V; Q];

            #[target_feature(enable = $features)]
            #[inline]
            unsafe fn rows(d: *const u8) -> (Row, Row) {
                unsafe { (core::array::from_fn(|q| ld(d.add(W * q))), core::array::from_fn(|q| ld(d.add(64 + W * q)))) }
            }

            /// Product sum of one position, per 128-bit lane (fold for the
            /// real sum): previous rows `p*`, current rows `x`, `y`, key `k`.
            #[target_feature(enable = $features)]
            #[inline]
            unsafe fn position(px: &Row, py: &Row, x: &Row, y: &Row, k: *const u8) -> V {
                unsafe {
                    let mut s = vzero();
                    for q in 0..Q {
                        let t = vxor3(px[q], x[q], ld(k.add(W * q)));
                        let u = vxor3(py[q], y[q], ld(k.add(64 + W * q)));
                        s = vxor3(s, ($clo)(t, u), ($chi)(t, u));
                    }
                    s
                }
            }

            #[target_feature(enable = $features)]
            #[inline]
            unsafe fn endpoint(px: &Row, py: &Row, k: *const u8) -> V {
                unsafe { position(px, py, &[vzero(); Q], &[vzero(); Q], k) }
            }

            /// Running sums of a chunk, per 128-bit lane.
            struct Sums {
                b: [V; 8],
                f: [V; 3],
                px: Row,
                py: Row,
            }

            impl Sums {
                #[target_feature(enable = $features)]
                #[inline]
                fn zero() -> Self {
                    Sums { b: [vzero(); 8], f: [vzero(); 3], px: [vzero(); Q], py: [vzero(); Q] }
                }

                #[target_feature(enable = $features)]
                #[inline]
                unsafe fn load(w: &[u128; 24]) -> Self {
                    unsafe {
                        let prev = w.as_ptr().add(16) as *const u8;
                        let (px, py) = rows(prev);
                        Sums {
                            b: core::array::from_fn(|i| widen(ldu(&w[i]))),
                            f: core::array::from_fn(|i| widen(ldu(&w[8 + i]))),
                            px,
                            py,
                        }
                    }
                }

                /// Store folded sums and the previous block; writes every word
                /// the SIMD state layout uses (0..11 and 16..24), and zeroes
                /// the rest. `w` may point to uninitialized words, so it is
                /// only written through raw pointers.
                #[target_feature(enable = $features)]
                #[inline]
                unsafe fn store(&self, w: *mut u128) {
                    unsafe {
                        for i in 0..8 {
                            stu_raw(w.add(i), fold(self.b[i]));
                        }
                        for i in 0..3 {
                            stu_raw(w.add(8 + i), fold(self.f[i]));
                        }
                        for i in 11..16 {
                            w.add(i).write(0);
                        }
                        let prev = w.add(16) as *mut u8;
                        for q in 0..Q {
                            core::ptr::copy_nonoverlapping(&self.px[q] as *const V as *const u8, prev.add(W * q), W);
                            core::ptr::copy_nonoverlapping(&self.py[q] as *const V as *const u8, prev.add(64 + W * q), W);
                        }
                    }
                }

                /// Absorb whole groups `u0..u0 + n` at `d` with key rows at `k`.
                #[target_feature(enable = $features)]
                #[inline]
                unsafe fn groups(&mut self, mut d: *const u8, mut k: *const u8, u0: usize, n: usize) {
                    unsafe {
                        for u in u0..u0 + n {
                            let mut c = vzero();
                            for v in 0..8 {
                                let (x, y) = rows(d.add(BLOCK * v));
                                let s = position(&self.px, &self.py, &x, &y, k.add(BLOCK * v));
                                self.b[v] = vxor(self.b[v], s);
                                c = vxor(c, s);
                                self.px = x;
                                self.py = y;
                            }
                            for bit in 0..3 {
                                if u >> bit & 1 == 1 {
                                    self.f[bit] = vxor(self.f[bit], c);
                                }
                            }
                            d = d.add(1024);
                            k = k.add(1024);
                        }
                    }
                }
            }

            /// Absorb and close `count` whole chunks at `d`; `core` must be at a
            /// chunk boundary.
            #[target_feature(enable = $features)]
            pub unsafe fn chunks(core: &mut Core, key: &Key, d: *const u8, count: usize) {
                unsafe {
                    let table = key.rows();
                    let mut outer = ldu(&core.outer);
                    for i in 0..count {
                        let mut s = Sums::zero();
                        s.groups(d.add(CHUNK * i), table, 0, 8);
                        let e = fold(endpoint(&s.px, &s.py, table.add(BLOCK * CHUNK_BLOCKS)));
                        let b = s.b.map(|v| fold(v));
                        let f = s.f.map(|v| fold(v));
                        outer = outer_step(key, outer, &b, &f, e);
                    }
                    stu(&mut core.outer, outer);
                    core.closed |= count > 0;
                }
            }

            /// Absorb `n` whole groups at `d`, closing the chunk if it fills.
            #[target_feature(enable = $features)]
            pub unsafe fn groups(core: &mut Core, key: &Key, d: *const u8, n: usize) {
                unsafe {
                    let pos = core.pos;
                    let mut s = match core.words() {
                        Some(w) => Sums::load(w),
                        None => Sums::zero(),
                    };
                    s.groups(d, key.rows().add(BLOCK * pos), pos / GROUP_BLOCKS, n);
                    let pos = pos + GROUP_BLOCKS * n;
                    if pos == CHUNK_BLOCKS {
                        let e = fold(endpoint(&s.px, &s.py, key.rows().add(BLOCK * CHUNK_BLOCKS)));
                        let b = s.b.map(|v| fold(v));
                        let f = s.f.map(|v| fold(v));
                        let o = outer_step(key, ldu(&core.outer), &b, &f, e);
                        stu(&mut core.outer, o);
                        core.pos = 0;
                        core.closed = true;
                    } else {
                        s.store(core.words_ptr());
                        core.pos = pos;
                    }
                }
            }

            /// A message of `SHORT_MAX..=S2_MAX` bytes: the affine form.
            #[target_feature(enable = $features)]
            pub unsafe fn medium(key: &Key, msg: &[u8]) -> u128 {
                unsafe { super::medium(key, msg) }
            }

            /// The output after `core` and `pending` (at most 1024 bytes).
            #[target_feature(enable = $features)]
            pub unsafe fn finish(core: &Core, key: &Key, pending: &[u8], len: u64) -> u128 {
                unsafe {
                    let n = pending.len().div_ceil(BLOCK);
                    let full = pending.len() / BLOCK;
                    let mut pad = core::mem::MaybeUninit::<[u8; BLOCK]>::uninit();
                    let last = if full == n {
                        pending.as_ptr().add(BLOCK * n.saturating_sub(1))
                    } else {
                        pad.write($pad(&pending[BLOCK * full..])).as_ptr()
                    };
                    let d = pending.as_ptr();
                    let outer = core.closed.then(|| ldu(&core.outer));
                    let mut w = Wide64::new();
                    w.mul_len(key, len);
                    match core.words() {
                        None if n == 0 => {
                            return from_x(xxor(xxor(w.reduce(), outer.unwrap_or(xzero())), ldu(&key.s)));
                        },
                        None => small(key, d, last, n, outer, &mut w),
                        Some(words) => fin(key, words, core.pos, d, last, n, outer, &mut w),
                    }
                    from_x(xxor(w.reduce(), ldu(&key.s)))
                }
            }

            /// A final chunk of `n <= 8` blocks from a fresh chunk state:
            /// `h0 R + (sum_b x^b P_b) R2` from the position sums.
            #[target_feature(enable = $features)]
            #[inline]
            unsafe fn small(key: &Key, d: *const u8, last: *const u8, n: usize, outer: Option<X>, w: &mut Wide64) {
                unsafe {
                    let table = key.rows();
                    let mut sums = [xzero(); 9];
                    let mut px = [vzero(); Q];
                    let mut py = [vzero(); Q];
                    for j in 0..n {
                        let (x, y) = rows(if j + 1 < n { d.add(BLOCK * j) } else { last });
                        sums[j] = fold(position(&px, &py, &x, &y, table.add(BLOCK * j)));
                        px = x;
                        py = y;
                    }
                    sums[n] = fold(endpoint(&px, &py, table.add(BLOCK * n)));
                    let mut h0 = outer.unwrap_or(xzero());
                    for s in &sums[..=n] {
                        h0 = xxor(h0, *s);
                    }
                    w.mul_r(key, h0);
                    if n == 1 {
                        // One position: `P_0` alone.
                        w.mul_r2(key, sums[1]);
                        return;
                    }
                    // `sum_j j s_j = sum_b x^b P_b`, `P_b` the sums of the
                    // positions with bit `b` set: shifts, then one product.
                    // (Index loops: LLVM left the iterator adapters over
                    // vectors out of line here.)
                    let mut z = [xzero(); 6];
                    for j in 1..=n {
                        for b in 0..4 {
                            if j >> b & 1 == 1 {
                                z[b] = xxor(z[b], sums[j]);
                            }
                        }
                    }
                    w.mul_r2(key, shifted_sum(z));
                }
            }

            /// A final chunk continuing `pos` (a nonzero multiple of 8) blocks
            /// with `n <= 8` more.
            #[target_feature(enable = $features)]
            #[inline]
            #[allow(clippy::too_many_arguments)]
            unsafe fn fin(
                key: &Key,
                words: &[u128; 24],
                pos: usize,
                d: *const u8,
                last: *const u8,
                n: usize,
                outer: Option<X>,
                w: &mut Wide64,
            ) {
                unsafe {
                    let table = key.rows().add(BLOCK * pos);
                    let s = Sums::load(words);
                    let mut b = s.b.map(|v| fold(v));
                    let mut f = s.f.map(|v| fold(v));
                    let (mut px, mut py) = (s.px, s.py);
                    let mut cu = xzero();
                    for v in 0..n {
                        let (x, y) = rows(if v + 1 < n { d.add(BLOCK * v) } else { last });
                        let sv = fold(position(&px, &py, &x, &y, table.add(BLOCK * v)));
                        b[v] = xxor(b[v], sv);
                        cu = xxor(cu, sv);
                        px = x;
                        py = y;
                    }
                    let sv = fold(endpoint(&px, &py, table.add(BLOCK * n)));
                    let mut e = xzero();
                    let u = pos / GROUP_BLOCKS;
                    if n < GROUP_BLOCKS {
                        b[n] = xxor(b[n], sv);
                        cu = xxor(cu, sv);
                    } else if pos + n == CHUNK_BLOCKS {
                        e = sv;
                    } else {
                        b[0] = xxor(b[0], sv);
                        add_planes(&mut f, u + 1, sv);
                    }
                    add_planes(&mut f, u, cu);
                    // Planes of groups not reached are zero.
                    let (h0, z) = planes(&b, &f, e);
                    w.mul_r(key, xxor(outer.unwrap_or(xzero()), h0));
                    w.mul_r2(key, shifted_sum(z));
                }
            }

            /// The short path for messages of fewer than 32 bytes.
            #[target_feature(enable = $features)]
            pub unsafe fn short(key: &Key, x0: u128, x1: Option<u128>) -> u128 {
                unsafe { super::short(key, x0, x1) }
            }
        }
    };
}

backend!(
    sse, "sse2,ssse3,sse4.1,pclmulqdq", __m128i, 4,
    load = |p: *const u8| _mm_loadu_si128(p as *const __m128i),
    zero = || _mm_setzero_si128(),
    xor = |a, b| _mm_xor_si128(a, b),
    xor3 = |a, b, c| _mm_xor_si128(_mm_xor_si128(a, b), c),
    clmul_lo = |a, b| _mm_clmulepi64_si128::<0x00>(a, b),
    clmul_hi = |a, b| _mm_clmulepi64_si128::<0x11>(a, b),
    fold = |a| a,
    widen = |a| a,
    pad = padded_block,
);

backend!(
    avx2, "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2,vpclmulqdq", __m256i, 2,
    load = |p: *const u8| _mm256_loadu_si256(p as *const __m256i),
    zero = || _mm256_setzero_si256(),
    xor = |a, b| _mm256_xor_si256(a, b),
    xor3 = |a, b, c| _mm256_xor_si256(_mm256_xor_si256(a, b), c),
    clmul_lo = |a, b| _mm256_clmulepi64_epi128::<0x00>(a, b),
    clmul_hi = |a, b| _mm256_clmulepi64_epi128::<0x11>(a, b),
    fold = |a| _mm_xor_si128(_mm256_castsi256_si128(a), _mm256_extracti128_si256::<1>(a)),
    widen = |a| _mm256_zextsi128_si256(a),
    pad = padded_block32,
);

// Test-only: the AVX2 kernel with each 256-bit carryless multiply split into
// two 128-bit ones, to exercise the two-vectors-per-row structure on
// emulators that lack VPCLMULQDQ.
#[cfg(feature = "emulated-vpclmul")]
backend!(
    avx2emu, "sse2,ssse3,sse4.1,pclmulqdq,avx,avx2", __m256i, 2,
    load = |p: *const u8| _mm256_loadu_si256(p as *const __m256i),
    zero = || _mm256_setzero_si256(),
    xor = |a, b| _mm256_xor_si256(a, b),
    xor3 = |a, b, c| _mm256_xor_si256(_mm256_xor_si256(a, b), c),
    clmul_lo = |a: __m256i, b: __m256i| _mm256_set_m128i(
        _mm_clmulepi64_si128::<0x00>(_mm256_extracti128_si256::<1>(a), _mm256_extracti128_si256::<1>(b)),
        _mm_clmulepi64_si128::<0x00>(_mm256_castsi256_si128(a), _mm256_castsi256_si128(b)),
    ),
    clmul_hi = |a: __m256i, b: __m256i| _mm256_set_m128i(
        _mm_clmulepi64_si128::<0x11>(_mm256_extracti128_si256::<1>(a), _mm256_extracti128_si256::<1>(b)),
        _mm_clmulepi64_si128::<0x11>(_mm256_castsi256_si128(a), _mm256_castsi256_si128(b)),
    ),
    fold = |a| _mm_xor_si128(_mm256_castsi256_si128(a), _mm256_extracti128_si256::<1>(a)),
    widen = |a| _mm256_zextsi128_si256(a),
    pad = padded_block32,
);
