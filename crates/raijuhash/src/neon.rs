//! AArch64 backend: PMULL for carryless products, optionally EOR3.
//!
//! Every block contributes the XOR `S` of its eight lane products. Position
//! `j = 8u + v` of a chunk adds `S` to `B[v]` and to `C[u]`; since the
//! combination column of position `j` is `(1, j)` with `j = x^3 u + v` as a
//! polynomial, the chunk pair follows from these sixteen sums. Whole groups
//! of eight blocks run in the asm kernels of `neon_asm`; the final partial
//! group and the output use intrinsics with a single field reduction.

use core::arch::aarch64::*;

use crate::params::{BLOCK, CHUNK, CHUNK_BLOCKS, SHORT_MAX};
use crate::state::{Core, GROUP_BLOCKS, padded_block};
use crate::{FieldKey, Key, S2_MAX, S2Coeffs};

type V = uint64x2_t;

/// The block before a chunk's first: the two-chunk kernels read it as the
/// previous block of position 0.
static ZERO_BLOCK: [u8; BLOCK] = [0; BLOCK];

/// Kernel choice by the number of chunks hashed in one call (measured on
/// Apple M1). Up to 128 KiB the data stays in L1, where the one-lane-pair
/// two-chunk kernel is fastest; above, loads come from L2 and the kernel
/// spreading loads over four L1 banks wins; from 10 MiB the data streams
/// from memory and that kernel also prefetches ahead into L2.
const L1_CHUNKS: usize = 16;
const MEMORY_CHUNKS: usize = 1280;

// The assembly reads R, swapped R, R2, swapped R2 as four consecutive u128s.
const _: () = {
    assert!(core::mem::size_of::<FieldKey>() == 32);
    assert!(core::mem::offset_of!(Key, r2) == core::mem::offset_of!(Key, r) + 32);
};

/// Pointer to both outer keys, derived from the whole `Key` so it covers
/// both fields without first borrowing just `r`.
#[inline(always)]
fn outer_keys_ptr(key: &Key) -> *const u128 {
    let base = (key as *const Key).cast::<u8>();
    // SAFETY: `r` is within `key`; the assertions above check that the
    // assembly's four-word read also covers the adjacent, initialized `r2`.
    unsafe { base.add(core::mem::offset_of!(Key, r)).cast() }
}

#[inline(always)]
unsafe fn ld(p: *const u8) -> V {
    unsafe { vreinterpretq_u64_u8(vld1q_u8(p)) }
}

#[inline(always)]
fn zero() -> V {
    unsafe { vdupq_n_u64(0) }
}

#[inline(always)]
fn xor(a: V, b: V) -> V {
    unsafe { veorq_u64(a, b) }
}

#[inline(always)]
fn clmul_lo(a: V, b: V) -> V {
    unsafe { vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64(a, 0), vgetq_lane_u64(b, 0))) }
}

#[inline(always)]
fn clmul_hi(a: V, b: V) -> V {
    unsafe {
        vreinterpretq_u64_p128(vmull_high_p64(vreinterpretq_p64_u64(a), vreinterpretq_p64_u64(b)))
    }
}

/// Load a `u128` stored in memory straight into a vector register.
#[inline(always)]
fn ldu(x: &u128) -> V {
    unsafe { vld1q_u64(x as *const u128 as *const u64) }
}

#[inline(always)]
fn stu(x: &mut u128, v: V) {
    unsafe { vst1q_u64(x as *mut u128 as *mut u64, v) }
}

#[inline(always)]
fn from_v(v: V) -> u128 {
    unsafe { vgetq_lane_u64(v, 0) as u128 | (vgetq_lane_u64(v, 1) as u128) << 64 }
}

#[inline(always)]
fn len_v(len: u64) -> V {
    unsafe { vcombine_u64(vcreate_u64(len), vcreate_u64(0)) }
}

/// Three-way XOR; `veor3q` where the target has FEAT_SHA3.
#[inline(always)]
fn xor3_any(a: V, b: V, c: V) -> V {
    #[cfg(target_feature = "sha3")]
    return unsafe { veor3q_u64(a, b, c) };
    #[cfg(not(target_feature = "sha3"))]
    xor(xor(a, b), c)
}

