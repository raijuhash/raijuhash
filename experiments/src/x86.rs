//! AVX-512 prototypes of the replacement candidates in CANDIDATES.md §9, with
//! scalar oracles. Throwaway research code: the definitions here are
//! performance models of the candidates (complete arithmetic, full chunk
//! close and joint outer polynomial), not frozen specifications.
//!
//! Every design hashes whole chunks only; the benchmark inputs are multiples
//! of the chunk. The outer layer is the joint polynomial of §9.2:
//! `P = (P + Z_0) R + sum_j Z_j A_j`, then `H = P + L T + S`.

use core::arch::x86_64::*;

pub type Z = __m512i;
type X = __m128i;

// ---------------------------------------------------------------- scalar ---

pub fn clmul64(a: u64, b: u64) -> u128 {
    let mut r = 0u128;
    for i in 0..64 {
        if b >> i & 1 == 1 {
            r ^= (a as u128) << i;
        }
    }
    r
}

/// `lo + hi x^128` modulo `x^128 + x^7 + x^2 + x + 1`.
pub fn reduce256(lo: u128, hi: u128) -> u128 {
    let t = hi ^ (hi << 1) ^ (hi << 2) ^ (hi << 7);
    let o = (hi >> 127) ^ (hi >> 126) ^ (hi >> 121);
    lo ^ t ^ o ^ (o << 1) ^ (o << 2) ^ (o << 7)
}

pub fn gf_mul(a: u128, b: u128) -> u128 {
    let (a0, a1) = (a as u64, (a >> 64) as u64);
    let (b0, b1) = (b as u64, (b >> 64) as u64);
    let lo = clmul64(a0, b0);
    let hi = clmul64(a1, b1);
    let mid = clmul64(a0, b1) ^ clmul64(a1, b0);
    reduce256(lo ^ (mid << 64), hi ^ (mid >> 64))
}

fn rd64(p: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(p[8 * i..8 * i + 8].try_into().unwrap())
}
fn rd32(p: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(p[4 * i..4 * i + 4].try_into().unwrap())
}

/// v1's chunk compressor (SPEC §4.2) for a full 64-block chunk given as its
/// 64 blocks, keyed by 65 rows at `k`.
pub fn ref_chain64(blocks: &[&[u8]; 64], k: &[u8]) -> (u128, u128) {
    let (mut h0, mut h1) = (0u128, 0u128);
    let zero = [0u8; 128];
    for j in 0..=64 {
        let prev: &[u8] = if j == 0 { &zero } else { blocks[j - 1] };
        let cur: &[u8] = if j == 64 { &zero } else { blocks[j] };
        let row = &k[128 * j..128 * j + 128];
        let mut s = 0u128;
        for l in 0..8 {
            let tx = rd64(prev, l) ^ rd64(cur, l) ^ rd64(row, l);
            let ty = rd64(prev, 8 + l) ^ rd64(cur, 8 + l) ^ rd64(row, 8 + l);
            s ^= clmul64(tx, ty);
        }
        if j < 64 {
            h0 ^= s;
            h1 ^= gf_mul(j as u128, s);
        } else {
            h1 ^= s;
        }
    }
    (h0, h1)
}

/// Multimixer-128 (Definition 9) on a 512-byte transposed tile keyed by the
/// 512-byte `k`, added into the eight 64-bit sums `z`. Word `16c + b` of the
/// tile is coordinate `c` (x0..x3, y0..y3) of Multimixer block `b`.
pub fn ref_mm_tile(z: &mut [u64; 8], d: &[u8], k: &[u8]) {
    for b in 0..16 {
        let w = |c: usize| rd32(d, 16 * c + b).wrapping_add(rd32(k, 16 * c + b));
        let x = [w(0), w(1), w(2), w(3)];
        let y = [w(4), w(5), w(6), w(7)];
        for i in 0..4 {
            // circ(1,1,1,0) x and circ(0,1,1,1) y.
            let u = x[i].wrapping_add(x[(i + 1) % 4]).wrapping_add(x[(i + 2) % 4]);
            let v = y[(i + 1) % 4].wrapping_add(y[(i + 2) % 4]).wrapping_add(y[(i + 3) % 4]);
            z[i] = z[i].wrapping_add(x[i] as u64 * y[i] as u64);
            z[4 + i] = z[4 + i].wrapping_add(u as u64 * v as u64);
        }
    }
}

/// Field parameters of the outer layer: `r`, `a[j]` for coordinates 1.., `t`, `s`.
#[derive(Clone)]
pub struct Outer {
    pub r: u128,
    pub a: [u128; 7],
    pub t: u128,
    pub s: u128,
}

pub fn ref_outer(o: &Outer, p: u128, coords: &[u128]) -> u128 {
    let mut n = gf_mul(p ^ coords[0], o.r);
    for (j, &c) in coords[1..].iter().enumerate() {
        n ^= gf_mul(c, o.a[j]);
    }
    n
}

pub fn pack_u64s(z: &[u64]) -> Vec<u128> {
    z.chunks(2).map(|c| c[0] as u128 | (c[1] as u128) << 64).collect()
}

// ------------------------------------------------------------ AVX-512 ops ---

/// `p`, hidden from the optimizer, so that key loads are not hoisted out of
/// a chunk loop (LLVM otherwise copies the whole table to the stack).
#[inline(always)]
pub fn opaque(mut p: *const u8) -> *const u8 {
    unsafe { core::arch::asm!("/* {0} */", inout(reg) p, options(nomem, nostack, preserves_flags)) };
    p
}

#[inline(always)]
pub unsafe fn ld(p: *const u8) -> Z {
    unsafe { _mm512_loadu_si512(p.cast()) }
}
#[inline(always)]
pub unsafe fn zero() -> Z {
    unsafe { _mm512_setzero_si512() }
}
#[inline(always)]
pub unsafe fn xor(a: Z, b: Z) -> Z {
    unsafe { _mm512_xor_si512(a, b) }
}
#[inline(always)]
pub unsafe fn x3(a: Z, b: Z, c: Z) -> Z {
    unsafe { _mm512_ternarylogic_epi64::<0x96>(a, b, c) }
}
#[inline(always)]
pub unsafe fn x4(a: Z, b: Z, c: Z, d: Z) -> Z {
    unsafe { x3(a, b, xor(c, d)) }
}
#[inline(always)]
pub unsafe fn clo(a: Z, b: Z) -> Z {
    unsafe { _mm512_clmulepi64_epi128::<0x00>(a, b) }
}
#[inline(always)]
pub unsafe fn chi(a: Z, b: Z) -> Z {
    unsafe { _mm512_clmulepi64_epi128::<0x11>(a, b) }
}
#[inline(always)]
unsafe fn shl<const K: i32>(a: Z) -> Z {
    unsafe { _mm512_shldi_epi64::<K>(a, _mm512_bslli_epi128::<8>(a)) }
}
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
/// The lane sums (XOR) of `a` and `b` as `(A, A, B, B)`.
#[inline(always)]
unsafe fn fold2(a: Z, b: Z) -> Z {
    unsafe {
        let s = xor(_mm512_shuffle_i64x2::<0b01_00_01_00>(a, b), _mm512_shuffle_i64x2::<0b11_10_11_10>(a, b));
        xor(s, _mm512_shuffle_i64x2::<0b10_11_00_01>(s, s))
    }
}
#[inline(always)]
unsafe fn fold_x(a: Z) -> X {
    unsafe {
        let h = _mm256_xor_si256(_mm512_castsi512_si256(a), _mm512_extracti64x4_epi64::<1>(a));
        _mm_xor_si128(_mm256_castsi256_si128(h), _mm256_extracti128_si256::<1>(h))
    }
}
/// `lo + hi x^128` reduced, one xmm lane, with two carryless multiplies.
#[inline(always)]
unsafe fn reduce_x(lo: X, mid: X, hi: X) -> X {
    unsafe {
        let poly = _mm_set1_epi64x(0x87);
        let t = _mm_clmulepi64_si128::<0x01>(hi, poly);
        let w2 = _mm_xor_si128(_mm_xor_si128(hi, _mm_srli_si128::<8>(mid)), _mm_srli_si128::<8>(t));
        let lo = _mm_xor_si128(_mm_xor_si128(lo, _mm_slli_si128::<8>(mid)), _mm_slli_si128::<8>(t));
        _mm_xor_si128(lo, _mm_clmulepi64_si128::<0x00>(w2, poly))
    }
}

