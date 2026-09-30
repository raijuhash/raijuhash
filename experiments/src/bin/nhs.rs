//! CANDIDATES.md §9.7, a new short/medium family: NH over GF(2^128) on a
//! fixed-size encoding, one class per size, each with independent keys.
//!
//! Class c (32, 64 or 128 bytes) takes messages of `c/2 .. c` bytes (class 32:
//! 0..32), zero-pads them to `c` bytes with the length in the last byte, and
//! reads `2n = c/16` field elements `X_i`. With independent uniform `K_i`, `S`:
//!
//! ```text
//! H = sum_{i<n} (X_i + K_i)(X_{i+n} + K_{i+n}) + S
//! ```
//!
//! In characteristic 2 the `K_i K_{i+n}` terms cancel in any difference, which
//! is `sum_i K_{i+n} dX_i + K_i dX_{i+n}` plus key-free terms: affine in the
//! key with a nonzero coefficient when the encodings differ, so each class is
//! exactly `1/2^128`-AXU. Different classes use independent keys and `S`.
//! (A research sketch, not a reviewed specification.)

#[cfg(target_arch = "x86_64")]
mod run {
    use core::arch::x86_64::*;
    use std::hint::black_box;
    use std::time::Instant;

    use experiments::x86::gf_mul;

    type Z = __m512i;

    /// Keys of the three classes: 2, 4 and 8 elements, then `S` per class.
    #[repr(C, align(64))]
    pub struct NhKey {
        k32: [u128; 2],
        _pad32: [u128; 2],
        k64: [u128; 4],
        k128: [u128; 8],
        k256: [u128; 16],
        k512: [u128; 32],
        s: [u128; 5],
    }

    impl NhKey {
        fn new(seed: u64) -> NhKey {
            let mut x = seed;
            let mut f = || {
                let mut r = 0u128;
                for _ in 0..2 {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    r = r << 64 | x as u128;
                }
                r
            };
            NhKey {
                k32: [f(), f()],
                _pad32: [0, 0],
                k64: [f(), f(), f(), f()],
                k128: [f(), f(), f(), f(), f(), f(), f(), f()],
                k256: core::array::from_fn(|_| f()),
                k512: core::array::from_fn(|_| f()),
                s: [f(), f(), f(), f(), f()],
            }
        }
    }

    fn encode(msg: &[u8], c: usize) -> Vec<u128> {
        let mut b = vec![0u8; c];
        b[..msg.len()].copy_from_slice(msg);
        b[c - 1] = msg.len() as u8;
        b.chunks(16).map(|w| u128::from_le_bytes(w.try_into().unwrap())).collect()
    }

    pub fn reference(k: &NhKey, msg: &[u8]) -> u128 {
        let (c, keys, s): (usize, &[u128], u128) = match msg.len() {
            0..32 => (32, &k.k32, k.s[0]),
            32..64 => (64, &k.k64, k.s[1]),
            64..128 => (128, &k.k128, k.s[2]),
            128..256 => (256, &k.k256, k.s[3]),
            _ => (512, &k.k512, k.s[4]),
        };
        let x = encode(msg, c);
        let n = x.len() / 2;
        (0..n).fold(s, |h, i| h ^ gf_mul(x[i] ^ keys[i], x[i + n] ^ keys[i + n]))
    }

    #[inline(always)]
    unsafe fn shl<const K: i32>(a: Z) -> Z {
        unsafe { _mm512_shldi_epi64::<K>(a, _mm512_bslli_epi128::<8>(a)) }
    }

    /// `lo + mid x^64 + hi x^128` per lane, reduced with shifts; then the sum
    /// of the four lanes.
    #[inline(always)]
    unsafe fn finish(lo: Z, mid: Z, hi: Z) -> __m128i {
        unsafe {
            let x3 = |a, b, c| _mm512_ternarylogic_epi64::<0x96>(a, b, c);
            let lo = _mm512_xor_si512(lo, _mm512_bslli_epi128::<8>(mid));
            let hi = _mm512_xor_si512(hi, _mm512_bsrli_epi128::<8>(mid));
            let t = x3(x3(hi, shl::<1>(hi), shl::<2>(hi)), shl::<7>(hi), lo);
            let o = x3(_mm512_srli_epi64::<63>(hi), _mm512_srli_epi64::<62>(hi), _mm512_srli_epi64::<57>(hi));
            let o = _mm512_bsrli_epi128::<8>(o);
            let r = x3(t, x3(o, _mm512_slli_epi64::<1>(o), _mm512_slli_epi64::<2>(o)), _mm512_slli_epi64::<7>(o));
            let h = _mm256_xor_si256(_mm512_castsi512_si256(r), _mm512_extracti64x4_epi64::<1>(r));
            _mm_xor_si128(_mm256_castsi256_si128(h), _mm256_extracti128_si256::<1>(h))
        }
    }