/// Unreduced 256-bit sum of products: `lo + mid x^64 + hi x^128`.
struct Wide {
    lo: V,
    mid: V,
    hi: V,
}

impl Wide {
    #[inline(always)]
    fn new() -> Self {
        Wide { lo: zero(), mid: zero(), hi: zero() }
    }

    /// Accumulate `a * k` with a schoolbook product.
    #[inline(always)]
    fn mul(&mut self, a: V, k: &FieldKey) {
        let (kv, ks) = (ldu(&k.k), ldu(&k.swapped));
        self.lo = xor(self.lo, clmul_lo(a, kv));
        self.hi = xor(self.hi, clmul_hi(a, kv));
        self.mid = xor(self.mid, xor(clmul_lo(a, ks), clmul_hi(a, ks)));
    }

    /// Accumulate `a * k` for a 64-bit `a` held in lane 0.
    #[inline(always)]
    fn mul64(&mut self, a: V, k: &FieldKey) {
        let (kv, ks) = (ldu(&k.k), ldu(&k.swapped));
        self.lo = xor(self.lo, clmul_lo(a, kv));
        self.mid = xor(self.mid, clmul_lo(a, ks));
    }

    /// Reduce modulo x^128 + x^7 + x^2 + x + 1.
    #[inline(always)]
    fn reduce(self) -> V {
        unsafe {
            let z = zero();
            let poly = vdupq_n_u64(0x87);
            // Word 3 is `hi.hi` alone (`mid` only reaches words 1..2), so its
            // fold into words 1..2 starts without waiting for `mid`.
            let t = clmul_hi(self.hi, poly);
            // Word 2 after both additions, then its fold into words 0..1.
            let w2 = xor3_any(self.hi, vextq_u64(self.mid, z, 1), vextq_u64(t, z, 1));
            let lo = xor3_any(self.lo, vextq_u64(z, self.mid, 1), vextq_u64(z, t, 1));
            xor(lo, clmul_lo(w2, poly))
        }
    }
}

/// `sum_k x^k * z_k` for the six small shifts used by the chunk fold.
#[inline(always)]
fn shifted_sum(z: [V; 6]) -> V {
    unsafe {
        let sl = xor(
            xor(xor(z[0], vshlq_n_u64::<1>(z[1])), xor(vshlq_n_u64::<2>(z[2]), vshlq_n_u64::<3>(z[3]))),
            xor(vshlq_n_u64::<4>(z[4]), vshlq_n_u64::<5>(z[5])),
        );
        let sr = xor(
            xor(vshrq_n_u64::<63>(z[1]), vshrq_n_u64::<62>(z[2])),
            xor(vshrq_n_u64::<61>(z[3]), xor(vshrq_n_u64::<60>(z[4]), vshrq_n_u64::<59>(z[5]))),
        );
        let zv = zero();
        // Bits carried out of the low word move up; bits carried out of the
        // high word sit above x^128 and are reduced by x^128 = 0x87.
        let low = xor(sl, vextq_u64(zv, sr, 1));
        let over = vextq_u64(sr, zv, 1);
        xor(low, clmul_lo(over, vdupq_n_u64(0x87)))
    }
}

/// XOR of four vectors.
#[inline(always)]
fn x4(a: V, b: V, c: V, d: V) -> V {
    xor(xor(a, b), xor(c, d))
}

/// `h0` and the six plane sums whose weighted total
/// `z0 + x z1 + ... + x^5 z5` is `h1`. Position `j = 8u + v` has column
/// `(1, j)`, so bit `b < 3` of `j` comes from `v` (planes of `B`) and bit
/// `3 + b` from `u` (the planes `F[b]` kept by the kernels); `e` is the end
/// position of a full chunk, column `(0, 1)`.
#[inline(always)]
fn planes(b: &[V; 8], f: &[V; 3], e: V) -> (V, [V; 6]) {
    let h0 = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
    let z = [
        xor(e, x4(b[1], b[3], b[5], b[7])),
        x4(b[2], b[3], b[6], b[7]),
        x4(b[4], b[5], b[6], b[7]),
        f[0],
        f[1],
        f[2],
    ];
    (h0, z)
}