/// Unreduced 128x128 products of the lanes of `a` and `k`, accumulated into
/// `(lo, mid, hi)`.
#[inline(always)]
unsafe fn mul_acc(acc: &mut (Z, Z, Z), a: Z, k: Z) {
    unsafe {
        acc.0 = xor(acc.0, clo(a, k));
        acc.2 = xor(acc.2, chi(a, k));
        acc.1 = x3(acc.1, _mm512_clmulepi64_epi128::<0x01>(a, k), _mm512_clmulepi64_epi128::<0x10>(a, k));
    }
}

/// As `mul_acc` with three multiplies (Karatsuba): `kx` holds `k0 ^ k1` in
/// the low word of each lane. `acc` gets `(lo, mid, hi)` with `mid` already
/// corrected, so `finish_acc` applies unchanged.
#[inline(always)]
unsafe fn mul_acc_k(acc: &mut (Z, Z, Z), a: Z, k: Z, kx: Z) {
    unsafe {
        let lo = clo(a, k);
        let hi = chi(a, k);
        let s = xor(a, _mm512_bsrli_epi128::<8>(a));
        acc.0 = xor(acc.0, lo);
        acc.2 = xor(acc.2, hi);
        acc.1 = xor(acc.1, x3(clo(s, kx), lo, hi));
    }
}

/// `lo + mid x^64 + hi x^128` per lane, reduced with shifts, then the lane
/// sum: no carryless multiplies.
#[inline(always)]
unsafe fn finish_acc_shift(acc: (Z, Z, Z)) -> X {
    unsafe {
        let lo = xor(acc.0, _mm512_bslli_epi128::<8>(acc.1));
        let hi = xor(acc.2, _mm512_bsrli_epi128::<8>(acc.1));
        let t = x3(x3(hi, shl::<1>(hi), shl::<2>(hi)), shl::<7>(hi), lo);
        let o = x3(_mm512_srli_epi64::<63>(hi), _mm512_srli_epi64::<62>(hi), _mm512_srli_epi64::<57>(hi));
        fold_x(xor(t, times_poly(_mm512_bsrli_epi128::<8>(o))))
    }
}

/// Sum over all lanes of the products in `acc`, reduced.
#[inline(always)]
unsafe fn finish_acc(acc: (Z, Z, Z)) -> X {
    unsafe { reduce_x(fold_x(acc.0), fold_x(acc.1), fold_x(acc.2)) }
}

/// Eight vectors of 64-bit lanes to their lane sums packed as
/// `(z0|z1, z2|z3, z4|z5, z6|z7)`, one 128-bit lane each.
#[inline(always)]
unsafe fn hsum8(z: &[Z; 8]) -> Z {
    unsafe {
        let p = |a: Z, b: Z| _mm512_add_epi64(_mm512_unpacklo_epi64(a, b), _mm512_unpackhi_epi64(a, b));
        let (t01, t23, t45, t67) = (p(z[0], z[1]), p(z[2], z[3]), p(z[4], z[5]), p(z[6], z[7]));
        let q = |a: Z, b: Z| {
            _mm512_add_epi64(_mm512_shuffle_i64x2::<0b10_00_10_00>(a, b), _mm512_shuffle_i64x2::<0b11_01_11_01>(a, b))
        };
        let (s0, s1) = (q(t01, t23), q(t45, t67));
        q(s0, s1)
    }
}

// ------------------------------------------------------ kernel pieces ---

/// Carryless chain state of the chunk in progress (as v1's `bulk`).
#[derive(Clone, Copy)]
pub struct Chain {
    pub b: [Z; 8],
    f: [Z; 3],
    px: Z,
    py: Z,
}

impl Chain {
    #[inline(always)]
    pub unsafe fn new() -> Chain {
        unsafe { Chain { b: [zero(); 8], f: [zero(); 3], px: zero(), py: zero() } }
    }

    /// Block `v` of a group at `p`, key row at `k`.
    #[inline(always)]
    pub unsafe fn block(&mut self, v: usize, p: *const u8, k: *const u8) {
        unsafe {
            let (x, y) = (ld(p), ld(p.add(64)));
            let t = x3(self.px, x, ld(k));
            let w = x3(self.py, y, ld(k.add(64)));
            self.b[v] = x3(self.b[v], clo(t, w), chi(t, w));
            self.px = x;
            self.py = y;
        }
    }

    /// After group `u`: `F[0]` sums every running total, `F[1]` the odd
    /// ones, `F[2] = T_3 + T_7` (see v1's `bulk`).
    #[inline(always)]
    pub unsafe fn end_group(&mut self, u: usize) {
        unsafe {
            let b = &self.b;
            let t = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
            self.f[0] = xor(self.f[0], t);
            if u & 1 == 1 {
                self.f[1] = xor(self.f[1], t);
            }
            if u == 3 || u == 7 {
                self.f[2] = xor(self.f[2], t);
            }
        }
    }

    /// Diagnostic: every accumulator, XORed.
    #[inline(always)]
    pub unsafe fn digest_all(&self) -> Z {
        unsafe {
            let b = &self.b;
            let t = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
            x4(t, x3(self.f[0], self.f[1], self.f[2]), self.px, self.py)
        }
    }

    pub unsafe fn set_prev(&mut self, px: Z, py: Z) {
        self.px = px;
        self.py = py;
    }

    /// Group totals in memory (`ts[u]`), for a group index known only at
    /// run time: keeps the three plane registers free inside the loop.
    #[inline(always)]
    pub unsafe fn end_group_mem(&mut self, ts: &mut [Z; 8], u: usize) {
        unsafe {
            let b = &self.b;
            ts[u] = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
        }
    }

    /// Planes from the totals stored by `end_group_mem`.
    #[inline(always)]
    pub unsafe fn planes_from(&mut self, t: &[Z; 8]) {
        unsafe {
            let odd = x4(t[1], t[3], t[5], t[7]);
            self.f = [xor(odd, x4(t[0], t[2], t[4], t[6])), odd, xor(t[3], t[7])];
        }
    }

