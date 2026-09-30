//! CANDIDATES.md §9 prototypes on x86: checks each SIMD kernel against its
//! scalar oracle, then times it against RaijuHash v1 (`Key::hash`) at about
//! the same message sizes. Usage: cand [designs...] (default: all)

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

    fn best_of(f: &mut dyn FnMut()) -> f64 {
        (0..3).map(|_| time_ns(f, 0.02, 5)).fold(f64::INFINITY, f64::min)
    }

    struct Design {
        name: &'static str,
        chunk: usize,
        key: usize,
        reference: fn(&Keys, &[u8]) -> u128,
        simd: unsafe fn(&Keys, &[u8]) -> u128,
    }

    fn designs() -> Vec<Design> {
        vec![
            Design { name: "h0 (v1 chain, joint outer)", chunk: h_chunk(0), key: h_key(0), reference: ref_h::<0>, simd: simd_h::<0> },
            Design { name: "h1 (8K cl + 4K MM)", chunk: h_chunk(1), key: h_key(1), reference: ref_h::<1>, simd: simd_h::<1> },
            Design { name: "h2 (8K cl + 8K MM)", chunk: h_chunk(2), key: h_key(2), reference: ref_h::<2>, simd: simd_h::<2> },
            Design { name: "h4 (16x(4 cl blocks + 1 MM tile))", chunk: H4_CHUNK, key: H4_KEY, reference: ref_h4, simd: simd_h4 },
            Design { name: "hx4 (asm, 4 cl blocks per tile)", chunk: hx_chunk(4), key: hx_key(4), reference: ref_hx::<4>, simd: simd_hx4 },
            Design { name: "hx8 (asm, 8 cl blocks per tile)", chunk: hx_chunk(8), key: hx_key(8), reference: ref_hx::<8>, simd: simd_hx8 },
            Design { name: "hx16 (asm, 16 cl blocks per tile)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hx16 },
            Design { name: "hx32 (asm, 32 cl blocks per tile)", chunk: hx_chunk(32), key: hx_key(32), reference: ref_hx::<32>, simd: simd_hx32 },
            Design { name: "hx16f (asm, lighter close)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hx16_fast },
            Design { name: "hx8f (asm, lighter close)", chunk: hx_chunk(8), key: hx_key(8), reference: ref_hx::<8>, simd: simd_hx8_fast },
            Design { name: "hxf8 (asm incl. close, 8 per tile)", chunk: hx_chunk(8), key: hx_key(8), reference: ref_hx::<8>, simd: simd_hxf8 },
            Design { name: "hxf16 (asm incl. close, 16 per tile)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hxf16 },
            Design { name: "hxf32 (asm incl. close, 32 per tile)", chunk: hx_chunk(32), key: hx_key(32), reference: ref_hx::<32>, simd: simd_hxf32 },
            Design { name: "hxf16pf1k (asm, prefetch 1 KiB)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hxf16_pf1k },
            Design { name: "hxf16pf2k (asm, prefetch 2 KiB)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hxf16_pf2k },
            Design { name: "hxf16pf4k (asm, prefetch 4 KiB)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hxf16_pf4k },
            Design { name: "hx8seq (asm, 8 blocks then tile)", chunk: hx_chunk(8), key: hx_key(8), reference: ref_hx::<8>, simd: simd_hx8_seq },
            Design { name: "hx16seq (asm, 16 blocks then tile)", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hx16_seq },
            Design { name: "m (MM only, 8K)", chunk: M_CHUNK, key: M_CHUNK, reference: ref_m, simd: simd_m },
            Design { name: "diag-h0-noclose", chunk: h_chunk(0), key: h_key(0), reference: ref_h::<0>, simd: simd_h_noclose::<0> },
            Design { name: "diag-h1-noclose", chunk: h_chunk(1), key: h_key(1), reference: ref_h::<1>, simd: simd_h_noclose::<1> },
            Design { name: "diag-h2-noclose", chunk: h_chunk(2), key: h_key(2), reference: ref_h::<2>, simd: simd_h_noclose::<2> },
            Design { name: "diag-hx16-noclose", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hx16_noclose },
            Design { name: "diag-hx16-unchained", chunk: hx_chunk(16), key: hx_key(16), reference: ref_hx::<16>, simd: simd_hx16_unchained },
            Design { name: "diag-m-noclose", chunk: M_CHUNK, key: M_CHUNK, reference: ref_m, simd: simd_m_noclose },
        ]
    }

    pub fn main() {
        let filter: Vec<String> = std::env::args().skip(1).collect();
        let v1 = raijuhash::Key::from_seed(7);
        eprintln!("v1 backend: {:?}", v1.backend());
        let mut buf = vec![0u8; (4 << 20) + 16384];
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        for b in buf.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *b = s as u8;
        }
        let off = (4096usize.wrapping_sub(buf.as_ptr() as usize)) % 16384;
        let data = &buf[off..];
        if filter.first().map(String::as_str) == Some("--loop") {
            // --loop <design prefix|v1> <bytes> <iters>: one case, for perf.
            let (name, bytes, iters): (&str, usize, u64) = (&filter[1], filter[2].parse().unwrap(), filter[3].parse().unwrap());
            if name == "v1" {
                let m = &data[..bytes];
                for _ in 0..iters {
                    black_box(v1.hash(black_box(m)));
                }
                return;
            }
            let d = designs().into_iter().find(|d| d.name.starts_with(name)).unwrap();
            let keys = Keys::new(d.key, 1);
            let m = &data[..bytes / d.chunk * d.chunk];
            for _ in 0..iters {
                black_box(unsafe { (d.simd)(&keys, black_box(m)) });
            }
            return;
        }
        for d in designs() {
            if !filter.is_empty() && !filter.iter().any(|f| d.name.starts_with(f.as_str())) {
                continue;
            }
            let keys = Keys::new(d.key, 0x1234 ^ d.chunk as u64);
            for n in [1, 2, 3] {
                let m = &data[..n * d.chunk];
                let (r, v) = ((d.reference)(&keys, m), unsafe { (d.simd)(&keys, m) });
                if d.name.starts_with("diag") {
                    break;
                }
                assert_eq!(r, v, "{}: {n} chunks", d.name);
            }
            println!("{} ({} B chunks): matches its oracle", d.name, d.chunk);
            println!("{:>9} {:>10} {:>10} {:>8} {:>8} {:>7}", "bytes", "v1 ns", "cand ns", "v1 GB/s", "cand", "ratio");
            for target in [16384usize, 65536, 262144, 1 << 20, 4 << 20] {
                let n = (target / d.chunk).max(1);
                let m = &data[..n * d.chunk];
                let a = best_of(&mut || {
                    black_box(v1.hash(black_box(m)));
                });
                let b = best_of(&mut || {
                    black_box(unsafe { (d.simd)(&keys, black_box(m)) });
                });
                let len = m.len() as f64;
                println!("{:>9} {a:>10.1} {b:>10.1} {:>8.1} {:>8.1} {:>7.3}", m.len(), len / a, len / b, a / b);
            }
        }
    }
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    run::main();
}
