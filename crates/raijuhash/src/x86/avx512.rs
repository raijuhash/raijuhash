//! AVX-512 backend: VPCLMULQDQ on 512-bit vectors (with AVX-512 BW, VL and
//! VBMI2), tuned on AMD Zen 5.
//!
//! A 64-byte row is one vector and the two carryless multiplies of a
//! position give its eight lane products, summed per 128-bit lane. Sums stay
//! per lane until a chunk or message ends: folding the lanes is linear, so it
//! happens once. On Zen 5 `vpclmulqdq` is microcoded (four ops) and issues
//! every other cycle at any width, so a block costs at least four cycles;
//! the kernels keep other work off that port and out of the front end:
//!
//! - whole chunks go one at a time with one accumulator per block and
//!   software prefetch for inputs that outgrow L2 but not L3 (two chunks in
//!   step, sharing key loads, measured slower);
//! - the chunk fold `h1 = sum_k x^k z_k` uses shifts, and so does the
//!   reduction in the bulk kernel; paths limited by instruction count
//!   rather than by the multiplier reduce with two carryless multiplies;
//! - the three products of an output, `h0 R`, `h1 R2` and `L T`, are one
//!   packed vector multiply;
//! - a partial last block is read with masked loads, so it is never copied,
//!   and a streaming hash keeps only `h0` and `h1` between calls.

use core::arch::x86_64::*;

use crate::params::{BLOCK, CHUNK, CHUNK_BLOCKS, SHORT_MAX};
use crate::state::{Core, GROUP, GROUP_BLOCKS};
use crate::{Key, S2_MAX};

type Z = __m512i;
type X = __m128i;

// The helpers below are only called from functions with the features of
// this backend enabled, into which they are always inlined.

#[inline(always)]
unsafe fn ld(p: *const u8) -> Z {
    unsafe { _mm512_loadu_si512(p.cast()) }
}

/// The bytes of `p[..64]` selected by `m` (bit `i` for byte `i`), zero
/// elsewhere. Masked-off bytes are not accessed.
#[inline(always)]
unsafe fn ldm(p: *const u8, m: u64) -> Z {
    unsafe { _mm512_maskz_loadu_epi8(m, p.cast()) }
}

/// Mask of the first `n` bytes of a vector (all of them for `n >= 64`).
#[inline(always)]
unsafe fn first(n: usize) -> u64 {
    unsafe { _bzhi_u64(u64::MAX, n as u32) }
}

#[inline(always)]
unsafe fn zero() -> Z {
    unsafe { _mm512_setzero_si512() }
}

#[inline(always)]
unsafe fn xor(a: Z, b: Z) -> Z {
    unsafe { _mm512_xor_si512(a, b) }
}

#[inline(always)]
unsafe fn x3(a: Z, b: Z, c: Z) -> Z {
    unsafe { _mm512_ternarylogic_epi64::<0x96>(a, b, c) }
}

#[inline(always)]
unsafe fn x4(a: Z, b: Z, c: Z, d: Z) -> Z {
    unsafe { x3(a, b, xor(c, d)) }
}

#[inline(always)]
unsafe fn clo(a: Z, b: Z) -> Z {
    unsafe { _mm512_clmulepi64_epi128::<0x00>(a, b) }
}

#[inline(always)]
unsafe fn chi(a: Z, b: Z) -> Z {
    unsafe { _mm512_clmulepi64_epi128::<0x11>(a, b) }
}

#[inline(always)]
unsafe fn widen(a: X) -> Z {
    unsafe { _mm512_zextsi128_si512(a) }
}

#[inline(always)]
unsafe fn ldx(x: &u128) -> X {
    unsafe { _mm_loadu_si128((x as *const u128).cast()) }
}

#[inline(always)]
unsafe fn from_x(v: X) -> u128 {
    let mut r = 0u128;
    unsafe { _mm_storeu_si128((&mut r as *mut u128).cast(), v) };
    r
}

/// Each 128-bit lane shifted left by `K` bits, `0 < K < 64`.
#[inline(always)]
unsafe fn shl<const K: i32>(a: Z) -> Z {
    unsafe { _mm512_shldi_epi64::<K>(a, _mm512_bslli_epi128::<8>(a)) }
}

/// `o * (x^7 + x^2 + x + 1)` for `o` of at most 57 bits in the low word of
/// each lane.
#[inline(always)]
unsafe fn times_poly(o: Z) -> Z {
    unsafe { xor(x3(o, _mm512_slli_epi64::<1>(o), _mm512_slli_epi64::<2>(o)), _mm512_slli_epi64::<7>(o)) }
}