    /// `end_group` for a group index known only at run time.
    #[inline(always)]
    pub unsafe fn end_group_dyn(&mut self, u: usize) {
        unsafe {
            let b = &self.b;
            let t = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
            self.f[0] = xor(self.f[0], t);
            let m1 = 0u8.wrapping_sub((u & 1) as u8);
            let m2 = 0u8.wrapping_sub((u & 3 == 3) as u8);
            self.f[1] = _mm512_mask_xor_epi64(self.f[1], m1, self.f[1], t);
            self.f[2] = _mm512_mask_xor_epi64(self.f[2], m2, self.f[2], t);
        }
    }

    /// Endpoint at key row `k` (column `(0, 1)`), then `(h0, h1)` reduced as
    /// `(A, A, B, B)` lanes.
    #[inline(always)]
    pub unsafe fn close(&self, k: *const u8) -> Z {
        unsafe {
            let t = xor(self.px, ld(k));
            let w = xor(self.py, ld(k.add(64)));
            let e = xor(clo(t, w), chi(t, w));
            let b = &self.b;
            let h0 = xor(x4(b[0], b[1], b[2], b[3]), x4(b[4], b[5], b[6], b[7]));
            let z = [
                x3(e, xor(b[1], b[3]), xor(b[5], b[7])),
                x4(b[2], b[3], b[6], b[7]),
                x4(b[4], b[5], b[6], b[7]),
                self.f[0],
                self.f[1],
                self.f[2],
            ];
            fold2(h0, shifted_sum(z))
        }
    }
}

/// Multimixer-128 on a transposed 512-byte tile at `p`, key at `k`.
#[inline(always)]
pub unsafe fn mm_tile(z: &mut [Z; 8], p: *const u8, k: *const u8) {
    unsafe {
        let a = |c: usize| _mm512_add_epi32(ld(p.add(64 * c)), ld(k.add(64 * c)));
        let x = [a(0), a(1), a(2), a(3)];
        let y = [a(4), a(5), a(6), a(7)];
        let sx = _mm512_add_epi32(_mm512_add_epi32(x[0], x[1]), _mm512_add_epi32(x[2], x[3]));
        let sy = _mm512_add_epi32(_mm512_add_epi32(y[0], y[1]), _mm512_add_epi32(y[2], y[3]));
        let prod = |a: Z, b: Z| {
            _mm512_add_epi64(
                _mm512_mul_epu32(a, b),
                _mm512_mul_epu32(_mm512_srli_epi64::<32>(a), _mm512_srli_epi64::<32>(b)),
            )
        };
        for i in 0..4 {
            let u = _mm512_sub_epi32(sx, x[(i + 3) & 3]);
            let v = _mm512_sub_epi32(sy, y[i]);
            z[i] = _mm512_add_epi64(z[i], prod(x[i], y[i]));
            z[4 + i] = _mm512_add_epi64(z[4 + i], prod(u, v));
        }
    }
}

/// A Multimixer tile split into pieces that can be interleaved with
/// carryless blocks: `load` (key addition and the two sums), then `prod(i)`
/// for `i = 0..4`.
#[derive(Clone, Copy)]
pub struct TileParts {
    x: [Z; 4],
    y: [Z; 4],
    sx: Z,
    sy: Z,
}

impl TileParts {
    #[inline(always)]
    pub unsafe fn load(p: *const u8, k: *const u8) -> TileParts {
        unsafe {
            let a = |c: usize| _mm512_add_epi32(ld(p.add(64 * c)), ld(k.add(64 * c)));
            let x = [a(0), a(1), a(2), a(3)];
            let y = [a(4), a(5), a(6), a(7)];
            let sx = _mm512_add_epi32(_mm512_add_epi32(x[0], x[1]), _mm512_add_epi32(x[2], x[3]));
            let sy = _mm512_add_epi32(_mm512_add_epi32(y[0], y[1]), _mm512_add_epi32(y[2], y[3]));
            TileParts { x, y, sx, sy }
        }
    }

    #[inline(always)]
    pub unsafe fn prod(&self, z: &mut [Z; 8], i: usize) {
        unsafe {
            let prod = |a: Z, b: Z| {
                _mm512_add_epi64(
                    _mm512_mul_epu32(a, b),
                    _mm512_mul_epu32(_mm512_srli_epi64::<32>(a), _mm512_srli_epi64::<32>(b)),
                )
            };
            let u = _mm512_sub_epi32(self.sx, self.x[(i + 3) & 3]);
            let v = _mm512_sub_epi32(self.sy, self.y[i]);
            z[i] = _mm512_add_epi64(z[i], prod(self.x[i], self.y[i]));
            z[4 + i] = _mm512_add_epi64(z[4 + i], prod(u, v));
        }
    }
}

// -------------------------------------------------------------- designs ---

/// A 64-byte-aligned byte buffer.
pub struct Aligned {
    v: Vec<[u8; 64]>,
    len: usize,
}

impl Aligned {
    pub fn new(len: usize) -> Aligned {
        Aligned { v: vec![[0; 64]; len.div_ceil(64) + 1], len }
    }
    fn base(&self) -> usize {
        (64 - self.v.as_ptr() as usize % 64) % 64
    }
    pub fn as_ptr(&self) -> *const u8 {
        unsafe { (self.v.as_ptr() as *const u8).add(self.base()) }
    }
}

impl core::ops::Deref for Aligned {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.as_ptr(), self.len) }
    }
}

impl core::ops::DerefMut for Aligned {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.as_ptr() as *mut u8, self.len) }
    }
}

/// A design's prepared key: compression key bytes and packed outer keys.
pub struct Keys {
    pub comp: Aligned,
    /// Per 512-byte tile after the 65 chain rows: the dword sums
    /// `k0 + .. + k3` and `k4 + .. + k7` of its eight key vectors (derived,
    /// not extra key material).
    pub tile_sums: Aligned,
    pub outer: Outer,
    /// `(R, A1, A2, A3)` and `(A4, A5, A6, 0)` as 64-byte vectors.
    pub pk: [[u128; 4]; 2],
    /// The same with `k0 ^ k1` in the low word of each lane (Karatsuba).
    pub pkx: [[u128; 4]; 2],
}

impl Keys {
    pub fn new(comp_bytes: usize, seed: u64) -> Keys {
        let mut s = seed;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut comp = Aligned::new(comp_bytes);
        for b in comp.iter_mut() {
            *b = next() as u8;
        }
        let mut f = || next() as u128 | (next() as u128) << 64;
        let outer = Outer { r: f(), a: [f(), f(), f(), f(), f(), f(), f()], t: f(), s: f() };
        let pk = [[outer.r, outer.a[0], outer.a[1], outer.a[2]], [outer.a[3], outer.a[4], outer.a[5], 0]];
        let tiles = comp_bytes.saturating_sub(65 * 128) / 512;
        let mut tile_sums = Aligned::new(128 * tiles + 64);
        for t in 0..tiles {
            let kt = &comp[65 * 128 + 512 * t..];
            for half in 0..2 {
                for w in 0..16 {
                    let s = (0..4).fold(0u32, |acc, c| acc.wrapping_add(rd32(kt, 16 * (4 * half + c) + w)));
                    tile_sums[128 * t + 64 * half + 4 * w..][..4].copy_from_slice(&s.to_le_bytes());
                }
            }
        }
        let kx = |v: u128| (v as u64 ^ (v >> 64) as u64) as u128;
        let pkx = pk.map(|row| row.map(kx));
        Keys { comp, tile_sums, outer, pk, pkx }
    }
}

