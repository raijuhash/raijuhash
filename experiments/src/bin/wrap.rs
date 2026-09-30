//! `Key::hash` through one non-inlined wrapper, so that only the library's
//! code differs between builds (harness closures change their inlining with
//! the size of an inlined `Key::hash`, which moves results by 5-30%).
//! Throughput of independent calls at fixed lengths, and over random lengths
//! and 64-byte-aligned random offsets (2^16 cases). Usage: wrap [len ...];
//! `RJ_BACKEND=sse|avx2` forces a backend.
use std::hint::black_box;
use std::time::Instant;

use raijuhash::Key;

#[inline(never)]
fn h(key: &Key, m: &[u8]) -> u128 {
    key.hash(m)
}

fn best(f: &mut dyn FnMut(), n: u64) -> f64 {
    let mut b = f64::INFINITY;
    for _ in 0..9 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        b = b.min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
    }
    b
}

fn main() {
    // `RJ_BACKEND=sse|avx2` forces a backend (with a pattern key).
    let key = match std::env::var("RJ_BACKEND").as_deref() {
        Ok(name) => {
            let backend = match name {
                "sse" => raijuhash::Backend::X86Sse,
                "avx2" => raijuhash::Backend::X86Avx2,
                _ => raijuhash::Backend::X86Avx512,
            };
            let bytes: Vec<u8> = (0..raijuhash::KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
            Key::with_backend(bytes.as_slice().try_into().unwrap(), backend)
        },
        Err(_) => Key::from_seed(7),
    };
    let buf: Vec<u8> = (0..(1 << 17) + 4096).map(|i| (i * 7 + 3) as u8).collect();
    let lens: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    print!("fixed ns:");
    for &len in &lens {
        let m = &buf[64..64 + len];
        let t = best(&mut || { black_box(h(&key, black_box(m))); }, 1_000_000);
        print!(" {len}:{t:.2}");
    }
    println!();
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    print!("random ns:");
    for (lo, hi) in [(1usize, 31usize), (32, 64), (1, 64), (65, 256), (1, 1024)] {
        let cases: Vec<(usize, usize)> =
            (0..1 << 16).map(|_| ((next() as usize % (1 << 17)) & !63, lo + next() as usize % (hi - lo + 1))).collect();
        let t = best(&mut || { for &(o, l) in &cases { black_box(h(&key, black_box(&buf[o..o + l]))); } }, 1) / cases.len() as f64;
        print!(" {lo}-{hi}:{t:.2}");
    }
    println!();
}