/// `sum_k x^k z[k]` reduced, per lane.
#[inline(always)]
unsafe fn shifted_sum(z: [Z; 6]) -> Z {
    unsafe {
        let low = xor(x3(z[0], shl::<1>(z[1]), shl::<2>(z[2])), x3(shl::<3>(z[3]), shl::<4>(z[4]), shl::<5>(z[5])));
        let over = xor(
            x3(_mm512_srli_epi64::<63>(z[1]), _mm512_srli_epi64::<62>(z[2]), _mm512_srli_epi64::<61>(z[3])),
            xor(_mm512_srli_epi64::<60>(z[4]), _mm512_srli_epi64::<59>(z[5])),
        );
        xor(low, times_poly(_mm512_bsrli_epi128::<8>(over)))
    }
}

/// The sum of the four lanes.
#[inline(always)]
unsafe fn fold(a: Z) -> X {
    unsafe {
        let h = _mm256_xor_si256(_mm512_castsi512_si256(a), _mm512_extracti64x4_epi64::<1>(a));
        _mm_xor_si128(_mm256_castsi256_si128(h), _mm256_extracti128_si256::<1>(h))
    }
}

/// The lane sums of `a` and `b` as `(A, A, B, B)`.
#[inline(always)]
unsafe fn fold2(a: Z, b: Z) -> Z {
    unsafe {
        let s = xor(_mm512_shuffle_i64x2::<0b01_00_01_00>(a, b), _mm512_shuffle_i64x2::<0b11_10_11_10>(a, b));
        xor(s, _mm512_shuffle_i64x2::<0b10_11_00_01>(s, s))
    }
}

/// `h0 R + h1 R2 + L T`, reduced, for lane sums `h0v` and `h1v`. The two sums
/// are folded into lanes 0 and 2, `L` goes into lane 1, and one multiply by
/// the packed keys `K = (R, T, R2, 0)` gives all three products. With
/// `K' = x^64 K` also prepared, `a K = (a.lo K.lo + a.hi K'.lo) + (a.lo K.hi
/// + a.hi K'.hi) x^64`, so only the top word of the second sum folds back.
/// `MUL` folds it with a carryless multiply rather than shifts.
#[inline(always)]
unsafe fn mix<const MUL: bool>(key: &Key, h0v: Z, h1v: Z, len: u64) -> X {
    unsafe {
        let w = fold2(h0v, h1v);
        let a = _mm512_mask_blend_epi64(0b0000_1100, w, _mm512_maskz_set1_epi64(0b0000_0100, len as i64));
        let p = key.pk.as_ptr() as *const u8;
        let (k, k64) = (ld(p), ld(p.add(64)));
        let lo = xor(clo(a, k), _mm512_clmulepi64_epi128::<0x01>(a, k64));
        let mid = xor(_mm512_clmulepi64_epi128::<0x10>(a, k), chi(a, k64));
        let v = xor(lo, _mm512_bslli_epi128::<8>(mid));
        let top = if MUL {
            _mm512_clmulepi64_epi128::<0x01>(mid, _mm512_set1_epi64(0x87))
        } else {
            // `o (x^7 + x^2 + x + 1)` for the 64-bit `o`, lane shifts.
            let o = _mm512_bsrli_epi128::<8>(mid);
            x4(o, shl::<1>(o), shl::<2>(o), shl::<7>(o))
        };
        fold(xor(v, top))
    }
}

/// Lane sums of the products of one position: previous rows `px`, `py`,
/// current rows `x`, `y`, key row at `k`.
#[inline(always)]
unsafe fn position(px: Z, py: Z, x: Z, y: Z, k: *const u8) -> Z {
    unsafe {
        let t = x3(px, x, ld(k));
        let u = x3(py, y, ld(k.add(64)));
        xor(clo(t, u), chi(t, u))
    }
}

/// `h0` and the six planes whose weighted sum is `h1` (see `neon::planes`).
#[inline(always)]
unsafe fn planes(b: &[Z; 8], f: &[Z; 3], e: Z) -> (Z, [Z; 6]) {
    unsafe {
        let h0 = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
        let z = [
            x3(e, xor(b[1], b[3]), xor(b[5], b[7])),
            x4(b[2], b[3], b[6], b[7]),
            x4(b[4], b[5], b[6], b[7]),
            f[0],
            f[1],
            f[2],
        ];
        (h0, z)
    }
}