/// `(outer + h0) R + h1 R2` for bulk chunks, where throughput matters more
/// than latency: `h1` comes from shifts, then two products.
#[inline(always)]
fn outer_step(key: &Key, outer: V, b: &[V; 8], f: &[V; 3], e: V) -> V {
    let (h0, z) = planes(b, f, e);
    let mut w = Wide::new();
    w.mul(xor(outer, h0), &key.r);
    w.mul(shifted_sum(z), &key.r2);
    w.reduce()
}

/// Byte indices 0..32, for moving a message tail with `TBL`.
static IOTA: [u8; 32] = {
    let mut a = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        a[i] = i as u8;
        i += 1;
    }
    a
};

/// The `S2Coeffs` of a key, from its first two table rows and `R`, `R2`, `S`.
///
/// # Safety
/// The CPU must support `neon` and `aes`; `table` must be readable for two
/// rows.
#[target_feature(enable = "neon,aes")]
pub(crate) unsafe fn s2_prepare(table: *const u8, r: u128, r2: u128, s: u128) -> S2Coeffs {
    // SAFETY: the reads stay within the first two rows.
    let word = |j: usize, i: usize| unsafe { core::ptr::read_unaligned(table.add(BLOCK * j + 8 * i) as *const u64) };
    let lane = |x: u64| vcombine_u64(vcreate_u64(x), vcreate_u64(0));
    let (w0, w1) = (FieldKey::new(r), FieldKey::new(r ^ r2));
    let e: [u128; 8] = core::array::from_fn(|l| {
        let mut w = Wide::new();
        w.mul64(lane(word(0, 8 + l)), &w0);
        w.mul64(lane(word(1, 8 + l)), &w1);
        from_v(w.reduce())
    });
    let (mut n0, mut n1) = (zero(), zero());
    for l in 0..8 {
        n0 = xor(n0, clmul_lo(lane(word(0, l)), lane(word(0, 8 + l))));
        n1 = xor(n1, clmul_lo(lane(word(1, l)), lane(word(1, 8 + l))));
    }
    let mut w = Wide::new();
    w.mul(n0, &w0);
    w.mul(n1, &w1);
    let (lo, hi) = (|x: u128| x as u64 as u128, |x: u128| x >> 64);
    S2Coeffs {
        ka: core::array::from_fn(|q| lo(e[2 * q]) | hi(e[2 * q + 1]) << 64),
        kb: core::array::from_fn(|q| hi(e[2 * q]) | lo(e[2 * q + 1]) << 64),
        c: from_v(w.reduce()) ^ s,
    }
}

/// `x * k` in GF(2^128).
///
/// # Safety
/// The CPU must support `neon` and `aes`.
#[target_feature(enable = "neon,aes")]
pub(crate) unsafe fn mul_key(x: u128, k: &FieldKey) -> u128 {
    let mut w = Wide::new();
    w.mul(vcombine_u64(vcreate_u64(x as u64), vcreate_u64((x >> 64) as u64)), k);
    from_v(w.reduce())
}

/// Add `x` to the planes `F[b]` for the bits `b` set in `u`.
#[inline(always)]
fn add_planes(f: &mut [V; 3], u: usize, x: V) {
    for (b, fb) in f.iter_mut().enumerate() {
        if u >> b & 1 == 1 {
            *fb = xor(*fb, x);
        }
    }
}

