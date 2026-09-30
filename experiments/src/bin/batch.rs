//! CANDIDATES.md X13: hashing independent short messages (0..=31 bytes)
//! four at a time, one message per 128-bit lane, against `Key::hash` per
//! message. Same outputs (SPEC §3): checked for every length.

#[cfg(target_arch = "x86_64")]
mod run {
    use core::arch::x86_64::*;
    use std::hint::black_box;
    use std::time::Instant;

    use raijuhash::KEY_BYTES;

    type Z = __m512i;

    /// The field elements of SPEC §3: `A`, `B`, `S` (from the key bytes).
    pub struct ShortKey {
        a: u128,
        b: u128,
        s: u128,
    }

    impl ShortKey {
        fn new(bytes: &[u8; KEY_BYTES]) -> ShortKey {
            // The table (65 rows of 128 bytes), then A, B, R, R2, T, S, V.
            let f = |i: usize| u128::from_le_bytes(bytes[65 * 128 + 16 * i..65 * 128 + 16 * i + 16].try_into().unwrap());
            ShortKey { a: f(0), b: f(1), s: f(5) }
        }
    }

    #[inline(always)]
    unsafe fn shl<const K: i32>(a: Z) -> Z {
        unsafe { _mm512_shldi_epi64::<K>(a, _mm512_bslli_epi128::<8>(a)) }
    }

    /// `lo + hi x^128` reduced modulo `x^128 + x^7 + x^2 + x + 1`, per lane.
    #[inline(always)]
    unsafe fn reduce(lo: Z, hi: Z) -> Z {
        unsafe {
            let x3 = |a, b, c| _mm512_ternarylogic_epi64::<0x96>(a, b, c);
            let t = x3(x3(hi, shl::<1>(hi), shl::<2>(hi)), shl::<7>(hi), lo);
            let o = x3(_mm512_srli_epi64::<63>(hi), _mm512_srli_epi64::<62>(hi), _mm512_srli_epi64::<57>(hi));
            let o = _mm512_bsrli_epi128::<8>(o);
            let po = x3(o, _mm512_slli_epi64::<1>(o), _mm512_xor_si512(_mm512_slli_epi64::<2>(o), _mm512_slli_epi64::<7>(o)));
            _mm512_xor_si512(t, po)
        }
    }

    /// Four messages of fewer than 32 bytes, one per lane.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2")]
    pub unsafe fn hash4<const KARATSUBA: bool>(k: &ShortKey, m: [&[u8]; 4], out: &mut [u128; 4]) {
        unsafe {
            let (mut x0, mut x1) = (_mm512_setzero_si512(), _mm512_setzero_si512());
            macro_rules! lane {
                ($i:literal) => {{
                    let msg = m[$i];
                    let len = msg.len();
                    debug_assert!(len < 32);
                    let at = if len < 16 { 15 } else { 31 };
                    let y = _mm256_maskz_loadu_epi8(_bzhi_u32(u32::MAX, len as u32), msg.as_ptr().cast());
                    let y = _mm256_mask_set1_epi8(y, 1u32 << at, len as i8);
                    x0 = _mm512_inserti32x4::<$i>(x0, _mm256_castsi256_si128(y));
                    x1 = _mm512_inserti32x4::<$i>(x1, _mm256_extracti128_si256::<1>(y));
                }};
            }
            lane!(0);
            lane!(1);
            lane!(2);
            lane!(3);
            let bc = |v: u128| _mm512_broadcast_i32x4(_mm_loadu_si128((&v as *const u128).cast()));
            let (a, b) = (bc(k.a), bc(k.b));
            let (as_, bs) = (bc(k.a.rotate_left(64)), bc(k.b.rotate_left(64)));
            let cl = |x, y| _mm512_clmulepi64_epi128::<0x00>(x, y);
            let ch = |x, y| _mm512_clmulepi64_epi128::<0x11>(x, y);
            let x3 = |p, q, r| _mm512_ternarylogic_epi64::<0x96>(p, q, r);
            let (lo, hi, mid) = if KARATSUBA {
                // mid = (x0 + x1)(k0 + k1) + lo + hi, summed over both products.
                let kx = |v: u128| bc((v as u64 ^ (v >> 64) as u64) as u128);
                let fold = |x: Z| _mm512_xor_si512(x, _mm512_bsrli_epi128::<8>(x));
                let lo = _mm512_xor_si512(cl(x0, a), cl(x1, b));
                let hi = _mm512_xor_si512(ch(x0, a), ch(x1, b));
                let m = _mm512_xor_si512(cl(fold(x0), kx(k.a)), cl(fold(x1), kx(k.b)));
                (lo, hi, x3(m, lo, hi))
            } else {
                let lo = _mm512_xor_si512(cl(x0, a), cl(x1, b));
                let hi = _mm512_xor_si512(ch(x0, a), ch(x1, b));
                (lo, hi, x3(x3(cl(x0, as_), ch(x0, as_), cl(x1, bs)), ch(x1, bs), _mm512_setzero_si512()))
            };
            let h = reduce(_mm512_xor_si512(lo, _mm512_bslli_epi128::<8>(mid)), _mm512_xor_si512(hi, _mm512_bsrli_epi128::<8>(mid)));
            _mm512_storeu_si512(out.as_mut_ptr().cast(), _mm512_xor_si512(h, bc(k.s)));
        }
    }