    /// Per lane `a * b` unreduced, as `(lo, mid, hi)`.
    #[inline(always)]
    unsafe fn prod(a: Z, b: Z) -> (Z, Z, Z) {
        unsafe {
            let lo = _mm512_clmulepi64_epi128::<0x00>(a, b);
            let hi = _mm512_clmulepi64_epi128::<0x11>(a, b);
            let mid = _mm512_xor_si512(_mm512_clmulepi64_epi128::<0x01>(a, b), _mm512_clmulepi64_epi128::<0x10>(a, b));
            (lo, mid, hi)
        }
    }

    /// Classes of `C = 128 V` bytes (`V` vectors per half): `X_i` pairs with
    /// `X_{i + 4V}`, vector `v` with vector `v + V`.
    /// Per lane `a * b` by Karatsuba, as `(lo, m, hi)` with
    /// `m = (a0 + a1)(b0 + b1)`; the middle term is `m + lo + hi`, applied once
    /// to the sums.
    #[inline(always)]
    unsafe fn prod_k(a: Z, b: Z) -> (Z, Z, Z) {
        unsafe {
            let lo = _mm512_clmulepi64_epi128::<0x00>(a, b);
            let hi = _mm512_clmulepi64_epi128::<0x11>(a, b);
            let fa = _mm512_xor_si512(a, _mm512_bsrli_epi128::<8>(a));
            let fb = _mm512_xor_si512(b, _mm512_bsrli_epi128::<8>(b));
            (lo, _mm512_clmulepi64_epi128::<0x00>(fa, fb), hi)
        }
    }

    #[inline(always)]
    unsafe fn big<const V: usize>(keys: *const u8, s: u128, msg: &[u8]) -> u128 {
        unsafe { big_g::<V, false>(keys, s, msg) }
    }

    #[inline(always)]
    unsafe fn big_g<const V: usize, const KARA: bool>(keys: *const u8, s: u128, msg: &[u8]) -> u128 {
        unsafe {
            let (len, p) = (msg.len(), msg.as_ptr());
            let c = 128 * V;
            let (mut lo, mut mid, mut hi) = (_mm512_setzero_si512(), _mm512_setzero_si512(), _mm512_setzero_si512());
            let vec = |j: usize| {
                let start = 64 * j;
                let n = len.saturating_sub(start).min(64);
                let mut m = if n == 64 {
                    _mm512_loadu_si512(p.add(start).cast())
                } else {
                    _mm512_maskz_loadu_epi8(_bzhi_u64(u64::MAX, n as u32), p.add(start).cast())
                };
                if j == 2 * V - 1 {
                    m = _mm512_mask_set1_epi8(m, 1u64 << 63, len as i8);
                }
                let _ = c;
                _mm512_xor_si512(m, _mm512_loadu_si512(keys.add(64 * j).cast()))
            };
            for v in 0..V {
                let (l, m, h) = if KARA { prod_k(vec(v), vec(v + V)) } else { prod(vec(v), vec(v + V)) };
                lo = _mm512_xor_si512(lo, l);
                mid = _mm512_xor_si512(mid, m);
                hi = _mm512_xor_si512(hi, h);
            }
            if KARA {
                mid = _mm512_ternarylogic_epi64::<0x96>(mid, lo, hi);
            }
            let h = finish(lo, mid, hi);
            let mut r = 0u128;
            _mm_storeu_si128((&mut r as *mut u128).cast(), h);
            r ^ s
        }
    }

    /// `lo + mid x^64 + hi x^128` reduced with two carryless multiplies
    /// (v1's `Wide::reduce`), 128-bit.
    #[inline(always)]
    unsafe fn reduce_x(lo: __m128i, mid: __m128i, hi: __m128i) -> __m128i {
        unsafe {
            let poly = _mm_set1_epi64x(0x87);
            let t = _mm_clmulepi64_si128::<0x01>(hi, poly);
            let w2 = _mm_xor_si128(_mm_xor_si128(hi, _mm_srli_si128::<8>(mid)), _mm_srli_si128::<8>(t));
            let lo = _mm_xor_si128(_mm_xor_si128(lo, _mm_slli_si128::<8>(mid)), _mm_slli_si128::<8>(t));
            _mm_xor_si128(lo, _mm_clmulepi64_si128::<0x00>(w2, poly))
        }
    }