/// Final `P + L T + S`.
pub fn ref_final(o: &Outer, p: u128, len: usize) -> u128 {
    p ^ gf_mul(len as u128, o.t) ^ o.s
}

#[inline(always)]
unsafe fn final_x(keys: &Keys, p: X, len: usize) -> u128 {
    unsafe {
        let l = _mm_set_epi64x(0, len as i64);
        let t = _mm_loadu_si128((&keys.outer.t as *const u128).cast());
        let lo = _mm_clmulepi64_si128::<0x00>(l, t);
        let mid = _mm_clmulepi64_si128::<0x10>(l, t);
        let r = reduce_x(lo, mid, _mm_setzero_si128());
        let r = _mm_xor_si128(_mm_xor_si128(r, p), _mm_loadu_si128((&keys.outer.s as *const u128).cast()));
        let mut out = 0u128;
        _mm_storeu_si128((&mut out as *mut u128).cast(), r);
        out
    }
}

// --- H: hybrid. Chunk = 8 groups of (8 carryless blocks + T MM tiles). ---

/// Bytes of an H chunk with `t` tiles per group.
pub const fn h_chunk(t: usize) -> usize {
    8 * (1024 + 512 * t)
}
/// Compression key bytes: 65 chain rows, then `8 t` tiles.
pub const fn h_key(t: usize) -> usize {
    65 * 128 + 8 * t * 512
}

pub fn ref_h<const T: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    let cs = h_chunk(T);
    assert!(msg.len() % cs == 0);
    let k: &[u8] = &keys.comp;
    let mut p = 0u128;
    for c in msg.chunks(cs) {
        let mut blocks: [&[u8]; 64] = [&[]; 64];
        let mut z = [0u64; 8];
        for u in 0..8 {
            let g = &c[u * (1024 + 512 * T)..];
            for v in 0..8 {
                blocks[8 * u + v] = &g[128 * v..128 * v + 128];
            }
            for t in 0..T {
                let ko = 65 * 128 + 512 * (T * u + t);
                ref_mm_tile(&mut z, &g[1024 + 512 * t..], &k[ko..ko + 512]);
            }
        }
        let (h0, h1) = ref_chain64(&blocks, &k[..65 * 128]);
        let mut coords = vec![h0, h1];
        if T > 0 {
            coords.extend(pack_u64s(&z));
        }
        p = ref_outer(&keys.outer, p, &coords);
    }
    ref_final(&keys.outer, p, msg.len())
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_h<const T: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe { simd_h_impl::<T, true>(keys, msg) }
}

/// `P` after the whole chunks of `msg` from `P = 0` (the `h0` design, v1's
/// chunk compressor and outer step), without `L T + S`: the part a parallel
/// hash combines as `P <- P R^m + P_task`.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn h0_outer(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let k = keys.comp.as_ptr();
        let k1 = ld(keys.pk.as_ptr() as *const u8);
        let mut p = _mm_setzero_si128();
        for c in 0..msg.len() / 8192 {
            let d = msg.as_ptr().add(c * 8192);
            let k = opaque(k);
            let mut ch = Chain::new();
            let groups = opaque(8 as *const u8) as usize;
            let mut ts = [zero(); 8];
            for u in 0..groups {
                let (g, kc) = (d.add(1024 * u), k.add(1024 * u));
                for v in 0..8 {
                    ch.block(v, g.add(128 * v), kc.add(128 * v));
                }
                ch.end_group_mem(&mut ts, u & 7);
            }
            ch.planes_from(&ts);
            let hh = _mm512_xor_si512(ch.close(k.add(128 * 64)), _mm512_zextsi128_si512(p));
            let hh = _mm512_shuffle_i64x2::<0b00_00_10_00>(hh, hh);
            let mut acc = (zero(), zero(), zero());
            mul_acc(&mut acc, _mm512_maskz_mov_epi64(0b0000_1111, hh), k1);
            p = finish_acc(acc);
        }
        let mut r = 0u128;
        _mm_storeu_si128((&mut r as *mut u128).cast(), p);
        r
    }
}

/// `a b` in GF(2^128) with PCLMULQDQ.
#[target_feature(enable = "pclmulqdq,sse4.1")]
pub unsafe fn gf_mul_hw(a: u128, b: u128) -> u128 {
    unsafe {
        let (x, y) = (_mm_loadu_si128((&a as *const u128).cast()), _mm_loadu_si128((&b as *const u128).cast()));
        let lo = _mm_clmulepi64_si128::<0x00>(x, y);
        let hi = _mm_clmulepi64_si128::<0x11>(x, y);
        let mid = _mm_xor_si128(_mm_clmulepi64_si128::<0x01>(x, y), _mm_clmulepi64_si128::<0x10>(x, y));
        let mut r = 0u128;
        _mm_storeu_si128((&mut r as *mut u128).cast(), reduce_x(lo, mid, hi));
        r
    }
}

/// Diagnostic: `simd_h` with the chain and outer close skipped (not a hash).
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_h_noclose<const T: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe { simd_h_impl::<T, false>(keys, msg) }
}

#[inline(always)]
unsafe fn simd_h_impl<const T: usize, const CLOSE: bool>(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let cs = h_chunk(T);
        let k = keys.comp.as_ptr();
        let pk = keys.pk.as_ptr() as *const u8;
        let (k1, k2) = (ld(pk), ld(pk.add(64)));
        let mut p = _mm_setzero_si128();
        for c in 0..msg.len() / cs {
            let d = msg.as_ptr().add(c * cs);
            let k = opaque(k);
            let mut ch = Chain::new();
            let mut z = [zero(); 8];
            // A real loop (the group count is hidden): fully unrolled, LLVM
            // hoists the products of all 64 blocks and spills them.
            let groups = opaque(8 as *const u8) as usize;
            let mut ts = [zero(); 8];
            for u in 0..groups {
                let g = d.add(u * (1024 + 512 * T));
                let kc = k.add(1024 * u);
                for v in 0..8 {
                    ch.block(v, g.add(128 * v), kc.add(128 * v));
                }
                ch.end_group_mem(&mut ts, u & 7);
                for t in 0..T {
                    mm_tile(&mut z, g.add(1024 + 512 * t), k.add(65 * 128 + 512 * (T * u + t)));
                }
            }
            ch.planes_from(&ts);
            if !CLOSE {
                let mut all = ch.digest_all();
                for zi in z {
                    all = xor(all, zi);
                }
                p = _mm_xor_si128(p, fold_x(all));
                continue;
            }
            // (h0, h0, h1, h1): lane 0 gets P, lane 1 is dropped below.
            let hh = _mm512_xor_si512(ch.close(k.add(128 * 64)), _mm512_zextsi128_si512(p));
            let hh = _mm512_shuffle_i64x2::<0b00_00_10_00>(hh, hh); // (h0+P, h1, .., ..)
            let mut acc = (zero(), zero(), zero());
            if T > 0 {
                let w = hsum8(&z);
                let a1 = _mm512_shuffle_i64x2::<0b01_00_01_00>(hh, w);
                let a2 = _mm512_shuffle_i64x2::<0b00_00_11_10>(w, zero());
                mul_acc(&mut acc, a1, k1);
                mul_acc(&mut acc, a2, k2);
            } else {
                let a1 = _mm512_maskz_mov_epi64(0b0000_1111, hh);
                mul_acc(&mut acc, a1, k1);
            }
            p = finish_acc(acc);
        }
        final_x(keys, p, msg.len())
    }
}