/// Add `x` to the planes `F[b]` for the bits `b` of `u`, without branches.
#[inline(always)]
unsafe fn add_planes(f: &mut [Z; 3], u: usize, x: Z) {
    for (b, fb) in f.iter_mut().enumerate() {
        let m = 0u8.wrapping_sub((u >> b & 1) as u8);
        *fb = unsafe { _mm512_mask_xor_epi64(*fb, m, *fb, x) };
    }
}

/// `h0 R + h1 R2 + L T` of a chunk whose sums are `b`, `f` and end position
/// `e`, plus lane sums `h0s`, `h1s` added to `h0` and `h1` (earlier
/// positions, and `outer` in lane 0 of `h0s`).
#[inline(always)]
unsafe fn close<const MUL: bool>(key: &Key, b: &[Z; 8], f: &[Z; 3], e: Z, h0s: Z, h1s: Z, len: u64) -> X {
    unsafe {
        let (h0, z) = planes(b, f, e);
        mix::<MUL>(key, xor(h0, h0s), xor(shifted_sum(z), h1s), len)
    }
}

/// Inputs of `PREFETCH_CHUNKS..PREFETCH_CHUNKS_END` chunks (512 KiB up to
/// 32 MiB) no longer fit in L2 with the key but can still come from L3,
/// where prefetching `PREFETCH` bytes ahead into L1 helps. Streaming from
/// memory it costs about 15%: the hardware prefetcher does better alone.
/// (Zen 5: prefetching into L2, or farther ahead, measured slower.)
const PREFETCH_CHUNKS: usize = 64;
const PREFETCH_CHUNKS_END: usize = 4096;
const PREFETCH: usize = 4096;

/// Absorb and close `count` whole chunks at `d`; `core` must be at a chunk
/// boundary.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn chunks(core: &mut Core, key: &Key, d: *const u8, count: usize) {
    unsafe {
        let table = key.rows();
        let mut outer = widen(ldx(&core.outer));
        if !(PREFETCH_CHUNKS..PREFETCH_CHUNKS_END).contains(&count) {
            for i in 0..count {
                outer = bulk::<0>(key, table, d.add(CHUNK * i), outer);
            }
        } else {
            for i in 0..count {
                outer = bulk::<PREFETCH>(key, table, d.add(CHUNK * i), outer);
            }
        }
        _mm_storeu_si128((&mut core.outer as *mut u128).cast(), _mm512_castsi512_si128(outer));
        core.closed |= count > 0;
    }
}

/// One whole chunk at `d`, prefetching `PF` bytes ahead (0: none). Two
/// chunks in step, sharing key loads, measured slower on Zen 5.
///
/// `vpclmulqdq` is microcoded on Zen 5 and competes with the rest of the
/// loop for the front end, so the loop keeps one accumulator per block: the
/// group sums `c_u` that the planes `F` need are differences of the running
/// totals `T_u = sum_v B[v]` after each group, taken once per group.
#[inline(always)]
unsafe fn bulk<const PF: usize>(key: &Key, table: *const u8, d: *const u8, outer: Z) -> Z {
    unsafe {
        let mut b = [zero(); 8];
        let (mut px, mut py) = (zero(), zero());
        let mut ts = [zero(); 8];
        for (u, tu) in ts.iter_mut().enumerate() {
            for (v, bv) in b.iter_mut().enumerate() {
                let j = GROUP_BLOCKS * u + v;
                let (k, p) = (table.add(BLOCK * j), d.add(BLOCK * j));
                if PF > 0 {
                    // Past the end of the input on the last chunks: a hint
                    // only, so the address is formed with `wrapping_add`
                    // (`add` must stay within the allocation).
                    _mm_prefetch::<_MM_HINT_T0>(p.wrapping_add(PF) as *const i8);
                    _mm_prefetch::<_MM_HINT_T0>(p.wrapping_add(PF + 64) as *const i8);
                }
                let (x, y) = (ld(p), ld(p.add(64)));
                let t = x3(px, x, ld(k));
                let w = x3(py, y, ld(k.add(64)));
                let (lo, hi) = (clo(t, w), chi(t, w));
                *bv = x3(*bv, lo, hi);
                px = x;
                py = y;
            }
            *tu = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
        }
        let e = position(px, py, zero(), zero(), table.add(BLOCK * CHUNK_BLOCKS));
        // With `c_u = T_u + T_{u-1}` (`T_{-1} = 0`): `F[0] = c_1 + c_3 + c_5 +
        // c_7` is the sum of all `T_u`, `F[1]` that of the odd ones, and
        // `F[2] = T_3 + T_7`.
        let t = &ts;
        let odd = x4(t[1], t[3], t[5], t[7]);
        let f = [xor(odd, x4(t[0], t[2], t[4], t[6])), odd, xor(t[3], t[7])];
        widen(close::<false>(key, &b, &f, e, outer, zero(), 0))
    }
}

