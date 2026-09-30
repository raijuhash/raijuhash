//! `Key::hash` throughput against the input's offset from a 64-byte
//! boundary (CANDIDATES.md X15). Usage: off [sizes...]
use std::hint::black_box;
use std::time::Instant;

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
    let key = raijuhash::Key::from_seed(7);
    let sizes: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let offs = [0usize, 1, 8, 16, 32, 63];
    print!("{:>8}", "bytes");
    for o in offs {
        print!(" {:>9}", format!("+{o} ns"));
    }
    println!();
    for sz in sizes {
        let buf: Vec<u8> = (0..sz + 16384 + 64).map(|i| i as u8).collect();
        let base = (4096usize.wrapping_sub(buf.as_ptr() as usize)) % 16384;
        let n = (200_000_000 / (sz + 64)).max(10) as u64;
        print!("{sz:>8}");
        let mut first = 0.0;
        for (i, o) in offs.iter().enumerate() {
            let m = &buf[base + o..base + o + sz];
            let t = best(&mut || { black_box(key.hash(black_box(m))); }, n);
            if i == 0 {
                first = t;
                print!(" {t:>9.1}");
            } else {
                print!(" {:>9}", format!("{t:.1} {:+.0}%", 100.0 * (t / first - 1.0)));
            }
        }
        println!();
    }
}
