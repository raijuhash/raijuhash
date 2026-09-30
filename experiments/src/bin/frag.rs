//! X6: a message fed to `Hasher::update` in fixed-size pieces, against one
//! `update` and `Key::hash`. Usage: frag [total_bytes ...]
use std::hint::black_box;
use std::time::Instant;

fn best(f: &mut dyn FnMut(), n: u64) -> f64 {
    f();
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
    let key = match std::env::var("RJ_BACKEND").as_deref() {
        Ok(name) => {
            let backend = match name {
                "sse" => raijuhash::Backend::X86Sse,
                "avx2" => raijuhash::Backend::X86Avx2,
                _ => raijuhash::Backend::X86Avx512,
            };
            let bytes: Vec<u8> = (0..raijuhash::KEY_BYTES).map(|i| (i * 13 + 5) as u8).collect();
            raijuhash::Key::with_backend(bytes.as_slice().try_into().unwrap(), backend)
        },
        Err(_) => raijuhash::Key::from_seed(7),
    };
    let totals: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    // `PIECE=n` times that piece size alone (for `perf`).
    let pieces: Vec<usize> = match std::env::var("PIECE") {
        Ok(p) => vec![p.parse().unwrap()],
        Err(_) => vec![1, 7, 16, 31, 64, 127, 128, 1000, 1024],
    };
    print!("{:>7} {:>8} {:>8}", "bytes", "hash", "1 upd");
    for &p in &pieces {
        print!(" {:>7}", format!("p{p}"));
    }
    println!();
    for total in totals {
        let msg: Vec<u8> = (0..total).map(|i| i as u8).collect();
        let n = (2_000_000 / (total as u64 + 64)).max(10);
        // `ONLY_PIECES` skips the whole-message cases (for `perf`).
        let only = std::env::var("ONLY_PIECES").is_ok();
        let one = if only { 0.0 } else { best(&mut || { black_box(key.hash(black_box(&msg))); }, n) };
        let upd = if only { 0.0 } else { best(&mut || { let mut h = key.hasher(); h.update(black_box(&msg)); black_box(h.finalize()); }, n) };
        print!("{total:>7} {one:>8.1} {upd:>8.1}");
        for &p in &pieces {
            let t = best(&mut || {
                let mut h = key.hasher();
                for c in black_box(&msg).chunks(p) {
                    h.update(c);
                }
                black_box(h.finalize());
            }, std::env::var("ITERS").map_or((n / (1 + (total / p) as u64 / 8)).max(1), |v| v.parse().unwrap()));
            print!(" {t:>7.1}");
        }
        println!();
    }
}