macro_rules! backend {
    ($name:ident, $features:literal, $xor3:path, $bulk:path, $bulk2:path, $bulk4:path, $bulk4pf:path, $tail:path, $groups:path, $groups0:path) => {
        pub mod $name {
            use super::*;

            /// Product sum of one encoded position: previous rows `p*`,
            /// current rows `x`, `y`, key row at `k`.
            #[inline(always)]
            unsafe fn position(px: &[V; 4], py: &[V; 4], x: &[V; 4], y: &[V; 4], k: *const u8) -> V {
                unsafe {
                    let mut p = [zero(); 8];
                    for q in 0..4 {
                        let t = $xor3(px[q], x[q], ld(k.add(16 * q)));
                        let u = $xor3(py[q], y[q], ld(k.add(64 + 16 * q)));
                        p[2 * q] = clmul_lo(t, u);
                        p[2 * q + 1] = clmul_hi(t, u);
                    }
                    $xor3($xor3(p[0], p[1], p[2]), $xor3(p[3], p[4], p[5]), xor(p[6], p[7]))
                }
            }

            /// Product sum of the end position after rows `p*`.
            #[inline(always)]
            unsafe fn endpoint(px: &[V; 4], py: &[V; 4], k: *const u8) -> V {
                unsafe { position(px, py, &[zero(); 4], &[zero(); 4], k) }
            }

            #[inline(always)]
            unsafe fn rows(d: *const u8) -> ([V; 4], [V; 4]) {
                unsafe {
                    (
                        [ld(d), ld(d.add(16)), ld(d.add(32)), ld(d.add(48))],
                        [ld(d.add(64)), ld(d.add(80)), ld(d.add(96)), ld(d.add(112))],
                    )
                }
            }

            /// Close a chunk whose `CHUNK_BLOCKS` blocks are summarized in `w`.
            #[inline(always)]
            unsafe fn close_full(core: &mut Core, key: &Key, w: &[u128; 24]) {
                unsafe {
                    let px = [0, 1, 2, 3].map(|i| ldu(&w[16 + i]));
                    let py = [0, 1, 2, 3].map(|i| ldu(&w[20 + i]));
                    let e = endpoint(&px, &py, key.rows().add(BLOCK * CHUNK_BLOCKS));
                    let b = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| ldu(&w[i]));
                    let f = [0, 1, 2].map(|i| ldu(&w[8 + i]));
                    let o = outer_step(key, ldu(&core.outer), &b, &f, e);
                    stu(&mut core.outer, o);
                    core.pos = 0;
                    core.closed = true;
                }
            }

            /// Absorb and close `count` whole chunks at `d`; `core` must be at a
            /// chunk boundary. Chunks go two at a time through a kernel that
            /// shares key loads between them, chosen by the input size (see
            /// `L1_CHUNKS`); an odd last chunk goes alone.
            #[target_feature(enable = $features)]
            pub unsafe fn chunks(core: &mut Core, key: &Key, d: *const u8, count: usize) {
                unsafe {
                    let keys = outer_keys_ptr(key);
                    let table = key.rows();
                    let pairs = count / 2;
                    let mut outer = core.outer;
                    if pairs > 0 {
                        let z = ZERO_BLOCK.as_ptr();
                        outer = if count <= L1_CHUNKS {
                            $bulk2(d, table, pairs, keys, z, outer)
                        } else if count < MEMORY_CHUNKS {
                            $bulk4(d, table, pairs, keys, z, outer)
                        } else {
                            $bulk4pf(d, table, pairs, keys, z, outer)
                        };
                    }
                    if count % 2 == 1 {
                        outer = $bulk(d.add(2 * pairs * CHUNK), table, 1, keys, outer);
                    }
                    core.outer = outer;
                    core.closed |= count > 0;
                }
            }

            /// The output of a message whose final partial chunk is `rest`
            /// (`0 < rest.len() < CHUNK`), after whole chunks summarized by
            /// `outer` (if any): one asm kernel keeps the chunk state in
            /// registers from the first block to the fold.
            #[target_feature(enable = $features)]
            pub unsafe fn tail(key: &Key, rest: &[u8], outer: Option<u128>, len: u64) -> u128 {
                unsafe {
                    debug_assert!(!rest.is_empty() && rest.len() < CHUNK);
                    let whole = rest.len() / BLOCK;
                    let mut pad = core::mem::MaybeUninit::<[u8; BLOCK]>::uninit();
                    let last = if rest.len() % BLOCK != 0 {
                        pad.write(padded_block(&rest[BLOCK * whole..])).as_ptr()
                    } else {
                        core::ptr::null()
                    };
                    let h = $tail(
                        rest.as_ptr(),
                        key.rows(),
                        whole / GROUP_BLOCKS,
                        whole % GROUP_BLOCKS,
                        last,
                        outer_keys_ptr(key),
                        key.r2x.as_ptr() as *const u128,
                        &key.t as *const FieldKey as *const u128,
                        key.s,
                        len,
                        outer.unwrap_or(0),
                    );
                    let _ = pad;
                    h
                }
            }

            /// Absorb `n` whole groups at `d`, closing the chunk if it fills.
            #[target_feature(enable = $features)]
            pub unsafe fn groups(core: &mut Core, key: &Key, d: *const u8, n: usize) {
                unsafe {
                    let pos = core.pos;
                    let table = key.rows().add(BLOCK * pos);
                    if pos == 0 {
                        // Fresh chunk: the kernel starts from zero registers and
                        // writes B, F and prev; the unused words are zeroed. The
                        // words are uninitialized until then, so they are only
                        // reached through the raw pointer.
                        let w = core.words_ptr();
                        $groups0(d, table, n, 0, w);
                        for i in 11..16 {
                            w.add(i).write(0);
                        }
                    } else {
                        $groups(d, table, n, pos / GROUP_BLOCKS, core.words_mut());
                    }
                    core.pos = pos + GROUP_BLOCKS * n;
                    if core.pos == CHUNK_BLOCKS {
                        let w = *core.words().unwrap();
                        close_full(core, key, &w);
                    }
                }
            }

            /// A message of `SHORT_MAX..=S2_MAX` bytes through the affine form
            /// of `S2Coeffs`: 19 carryless products and no length branches.
            #[target_feature(enable = $features)]
            pub unsafe fn medium(key: &Key, msg: &[u8]) -> u128 {
                let len = msg.len();
                debug_assert!((SHORT_MAX..=S2_MAX).contains(&len));
                let p = msg.as_ptr();
                // SAFETY: `msg[..32]` and `msg[len - 32..]` are in bounds.
                unsafe {
                    // X words 0..4 are always present. Words 4..8 are
                    // `msg[32..len]` zero-padded: the last 32 bytes moved down
                    // by `64 - len` with TBL, which gives zero past index 31.
                    let tail = uint8x16x2_t(vld1q_u8(p.add(len - 32)), vld1q_u8(p.add(len - 16)));
                    let shift = vdupq_n_u8((S2_MAX - len) as u8);
                    let idx = |o: usize| vaddq_u8(vld1q_u8(IOTA.as_ptr().add(o)), shift);
                    let x = [
                        ld(p),
                        ld(p.add(16)),
                        vreinterpretq_u64_u8(vqtbl2q_u8(tail, idx(0))),
                        vreinterpretq_u64_u8(vqtbl2q_u8(tail, idx(16))),
                    ];
                    medium_x(key, x, len)
                }
            }

            /// `C + L T + sum_l X[l] E[l]` for the eight X words in `x`.
            #[inline(always)]
            unsafe fn medium_x(key: &Key, x: [V; 4], len: usize) -> u128 {
                let k = &key.s2;
                let (mut lo, mut mid) = ([zero(); 4], [zero(); 4]);
                for q in 0..4 {
                    let (ka, kb) = (ldu(&k.ka[q]), ldu(&k.kb[q]));
                    lo[q] = xor(clmul_lo(x[q], ka), clmul_hi(x[q], kb));
                    mid[q] = xor(clmul_hi(x[q], ka), clmul_lo(x[q], kb));
                }
                // L T: its low half into `lo`, its high half into `mid`.
                let (lv, t) = (unsafe { vdupq_n_u64(len as u64) }, ldu(&key.t.k));
                let lo = $xor3($xor3(lo[0], lo[1], lo[2]), lo[3], clmul_lo(lv, t));
                let mid = $xor3($xor3(mid[0], mid[1], mid[2]), mid[3], clmul_hi(lv, t));
                // `lo + mid x^64`: the top word of `mid` folds back through
                // x^128 = x^7 + x^2 + x + 1.
                let fold = clmul_hi(mid, unsafe { vdupq_n_u64(0x87) });
                from_v(xor($xor3(lo, unsafe { vextq_u64(zero(), mid, 1) }, fold), ldu(&k.c)))
            }

            /// The output after `core` and `pending` (at most 1024 bytes).
            #[target_feature(enable = $features)]
            pub unsafe fn finish(core: &Core, key: &Key, pending: &[u8], len: u64) -> u128 {
                unsafe {
                    let n = pending.len().div_ceil(BLOCK);
                    let full = pending.len() / BLOCK;
                    // Only a partial last block is copied: stack stores ahead of
                    // the data loads could otherwise stall them (4K aliasing).
                    let mut pad = core::mem::MaybeUninit::<[u8; BLOCK]>::uninit();
                    let last = if full == n {
                        pending.as_ptr().add(BLOCK * n.saturating_sub(1))
                    } else {
                        pad.write(padded_block(&pending[BLOCK * full..])).as_ptr()
                    };
                    let d = pending.as_ptr();
                    let outer = core.closed.then(|| ldu(&core.outer));
                    let h = match core.words() {
                        None => match n {
                            0 => {
                                let mut w = Wide::new();
                                w.mul64(len_v(len), &key.t);
                                xor(w.reduce(), outer.unwrap_or(zero()))
                            },
                            1 => small::<1>(key, d, last, len, outer),
                            2 => small::<2>(key, d, last, len, outer),
                            3 => small::<3>(key, d, last, len, outer),
                            4 => small::<4>(key, d, last, len, outer),
                            5 => small::<5>(key, d, last, len, outer),
                            6 => small::<6>(key, d, last, len, outer),
                            7 => small::<7>(key, d, last, len, outer),
                            _ => small::<8>(key, d, last, len, outer),
                        },
                        Some(w) => {
                            let pos = core.pos;
                            match n {
                                0 => fin::<0>(key, w, pos, d, last, len, outer),
                                1 => fin::<1>(key, w, pos, d, last, len, outer),
                                2 => fin::<2>(key, w, pos, d, last, len, outer),
                                3 => fin::<3>(key, w, pos, d, last, len, outer),
                                4 => fin::<4>(key, w, pos, d, last, len, outer),
                                5 => fin::<5>(key, w, pos, d, last, len, outer),
                                6 => fin::<6>(key, w, pos, d, last, len, outer),
                                7 => fin::<7>(key, w, pos, d, last, len, outer),
                                _ => fin::<8>(key, w, pos, d, last, len, outer),
                            }
                        },
                    };
                    let _ = pad;
                    from_v(xor(h, ldu(&key.s)))
                }
            }

            /// A final chunk of `NB <= 8` blocks. Its contribution
            /// `h0 R + h1 R2` is `sum_j S_j (R + j R2)` over the position sums,
            /// with the weights precomputed in the key. All products share one
            /// reduction.
            #[target_feature(enable = $features)]
            #[inline(never)]
            unsafe fn small<const NB: usize>(key: &Key, d: *const u8, last: *const u8, len: u64, outer: Option<V>) -> V {
                unsafe {
                    let table = key.rows();
                    let mut px = [zero(); 4];
                    let mut py = [zero(); 4];
                    // Each position's sum is multiplied by its weight
                    // `W_j = R + j R2` as soon as it exists, so the products
                    // overlap the loads and only one product and the
                    // reduction follow the last position.
                    let mut w = Wide::new();
                    if let Some(o) = outer {
                        w.mul(o, &key.r);
                    }
                    for j in 0..NB {
                        let (x, y) = rows(if j + 1 < NB { d.add(BLOCK * j) } else { last });
                        w.mul(position(&px, &py, &x, &y, table.add(BLOCK * j)), &key.w[j]);
                        px = x;
                        py = y;
                    }
                    w.mul(endpoint(&px, &py, table.add(BLOCK * NB)), &key.w[NB]);
                    w.mul64(len_v(len), &key.t);
                    w.reduce()
                }
            }

            /// A final chunk continuing `pos` (a nonzero multiple of 8) absorbed
            /// blocks with `NB <= 8` more: positions `pos + v` feed `B[v]` and
            /// `C[pos / 8]`, then the end position, the fold into parallel
            /// products and one reduction.
            #[target_feature(enable = $features)]
            #[inline(never)]
            unsafe fn fin<const NB: usize>(
                key: &Key,
                w: &[u128; 24],
                pos: usize,
                d: *const u8,
                last: *const u8,
                len: u64,
                outer: Option<V>,
            ) -> V {
                unsafe {
                    let table = key.rows().add(BLOCK * pos);
                    let mut b = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| ldu(&w[i]));
                    let mut f = [0, 1, 2].map(|i| ldu(&w[8 + i]));
                    let mut px = [0, 1, 2, 3].map(|i| ldu(&w[16 + i]));
                    let mut py = [0, 1, 2, 3].map(|i| ldu(&w[20 + i]));
                    let mut cu = zero();
                    for v in 0..NB {
                        let (x, y) = rows(if v + 1 < NB { d.add(BLOCK * v) } else { last });
                        let s = position(&px, &py, &x, &y, table.add(BLOCK * v));
                        b[v] = xor(b[v], s);
                        cu = xor(cu, s);
                        px = x;
                        py = y;
                    }
                    let s = endpoint(&px, &py, table.add(BLOCK * NB));
                    let mut e = zero();
                    let u = pos / GROUP_BLOCKS;
                    if NB < GROUP_BLOCKS {
                        b[NB] = xor(b[NB], s);
                        cu = xor(cu, s);
                    } else if pos + NB == CHUNK_BLOCKS {
                        e = s;
                    } else {
                        b[0] = xor(b[0], s);
                        add_planes(&mut f, u + 1, s);
                    }
                    add_planes(&mut f, u, cu);
                    let (h0, z) = planes(&b, &f, e);
                    let mut wd = Wide::new();
                    wd.mul(xor(outer.unwrap_or(zero()), h0), &key.r);
                    wd.mul(z[0], &key.r2);
                    wd.mul(z[1], &key.r2x[0]);
                    wd.mul(z[2], &key.r2x[1]);
                    // F[b] collects groups u with bit b set, so it is zero until
                    // positions reach group 2^b; skip those products.
                    let last = pos + NB;
                    for i in 0..3 {
                        if last >= GROUP_BLOCKS << i {
                            wd.mul(z[3 + i], &key.r2x[2 + i]);
                        }
                    }
                    wd.mul64(len_v(len), &key.t);
                    wd.reduce()
                }
            }

            /// The short path for messages of fewer than 32 bytes.
            #[target_feature(enable = $features)]
            pub unsafe fn short(key: &Key, x0: u128, x1: Option<u128>) -> u128 {
                let mut w = Wide::new();
                w.mul(ldu(&x0), &key.a);
                if let Some(x1) = x1 {
                    w.mul(ldu(&x1), &key.b);
                }
                from_v(w.reduce()) ^ key.s
            }
        }
    };
}

#[inline(always)]
fn xor3_sha3(a: V, b: V, c: V) -> V {
    unsafe { veor3q_u64(a, b, c) }
}

#[inline(always)]
fn xor3_plain(a: V, b: V, c: V) -> V {
    xor(xor(a, b), c)
}

backend!(eor3, "neon,aes,sha3", xor3_sha3, crate::neon_asm::bulk_eor3, crate::neon_asm::bulk2_eor3, crate::neon_asm::bulk4_eor3, crate::neon_asm::bulk4pf_eor3, crate::neon_asm::tail_eor3, crate::neon_asm::groups_eor3, crate::neon_asm::groups0_eor3);
backend!(plain, "neon,aes", xor3_plain, crate::neon_asm::bulk_plain, crate::neon_asm::bulk2_plain, crate::neon_asm::bulk4_plain, crate::neon_asm::bulk4pf_plain, crate::neon_asm::tail_plain, crate::neon_asm::groups_plain, crate::neon_asm::groups0_plain);
