//! CANDIDATES.md §9.4 (G64) one-chunk close vs v1's close on the same
//! position code, fresh-chunk messages of 1..=8 whole blocks, plus the real
//! v1 one-shot. Throughput of back-to-back calls and a dependent chain.

#[cfg(target_arch = "x86_64")]
mod run {
    use std::hint::black_box;
    use std::time::Instant;

    use experiments::x86::*;

    fn time_ns(f: &mut dyn FnMut(), target: f64, rounds: usize) -> f64 {
        let t = Instant::now();
        let mut it = 0u64;
        while t.elapsed().as_secs_f64() < target {
            f();
            it += 1;
        }
        let iters = ((target / (t.elapsed().as_secs_f64() / it as f64)) as u64).max(1);
        let mut best = f64::INFINITY;
        for _ in 0..rounds {
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            best = best.min(t.elapsed().as_secs_f64() * 1e9 / iters as f64);
        }
        best
    }

    fn best(f: &mut dyn FnMut()) -> f64 {
        (0..3).map(|_| time_ns(f, 0.02, 5)).fold(f64::INFINITY, f64::min)
    }

    macro_rules! size {
        ($nb:literal, $keys:expr, $pk:expr, $v1:expr, $buf:expr) => {{
            let keys = $keys;
            let pk = $pk;
            let m: &mut [u8] = &mut $buf[..128 * $nb];
            // Oracles.
            for s in 0..3u8 {
                m[0] ^= s;
                assert_eq!(unsafe { simd_v1_small::<$nb>(keys, pk, m) }, ref_v1_small(keys, m));
                assert_eq!(unsafe { simd_g64_small::<$nb>(keys, m) }, ref_g64_small(keys, m));
                assert_eq!(unsafe { simd_g64b_small::<$nb>(keys, m) }, ref_g64_small(keys, m));
            }
            let m: &[u8] = m;
            let a = best(&mut || { black_box($v1.hash(black_box(m))); });
            let b = best(&mut || { black_box(unsafe { simd_v1_small::<$nb>(keys, pk, black_box(m)) }); });
            let c = best(&mut || { black_box(unsafe { simd_g64b_small::<$nb>(keys, black_box(m)) }); });
            // Latency: each call's input depends on the previous output.
            let mut mm = m.to_vec();
            let la = best(&mut || { let h = $v1.hash(&mm); mm[0] ^= h as u8; });
            let lb = best(&mut || { let h = unsafe { simd_v1_small::<$nb>(keys, pk, &mm) }; mm[0] ^= h as u8; });
            let lc = best(&mut || { let h = unsafe { simd_g64b_small::<$nb>(keys, &mm) }; mm[0] ^= h as u8; });
            println!("{:>6} {a:>9.2} {b:>9.2} {c:>9.2} {:>7.3} | {la:>9.2} {lb:>9.2} {lc:>9.2} {:>7.3}", 128 * $nb, b / c, lb / lc);
        }};
    }

    pub fn main() {
        let v1 = raijuhash::Key::from_seed(7);
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 {
            // <real|proto|g64> <iters>: 128-byte one-shots in a loop, for perf.
            let keys = Keys::new(65 * 128, 99);
            let pk = V1Pk::new(&keys);
            let mut buf = experiments::x86::Aligned::new(8192);
            let m: &[u8] = &buf[..128];
            let n: u64 = args[2].parse().unwrap();
            for _ in 0..n {
                match args[1].as_str() {
                    "real" => { black_box(v1.hash(black_box(m))); }
                    "proto" => { black_box(unsafe { simd_v1_small::<1>(&keys, &pk, black_box(m)) }); }
                    _ => { black_box(unsafe { simd_g64b_small::<1>(&keys, black_box(m)) }); }
                }
            }
            let _ = &mut buf;
            return;
        }
        let keys = Keys::new(65 * 128, 99);
        let pk = V1Pk::new(&keys);
        let mut buf = experiments::x86::Aligned::new(8192);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (i * 29 + 3) as u8;
        }
        println!("oracles checked per size; ns per hash");
        println!("{:>6} {:>9} {:>9} {:>9} {:>7} | {:>9} {:>9} {:>9} {:>7}", "bytes", "v1 real", "v1 proto", "g64", "gain", "lat v1", "lat v1p", "lat g64", "gain");
        size!(1, &keys, &pk, v1, buf);
        size!(2, &keys, &pk, v1, buf);
        size!(3, &keys, &pk, v1, buf);
        size!(4, &keys, &pk, v1, buf);
        size!(6, &keys, &pk, v1, buf);
        size!(8, &keys, &pk, v1, buf);
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}
