//! Item 12 of progress.md: can full-round AES on Zen 5's otherwise idle AES
//! units add throughput next to v1's carryless chain? Bare loops, hot data:
//! per 1 KiB group, v1's eight chain blocks (`experiments::x86::Chain`) plus
//! `A` 64-byte vectors of `AES-128(M + K)` (four 16-byte blocks each, ten
//! rounds, broadcast round keys) summed into an accumulator.
//! Usage: aesmix
#[cfg(target_arch = "x86_64")]
mod run {
    use core::arch::x86_64::*;
    use std::hint::black_box;
    use std::time::Instant;

    use experiments::x86::{Chain, ld, xor};

    type Z = __m512i;

    fn ns_per_cycle() -> f64 {
        let t = Instant::now();
        let n = 20_000_000u64;
        unsafe {
            let mut x: u64 = 0;
            core::arch::asm!(
                "2:",
                ".rept 100", "add {x}, 1", ".endr",
                "dec {n}", "jnz 2b",
                x = inout(reg) x, n = inout(reg) n / 10 => _,
            );
            black_box(x);
        }
        t.elapsed().as_secs_f64() * 1e9 / (n as f64 / 10.0 * 100.0)
    }

    #[inline(always)]
    unsafe fn aes10(x: Z, rk: &[Z; 11]) -> Z {
        unsafe {
            let mut t = xor(x, rk[0]);
            for k in &rk[1..10] {
                t = _mm512_aesenc_epi128(t, *k);
            }
            _mm512_aesenclast_epi128(t, rk[10])
        }
    }

    /// `groups` groups of eight chain blocks and `A` AES vectors; returns a
    /// digest so that nothing is optimized away.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,vpclmulqdq,vaes")]
    unsafe fn hybrid<const A: usize>(d: *const u8, groups: usize, k: *const u8, ak: *const u8, rk: &[Z; 11]) -> Z {
        unsafe {
            let mut c = Chain::new();
            let mut acc = [_mm512_setzero_si512(); 2];
            let stride = 1024 + 64 * A;
            for g in 0..groups {
                let p = d.add(stride * g);
                let q = p.add(1024);
                for v in 0..8 {
                    c.block(v, p.add(128 * v), k.add(128 * v));
                    // Spread the AES vectors between the blocks.
                    let mut j = v;
                    while j < A {
                        let x = xor(ld(q.add(64 * j)), ld(ak.add(64 * j)));
                        acc[j & 1] = xor(acc[j & 1], aes10(x, rk));
                        j += 8;
                    }
                }
                c.end_group_dyn(g & 7);
            }
            xor(c.digest_all(), xor(acc[0], acc[1]))
        }
    }

    /// AES vectors only.
    #[target_feature(enable = "avx512f,avx512bw,avx512vl,vpclmulqdq,vaes")]
    unsafe fn aes_only(d: *const u8, vectors: usize, ak: *const u8, rk: &[Z; 11]) -> Z {
        unsafe {
            let mut acc = [_mm512_setzero_si512(); 4];
            for j in 0..vectors {
                let x = xor(ld(d.add(64 * j)), ld(ak.add(64 * (j & 15))));
                acc[j & 3] = xor(acc[j & 3], aes10(x, rk));
            }
            xor(xor(acc[0], acc[1]), xor(acc[2], acc[3]))
        }
    }

    fn best(f: &mut dyn FnMut()) -> f64 {
        f();
        let mut b = f64::INFINITY;
        for _ in 0..9 {
            let t = Instant::now();
            f();
            b = b.min(t.elapsed().as_secs_f64());
        }
        b
    }

    #[repr(C, align(64))]
    struct Buf([u8; 1 << 16]);

    pub fn main() {
        let npc = ns_per_cycle();
        println!("clock {:.3} GHz", 1.0 / npc);
        let mut data = Box::new(Buf([0; 1 << 16]));
        for (i, b) in data.0.iter_mut().enumerate() {
            *b = (i * 7 + 1) as u8;
        }
        let keys: Vec<u8> = (0..4096).map(|i| (i * 13 + 5) as u8).collect();
        let keys = Box::new(keys);
        unsafe {
            let rk: [Z; 11] = core::array::from_fn(|i| _mm512_set1_epi64(0x0123_4567_89ab_cdef ^ i as i64 * 0x1111));
            let (d, k, ak) = (data.0.as_ptr(), keys.as_ptr(), keys.as_ptr().add(1152));
            let reps = 20_000;
            macro_rules! case {
                ($a:literal) => {{
                    let stride = 1024 + 64 * $a;
                    let groups = 32 * 1024 / stride;
                    let s = best(&mut || for _ in 0..reps { black_box(hybrid::<$a>(black_box(d), groups, k, ak, &rk)); });
                    let bytes = (stride * groups * reps) as f64;
                    let cyc = s * 1e9 / npc;
                    println!("8 chain blocks + {:>2} AES vectors per group: {:5.1} B/cycle ({:.1} cycles per group)", $a, bytes / cyc, cyc / (groups * reps) as f64);
                }};
            }
            case!(0);
            case!(2);
            case!(4);
            case!(5);
            case!(6);
            case!(7);
            case!(8);
            case!(10);
            case!(12);
            let vectors = 256;
            let s = best(&mut || for _ in 0..reps { black_box(aes_only(black_box(d), vectors, ak, &rk)); });
            let cyc = s * 1e9 / npc;
            println!("AES vectors only: {:.1} B/cycle ({:.2} cycles per vector)", (64 * vectors * reps) as f64 / cyc, cyc / (vectors * reps) as f64);
        }
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}