/// Diagnostic: `simd_m` with the close only at the end (not a hash).
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_m_noclose(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let k = keys.comp.as_ptr();
        let k1 = ld(keys.pk.as_ptr() as *const u8);
        let mut z = [zero(); 8];
        for c in 0..msg.len() / M_CHUNK {
            let d = msg.as_ptr().add(c * M_CHUNK);
            let k = opaque(k);
            for t in 0..16 {
                mm_tile(&mut z, d.add(512 * t), k.add(512 * t));
            }
        }
        let mut acc = (zero(), zero(), zero());
        mul_acc(&mut acc, hsum8(&z), k1);
        final_x(keys, finish_acc(acc), msg.len())
    }
}

// --- H4: hybrid with 4-block groups. Chunk = 16 groups of (4 carryless
// blocks + 1 MM tile) = 16 KiB; the carryless digest is v1's chunk
// compressor on the 64 carryless blocks, the MM digest the same as h2's
// on its 16 tiles (tile u after group u). ---

pub const H4_CHUNK: usize = 16 * (512 + 512);
pub const H4_KEY: usize = 65 * 128 + 16 * 512;

pub fn ref_h4(keys: &Keys, msg: &[u8]) -> u128 {
    assert!(msg.len() % H4_CHUNK == 0);
    let k: &[u8] = &keys.comp;
    let mut p = 0u128;
    for c in msg.chunks(H4_CHUNK) {
        let mut blocks: [&[u8]; 64] = [&[]; 64];
        let mut z = [0u64; 8];
        for u in 0..16 {
            let g = &c[1024 * u..];
            for v in 0..4 {
                blocks[4 * u + v] = &g[128 * v..128 * v + 128];
            }
            let ko = 65 * 128 + 512 * u;
            ref_mm_tile(&mut z, &g[512..], &k[ko..ko + 512]);
        }
        let (h0, h1) = ref_chain64(&blocks, &k[..65 * 128]);
        let mut coords = vec![h0, h1];
        coords.extend(pack_u64s(&z));
        p = ref_outer(&keys.outer, p, &coords);
    }
    ref_final(&keys.outer, p, msg.len())
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_h4(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let k = keys.comp.as_ptr();
        let pk = keys.pk.as_ptr() as *const u8;
        let (k1, k2) = (ld(pk), ld(pk.add(64)));
        let mut p = _mm_setzero_si128();
        for c in 0..msg.len() / H4_CHUNK {
            let d = msg.as_ptr().add(c * H4_CHUNK);
            let k = opaque(k);
            let mut b = [zero(); 4];
            let (mut px, mut py) = (zero(), zero());
            let mut z = [zero(); 8];
            let mut ts = [zero(); 16];
            let groups = opaque(16 as *const u8) as usize;
            for u in 0..groups {
                let g = d.add(1024 * u);
                let kc = k.add(512 * u);
                for (v, bv) in b.iter_mut().enumerate() {
                    let (p, kk) = (g.add(128 * v), kc.add(128 * v));
                    let (x, y) = (ld(p), ld(p.add(64)));
                    let t = x3(px, x, ld(kk));
                    let w = x3(py, y, ld(kk.add(64)));
                    *bv = x3(*bv, clo(t, w), chi(t, w));
                    px = x;
                    py = y;
                }
                ts[u & 15] = x4(b[0], b[1], b[2], b[3]);
                mm_tile(&mut z, g.add(512), k.add(65 * 128 + 512 * u));
            }
            // Endpoint (column (0, 1)), planes, (h0, h1).
            let ke = k.add(128 * 64);
            let e = {
                let t = xor(px, ld(ke));
                let w = xor(py, ld(ke.add(64)));
                xor(clo(t, w), chi(t, w))
            };
            let t = &ts;
            let odd = x4(x4(t[1], t[3], t[5], t[7]), t[9], t[11], xor(t[13], t[15]));
            let even = x4(x4(t[0], t[2], t[4], t[6]), t[8], t[10], xor(t[12], t[14]));
            let planes = [
                x3(e, b[1], b[3]),
                xor(b[2], b[3]),
                xor(odd, even),
                odd,
                x4(t[3], t[7], t[11], t[15]),
                xor(t[7], t[15]),
            ];
            let h0 = x4(b[0], b[1], b[2], b[3]);
            let hh = _mm512_xor_si512(fold2(h0, shifted_sum(planes)), _mm512_zextsi128_si512(p));
            let hh = _mm512_shuffle_i64x2::<0b00_00_10_00>(hh, hh);
            let w = hsum8(&z);
            let a1 = _mm512_shuffle_i64x2::<0b01_00_01_00>(hh, w);
            let a2 = _mm512_shuffle_i64x2::<0b00_00_11_10>(w, zero());
            let mut acc = (zero(), zero(), zero());
            mul_acc(&mut acc, a1, k1);
            mul_acc(&mut acc, a2, k2);
            p = finish_acc(acc);
        }
        final_x(keys, p, msg.len())
    }
}

// --- HX: hybrid in hand-scheduled asm. Chunk = 64/N super-groups of
// [N carryless blocks][one MM tile]. ---

pub const fn hx_chunk(n: usize) -> usize {
    64 * 128 + 64 / n * 512
}
pub const fn hx_key(n: usize) -> usize {
    65 * 128 + 64 / n * 512
}

pub fn ref_hx<const N: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    let cs = hx_chunk(N);
    assert!(msg.len() % cs == 0);
    let k: &[u8] = &keys.comp;
    let mut p = 0u128;
    for c in msg.chunks(cs) {
        let mut blocks: [&[u8]; 64] = [&[]; 64];
        let mut z = [0u64; 8];
        for s in 0..64 / N {
            let g = &c[s * (128 * N + 512)..];
            for v in 0..N {
                blocks[N * s + v] = &g[128 * v..128 * v + 128];
            }
            let ko = 65 * 128 + 512 * s;
            ref_mm_tile(&mut z, &g[128 * N..], &k[ko..ko + 512]);
        }
        let (h0, h1) = ref_chain64(&blocks, &k[..65 * 128]);
        let mut coords = vec![h0, h1];
        coords.extend(pack_u64s(&z));
        p = ref_outer(&keys.outer, p, &coords);
    }
    ref_final(&keys.outer, p, msg.len())
}