/// Sums of a chunk in progress, per lane: `b` and `f` for the positions of
/// the current call, and `h0`, `h1` for those of earlier calls.
///
/// Between calls the chunk is kept as its sums `h0` and `h1` alone, per
/// lane (words 0..4 and 4..8 of `Core`), and the previous block (words
/// 16..24): `h1` is linear in the position sums, so the planes of later
/// positions just add to it. Storing two values instead of the eleven of
/// `b` and `f` makes both halves of a streaming hash cheaper, and keeping
/// their lanes skips a fold per call (the close folds them anyway). The
/// other words are not used, and may be uninitialized.
#[derive(Clone, Copy)]
struct Sums {
    b: [Z; 8],
    f: [Z; 3],
    h0: Z,
    h1: Z,
    px: Z,
    py: Z,
}

impl Sums {
    #[inline(always)]
    unsafe fn zero() -> Sums {
        unsafe { Sums { b: [zero(); 8], f: [zero(); 3], h0: zero(), h1: zero(), px: zero(), py: zero() } }
    }

    /// From the state words at `w` (`h0`, `h1` in words 0..8, the previous
    /// block in 16..24).
    #[inline(always)]
    unsafe fn load(w: *const u128) -> Sums {
        unsafe {
            let prev = w.add(16) as *const u8;
            Sums {
                b: [zero(); 8],
                f: [zero(); 3],
                h0: ld(w.cast()),
                h1: ld(w.add(4).cast()),
                px: ld(prev),
                py: ld(prev.add(64)),
            }
        }
    }

    /// Store `h0`, `h1` (with this call's positions) and the previous block.
    /// `w` may point to uninitialized words, so it is only written through
    /// raw pointers.
    #[inline(always)]
    unsafe fn store(&self, w: *mut u128) {
        unsafe {
            let (h0, z) = planes(&self.b, &self.f, zero());
            _mm512_storeu_si512(w.cast(), xor(h0, self.h0));
            _mm512_storeu_si512(w.add(4).cast(), xor(shifted_sum(z), self.h1));
            let prev = w.add(16) as *mut u8;
            _mm512_storeu_si512(prev.cast(), self.px);
            _mm512_storeu_si512(prev.add(64).cast(), self.py);
        }
    }

    /// Absorb whole groups `u0..u0 + n` at `d`, keys from `table` (row 0).
    /// As in `bulk`, a group's sum is the change of the running total of `b`.
    #[inline(always)]
    unsafe fn groups(&mut self, table: *const u8, mut d: *const u8, u0: usize, n: usize) {
        unsafe {
            let total = |b: &[Z; 8]| xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
            let mut before = total(&self.b);
            for u in u0..u0 + n {
                for v in 0..8 {
                    let k = table.add(BLOCK * (GROUP_BLOCKS * u + v));
                    let (x, y) = (ld(d), ld(d.add(64)));
                    let t = x3(self.px, x, ld(k));
                    let w = x3(self.py, y, ld(k.add(64)));
                    self.b[v] = x3(self.b[v], clo(t, w), chi(t, w));
                    self.px = x;
                    self.py = y;
                    d = d.add(BLOCK);
                }
                let after = total(&self.b);
                add_planes(&mut self.f, u, xor(after, before));
                before = after;
            }
        }
    }
}

/// Absorb whole groups (`data.len()` a multiple of `GROUP`), closing chunks
/// as they fill: `Key::absorb` for this backend, kept out of the generic
/// function so that its frame stays small.
///
/// # Safety
/// The CPU must support this backend, and the key's table must be complete.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
#[inline(never)]
pub unsafe fn absorb(core: &mut Core, key: &Key, mut data: &[u8]) {
    unsafe {
        while !data.is_empty() {
            let n = if core.pos == 0 && data.len() >= CHUNK {
                let n = data.len() / CHUNK;
                chunks(core, key, data.as_ptr(), n);
                n * CHUNK
            } else {
                let n = (data.len() / GROUP).min((CHUNK_BLOCKS - core.pos) / GROUP_BLOCKS);
                groups(core, key, data.as_ptr(), n);
                n * GROUP
            };
            data = &data[n..];
        }
    }
}