    /// Classes 32 and 64 with 128/256-bit operations only (no cross-lane
    /// 512-bit shuffles), for latency.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    pub unsafe fn nh_lean(k: &NhKey, msg: &[u8]) -> u128 {
        unsafe {
            let len = msg.len();
            if len >= 64 {
                return nh(k, msg);
            }
            let p = msg.as_ptr();
            let (lo, mid, hi, s) = if len < 32 {
                let m = _mm256_mask_set1_epi8(_mm256_maskz_loadu_epi8(_bzhi_u32(u32::MAX, len as u32), p.cast()), 1u32 << 31, len as i8);
                let kv = _mm256_loadu_si256(k.k32.as_ptr().cast());
                let x = _mm256_xor_si256(m, kv);
                let (a, b) = (_mm256_castsi256_si128(x), _mm256_extracti128_si256::<1>(x));
                let lo = _mm_clmulepi64_si128::<0x00>(a, b);
                let hi = _mm_clmulepi64_si128::<0x11>(a, b);
                let mid = _mm_xor_si128(_mm_clmulepi64_si128::<0x01>(a, b), _mm_clmulepi64_si128::<0x10>(a, b));
                (lo, mid, hi, k.s[0])
            } else {
                let m = _mm512_mask_set1_epi8(_mm512_maskz_loadu_epi8(_bzhi_u64(u64::MAX, len as u32), p.cast()), 1u64 << 63, len as i8);
                let x = _mm512_xor_si512(m, _mm512_loadu_si512(k.k64.as_ptr().cast()));
                let (a, b) = (_mm512_castsi512_si256(x), _mm512_extracti64x4_epi64::<1>(x));
                let f = |v: __m256i| _mm_xor_si128(_mm256_castsi256_si128(v), _mm256_extracti128_si256::<1>(v));
                let lo = f(_mm256_clmulepi64_epi128::<0x00>(a, b));
                let hi = f(_mm256_clmulepi64_epi128::<0x11>(a, b));
                let mid = f(_mm256_xor_si256(_mm256_clmulepi64_epi128::<0x01>(a, b), _mm256_clmulepi64_epi128::<0x10>(a, b)));
                (lo, mid, hi, k.s[1])
            };
            let mut r = 0u128;
            _mm_storeu_si128((&mut r as *mut u128).cast(), reduce_x(lo, mid, hi));
            r ^ s
        }
    }

    /// Classes 128..512 by Karatsuba (the 128 class as `big::<1>`).
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    pub unsafe fn nh_kara(k: &NhKey, msg: &[u8]) -> u128 {
        unsafe {
            match msg.len() {
                0..64 => nh_lean(k, msg),
                64..128 => big_g::<1, true>(k.k128.as_ptr() as *const u8, k.s[2], msg),
                128..256 => big_g::<2, true>(k.k256.as_ptr() as *const u8, k.s[3], msg),
                _ => big_g::<4, true>(k.k512.as_ptr() as *const u8, k.s[4], msg),
            }
        }
    }

    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    pub unsafe fn nh(k: &NhKey, msg: &[u8]) -> u128 {
        unsafe {
            let len = msg.len();
            if len >= 256 {
                return big::<4>(k.k512.as_ptr() as *const u8, k.s[4], msg);
            }
            if len >= 128 {
                return big::<2>(k.k256.as_ptr() as *const u8, k.s[3], msg);
            }
            let p = msg.as_ptr();
            let bz = |n: usize| _bzhi_u64(u64::MAX, n as u32);
            let (a, b, s) = if len < 64 {
                // Classes 32 and 64 in one 512-bit load: `X_i` pairs with
                // `X_{i+n}`; the unused lanes (class 32: 2 and 3 of both)
                // are zero and contribute nothing.
                let (at, kp, s) = if len < 32 { (31, k.k32.as_ptr(), k.s[0]) } else { (63, k.k64.as_ptr(), k.s[1]) };
                let m = _mm512_mask_set1_epi8(_mm512_maskz_loadu_epi8(bz(len), p.cast()), 1u64 << at, len as i8);
                let kv = _mm512_loadu_si512(kp.cast());
                let x = _mm512_xor_si512(m, kv);
                // Class 32: (X0 + K0)(X1 + K1): lanes 0 and 1. Class 64: lanes
                // (0, 1) against (2, 3).
                let (a, b) = if len < 32 {
                    (_mm512_maskz_mov_epi64(0x03, x), _mm512_maskz_mov_epi64(0x03, _mm512_shuffle_i64x2::<0b00_00_00_01>(x, x)))
                } else {
                    (_mm512_maskz_mov_epi64(0x0f, x), _mm512_maskz_mov_epi64(0x0f, _mm512_shuffle_i64x2::<0b00_00_11_10>(x, x)))
                };
                (a, b, s)
            } else {
                let m0 = _mm512_loadu_si512(p.cast());
                let m1 = _mm512_mask_set1_epi8(_mm512_maskz_loadu_epi8(bz(len - 64), p.add(64).cast()), 1u64 << 63, len as i8);
                let kv = k.k128.as_ptr() as *const u8;
                let a = _mm512_xor_si512(m0, _mm512_loadu_si512(kv.cast()));
                let b = _mm512_xor_si512(m1, _mm512_loadu_si512(kv.add(64).cast()));
                (a, b, k.s[2])
            };
            let (lo, mid, hi) = prod(a, b);
            let h = finish(lo, mid, hi);
            let mut r = 0u128;
            _mm_storeu_si128((&mut r as *mut u128).cast(), h);
            r ^ s
        }
    }

    pub fn main() {
        let k = NhKey::new(0x5eed);
        let data: Vec<u8> = (0..16384).map(|i| (i * 37 + 11) as u8).collect();
        for len in 0..512 {
            for off in [0usize, 3, 61] {
                let m = &data[off..off + len];
                assert_eq!(unsafe { nh(&k, m) }, reference(&k, m), "len {len}");
                assert_eq!(unsafe { nh_lean(&k, m) }, reference(&k, m), "lean len {len}");
                assert_eq!(unsafe { nh_kara(&k, m) }, reference(&k, m), "kara len {len}");
            }
        }
        println!("NH family matches its oracle for every length 0..=511");
        let v1 = raijuhash::Key::from_seed(7);
        println!("{:>6} {:>12} {:>12} {:>7} | {:>12} {:>12} {:>7}", "bytes", "v1 thr ns", "NH thr ns", "v1/NH", "v1 lat ns", "NH lat ns", "v1/NH");
        for len in [1usize, 15, 16, 31, 32, 48, 63, 64, 65, 100, 127, 128, 192, 255, 256, 384, 511] {
            let m = &data[..len];
            let a = best(&mut || { black_box(v1.hash(black_box(m))); });
            let b = best(&mut || { black_box(unsafe { nh(&k, black_box(m)) }); });
            // Latency: the next message offset depends on the last output.
            let two = &data[..2 * len + 64];
            let mut h = 0u128;
            let la = best(&mut || { let o = (h as usize & 1) * len; h = v1.hash(&two[o..o + len]); });
            let lb = best(&mut || { let o = (h as usize & 1) * len; h = unsafe { nh(&k, &two[o..o + len]) }; });
            // Store-then-hash latency: byte 0 is rewritten from the last output.
            let mut mm = m.to_vec();
            let sa = best(&mut || { let h = v1.hash(&mm); mm[0] ^= h as u8; });
            let sb = best(&mut || { let h = unsafe { nh(&k, &mm) }; mm[0] ^= h as u8; });
            let sl = best(&mut || { let h = unsafe { nh_lean(&k, &mm) }; mm[0] ^= h as u8; });
            let bl = best(&mut || { black_box(unsafe { nh_lean(&k, black_box(m)) }); });
            let bk = best(&mut || { black_box(unsafe { nh_kara(&k, black_box(m)) }); });
            let sk = best(&mut || { let h = unsafe { nh_kara(&k, &mm) }; mm[0] ^= h as u8; });
            println!("{len:>6} {a:>12.2} {b:>12.2} {:>7.2} | {la:>12.2} {lb:>12.2} {:>7.2} | store-lat {sa:>6.2} {sb:>6.2} | lean thr {bl:>5.2} store-lat {sl:>6.2} | kara thr {bk:>5.2} store-lat {sk:>6.2}", a / b, la / lb);
        }
    }

    fn best(f: &mut dyn FnMut()) -> f64 {
        let n = 2_000_000u64;
        let mut b = f64::INFINITY;
        for _ in 0..11 {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            b = b.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
        }
        b
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}