type Body = unsafe fn(*const u8, *const u8, *const u8, *const u8, *mut u8, *mut u8);

/// Diagnostic: the asm bodies without the chunk close (not a hash).
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_hx16_noclose(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let cs = hx_chunk(16);
        let k = keys.comp.as_ptr();
        let mut out = [zero(); 18];
        let mut ts = [zero(); 8];
        let mut acc = zero();
        for c in 0..msg.len() / cs {
            let d = msg.as_ptr().add(c * cs);
            crate::x86_asm::hyb16(d, k, k.add(65 * 128), keys.tile_sums.as_ptr(), ts.as_mut_ptr().cast(), out.as_mut_ptr().cast());
            acc = x3(acc, out[0], out[8]);
        }
        let mut r = 0u128;
        _mm_storeu_si128((&mut r as *mut u128).cast(), fold_x(acc));
        r
    }
}

#[inline(always)]
unsafe fn simd_hx_with(keys: &Keys, msg: &[u8], cs: usize, body: Body) -> u128 {
    unsafe { simd_hx_close::<false>(keys, msg, cs, body) }
}

#[inline(always)]
unsafe fn simd_hx_close<const FAST: bool>(keys: &Keys, msg: &[u8], cs: usize, body: Body) -> u128 {
    unsafe { simd_hx_close2::<FAST, true>(keys, msg, cs, body) }
}

/// Diagnostic: `CHAIN = false` computes every close but does not feed `P`
/// into the next one (not a hash).
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_hx16_unchained(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe { simd_hx_close2::<true, false>(keys, msg, hx_chunk(16), crate::x86_asm::hyb16) }
}