/// Absorb `n` whole groups at `d`, closing the chunk if it fills.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn groups(core: &mut Core, key: &Key, d: *const u8, n: usize) {
    unsafe {
        let pos = core.pos;
        let mut s = if pos > 0 { Sums::load(core.words_ptr()) } else { Sums::zero() };
        s.groups(key.rows(), d, pos / GROUP_BLOCKS, n);
        let pos = pos + GROUP_BLOCKS * n;
        if pos == CHUNK_BLOCKS {
            let e = position(s.px, s.py, zero(), zero(), key.rows().add(BLOCK * CHUNK_BLOCKS));
            let o = close::<true>(key, &s.b, &s.f, e, xor(widen(ldx(&core.outer)), s.h0), s.h1, 0);
            _mm_storeu_si128((&mut core.outer as *mut u128).cast(), o);
            core.pos = 0;
            core.closed = true;
        } else {
            s.store(core.words_ptr());
            core.pos = pos;
        }
    }
}

/// The last `NB <= 8` blocks of a message after `pos` (a multiple of 8)
/// blocks of its last chunk summarized by `s`: positions `pos + v`, the end
/// position, and the output without `S`. The last block has `r` bytes
/// (`1..=128`) and is read with masked loads.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn fin<const NB: usize>(key: &Key, mut s: Sums, pos: usize, d: *const u8, r: usize, outer: Z, len: u64) -> X {
    unsafe {
        let table = key.rows().add(BLOCK * pos);
        let mut cu = zero();
        for v in 0..NB {
            let p = d.add(BLOCK * v);
            let (x, y) = if v + 1 < NB {
                (ld(p), ld(p.add(64)))
            } else {
                // With `r <= 64` the second half lies past the input: it is
                // read with an empty mask (no access), at a `wrapping_add`
                // address.
                (ldm(p, first(r)), ldm(p.wrapping_add(64), first(r.saturating_sub(64))))
            };
            let sv = position(s.px, s.py, x, y, table.add(BLOCK * v));
            s.b[v] = xor(s.b[v], sv);
            cu = xor(cu, sv);
            s.px = x;
            s.py = y;
        }
        let sv = position(s.px, s.py, zero(), zero(), table.add(BLOCK * NB));
        let u = pos / GROUP_BLOCKS;
        let mut e = zero();
        if NB < GROUP_BLOCKS {
            s.b[NB] = xor(s.b[NB], sv);
            cu = xor(cu, sv);
        } else if pos + NB == CHUNK_BLOCKS {
            e = sv;
        } else {
            s.b[0] = xor(s.b[0], sv);
            add_planes(&mut s.f, u + 1, sv);
        }
        add_planes(&mut s.f, u, cu);
        close::<true>(key, &s.b, &s.f, e, xor(outer, s.h0), s.h1, len)
    }
}

/// The last `NB` (`1..=8`) blocks of a message from a fresh chunk, with
/// `outer` from whole chunks before: like `fin` with all sums zero, which
/// the compiler folds away. The last block has `r` bytes.
#[inline(always)]
unsafe fn small<const NB: usize>(key: &Key, d: *const u8, r: usize, outer: Z, len: u64) -> X {
    unsafe {
        let table = key.rows();
        let mut p = [zero(); 9];
        let (mut px, mut py) = (zero(), zero());
        for (j, pj) in p.iter_mut().enumerate().take(NB) {
            let q = d.add(BLOCK * j);
            let (x, y) = if j + 1 < NB {
                (ld(q), ld(q.add(64)))
            } else {
                // As in `fin`: an empty mask past the input.
                (ldm(q, first(r)), ldm(q.wrapping_add(64), first(r.saturating_sub(64))))
            };
            *pj = position(px, py, x, y, table.add(BLOCK * j));
            px = x;
            py = y;
        }
        p[NB] = position(px, py, zero(), zero(), table.add(BLOCK * NB));
        // Position `j <= 8` has column `(1, j)`: `h0` sums all, plane `k`
        // those with bit `k` of `j`.
        let plane = |k: usize| (0..=NB).filter(|j| j >> k & 1 == 1).fold(zero(), |a, j| xor(a, p[j]));
        let h0 = (0..=NB).fold(outer, |a, j| xor(a, p[j]));
        let h1 = shifted_sum([plane(0), plane(1), plane(2), plane(3), zero(), zero()]);
        mix::<true>(key, h0, h1, len)
    }
}