    #[target_feature(enable = "sse2,sse4.1,aes")]
    unsafe fn schedule(key: u128) -> [u128; 11] {
        unsafe {
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
            let k0 = _mm_set_epi64x((key >> 64) as i64, key as i64);
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
            [k0, k1, k2, k3, k4, k5, k6, k7, k8, k9, k10].map(|k| {
                let mut r = 0u128;
                _mm_storeu_si128((&mut r as *mut u128).cast(), k);
                r
            })
        }
    }

    /// Four MAC tags (SPEC §6: `AES(N) + AES(2^127 + (N + h))`, the low 127
    /// bits of `N` and `h`) for four short messages and nonces: the hashes
    /// from `hash4`, then both AES blocks of all four tags as two 512-bit
    /// VAES chains.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,avx512vbmi2,vpclmulqdq,pclmulqdq,bmi2,aes,vaes")]
    pub unsafe fn tag4(k: &ShortKey, rk: &[u128; 11], nonce: [u128; 4], m: [&[u8]; 4], out: &mut [u128; 4]) {
        unsafe {
            let mut h = [0u128; 4];
            hash4::<false>(k, m, &mut h);
            const TOP: u128 = 1 << 127;
            let n: [u128; 4] = nonce.map(|v| v & !TOP);
            let ld = |v: &[u128; 4]| _mm512_loadu_si512(v.as_ptr().cast());
            let hv = ld(&h);
            let nv = ld(&n);
            // Second blocks: TOP | (n ^ h), with h truncated to 127 bits.
            let top = _mm512_broadcast_i32x4(_mm_set_epi64x(i64::MIN, 0));
            let low127 = _mm512_broadcast_i32x4(_mm_set_epi64x(i64::MAX, -1));
            let yv = _mm512_or_si512(top, _mm512_and_si512(_mm512_xor_si512(nv, hv), low127));
            let rkb = |i: usize| _mm512_broadcast_i32x4(_mm_loadu_si128((&rk[i] as *const u128).cast()));
            let (mut x, mut y) = (_mm512_xor_si512(nv, rkb(0)), _mm512_xor_si512(yv, rkb(0)));
            for i in 1..10 {
                x = _mm512_aesenc_epi128(x, rkb(i));
                y = _mm512_aesenc_epi128(y, rkb(i));
            }
            let z = _mm512_setzero_si512();
            let t = _mm512_xor_si512(_mm512_aesenclast_epi128(x, z), _mm512_aesenclast_epi128(y, z));
            _mm512_storeu_si512(out.as_mut_ptr().cast(), t);
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let kb: Vec<u8> = (0..KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
        let kb: &[u8; KEY_BYTES] = kb.as_slice().try_into().unwrap();
        let key = raijuhash::Key::from_entropy(kb);
        let sk = ShortKey::new(kb);
        let data: Vec<u8> = (0..140_000).map(|i| (i * 7 + 3) as u8).collect();
        if args.len() > 2 {
            // <mac|hash> <len>: the per-message loop only, for perf.
            let len: usize = args[2].parse().unwrap();
            let mac = raijuhash::Mac::from_parts(kb, 0x0123_4567_89ab_cdef_0f1e_2d3c_4b5a_6978);
            let msgs: Vec<&[u8]> = (0..4096).map(|i| &data[32 * i..32 * i + len]).collect();
            let mut out = vec![0u128; 4096];
            for _ in 0..5000 {
                for (i, (o, m)) in out.iter_mut().zip(&msgs).enumerate() {
                    *o = if args[1] == "mac" { mac.tag(i as u128, black_box(m)) } else { key.hash(black_box(m)) };
                }
                black_box(&out);
            }
            return;
        }
        // Correctness: every length 0..=31, several offsets.
        for len in 0..32 {
            for off in [0usize, 1, 17, 63] {
                let ms: [&[u8]; 4] = core::array::from_fn(|i| &data[off + 40 * i..off + 40 * i + len]);
                let mut out = [0u128; 4];
                for kara in [false, true] {
                    if kara {
                        unsafe { hash4::<true>(&sk, ms, &mut out) };
                    } else {
                        unsafe { hash4::<false>(&sk, ms, &mut out) };
                    }
                    for i in 0..4 {
                        assert_eq!(out[i], key.hash(ms[i]), "len {len} lane {i}");
                    }
                }
            }
        }
        println!("hash4 matches Key::hash for every length 0..=31");
        let aes_key = 0x0123_4567_89ab_cdef_0f1e_2d3c_4b5a_6978u128;
        let mac = raijuhash::Mac::from_parts(kb, aes_key);
        let rk = unsafe { schedule(aes_key) };
        for len in 0..32 {
            let ms: [&[u8]; 4] = core::array::from_fn(|i| &data[5 + 40 * i..5 + 40 * i + len]);
            let nonces: [u128; 4] = core::array::from_fn(|i| (len as u128) << 100 | i as u128 | (i as u128) << 127);
            let mut out = [0u128; 4];
            unsafe { tag4(&sk, &rk, nonces, ms, &mut out) };
            for i in 0..4 {
                assert_eq!(out[i], mac.tag(nonces[i], ms[i]), "tag len {len} lane {i}");
            }
        }
        println!("tag4 matches Mac::tag for every length 0..=31");
        // Throughput over 4096 messages at 32-byte strides.
        for len in [8usize, 16, 24, 31] {
            let msgs: Vec<&[u8]> = (0..4096).map(|i| &data[32 * i..32 * i + len]).collect();
            let mut out = vec![0u128; 4096];
            let t1 = time(&mut || {
                for (o, m) in out.iter_mut().zip(&msgs) {
                    *o = key.hash(black_box(m));
                }
                black_box(&out);
            });
            let t4 = time(&mut || {
                for (o, m) in out.chunks_exact_mut(4).zip(msgs.chunks_exact(4)) {
                    unsafe { hash4::<false>(&sk, [m[0], m[1], m[2], m[3]], o.try_into().unwrap()) };
                }
                black_box(&out);
            });
            let tk = time(&mut || {
                for (o, m) in out.chunks_exact_mut(4).zip(msgs.chunks_exact(4)) {
                    unsafe { hash4::<true>(&sk, [m[0], m[1], m[2], m[3]], o.try_into().unwrap()) };
                }
                black_box(&out);
            });
            println!("{len:>3} B: Key::hash {:.2} ns/msg, hash4 {:.2} ns/msg ({:.2}x), Karatsuba {:.2} ns/msg ({:.2}x)", t1 / 4096.0, t4 / 4096.0, t1 / t4, tk / 4096.0, t1 / tk);
            let tm1 = time(&mut || {
                for (i, (o, m)) in out.iter_mut().zip(&msgs).enumerate() {
                    *o = mac.tag(i as u128, black_box(m));
                }
                black_box(&out);
            });
            let tm4 = time(&mut || {
                for (j, (o, m)) in out.chunks_exact_mut(4).zip(msgs.chunks_exact(4)).enumerate() {
                    let n = [4 * j as u128, 4 * j as u128 + 1, 4 * j as u128 + 2, 4 * j as u128 + 3];
                    unsafe { tag4(&sk, &rk, n, [m[0], m[1], m[2], m[3]], o.try_into().unwrap()) };
                }
                black_box(&out);
            });
            println!("       Mac::tag {:.2} ns/tag, tag4 {:.2} ns/tag ({:.2}x)", tm1 / 4096.0, tm4 / 4096.0, tm1 / tm4);
        }
    }

    fn time(f: &mut dyn FnMut()) -> f64 {
        for _ in 0..50 {
            f();
        }
        let mut best = f64::INFINITY;
        for _ in 0..21 {
            let t = Instant::now();
            for _ in 0..50 {
                f();
            }
            best = best.min(t.elapsed().as_secs_f64() * 1e9 / 50.0);
        }
        best
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}