#[inline(always)]
unsafe fn simd_hx_close2<const FAST: bool, const CHAIN: bool>(keys: &Keys, msg: &[u8], cs: usize, body: Body) -> u128 {
    unsafe {
        let k = keys.comp.as_ptr();
        let pk = keys.pk.as_ptr() as *const u8;
        let (k1, k2) = (ld(pk), ld(pk.add(64)));
        let pkx = keys.pkx.as_ptr() as *const u8;
        let (kx1, kx2) = (ld(pkx), ld(pkx.add(64)));
        let mut p = _mm_setzero_si128();
        let mut unchained = _mm_setzero_si128();
        let mut out = [zero(); 18];
        let mut ts = [zero(); 8];
        for c in 0..msg.len() / cs {
            let d = msg.as_ptr().add(c * cs);
            body(d, k, k.add(65 * 128), keys.tile_sums.as_ptr(), ts.as_mut_ptr().cast(), out.as_mut_ptr().cast());
            let mut ch = Chain::new();
            ch.b.copy_from_slice(&out[..8]);
            ch.set_prev(out[16], out[17]);
            ch.planes_from(&ts);
            let z: [Z; 8] = out[8..16].try_into().unwrap();
            let hh = _mm512_xor_si512(ch.close(k.add(128 * 64)), _mm512_zextsi128_si512(p));
            let hh = _mm512_shuffle_i64x2::<0b00_00_10_00>(hh, hh);
            let w = hsum8(&z);
            let a1 = _mm512_shuffle_i64x2::<0b01_00_01_00>(hh, w);
            let a2 = _mm512_shuffle_i64x2::<0b00_00_11_10>(w, zero());
            let mut acc = (zero(), zero(), zero());
            if FAST {
                mul_acc_k(&mut acc, a1, k1, kx1);
                mul_acc_k(&mut acc, a2, k2, kx2);
                if CHAIN {
                    p = finish_acc_shift(acc);
                } else {
                    unchained = _mm_xor_si128(unchained, finish_acc_shift(acc));
                }
            } else {
                mul_acc(&mut acc, a1, k1);
                mul_acc(&mut acc, a2, k2);
                p = finish_acc(acc);
            }
        }
        final_x(keys, _mm_xor_si128(p, unchained), msg.len())
    }
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_hx16_fast(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe { simd_hx_close::<true>(keys, msg, hx_chunk(16), crate::x86_asm::hyb16) }
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_hx8_fast(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe { simd_hx_close::<true>(keys, msg, hx_chunk(8), crate::x86_asm::hyb8) }
}

macro_rules! hx_design {
    ($name:ident, $n:literal, $body:path) => {
        #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
        pub unsafe fn $name(keys: &Keys, msg: &[u8]) -> u128 {
            unsafe { simd_hx_with(keys, msg, hx_chunk($n), $body) }
        }
    };
}
hx_design!(simd_hx4, 4, crate::x86_asm::hyb4);
hx_design!(simd_hx8, 8, crate::x86_asm::hyb8);
hx_design!(simd_hx16, 16, crate::x86_asm::hyb16);
hx_design!(simd_hx32, 32, crate::x86_asm::hyb32);
hx_design!(simd_hx8_seq, 8, crate::x86_asm::hyb8_seq);
hx_design!(simd_hx16_seq, 16, crate::x86_asm::hyb16_seq);

// --- HXF: the hybrid entirely in asm, close included (`hybfull*`). ---

#[repr(C, align(64))]
struct Scratch([u8; 1024]);

macro_rules! hxf_design {
    ($name:ident, $n:literal, $body:path) => {
        #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
        pub unsafe fn $name(keys: &Keys, msg: &[u8]) -> u128 {
            unsafe {
                let mut sc = Scratch([0; 1024]);
                let pk = keys.pk.as_ptr() as *const u8;
                core::ptr::copy_nonoverlapping(pk, sc.0.as_mut_ptr().add(512), 128);
                let chunks = msg.len() / hx_chunk($n);
                $body(msg.as_ptr(), keys.comp.as_ptr(), keys.tile_sums.as_ptr(), sc.0.as_mut_ptr(), chunks);
                let p = _mm_loadu_si128(sc.0.as_ptr().add(640).cast());
                final_x(keys, p, msg.len())
            }
        }
    };
}
hxf_design!(simd_hxf8, 8, crate::x86_asm::hybfull8);
hxf_design!(simd_hxf16, 16, crate::x86_asm::hybfull16);
hxf_design!(simd_hxf32, 32, crate::x86_asm::hybfull32);
hxf_design!(simd_hxf16_pf1k, 16, crate::x86_asm::hybfull16_pf1024);
hxf_design!(simd_hxf16_pf2k, 16, crate::x86_asm::hybfull16_pf2048);
hxf_design!(simd_hxf16_pf4k, 16, crate::x86_asm::hybfull16_pf4096);

// --- M: Multimixer only. Chunk = 16 tiles (8 KiB); four coordinates. ---

pub const M_CHUNK: usize = 16 * 512;

pub fn ref_m(keys: &Keys, msg: &[u8]) -> u128 {
    assert!(msg.len() % M_CHUNK == 0);
    let mut p = 0u128;
    for c in msg.chunks(M_CHUNK) {
        let mut z = [0u64; 8];
        for t in 0..16 {
            ref_mm_tile(&mut z, &c[512 * t..], &keys.comp[512 * t..512 * t + 512]);
        }
        p = ref_outer(&keys.outer, p, &pack_u64s(&z));
    }
    ref_final(&keys.outer, p, msg.len())
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_m(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let k = keys.comp.as_ptr();
        let k1 = ld(keys.pk.as_ptr() as *const u8);
        let mut p = _mm_setzero_si128();
        for c in 0..msg.len() / M_CHUNK {
            let d = msg.as_ptr().add(c * M_CHUNK);
            let k = opaque(k);
            let mut z = [zero(); 8];
            for t in 0..16 {
                mm_tile(&mut z, d.add(512 * t), k.add(512 * t));
            }
            let w = xor(hsum8(&z), _mm512_zextsi128_si512(p));
            let mut acc = (zero(), zero(), zero());
            mul_acc(&mut acc, w, k1);
            p = finish_acc(acc);
        }
        final_x(keys, p, msg.len())
    }
}

// ----------------------------------------------------------------- G64 ---
// §9.4: the chain code with NH products and column coefficients in
// E = GF(2^64) = GF(2)[x]/(x^64 + x^4 + x^3 + x + 1). A one-chunk message
// (the direct branch) outputs (h0, h1) + (L T0, L T1) + S with no outer
// multiplication. Compared here with v1's close on the same position code,
// for fresh-chunk messages of 1..=8 whole blocks.

/// `v` with the bits of `x^64 = x^4 + x^3 + x + 1` applied: `v r(x)` for
/// `v` below 2^60 (so the product fits in 64 bits).
fn mulr(v: u64) -> u64 {
    v ^ v << 1 ^ v << 3 ^ v << 4
}

/// `v0 + v1 x^64 + over x^128` modulo `x^64 + x^4 + x^3 + x + 1`, for
/// `over` below 2^60.
pub fn reduce_p64(v: u128, over: u64) -> u64 {
    let (v0, v1) = (v as u64, (v >> 64) as u64);
    let v1 = v1 ^ mulr(over);
    let t = clmul64(v1, 0x1b);
    v0 ^ t as u64 ^ mulr((t >> 64) as u64)
}

pub fn e_mul(a: u64, b: u64) -> u64 {
    reduce_p64(clmul64(a, b), 0)
}

/// The position sums `S_j` (unreduced, 128-bit) of a fresh chunk of `n`
/// whole blocks, `j = 0..=n`, keyed by the rows at `k`.
fn ref_positions(msg: &[u8], k: &[u8]) -> Vec<u128> {
    let n = msg.len() / 128;
    let zero = [0u8; 128];
    (0..=n)
        .map(|j| {
            let prev: &[u8] = if j == 0 { &zero } else { &msg[128 * (j - 1)..128 * j] };
            let cur: &[u8] = if j == n { &zero } else { &msg[128 * j..128 * j + 128] };
            let row = &k[128 * j..128 * j + 128];
            (0..8).fold(0u128, |s, l| {
                let tx = rd64(prev, l) ^ rd64(cur, l) ^ rd64(row, l);
                let ty = rd64(prev, 8 + l) ^ rd64(cur, 8 + l) ^ rd64(row, 8 + l);
                s ^ clmul64(tx, ty)
            })
        })
        .collect()
}

/// v1 (SPEC §4) for a one-chunk message of whole blocks.
pub fn ref_v1_small(keys: &Keys, msg: &[u8]) -> u128 {
    let s = ref_positions(msg, &keys.comp);
    let (mut h0, mut h1) = (0u128, 0u128);
    for (j, &sj) in s.iter().enumerate() {
        h0 ^= sj;
        h1 ^= gf_mul(j as u128, sj);
    }
    let o = &keys.outer;
    gf_mul(h0, o.r) ^ gf_mul(h1, o.a[0]) ^ gf_mul(msg.len() as u128, o.t) ^ o.s
}

/// G64 direct branch for a one-chunk message of whole blocks.
pub fn ref_g64_small(keys: &Keys, msg: &[u8]) -> u128 {
    let s = ref_positions(msg, &keys.comp);
    let (mut h0, mut h1) = (0u64, 0u64);
    for (j, &sj) in s.iter().enumerate() {
        let e = reduce_p64(sj, 0);
        h0 ^= e;
        h1 ^= e_mul(j as u64, e);
    }
    let o = &keys.outer;
    let (t0, t1) = (o.t as u64, (o.t >> 64) as u64);
    let l = msg.len() as u64;
    let u = (h0 ^ e_mul(l, t0)) as u128 | ((h1 ^ e_mul(l, t1)) as u128) << 64;
    u ^ o.s
}

/// Per-lane position sums of a fresh chunk of `NB` whole blocks at `d`.
#[inline(always)]
unsafe fn fresh_positions<const NB: usize>(k: *const u8, d: *const u8) -> [Z; 9] {
    unsafe {
        let mut p = [zero(); 9];
        let (mut px, mut py) = (zero(), zero());
        for (j, pj) in p.iter_mut().enumerate().take(NB) {
            let q = d.add(128 * j);
            let (x, y) = (ld(q), ld(q.add(64)));
            let t = x3(px, x, ld(k.add(128 * j)));
            let w = x3(py, y, ld(k.add(128 * j + 64)));
            *pj = xor(clo(t, w), chi(t, w));
            px = x;
            py = y;
        }
        let t = xor(px, ld(k.add(128 * NB)));
        let w = xor(py, ld(k.add(128 * NB + 64)));
        p[NB] = xor(clo(t, w), chi(t, w));
        p
    }
}

#[inline(always)]
fn plane_of<const NB: usize>(p: &[Z; 9], k: usize) -> Z {
    unsafe { (0..=NB).filter(|j| j >> k & 1 == 1).fold(zero(), |a, j| xor(a, p[j])) }
}

/// v1's `mix` (AVX-512 backend): `h0 R + h1 R2 + L T` with one packed
/// multiply by `(R, T, R2, 0)` and a two-multiply reduction.
#[inline(always)]
unsafe fn v1_mix(pk: *const u8, h0v: Z, h1v: Z, len: u64) -> X {
    unsafe {
        let w = fold2(h0v, h1v);
        let a = _mm512_mask_blend_epi64(0b0000_1100, w, _mm512_maskz_set1_epi64(0b0000_0100, len as i64));
        let (k, ks) = (ld(pk), ld(pk.add(64)));
        let mid = xor(clo(a, ks), chi(a, ks));
        let (lo, hi) = (clo(a, k), chi(a, k));
        let poly = _mm512_set1_epi64(0x87);
        let t = _mm512_clmulepi64_epi128::<0x01>(hi, poly);
        let w2 = x3(hi, _mm512_bsrli_epi128::<8>(mid), _mm512_bsrli_epi128::<8>(t));
        let lo = x3(lo, _mm512_bslli_epi128::<8>(mid), _mm512_bslli_epi128::<8>(t));
        fold_x(xor(lo, clo(w2, poly)))
    }
}

/// `v r(x)` per 64-bit word, `v` below 2^60.
#[inline(always)]
unsafe fn mulr_x(v: X) -> X {
    unsafe {
        _mm_ternarylogic_epi64::<0x96>(v, _mm_slli_epi64::<1>(v), _mm_xor_si128(_mm_slli_epi64::<3>(v), _mm_slli_epi64::<4>(v)))
    }
}

/// Per 64-bit word `i`: `lo[i] + hi[i] x^64 + over[i] x^128` mod p64.
#[inline(always)]
unsafe fn reduce_p64_x(lo: X, hi: X, over: X) -> X {
    unsafe {
        let hi = _mm_xor_si128(hi, mulr_x(over));
        let t_lo = mulr_x(hi);
        let t_hi = _mm_ternarylogic_epi64::<0x96>(_mm_srli_epi64::<63>(hi), _mm_srli_epi64::<61>(hi), _mm_srli_epi64::<60>(hi));
        _mm_ternarylogic_epi64::<0x96>(lo, t_lo, mulr_x(t_hi))
    }
}

pub struct V1Pk(pub [u128; 8]);

impl V1Pk {
    pub fn new(keys: &Keys) -> V1Pk {
        let sw = |v: u128| v.rotate_left(64);
        let o = &keys.outer;
        V1Pk([o.r, o.t, o.a[0], 0, sw(o.r), sw(o.t), sw(o.a[0]), 0])
    }
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_v1_small<const NB: usize>(keys: &Keys, pk: &V1Pk, msg: &[u8]) -> u128 {
    unsafe {
        let p = fresh_positions::<NB>(keys.comp.as_ptr(), msg.as_ptr());
        let h0 = (0..=NB).fold(zero(), |a, j| xor(a, p[j]));
        let h1 = shifted_sum([plane_of::<NB>(&p, 0), plane_of::<NB>(&p, 1), plane_of::<NB>(&p, 2), plane_of::<NB>(&p, 3), zero(), zero()]);
        let h = v1_mix(pk.0.as_ptr() as *const u8, h0, h1, msg.len() as u64);
        let r = _mm_xor_si128(h, _mm_loadu_si128((&keys.outer.s as *const u128).cast()));
        let mut out = 0u128;
        _mm_storeu_si128((&mut out as *mut u128).cast(), r);
        out
    }
}

#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_g64_small<const NB: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let p = fresh_positions::<NB>(keys.comp.as_ptr(), msg.as_ptr());
        let h0v = (0..=NB).fold(zero(), |a, j| xor(a, p[j]));
        // h1 = sum_k x^k plane_k, unreduced: 128 low bits and the (at most
        // three) bits above.
        let pl = [plane_of::<NB>(&p, 0), plane_of::<NB>(&p, 1), plane_of::<NB>(&p, 2), plane_of::<NB>(&p, 3)];
        let h1lo = x4(pl[0], shl::<1>(pl[1]), shl::<2>(pl[2]), shl::<3>(pl[3]));
        let over = x3(_mm512_srli_epi64::<63>(pl[1]), _mm512_srli_epi64::<62>(pl[2]), _mm512_srli_epi64::<61>(pl[3]));
        let (a, b, o) = (fold_x(h0v), fold_x(h1lo), fold_x(over));
        // Words: lo = (h0.lo, h1.lo), hi = (h0.hi, h1.hi), over = (0, o.hi).
        let lo = _mm_unpacklo_epi64(a, b);
        let hi = _mm_unpackhi_epi64(a, b);
        let ov = _mm_unpackhi_epi64(_mm_setzero_si128(), o);
        let u = reduce_p64_x(lo, hi, ov);
        // (L T0, L T1) mod p64: independent of the message.
        let l = _mm_set_epi64x(0, msg.len() as i64);
        let t = _mm_loadu_si128((&keys.outer.t as *const u128).cast());
        let c0 = _mm_clmulepi64_si128::<0x00>(l, t);
        let c1 = _mm_clmulepi64_si128::<0x10>(l, t);
        let lt = reduce_p64_x(_mm_unpacklo_epi64(c0, c1), _mm_unpackhi_epi64(c0, c1), _mm_setzero_si128());
        let r = _mm_ternarylogic_epi64::<0x96>(u, lt, _mm_loadu_si128((&keys.outer.s as *const u128).cast()));
        let mut out = 0u128;
        _mm_storeu_si128((&mut out as *mut u128).cast(), r);
        out
    }
}

/// `simd_g64_small` with a leaner close: the length products reduced in one
/// step (`L < 2^60`), and the lane sums of `h0`, `h1` and the overflow
/// folded together.
#[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
pub unsafe fn simd_g64b_small<const NB: usize>(keys: &Keys, msg: &[u8]) -> u128 {
    unsafe {
        let p = fresh_positions::<NB>(keys.comp.as_ptr(), msg.as_ptr());
        let h0v = (0..=NB).fold(zero(), |a, j| xor(a, p[j]));
        let pl = [plane_of::<NB>(&p, 0), plane_of::<NB>(&p, 1), plane_of::<NB>(&p, 2), plane_of::<NB>(&p, 3)];
        let h1lo = x4(pl[0], shl::<1>(pl[1]), shl::<2>(pl[2]), shl::<3>(pl[3]));
        // The overflow bits of h1 are in the high word of each lane.
        let over = x3(_mm512_srli_epi64::<63>(pl[1]), _mm512_srli_epi64::<62>(pl[2]), _mm512_srli_epi64::<61>(pl[3]));
        // (A, A, B, B) lane sums of h0 and h1; the overflow folds with h1's
        // high word below.
        let w = fold2(h0v, h1lo);
        let ov = fold_x(over);
        // lo = (h0.lo, h1.lo), hi = (h0.hi, h1.hi): words 0, 4 and 1, 5 of w.
        let idx_lo = _mm512_set_epi64(0, 0, 0, 0, 0, 0, 4, 0);
        let idx_hi = _mm512_set_epi64(0, 0, 0, 0, 0, 0, 5, 1);
        let lo = _mm512_castsi512_si128(_mm512_permutexvar_epi64(idx_lo, w));
        let hi = _mm512_castsi512_si128(_mm512_permutexvar_epi64(idx_hi, w));
        let hi = _mm_xor_si128(hi, mulr_x(_mm_unpackhi_epi64(_mm_setzero_si128(), ov)));
        let t_lo = mulr_x(hi);
        let t_hi = _mm_ternarylogic_epi64::<0x96>(_mm_srli_epi64::<63>(hi), _mm_srli_epi64::<61>(hi), _mm_srli_epi64::<60>(hi));
        // (L T0, L T1): one step, the high words are below 2^60.
        let l = _mm_set_epi64x(0, msg.len() as i64);
        let t = _mm_loadu_si128((&keys.outer.t as *const u128).cast());
        let c0 = _mm_clmulepi64_si128::<0x00>(l, t);
        let c1 = _mm_clmulepi64_si128::<0x10>(l, t);
        let lt = _mm_xor_si128(_mm_unpacklo_epi64(c0, c1), mulr_x(_mm_unpackhi_epi64(c0, c1)));
        let u = _mm_ternarylogic_epi64::<0x96>(lo, t_lo, mulr_x(t_hi));
        let r = _mm_ternarylogic_epi64::<0x96>(u, lt, _mm_loadu_si128((&keys.outer.s as *const u128).cast()));
        let mut out = 0u128;
        _mm_storeu_si128((&mut out as *mut u128).cast(), r);
        out
    }
}