/// `small` for a runtime number of blocks `n` (`1..=8`).
#[inline(always)]
unsafe fn small_n(key: &Key, d: *const u8, n: usize, r: usize, outer: Z, len: u64) -> X {
    unsafe {
        match n {
            1 => small::<1>(key, d, r, outer, len),
            2 => small::<2>(key, d, r, outer, len),
            3 => small::<3>(key, d, r, outer, len),
            4 => small::<4>(key, d, r, outer, len),
            5 => small::<5>(key, d, r, outer, len),
            6 => small::<6>(key, d, r, outer, len),
            7 => small::<7>(key, d, r, outer, len),
            _ => small::<8>(key, d, r, outer, len),
        }
    }
}

/// `fin` for a runtime number of blocks `n <= 8`.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn fin_n(key: &Key, s: Sums, pos: usize, d: *const u8, n: usize, r: usize, outer: Z, len: u64) -> X {
    unsafe {
        match n {
            0 => fin::<0>(key, s, pos, d, r, outer, len),
            1 => fin::<1>(key, s, pos, d, r, outer, len),
            2 => fin::<2>(key, s, pos, d, r, outer, len),
            3 => fin::<3>(key, s, pos, d, r, outer, len),
            4 => fin::<4>(key, s, pos, d, r, outer, len),
            5 => fin::<5>(key, s, pos, d, r, outer, len),
            6 => fin::<6>(key, s, pos, d, r, outer, len),
            7 => fin::<7>(key, s, pos, d, r, outer, len),
            _ => fin::<8>(key, s, pos, d, r, outer, len),
        }
    }
}

/// `L T + S` alone: a message ending exactly at a chunk boundary.
#[inline(always)]
unsafe fn length_only(key: &Key, outer: X, len: u64) -> u128 {
    unsafe {
        let mut w = super::Wide64::new();
        w.mul_len(key, len);
        from_x(_mm_xor_si128(_mm_xor_si128(w.reduce(), outer), ldx(&key.s)))
    }
}

/// The output after `core` and `pending` (at most 1024 bytes).
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn finish(core: &Core, key: &Key, pending: &[u8], len: u64) -> u128 {
    unsafe {
        let n = pending.len().div_ceil(BLOCK);
        let outer = if core.closed { ldx(&core.outer) } else { _mm_setzero_si128() };
        // Bytes in the last block (0 with nothing pending, which only
        // `fin::<0>` or `length_only` see).
        let r = pending.len() - BLOCK * n.saturating_sub(1);
        let h = if core.pos > 0 {
            fin_n(key, Sums::load(core.words_raw()), core.pos, pending.as_ptr(), n, r, widen(outer), len)
        } else if n == 0 {
            return length_only(key, outer, len);
        } else {
            small_n(key, pending.as_ptr(), n, r, widen(outer), len)
        };
        from_x(_mm_xor_si128(h, ldx(&key.s)))
    }
}

/// The output of a message whose final partial chunk is `rest`
/// (`0 < rest.len() < CHUNK`), after whole chunks summarized by `outer` (if
/// any), with the chunk state in registers throughout.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn tail(key: &Key, rest: &[u8], outer: Option<u128>, len: u64) -> u128 {
    unsafe {
        debug_assert!(!rest.is_empty() && rest.len() < CHUNK);
        let nb = rest.len().div_ceil(BLOCK);
        let pos = (nb - 1) / GROUP_BLOCKS * GROUP_BLOCKS;
        let n = nb - pos;
        let r = rest.len() - BLOCK * (nb - 1);
        let outer = widen(outer.map_or(_mm_setzero_si128(), |o| ldx(&o)));
        let h = if pos == 0 {
            small_n(key, rest.as_ptr(), n, r, outer, len)
        } else {
            let mut s = Sums::zero();
            s.groups(key.rows(), rest.as_ptr(), 0, pos / GROUP_BLOCKS);
            fin_n(key, s, pos, rest.as_ptr().add(BLOCK * pos), n, r, outer, len)
        };
        from_x(_mm_xor_si128(h, ldx(&key.s)))
    }
}

