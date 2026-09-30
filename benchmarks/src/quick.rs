//! Quick timing at chosen sizes: RaijuHash streaming and one-shot.
//! Usage: quick [sizes...]; `RJ_BACKEND=sse|avx2|avx512` forces a
//! RaijuHash backend (with a fixed pattern key instead of a seeded one).
use std::hint::black_box;
use std::time::Instant;

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

fn main() {
    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let ck = match std::env::var("RJ_BACKEND").as_deref() {
        Ok(name) => {
            let backend = match name {
                "sse" => raijuhash::Backend::X86Sse,
                "avx2" => raijuhash::Backend::X86Avx2,
                "avx512" => raijuhash::Backend::X86Avx512,
                _ => raijuhash::Backend::Portable,
            };
            let bytes: Vec<u8> = (0..raijuhash::KEY_BYTES)
                .map(|i| (i * 13 + 5) as u8)
                .collect();
            raijuhash::Key::with_backend(bytes.as_slice().try_into().unwrap(), backend)
        }
        Err(_) => raijuhash::Key::from_seed(0x1234_5678_9abc_def0),
    };
    eprintln!("raijuhash backend: {:?}", ck.backend());
    println!(
        "{:>9} {:>11} {:>11} {:>11} {:>9}",
        "bytes", "stream ns", "oneshot ns", "stream GB/s", "hash GB/s"
    );
    for sz in sizes {
        let mut buf = vec![0u8; sz + 16384];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        let off = (4096usize.wrapping_sub(buf.as_ptr() as usize)) % 16384;
        let data = &buf[off..off + sz];
        let (mut b, mut c) = (f64::INFINITY, f64::INFINITY);
        for _ in 0..3 {
            b = b.min(time_ns(
                &mut || {
                    let mut h = ck.hasher();
                    h.update(black_box(data));
                    black_box(h.finalize());
                },
                0.02,
                5,
            ));
            c = c.min(time_ns(
                &mut || {
                    black_box(ck.hash(black_box(data)));
                },
                0.02,
                5,
            ));
        }
        println!(
            "{sz:>9} {b:>11.1} {c:>11.1} {:>11.2} {:>9.2}",
            sz as f64 / b,
            sz as f64 / c
        );
    }
}