/// A whole message of `S2_MAX + 1..=GROUP` bytes (one fresh group): `tail`
/// without its general-length cases, which LLVM then specializes better.
///
/// # Safety
/// The CPU must support this backend and `msg.len()` must be in range.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn group(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let len = msg.len();
        debug_assert!(len > S2_MAX && len <= GROUP);
        let n = len.div_ceil(BLOCK);
        let h = small_n(key, msg.as_ptr(), n, len - BLOCK * (n - 1), zero(), len as u64);
        from_x(_mm_xor_si128(h, ldx(&key.s)))
    }
}

/// A message of `SHORT_MAX..=S2_MAX` bytes through the affine form of
/// `S2Coeffs`: one masked load, four vector multiplies and a fold.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn medium(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let len = msg.len();
        debug_assert!((SHORT_MAX..=S2_MAX).contains(&len));
        // The X row: the message, zero-padded to 64 bytes.
        let x = ldm(msg.as_ptr(), first(len));
        let k = &key.s2;
        let (ka, kb) = (ld(k.ka.as_ptr() as *const u8), ld(k.kb.as_ptr() as *const u8));
        // Per lane, the products with the X word pair: `lo` at x^0, `mid`
        // at x^64 (see `S2Coeffs`).
        let lo = xor(clo(x, ka), chi(x, kb));
        let mid = xor(chi(x, ka), clo(x, kb));
        // `lo + mid x^64`: the part of `mid` above x^128 is at most 64 bits,
        // so one multiplication by x^128 = x^7 + x^2 + x + 1 absorbs it.
        let hi = _mm512_bsrli_epi128::<8>(mid);
        let v = x3(lo, _mm512_bslli_epi128::<8>(mid), hi);
        let v = x3(v, shl::<1>(hi), shl::<2>(hi));
        let h = fold(xor(v, shl::<7>(hi)));
        // L T, independent of the message: `L T.lo + L T.hi x^64`, both
        // products from one 256-bit multiply with `(T, T swapped)` (the
        // layout of `FieldKey`); `L < 2^7`, so the top word of `L T.hi`
        // has at most seven bits and folds back with shifts.
        let tq = _mm256_loadu_si256((&key.t as *const crate::FieldKey).cast());
        let p = _mm256_clmulepi64_epi128::<0x00>(_mm256_set1_epi64x(len as i64), tq);
        let (lo, mid) = (_mm256_castsi256_si128(p), _mm256_extracti128_si256::<1>(p));
        let o = _mm_bsrli_si128::<8>(mid);
        let top = _mm_ternarylogic_epi64::<0x96>(o, _mm_slli_epi64::<1>(o), _mm_slli_epi64::<2>(o));
        let lt = _mm_ternarylogic_epi64::<0x96>(lo, _mm_bslli_si128::<8>(mid), _mm_xor_si128(top, _mm_slli_epi64::<7>(o)));
        from_x(_mm_xor_si128(h, _mm_xor_si128(lt, ldx(&k.c))))
    }
}

/// The `S2Coeffs` of a key (see `neon::s2_prepare`) with 512-bit products:
/// the eight `E[l] = Y0[l] R + Y1[l] (R + R2)` come out of eight vector
/// multiplies, even words in one vector and odd words in another.
///
/// # Safety
/// The CPU must support this backend; `table` must be readable for two rows.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub(crate) unsafe fn s2_prepare(table: *const u8, r: u128, r2: u128, s: u128) -> crate::S2Coeffs {
    unsafe {
        let (x0, y0, x1, y1) = (ld(table), ld(table.add(64)), ld(table.add(BLOCK)), ld(table.add(BLOCK + 64)));
        let (w0, w1) = (_mm512_broadcast_i32x4(ldx(&r)), _mm512_broadcast_i32x4(ldx(&(r ^ r2))));
        // Word 0 (even) or 1 (odd) of each lane of `y` times both halves of
        // `w`: the low and middle parts of a 64 x 128-bit product.
        macro_rules! products {
            ($ylo:literal, $yhi:literal) => {{
                let lo = xor(
                    _mm512_clmulepi64_epi128::<$ylo>(y0, w0),
                    _mm512_clmulepi64_epi128::<$ylo>(y1, w1),
                );
                let mid = xor(
                    _mm512_clmulepi64_epi128::<$yhi>(y0, w0),
                    _mm512_clmulepi64_epi128::<$yhi>(y1, w1),
                );
                // `lo + mid x^64`, reduced: the top word of `mid` goes
                // around through x^128 = x^7 + x^2 + x + 1.
                let hi = _mm512_bsrli_epi128::<8>(mid);
                xor(x3(lo, _mm512_bslli_epi128::<8>(mid), hi), x3(shl::<1>(hi), shl::<2>(hi), shl::<7>(hi)))
            }};
        }
        let even = products!(0x00, 0x10);
        let odd = products!(0x01, 0x11);
        let (mut ka, mut kb) = ([0u128; 4], [0u128; 4]);
        // `ka[p] = (E[2p].lo, E[2p+1].hi)`, `kb[p] = (E[2p].hi, E[2p+1].lo)`.
        _mm512_storeu_si512(ka.as_mut_ptr().cast(), _mm512_mask_blend_epi64(0xaa, even, odd));
        _mm512_storeu_si512(kb.as_mut_ptr().cast(), _mm512_alignr_epi8::<8>(odd, even));
        // `C = S + n0 R + n1 (R + R2)` with the position sums of the rows.
        let n0 = fold(xor(clo(x0, y0), chi(x0, y0)));
        let n1 = fold(xor(clo(x1, y1), chi(x1, y1)));
        let mut w = super::Wide::new();
        w.mul(n0, &crate::FieldKey::new(r));
        w.mul(n1, &crate::FieldKey::new(r ^ r2));
        crate::S2Coeffs { ka, kb, c: from_x(w.reduce()) ^ s }
    }
}

/// Copy `src` (at most 1024 bytes) to the start of a hasher's buffer
/// `dst` in whole 64-byte stores, the last one zero-padded (it may write up
/// to 63 bytes past `src.len()`). The finishing kernels read the buffer with
/// 64-byte loads at the same offsets, which then forward from these stores;
/// a byte-exact copy made of narrower, overlapping stores would stall them.
///
/// # Safety
/// `dst` must be valid for writes of `src.len()` rounded up to 64 bytes.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
#[inline]
pub unsafe fn copy_group(dst: *mut u8, src: &[u8]) {
    unsafe {
        let (n, s) = (src.len(), src.as_ptr());
        let whole = n / 64;
        for i in 0..whole {
            _mm512_storeu_si512(dst.add(64 * i).cast(), ld(s.add(64 * i)));
        }
        if n % 64 != 0 {
            _mm512_storeu_si512(dst.add(64 * whole).cast(), ldm(s.add(64 * whole), first(n % 64)));
        }
    }
}

/// The short path for messages of fewer than 32 bytes.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn short(key: &Key, x0: u128, x1: Option<u128>) -> u128 {
    unsafe { super::short(key, x0, x1) }
}

/// A whole message of fewer than `SHORT_MAX` bytes: `x0 A + x1 B + S` as in
/// `super::short`, with `[x0 | x1]` one 256-bit masked load, so that both
/// terms share four multiplies and the fold, without length branches.
///
/// # Safety
/// The CPU must support this backend and `msg.len() < SHORT_MAX`.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn short_msg(key: &Key, msg: &[u8]) -> u128 {
    unsafe {
        let len = msg.len();
        debug_assert!(len < SHORT_MAX);
        let x = _mm256_maskz_loadu_epi8(_bzhi_u32(u32::MAX, len as u32), msg.as_ptr().cast());
        // The length byte: byte 15 of the last element, which is zero here.
        let x = _mm256_mask_set1_epi8(x, 1 << (15 + (len & 16)), len as i8);
        let ks = key.sk.as_ptr().cast::<__m256i>();
        let (k, k64) = (_mm256_loadu_si256(ks), _mm256_loadu_si256(ks.add(1)));
        let lo = _mm256_xor_si256(_mm256_clmulepi64_epi128::<0x00>(x, k), _mm256_clmulepi64_epi128::<0x01>(x, k64));
        let mid = _mm256_xor_si256(_mm256_clmulepi64_epi128::<0x10>(x, k), _mm256_clmulepi64_epi128::<0x11>(x, k64));
        let fold = _mm256_clmulepi64_epi128::<0x01>(mid, _mm256_set1_epi64x(0x87));
        let r = _mm256_ternarylogic_epi64::<0x96>(lo, _mm256_bslli_epi128::<8>(mid), fold);
        let r = _mm_xor_si128(_mm256_castsi256_si128(r), _mm256_extracti128_si256::<1>(r));
        from_x(_mm_xor_si128(r, ldx(&key.s)))
    }
}
